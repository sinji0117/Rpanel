//! rpanel-files — 사이트 파일 관리 도구.
//!
//! 패널이 `sudo -n -u <사이트유저> /usr/local/lib/rpanel/rpanel-files <op> ...` 로 실행합니다.
//! 사이트 유저 권한으로 동작하므로 운영체제 권한 자체가 1차 방어선이고,
//! 추가로 모든 경로를 사이트 유저 홈(/home/<user>) 안으로 제한합니다 (심볼릭 링크로 밖을 가리키는 것도 거부).
//!
//! ops
//!   list <rel>                     디렉토리 목록 (JSON)
//!   stat <rel>                     항목 정보 (JSON)
//!   read <rel>                     파일 내용 → stdout
//!   write <rel>                    stdin → 파일 (원자적 교체, 기존 권한 유지)
//!   mkdir <rel>                    디렉토리 생성 (상위 포함)
//!   rm <rel>                       파일/디렉토리 삭제 (링크는 링크만)
//!   mv <from> <to>                 이름 변경 / 이동
//!   chmod <rel> <octal>            권한 변경
//!   extract <archive> <dest-dir>   .zip / .tar.gz / .tgz / .tar 풀기
//!   run <dir> <command> [--env]    bash -lc <command> (--env 면 stdin 의 JSON 객체를 환경변수로)

use std::{
    collections::BTreeMap,
    ffi::CStr,
    fs,
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    process::{exit, Command},
};

use serde::Serialize;

fn fail(msg: impl AsRef<str>) -> ! {
    eprintln!("rpanel-files: {}", msg.as_ref());
    exit(2)
}

/// 기준 디렉토리: 현재 사용자의 홈 (root 로는 실행 거부)
fn base_dir() -> PathBuf {
    // sudo 는 env_reset 으로 환경변수를 지우므로, 패널이 이 값을 주입할 수 없습니다 (테스트용)
    if let Ok(b) = std::env::var("RPANEL_FILES_BASE") {
        return PathBuf::from(b);
    }
    let uid = unsafe { libc::getuid() };
    if uid == 0 {
        fail("root 로 실행할 수 없습니다");
    }
    let pw = unsafe { libc::getpwuid(uid) };
    if pw.is_null() {
        fail("사용자 정보를 찾을 수 없습니다");
    }
    let dir = unsafe { CStr::from_ptr((*pw).pw_dir) }
        .to_string_lossy()
        .into_owned();
    if !dir.starts_with("/home/") {
        fail(format!("사이트 유저가 아닙니다 (홈: {dir})"));
    }
    PathBuf::from(dir)
}

pub struct Jail {
    base: PathBuf,
}

impl Jail {
    pub fn new(base: PathBuf) -> io::Result<Self> {
        Ok(Self {
            base: fs::canonicalize(base)?,
        })
    }

    /// 상대 경로 문법 검사: 절대 경로·`..` 금지
    fn join(&self, rel: &str) -> Result<PathBuf, String> {
        let rel = rel.trim_start_matches("./");
        let p = Path::new(rel);
        if p.is_absolute() {
            return Err("절대 경로는 쓸 수 없습니다".into());
        }
        for c in p.components() {
            match c {
                Component::Normal(_) | Component::CurDir => {}
                _ => return Err("경로에 .. 를 쓸 수 없습니다".into()),
            }
        }
        if rel.contains('\0') {
            return Err("잘못된 경로".into());
        }
        Ok(self.base.join(p))
    }

    fn inside(&self, real: &Path) -> bool {
        real == self.base || real.starts_with(&self.base)
    }

    /// 존재하는 경로 (심볼릭 링크는 따라가서 홈 안인지 확인)
    pub fn existing(&self, rel: &str) -> Result<PathBuf, String> {
        let p = self.join(rel)?;
        let real = fs::canonicalize(&p).map_err(|_| format!("없는 경로입니다: {rel}"))?;
        if !self.inside(&real) {
            return Err("홈 디렉토리 밖을 가리킵니다".into());
        }
        Ok(real)
    }

    /// 항목 자체 (마지막 요소가 링크여도 따라가지 않음) — 삭제·이름변경용
    pub fn entry(&self, rel: &str) -> Result<PathBuf, String> {
        let p = self.join(rel)?;
        let name = p
            .file_name()
            .ok_or("홈 디렉토리 자체는 대상이 될 수 없습니다")?
            .to_owned();
        let parent = p.parent().ok_or("잘못된 경로")?;
        let real_parent =
            fs::canonicalize(parent).map_err(|_| "상위 폴더가 없습니다".to_string())?;
        if !self.inside(&real_parent) {
            return Err("홈 디렉토리 밖입니다".into());
        }
        let full = real_parent.join(name);
        if full == self.base {
            return Err("홈 디렉토리 자체는 대상이 될 수 없습니다".into());
        }
        Ok(full)
    }

    /// 새로 만들 경로 (상위 폴더는 존재해야 함). 이미 있으면 링크를 따라가도 홈 안이어야 함
    pub fn creatable(&self, rel: &str) -> Result<PathBuf, String> {
        let full = self.entry(rel)?;
        if fs::symlink_metadata(&full).is_ok() {
            let real = fs::canonicalize(&full).map_err(|_| "깨진 링크입니다".to_string())?;
            if !self.inside(&real) {
                return Err("홈 디렉토리 밖을 가리키는 링크입니다".into());
            }
            return Ok(real);
        }
        Ok(full)
    }
}

#[derive(Serialize)]
pub struct Entry {
    pub name: String,
    pub kind: &'static str,
    pub size: u64,
    pub mode: String,
    pub mtime: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

pub fn entry_of(path: &Path, name: String) -> io::Result<Entry> {
    let m = fs::symlink_metadata(path)?;
    let ft = m.file_type();
    let (kind, target) = if ft.is_symlink() {
        (
            "link",
            fs::read_link(path).ok().map(|t| t.display().to_string()),
        )
    } else if ft.is_dir() {
        ("dir", None)
    } else {
        ("file", None)
    };
    Ok(Entry {
        name,
        kind,
        size: if ft.is_dir() { 0 } else { m.len() },
        mode: format!("{:04o}", m.permissions().mode() & 0o7777),
        mtime: m.mtime(),
        target,
    })
}

fn json<T: Serialize>(v: &T) {
    let out = serde_json::to_string(v).unwrap();
    println!("{out}");
}

fn arg(args: &[String], i: usize, what: &str) -> String {
    args.get(i)
        .cloned()
        .unwrap_or_else(|| fail(format!("{what} 가 필요합니다")))
}

fn ok<T>(r: Result<T, String>) -> T {
    r.unwrap_or_else(|e| fail(e))
}

fn io_ok<T>(r: io::Result<T>, what: &str) -> T {
    r.unwrap_or_else(|e| fail(format!("{what}: {e}")))
}

pub fn run(args: Vec<String>, base: PathBuf) -> i32 {
    let jail = io_ok(Jail::new(base.clone()), "홈 디렉토리");
    let op = arg(&args, 1, "명령");
    match op.as_str() {
        "list" => {
            let dir = ok(jail.existing(&args.get(2).cloned().unwrap_or_default()));
            let mut entries = Vec::new();
            for e in io_ok(fs::read_dir(&dir), "목록") {
                let e = io_ok(e, "목록");
                let name = e.file_name().to_string_lossy().into_owned();
                if let Ok(en) = entry_of(&e.path(), name) {
                    entries.push(en);
                }
            }
            entries.sort_by(|a, b| {
                (a.kind != "dir")
                    .cmp(&(b.kind != "dir"))
                    .then(a.name.cmp(&b.name))
            });
            let rel = dir
                .strip_prefix(&jail.base)
                .unwrap_or(Path::new(""))
                .display()
                .to_string();
            #[derive(Serialize)]
            struct Out {
                path: String,
                entries: Vec<Entry>,
            }
            json(&Out { path: rel, entries });
        }
        "stat" => {
            let rel = arg(&args, 2, "경로");
            let p = ok(jail.existing(&rel));
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            json(&io_ok(entry_of(&p, name), "stat"));
        }
        "read" => {
            let p = ok(jail.existing(&arg(&args, 2, "경로")));
            if p.is_dir() {
                fail("폴더는 읽을 수 없습니다");
            }
            let mut f = io_ok(fs::File::open(&p), "열기");
            let mut out = io::stdout().lock();
            io_ok(io::copy(&mut f, &mut out), "읽기");
        }
        "write" => {
            let target = ok(jail.creatable(&arg(&args, 2, "경로")));
            if target.is_dir() {
                fail("같은 이름의 폴더가 있습니다");
            }
            let mode = fs::metadata(&target)
                .map(|m| m.permissions().mode() & 0o7777)
                .unwrap_or(0o644);
            let tmp = target.with_file_name(format!(
                ".{}.rpanel-tmp-{}",
                target.file_name().unwrap().to_string_lossy(),
                std::process::id()
            ));
            let mut f = io_ok(fs::File::create(&tmp), "쓰기");
            let n = match io::copy(&mut io::stdin().lock(), &mut f) {
                Ok(n) => n,
                Err(e) => {
                    let _ = fs::remove_file(&tmp);
                    fail(format!("쓰기: {e}"))
                }
            };
            io_ok(f.flush(), "쓰기");
            drop(f);
            io_ok(
                fs::set_permissions(&tmp, fs::Permissions::from_mode(mode)),
                "권한",
            );
            if let Err(e) = fs::rename(&tmp, &target) {
                let _ = fs::remove_file(&tmp);
                fail(format!("저장: {e}"));
            }
            println!("{n}");
        }
        "mkdir" => {
            let rel = arg(&args, 2, "경로");
            let p = ok(jail.join(&rel));
            io_ok(fs::create_dir_all(&p), "폴더 만들기");
            // 링크를 거쳐 밖에 만들어졌는지 사후 확인
            let real = io_ok(fs::canonicalize(&p), "폴더 확인");
            if !jail.inside(&real) {
                fail("홈 디렉토리 밖입니다");
            }
        }
        "rm" => {
            let p = ok(jail.entry(&arg(&args, 2, "경로")));
            let m = io_ok(fs::symlink_metadata(&p), "삭제");
            if m.is_dir() {
                io_ok(fs::remove_dir_all(&p), "삭제");
            } else {
                io_ok(fs::remove_file(&p), "삭제");
            }
        }
        "mv" => {
            let from = ok(jail.entry(&arg(&args, 2, "원래 경로")));
            let to = ok(jail.entry(&arg(&args, 3, "새 경로")));
            if fs::symlink_metadata(&to).is_ok() {
                fail("같은 이름이 이미 있습니다");
            }
            io_ok(fs::rename(&from, &to), "이름 변경");
        }
        "chmod" => {
            let p = ok(jail.existing(&arg(&args, 2, "경로")));
            let m = arg(&args, 3, "권한");
            let mode = u32::from_str_radix(&m, 8)
                .ok()
                .filter(|v| *v <= 0o777)
                .unwrap_or_else(|| fail("권한은 000~777 8진수입니다"));
            io_ok(
                fs::set_permissions(&p, fs::Permissions::from_mode(mode)),
                "권한 변경",
            );
        }
        "extract" => {
            let archive = ok(jail.existing(&arg(&args, 2, "압축 파일")));
            let dest = ok(jail.existing(&arg(&args, 3, "대상 폴더")));
            if !dest.is_dir() {
                fail("대상이 폴더가 아닙니다");
            }
            let name = archive.to_string_lossy().to_lowercase();
            let a = archive.to_string_lossy().into_owned();
            let d = dest.to_string_lossy().into_owned();
            let status = if name.ends_with(".zip") {
                Command::new("unzip")
                    .args(["-o", "-q", &a, "-d", &d])
                    .status()
            } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
                Command::new("tar")
                    .args(["-xzf", &a, "-C", &d, "--no-same-owner"])
                    .status()
            } else if name.ends_with(".tar") {
                Command::new("tar")
                    .args(["-xf", &a, "-C", &d, "--no-same-owner"])
                    .status()
            } else {
                fail("지원하는 형식: .zip .tar.gz .tgz .tar")
            };
            let st = io_ok(status, "압축 풀기");
            if !st.success() {
                fail(format!("압축 풀기 실패 (exit {})", st.code().unwrap_or(-1)));
            }
            // 압축 파일에 0600 등으로 저장된 항목도 웹서버(사이트 그룹)가 읽을 수 있게
            let _ = Command::new("chmod").args(["-R", "g+rX", &d]).status();
        }
        "run" => {
            let dir = ok(jail.existing(&arg(&args, 2, "폴더")));
            let cmd = arg(&args, 3, "명령");
            let mut input = String::new();
            if args.get(4).map(|s| s.as_str()) == Some("--env") {
                let _ = io::stdin().read_to_string(&mut input);
            }
            let env: BTreeMap<String, String> = if input.trim().is_empty() {
                BTreeMap::new()
            } else {
                serde_json::from_str(&input).unwrap_or_else(|_| fail("환경변수 JSON 형식 오류"))
            };
            let mut c = Command::new("/bin/bash");
            c.args(["-lc", &cmd]).current_dir(&dir).env("HOME", &base);
            for (k, v) in env {
                if k != "PATH" && k != "HOME" {
                    c.env(k, v);
                }
            }
            let st = io_ok(c.status(), "실행");
            return st.code().unwrap_or(1);
        }
        other => fail(format!("알 수 없는 명령: {other}")),
    }
    0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let base = base_dir();
    exit(run(args, base));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rpanel-files-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(d.join("htdocs/site")).unwrap();
        d
    }

    #[test]
    fn jail_rules() {
        let base = tmp();
        let outside = base
            .parent()
            .unwrap()
            .join(format!("outside-{}", std::process::id()));
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, base.join("htdocs/evil")).unwrap();
        std::os::unix::fs::symlink(base.join("htdocs/site"), base.join("htdocs/ok-link")).unwrap();
        let j = Jail::new(base.clone()).unwrap();

        assert!(j.existing("htdocs/site").is_ok());
        assert!(j.existing("").is_ok());
        assert!(j.existing("../etc").is_err());
        assert!(j.existing("/etc/passwd").is_err());
        assert!(j.existing("htdocs/site/../../..").is_err());
        assert!(j.existing("htdocs/evil/secret").is_err(), "symlink escape");
        assert!(
            j.existing("htdocs/ok-link").is_ok(),
            "symlink inside is fine"
        );
        assert!(
            j.creatable("htdocs/evil/new").is_err(),
            "create through escaping link"
        );
        assert!(
            j.creatable("htdocs/evil").is_err(),
            "overwrite escaping link target"
        );
        assert!(
            j.entry("htdocs/evil").is_ok(),
            "the link itself can be removed"
        );
        assert!(j.entry("").is_err(), "base itself cannot be removed");
        assert!(
            j.creatable("htdocs/missing/new.txt").is_err(),
            "parent must exist"
        );
        let _ = fs::remove_dir_all(&outside);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn entries() {
        let base = tmp();
        fs::write(base.join("htdocs/site/index.html"), "<h1>hi</h1>").unwrap();
        let e = entry_of(&base.join("htdocs/site/index.html"), "index.html".into()).unwrap();
        assert_eq!(e.kind, "file");
        assert_eq!(e.size, 11);
        assert_eq!(e.name, "index.html");
        let d = entry_of(&base.join("htdocs/site"), "site".into()).unwrap();
        assert_eq!(d.kind, "dir");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn ops_via_run() {
        let base = tmp();
        let a = |v: &[&str]| -> Vec<String> {
            std::iter::once("rpanel-files")
                .chain(v.iter().copied())
                .map(String::from)
                .collect()
        };
        assert_eq!(
            run(a(&["mkdir", "htdocs/site/assets/img"]), base.clone()),
            0
        );
        assert!(base.join("htdocs/site/assets/img").is_dir());
        fs::write(base.join("htdocs/site/a.txt"), "1").unwrap();
        assert_eq!(
            run(a(&["chmod", "htdocs/site/a.txt", "600"]), base.clone()),
            0
        );
        assert_eq!(
            fs::metadata(base.join("htdocs/site/a.txt"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            run(
                a(&["mv", "htdocs/site/a.txt", "htdocs/site/b.txt"]),
                base.clone()
            ),
            0
        );
        assert!(base.join("htdocs/site/b.txt").exists());
        assert_eq!(run(a(&["rm", "htdocs/site/assets"]), base.clone()), 0);
        assert!(!base.join("htdocs/site/assets").exists());
        assert_eq!(
            run(
                a(&["run", "htdocs/site", "echo hi > out.txt"]),
                base.clone()
            ),
            0
        );
        assert_eq!(
            fs::read_to_string(base.join("htdocs/site/out.txt"))
                .unwrap()
                .trim(),
            "hi"
        );
        assert_eq!(run(a(&["run", "htdocs/site", "exit 7"]), base.clone()), 7);
        let _ = fs::remove_dir_all(&base);
    }
}
