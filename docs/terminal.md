[← 사용 설명서](README.md)

# 웹 터미널

SSH 프로그램 없이 **브라우저에서 바로** 서버 명령을 실행합니다. 메뉴 **시스템 › 터미널**.

| 종류 | 누구 권한으로 | 기본 | 쓸 수 있는 사람 |
|---|---|---|---|
| **사이트 터미널** | 그 사이트의 리눅스 계정 (예: `blog`) — SSH 로 접속한 것과 같음 | 켜짐 | 관리자 |
| **서버 터미널 (root)** | root — 서버 전체 | **꺼짐** | 관리자 + 2단계 인증, 열 때마다 새 인증 코드 |

보기 전용 계정은 터미널을 쓸 수 없습니다.

## 사이트 터미널

1. **시스템 › 터미널** 에서 사이트를 고르고 **열기** (또는 사이트 화면의 **터미널** 탭 → **터미널 열기**)
2. 사이트 폴더(`/home/<계정>/htdocs/<도메인>`)에서 시작합니다.

```bash
ls -la
tail -f ../../logs/access.log
```

- 사이트 계정이 쓸 수 있는 파일만 다룰 수 있습니다. 다른 사이트나 시스템 설정은 못 건드립니다.
- 여러 개를 탭으로 열 수 있고, **전체 화면** 으로 크게 볼 수 있습니다 (`Shift + Esc` 로 원래대로).

## 서버 터미널 (root)

root 셸은 명령 하나로 서버 전체를 바꿀 수 있어서 **기본으로 꺼져 있고, 패널에서는 켤 수 없습니다.**
패널 비밀번호가 새더라도 서버 터미널이 꺼져 있으면 root 권한까지 넘어가지 않습니다.

### 켜기 · 끄기 (서버에서)

SSH 로 서버에 접속해서:

```bash
sudo rpanel-terminal on       # 켜기
sudo rpanel-terminal off      # 끄기 (열려 있는 서버 터미널도 모두 닫힘)
sudo rpanel-terminal status   # 켜져 있는지, 지금 열린 웹 터미널 목록
sudo rpanel-terminal close    # 열린 웹 터미널(사이트 · 서버) 모두 닫기
```

### 열기 (패널에서)

1. 내 계정에 [2단계 인증](security.md#2단계-인증-otp)을 켭니다 (안 켜져 있으면 열 수 없습니다).
2. **시스템 › 터미널 › 서버 터미널 (root)** 에 인증 앱의 6자리 코드를 넣고 **root 로 열기**.
3. 다시 열 때마다 새 코드가 필요합니다. 다 쓰면 `sudo rpanel-terminal off` 로 꺼 두는 것을 권장합니다.

## 안전장치

- **기록**: 열고 닫을 때 **로그 › 감사 기록** 에 남습니다 (`terminal.open` · `terminal.close` · 코드가 틀리면 `terminal.root_denied`). 서버 터미널은 열 때 **알림**도 가고, 서버 로그에도 남습니다: `journalctl -t rpanel-terminal`
- **자동 닫힘**: 30분 동안 입력이 없거나 12시간이 지나면 닫힙니다. 로그아웃하거나 **보안 › 내 로그인 세션** 에서 세션을 끊어도 닫힙니다.
- **개수 제한**: 한 사람 4개, 서버 전체 12개까지.
- **1회용 티켓**: 터미널 연결은 30초 동안만 쓸 수 있는 1회용 티켓으로 열립니다.

## 동작 방식 (기술)

- 패널(`rpanel` 사용자)은 셸을 직접 띄우지 않고 `sudo rpanel-helper term-site <사이트>` / `term-root <사용자 번호>` 를 의사 터미널(PTY)에 붙여 실행합니다.
- helper 가 권한을 다시 확인한 뒤 `systemd-run --pty` 로 **패널 서비스의 격리(ProtectHome · PrivateTmp) 밖**에서 일반 로그인과 같은 셸을 엽니다. 유닛 이름은 `rpanel-term-<사이트|root>-<난수>` 라서 `systemctl list-units 'rpanel-term-*'` 로 볼 수 있습니다.
- 서버 터미널은 `/etc/rpanel/terminal-root` 파일(root 소유)이 있을 때만 열립니다. 이 파일은 `rpanel-terminal on` 만 만들고, helper 에는 만드는 명령이 없습니다.
- API: `GET /api/terminal`, `POST /api/terminal/ticket`, WebSocket `GET /api/ws/terminal?ticket=&cols=&rows=` (바이너리 = 입력·출력, 텍스트 `{"type":"resize","cols","rows"}`).

## 문제가 생겼을 때

| 증상 | 확인할 것 |
|---|---|
| "연결이 끊겼습니다" 가 바로 뜸 | 패널 앞에 다른 프록시가 있다면 WebSocket(`Upgrade`) 전달이 켜져 있는지 |
| 서버 터미널이 계속 "꺼짐" | 서버에서 `sudo rpanel-terminal status` |
| 글자가 깨짐 | 서버 로케일: `locale -a \| grep -i utf` (없으면 `sudo locale-gen C.UTF-8`) |
