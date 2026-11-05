//! 웹 터미널용 의사 터미널(PTY).
//!
//! 패널(rpanel 사용자)은 셸을 직접 띄우지 않습니다. `sudo -n rpanel-helper term-site|term-root ...` 를
//! PTY 에 붙여 실행하고, helper 가 권한을 다시 확인한 뒤 `systemd-run --pty` 로 패널 샌드박스 밖에서
//! 사이트 유저(또는 root) 셸을 엽니다. 여기서는 PTY 를 만들고 바이트를 주고받는 일만 합니다.

use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    process::Stdio,
    time::Duration,
};

use tokio::{io::unix::AsyncFd, process::Child};

/// 화면 크기 (열 · 행). 너무 작거나 큰 값은 잘라 냅니다.
pub fn clamp_size(cols: u16, rows: u16) -> (u16, u16) {
    (cols.clamp(20, 500), rows.clamp(5, 200))
}

fn winsize(cols: u16, rows: u16) -> libc::winsize {
    let (cols, rows) = clamp_size(cols, rows);
    libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

/// PTY 에 붙여 실행 중인 프로세스
pub struct Pty {
    master: AsyncFd<OwnedFd>,
    child: Child,
}

impl Pty {
    /// `argv[0]` 을 PTY 의 새 세션 리더로 실행합니다. 환경변수는 `env` 만 넘깁니다.
    pub fn spawn(argv: &[String], env: &[(&str, &str)], cols: u16, rows: u16) -> io::Result<Self> {
        let Some((prog, args)) = argv.split_first() else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty command"));
        };
        let mut master: RawFd = -1;
        let mut slave: RawFd = -1;
        let ws = winsize(cols, rows);
        // SAFETY: openpty 가 두 fd 를 채워 줍니다. 실패 시 -1 반환.
        let rc = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                &ws,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openpty 가 방금 연 fd 의 소유권을 가져옵니다.
        let master = unsafe { OwnedFd::from_raw_fd(master) };
        let slave = unsafe { OwnedFd::from_raw_fd(slave) };
        set_cloexec(master.as_raw_fd())?;
        set_cloexec(slave.as_raw_fd())?;
        set_nonblocking(master.as_raw_fd())?;

        let mut cmd = tokio::process::Command::new(prog);
        cmd.args(args)
            .env_clear()
            .envs(env.iter().copied())
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave))
            .kill_on_drop(true);
        // SAFETY: fork 뒤 exec 전에는 async-signal-safe 함수(setsid, ioctl)만 부릅니다.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                // stdin(PTY slave)을 제어 터미널로
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn()?;
        // SAFETY: master 는 이 구조체가 소유한 유효한 fd 이고, AsyncFd 가 가진 동안 바꾸거나 닫지 않습니다.
        let master = unsafe { AsyncFd::register(master)? };
        Ok(Self { master, child })
    }

    /// 출력 읽기. 프로세스가 끝나 PTY 가 닫히면 `Ok(0)`.
    pub async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut guard = self.master.readable().await?;
            match guard.try_io(|fd| {
                // SAFETY: buf 길이만큼만 읽습니다.
                let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }) {
                Ok(Ok(n)) => return Ok(n),
                // 자식 쪽이 모두 닫히면 Linux 는 EIO 를 돌려줌 = 끝
                Ok(Err(e)) if e.raw_os_error() == Some(libc::EIO) => return Ok(0),
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => continue,
            }
        }
    }

    /// 입력 쓰기 (전부 쓸 때까지)
    pub async fn write_all(&self, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            let mut guard = self.master.writable().await?;
            match guard.try_io(|fd| {
                // SAFETY: data 길이만큼만 씁니다.
                let n = unsafe { libc::write(fd.as_raw_fd(), data.as_ptr().cast(), data.len()) };
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }) {
                Ok(Ok(n)) => data = &data[n..],
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => continue,
            }
        }
        Ok(())
    }

    /// 화면 크기 바꾸기 (커널이 셸에 SIGWINCH 를 보냄)
    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let ws = winsize(cols, rows);
        // SAFETY: TIOCSWINSZ 에 winsize 구조체 포인터를 넘깁니다.
        let rc = unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ as _, &ws) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// 끝났으면 종료 코드
    pub fn try_exit_code(&mut self) -> Option<i32> {
        match self.child.try_wait() {
            Ok(Some(st)) => Some(st.code().unwrap_or(-1)),
            _ => None,
        }
    }

    /// 닫기: PTY 를 닫아 셸에 HUP 을 보내고, 잠시 기다린 뒤 남아 있으면 강제로 끝냅니다.
    /// (sudo 는 root 로 돌기 때문에 패널이 신호를 직접 보낼 수 없어 PTY 닫기가 기본 수단)
    pub async fn close(self) -> Option<i32> {
        let Pty { master, mut child } = self;
        drop(master);
        match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(st)) => st.code(),
            _ => {
                let _ = child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
                None
            }
        }
    }
}

fn set_cloexec(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl 로 플래그만 바꿉니다.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl 로 플래그만 바꿉니다.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn read_until(p: &Pty, needle: &str) -> String {
        let mut out = String::new();
        let mut buf = [0u8; 4096];
        let _ = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let n = p.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                out.push_str(&String::from_utf8_lossy(&buf[..n]));
                if out.contains(needle) {
                    break;
                }
            }
        })
        .await;
        out
    }

    fn sh(script: &str) -> Vec<String> {
        vec!["/bin/sh".into(), "-c".into(), script.into()]
    }

    #[test]
    fn size_is_clamped() {
        assert_eq!(clamp_size(0, 0), (20, 5));
        assert_eq!(clamp_size(80, 24), (80, 24));
        assert_eq!(clamp_size(9999, 9999), (500, 200));
    }

    #[tokio::test]
    async fn runs_on_a_real_tty_and_echoes_input() {
        let p = Pty::spawn(
            &sh("test -t 0 && echo TTY-OK; read line; echo \"got:$line\""),
            &[("PATH", "/usr/bin:/bin")],
            80,
            24,
        )
        .unwrap();
        assert!(read_until(&p, "TTY-OK").await.contains("TTY-OK"));
        p.write_all(b"hello\r").await.unwrap();
        assert!(read_until(&p, "got:hello").await.contains("got:hello"));
        assert_eq!(p.close().await, Some(0));
    }

    #[tokio::test]
    async fn resize_reaches_the_program() {
        let p = Pty::spawn(
            &sh("read x; stty size"),
            &[("PATH", "/usr/bin:/bin")],
            80,
            24,
        )
        .unwrap();
        p.resize(132, 40).unwrap();
        p.write_all(b"\r").await.unwrap();
        assert!(read_until(&p, "40 132").await.contains("40 132"));
        p.close().await;
    }

    #[tokio::test]
    async fn eof_and_exit_code_when_program_ends() {
        let mut p = Pty::spawn(&sh("echo bye; exit 3"), &[], 80, 24).unwrap();
        let out = read_until(&p, "\u{0}never").await;
        assert!(out.contains("bye"));
        for _ in 0..50 {
            if p.try_exit_code().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(p.try_exit_code(), Some(3));
    }

    #[tokio::test]
    async fn close_hangs_up_a_running_shell() {
        let p = Pty::spawn(&sh("sleep 600"), &[], 80, 24).unwrap();
        let started = std::time::Instant::now();
        p.close().await;
        assert!(started.elapsed() < Duration::from_secs(8));
    }

    #[test]
    fn empty_command_is_rejected() {
        assert!(Pty::spawn(&[], &[], 80, 24).is_err());
    }
}
