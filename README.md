# Rpanel

[![license](https://img.shields.io/github/license/sinji0117/Rpanel?label=license&color=blue)](LICENSE)
[![language](https://img.shields.io/badge/language-Rust-dea584?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![python](https://img.shields.io/badge/apps-Rust%20%C2%B7%20Python-3776ab?logo=python&logoColor=white)](#이런-걸-할-수-있어요)
[![frontend](https://img.shields.io/badge/frontend-React%2019-61dafb?logo=react&logoColor=black)](frontend)
[![os](https://img.shields.io/badge/OS-Ubuntu%2024.04-e95420?logo=ubuntu&logoColor=white)](#준비물)
[![CI](https://github.com/sinji0117/Rpanel/actions/workflows/ci.yml/badge.svg)](https://github.com/sinji0117/Rpanel/actions/workflows/ci.yml)
[![version](https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2Fsinji0117%2FRpanel%2Fmain%2Fbackend%2FCargo.toml&query=%24.package.version&label=version&color=brightgreen&prefix=v)](backend/Cargo.toml)
[![stars](https://img.shields.io/github/stars/sinji0117/Rpanel?label=stars&color=yellow)](https://github.com/sinji0117/Rpanel/stargazers)
[![Buy Me a Coffee](https://img.shields.io/badge/Buy%20Me%20a%20Coffee-%ED%9B%84%EC%9B%90%ED%95%98%EA%B8%B0-ffdd00?logo=buymeacoffee&logoColor=black)](https://buymeacoffee.com/bandiuri)

**Rust·Python으로 만든 웹 서비스를 내 서버에 올리고 관리하는 무료 관리 화면(웹 패널)입니다.**

리눅스 명령어를 잘 몰라도 브라우저에서 클릭으로 사이트를 만들고, 프로그램을 올리고, https 자물쇠를 붙이고, 백업할 수 있습니다.
누구나 무료로 쓰고 고칠 수 있는 오픈소스(MIT 라이선스)입니다.

<p align="center">
  <a href="https://buymeacoffee.com/bandiuri" target="_blank" rel="noopener noreferrer"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" alt="Buy Me A Coffee — 후원하기" width="320"></a>
</p>
<p align="center"><b>Rpanel이 도움이 되셨다면 커피 한 잔으로 응원해 주세요 ☕</b></p>

---

## 이런 걸 할 수 있어요

- **사이트 만들기**: 주소(도메인)만 넣으면 사이트가 생깁니다.
  - **Rust 프로그램**: 내 컴퓨터에서 빌드한 실행 파일을 올리면 서비스로 실행됩니다.
  - **Python 프로그램**: Flask·FastAPI·Django 등. 파일과 `requirements.txt`를 올리면 가상환경을 만들고 패키지를 설치해서 실행합니다.
  - **HTML 사이트**: HTML·CSS·JS 파일을 올리면 바로 보입니다. React·Vue 등을 빌드한 결과물도 됩니다.
  - **연결(리버스 프록시)**: 다른 서버나 포트로 넘겨줍니다.
- **Git 자동 배포**: GitHub·GitLab에 코드를 올리면 서버가 알아서 빌드(`cargo build`, Python은 패키지 설치)하고 새 버전으로 바꿉니다. 실패하면 이전 버전으로 되돌아갑니다.
- **파일 올리기**: 브라우저에서 끌어다 놓거나, FileZilla 같은 프로그램으로 올립니다.
- **사이트 계정 (SSH · SFTP)**: 사이트마다 접속 계정을 여러 개 만들고, 수정·잠금·비밀번호 변경·삭제를 할 수 있습니다. 외주 개발자·디자이너에게 계정을 따로 줄 때 좋습니다.
- **https 자물쇠**: 무료 인증서(Let's Encrypt)를 클릭 한 번으로 받고, 자동으로 갱신합니다.
- **데이터베이스**: MariaDB와 Redis를 클릭 한 번으로 만들고, 접속 주소를 프로그램에 자동으로 알려 줍니다.
  - **DB 계정 관리**: 데이터베이스마다 계정을 더 만들거나 지울 수 있습니다 (예: 통계용 읽기 전용 계정).
  - **Redis 계정 관리**: 프로그램마다 따로 쓸 Redis 계정을 만들고, 쓸 수 있는 키와 읽기 전용 여부를 정합니다.
- **백업과 복원**: 백업 예약을 원하는 만큼 추가·수정·삭제합니다 (매시간·매일·매주, 앱·DB·전체). 문제가 생기면 그때로 되돌립니다.
- **예약 작업(크론)**: 정해진 시간마다 명령을 실행합니다.
- **상태 보기**: CPU·메모리·디스크 사용량, 서버 정보, 프로그램 로그, 방문·오류 기록을 대시보드에서 실시간으로 봅니다.
- **관리자 여러 명**: 패널에 로그인할 관리자 계정을 추가·수정·삭제합니다.
- **낮·밤 화면, 한국어·English**: 화면 테마(낮·밤·자동)와 언어를 고를 수 있습니다.

사이트마다 별도의 리눅스 계정으로 나뉘어 있어서, 한 사이트에 문제가 생겨도 다른 사이트는 안전합니다.

> **Rust와 Python 프로그램을 위한 패널입니다.** PHP(WordPress 등)나 Node.js 서버 프로그램은 실행하지 않습니다.
> (React·Vue 같은 화면은 빌드한 HTML 파일로 올리면 됩니다.)

---

## 준비물

| 항목 | 내용 |
|---|---|
| 서버 | **Ubuntu Server 24.04**를 새로 설치한 컴퓨터. 클라우드 서버(VPS)나 집의 남는 PC·가상머신 모두 됩니다. |
| 사양 | 메모리 **2GB 이상**(서버에서 Rust를 빌드하려면 4GB 이상 권장), 디스크 20GB 이상 |
| 인터넷 | 설치하는 동안 인터넷 연결이 필요합니다. |
| 도메인 (선택) | `example.com` 같은 주소. 없어도 설치와 둘러보기는 됩니다. |

> 다른 웹 호스팅 관리 패널이 이미 깔린 서버에는 설치하지 마세요. 아무것도 없는 깨끗한 서버를 권장합니다.

---

## 설치하기 (3단계)

### 1. 서버에 접속합니다

- **Windows**: `PowerShell`을 엽니다.
- **Mac**: `터미널`을 엽니다.

그리고 아래처럼 입력합니다. `사용자`와 `서버IP`는 내 서버 정보로 바꾸세요.

```bash
ssh 사용자@서버IP
```

### 2. 아래 한 줄을 복사해서 붙여 넣습니다

```bash
curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/install.sh | sudo bash
```

- 비밀번호를 물으면 서버 계정 비밀번호를 입력합니다. 입력하는 동안 화면에 아무것도 안 보이는 것이 정상입니다.
- **10~20분** 정도 걸립니다. 웹 서버(Nginx), 데이터베이스(MariaDB·Redis), Rust 등 필요한 프로그램을 모두 자동으로 설치합니다.
- 끝나면 `✔ Rpanel 설치 완료`와 접속 주소가 나옵니다.

### 3. 브라우저로 접속합니다

```
https://서버IP:8443
```

1. **"안전하지 않음" 경고가 나와도 정상입니다.** 처음에는 임시 인증서를 쓰기 때문입니다.
   - Chrome: `고급` → `계속 진행`을 누릅니다.
   - Safari: `세부사항 보기` → `이 웹 사이트 방문`을 누릅니다.
2. 첫 화면에서 **관리자 계정(이메일·비밀번호)을 만듭니다.**
3. 끝입니다. 이제 패널을 쓸 수 있습니다.

> **클라우드 서버(AWS, 오라클, 네이버클라우드 등)를 쓰는 경우**
> 업체 관리 화면의 "방화벽"이나 "보안 그룹"에서 포트 **22, 80, 443, 8443**을 열어 주세요. 이것을 안 하면 접속이 안 됩니다.

---

## 첫 사이트 만들기 (Rust 프로그램 올리기)

### 1. 도메인을 서버에 연결합니다

도메인을 산 곳(가비아, 후이즈, Cloudflare 등)의 **DNS 설정**에 아래 두 줄을 추가합니다.

| 종류 | 이름(호스트) | 값 |
|---|---|---|
| A | `@` (비워 두는 곳도 있음) | 서버의 공인 IP |
| A | `www` | 서버의 공인 IP |

반영되는 데 몇 분에서 몇 시간이 걸릴 수 있습니다.

### 2. 프로그램이 `PORT`를 읽도록 합니다

패널이 프로그램마다 빈 포트를 정해서 `PORT` 환경변수로 알려 줍니다. 프로그램은 그 포트로 `127.0.0.1`에서 열기만 하면 됩니다. [axum](https://github.com/tokio-rs/axum)을 쓴다면 이렇게 씁니다.

```rust
#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
    let app = axum::Router::new().route("/", axum::routing::get(|| async { "안녕하세요!" }));
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

### 3. 패널에서 사이트를 추가합니다

1. 왼쪽 메뉴에서 **사이트**를 누르고, **+ 사이트 추가**를 누릅니다. (대시보드의 **+ 사이트 추가** 버튼도 됩니다.)
2. 도메인에 `example.com www.example.com`처럼 입력합니다.
3. 종류는 **Rust**를 고르고 **사이트 만들기**를 누릅니다. 계정 이름·비밀번호·시작 명령은 자동으로 정해집니다. 바꾸고 싶으면 **고급 설정**을 엽니다.
4. 화면에 **접속 정보(사용자 이름·비밀번호)가 한 번만** 나옵니다. 꼭 메모해 두세요. 파일을 올릴 때 씁니다.

### 사이트 계정 관리 (SSH · SFTP)

사이트 → **사이트 계정** 탭에서 접속 계정을 관리합니다.

- **+ 계정 추가**: 계정 이름을 넣고 접속 방식을 고릅니다.
  - **SFTP 전용**: 파일만 올리고 내릴 수 있습니다 (터미널 접속 불가). 외주 디자이너 등에게 권합니다.
  - **SSH + SFTP**: 터미널 접속도 됩니다.
  - 비밀번호를 비우면 자동으로 만들어 줍니다. SSH 공개키만 넣으면 키로만 접속합니다.
- 추가 계정도 그 사이트의 파일(`htdocs`·`logs`)을 읽고 쓸 수 있습니다. 홈 폴더의 `htdocs`가 사이트 폴더로 연결됩니다.
- **수정**(접속 방식·SSH 키·메모), **잠금**(접속만 막기, 사이트는 계속 동작), **비밀번호**(새로 만들기), **삭제**를 할 수 있습니다.
- 기본 계정은 지울 수 없고, 사이트를 지우면 모든 계정이 함께 지워집니다.

### 4. 실행 파일을 빌드해서 올립니다

1. 내 컴퓨터에서 빌드합니다.
   ```bash
   cargo build --release
   ```
   - 서버는 리눅스(보통 x86_64)입니다. Mac이나 Windows에서 빌드한다면 리눅스용으로 빌드해야 합니다. 가장 쉬운 방법은 [`cross`](https://github.com/cross-rs/cross)를 쓰는 것입니다: `cross build --release --target x86_64-unknown-linux-gnu`
   - 직접 빌드하기 번거로우면 아래 **"Git으로 자동 배포하기"**를 쓰세요. 서버가 대신 빌드합니다.
2. `target/release/` 안에 생긴 실행 파일을 올립니다. **이름은 `app`으로** 합니다.
   - **브라우저에서**: 사이트 → **파일** 탭에서 `업로드`를 누르거나 파일을 끌어다 놓습니다.
   - **FileZilla 같은 프로그램으로**: 프로토콜 **SFTP**, 호스트는 서버 IP, 포트 22, 사용자·비밀번호는 3단계에서 받은 접속 정보입니다.
3. 패널 위쪽의 **적용** 버튼을 누릅니다. 프로그램이 실행되고, 정상으로 응답하는지까지 확인합니다.

새 버전을 올릴 때도 똑같이 `app` 파일을 덮어쓰고 **적용**을 누르면 됩니다. 프로그램이 출력하는 로그는 **서비스 로그** 탭에서 봅니다.

### 5. https(자물쇠)를 붙입니다

사이트 → **SSL** 탭에서 `발급`을 누릅니다. 도메인이 이 서버를 가리키고 있어야 발급됩니다(1단계). 인증서는 자동으로 갱신됩니다.

---

## Python 프로그램 올리기

1. **사이트** → **+ 사이트 추가** → 종류 **Python**, 도메인을 넣고 만듭니다. 예시 프로그램(`app.py`)이 바로 실행됩니다.
2. 내 프로그램 파일과 `requirements.txt`(필요한 패키지 목록)를 **파일** 탭이나 SFTP로 올립니다.
3. **설정** 탭에서 시작 명령을 내 프로그램에 맞게 바꿉니다. 프로그램은 `PORT` 환경변수의 포트로 `127.0.0.1`에서 열어야 합니다.

   | 프레임워크 | 시작 명령 예 (`requirements.txt`에 적을 것) |
   |---|---|
   | 그냥 Python | `python app.py` |
   | Flask | `gunicorn -b 127.0.0.1:$PORT app:app` (`flask`, `gunicorn`) |
   | FastAPI | `uvicorn main:app --host 127.0.0.1 --port $PORT` (`fastapi`, `uvicorn`) |
   | Django | `gunicorn -b 127.0.0.1:$PORT mysite.wsgi` (`django`, `gunicorn`) |

4. **적용**을 누르면 사이트 폴더 안에 가상환경(`.venv`)을 만들고 패키지를 설치한 뒤 실행합니다. 패키지를 바꾸면 다시 **적용**을 누르세요.

Git 저장소에 있는 Python 프로그램은 아래 "Git으로 자동 배포하기"에서 종류 **Python**을 고르면 됩니다.

---

## Git으로 자동 배포하기

코드를 GitHub·GitLab에 올려 두었다면, 서버가 직접 빌드하고 실행하게 할 수 있습니다.

1. 왼쪽 메뉴 **프로젝트** → **+ 새 프로젝트**를 만들고, 그 안에서 **+ 새 앱**을 누릅니다.
2. 종류를 고르고 Git 저장소 주소를 넣습니다.
   - **Rust**: **바이너리 이름**(`Cargo.toml`의 `name`)을 넣습니다.
   - **Python**: **시작 명령**을 넣습니다 (위 표 참고). 배포할 때마다 새 가상환경에 `requirements.txt`를 설치합니다.
3. 비공개 저장소라면 앱의 **Git · 웹훅** 탭에 나오는 **Deploy key**를 GitHub/GitLab 저장소 설정에 등록합니다.
4. **배포**를 누르면 화면에 빌드 과정이 실시간으로 나옵니다.
5. 같은 탭의 **Webhook 주소**를 GitHub/GitLab에 등록하면, 앞으로는 코드를 올릴 때마다 자동으로 배포됩니다.

첫 빌드는 몇 분 걸릴 수 있습니다. 다음부터는 빌드 결과를 재사용해서 빨라집니다.

---

## 데이터베이스 쓰기

1. 사이트(또는 앱) → **데이터베이스** 탭에서 `+ 데이터베이스`를 누르고 `만들기`를 누릅니다.
2. 접속 주소가 `DATABASE_URL` 환경변수로 프로그램에 자동으로 전달됩니다. 코드에서는 이렇게 읽으면 됩니다.
   ```rust
   let url = std::env::var("DATABASE_URL")?; // mysql://사용자:비밀번호@127.0.0.1:3306/DB이름
   ```
3. Redis가 필요하면 같은 탭에서 Redis를 배정합니다. `REDIS_URL`로 전달됩니다.
4. 다른 설정값(API 키 등)은 **환경변수** 탭에 넣으면 프로그램에 전달됩니다. 바꾼 뒤에는 **적용**(또는 **배포**)을 눌러야 반영됩니다.

### 계정 추가·삭제 (왼쪽 메뉴 **MariaDB** · **Redis**)

**MariaDB** 메뉴:

- **+ 데이터베이스**: 앱과 상관없이 데이터베이스를 만들 수 있습니다. 나중에 **앱 연결**로 원하는 앱에 붙일 수 있습니다.
- **+ 계정**: 데이터베이스마다 계정을 더 만듭니다. 권한은 **읽기·쓰기** 또는 **읽기 전용** 중에 고릅니다. 비밀번호는 직접 정하거나 자동으로 만듭니다.
- **비밀번호 변경 / 삭제**: 계정 줄 오른쪽 버튼으로 합니다. (기본 계정은 지울 수 없고, 데이터베이스를 지우면 함께 지워집니다.)

**Redis** 메뉴:

- **+ Redis 계정**: 프로그램마다 Redis 계정을 따로 만듭니다.
  - **쓸 수 있는 키**: 예를 들어 `shop:*` 로 정하면 `shop:`으로 시작하는 키만 읽고 쓸 수 있습니다. 다른 프로그램의 데이터를 건드릴 수 없습니다.
  - **읽기 전용**을 켜면 읽기만 가능합니다. `FLUSHALL`처럼 전체를 지우는 명령은 어느 계정이든 막혀 있습니다.
- 계정을 만들거나 비밀번호를 바꾸면 **접속 주소**가 한 번 크게 표시됩니다. 목록에서 **보기**를 눌러 다시 볼 수 있습니다.

---

## 백업

- 사이트 → **백업** 탭에서 `지금 백업`을 누르면 바로 백업합니다. 데이터베이스와 사이트 파일이 함께 저장됩니다.
- **백업 예약**은 원하는 만큼 추가할 수 있습니다 (왼쪽 메뉴 **백업** → **+ 예약 추가**).
  - **대상**: 앱 하나 / 데이터베이스 하나 / 전체
  - **주기**: 매시간 / 매일 / 매주 (요일·시각 선택)
  - **보관 개수**: 최근 몇 개를 남길지. 오래된 것은 자동으로 지웁니다.
  - 예약마다 **지금 실행 · 끄기/켜기 · 수정 · 삭제**를 할 수 있습니다. 삭제할 때 그 예약이 만든 백업 파일도 지울지 고릅니다.
- 백업 목록에서 **다운로드**하거나, **복원**을 눌러 그 시점으로 되돌릴 수 있습니다.

> 백업 파일은 같은 서버에 저장됩니다. 서버 자체가 고장 나는 경우에 대비해서, 중요한 서비스는 백업 파일을 가끔 내 컴퓨터로 내려받아 두세요.

---

## 자주 묻는 질문

**집 공유기 뒤에 있거나, 앞에 다른 프록시 서버(Nginx Proxy Manager 등)가 있어요.**
설치 명령 끝에 옵션을 붙여서 "프록시 모드"로 설치하세요. `10.0.0.10` 자리에는 앞단 프록시 서버의 IP를 넣습니다.

```bash
curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/install.sh | sudo bash -s -- --mode=proxy --trusted-proxy=10.0.0.10
```

- 이 모드에서는 https 인증서를 **앞단 프록시에서** 발급합니다.
- 앞단 프록시에는 도메인마다 `이 서버 IP:80`으로 연결해 주세요.

**Nginx가 두 군데(앞단 프록시 서버와 이 서버)에 다 있어도 되나요?**
네, **서로 다른 서버라면 괜찮습니다.**
- 앞쪽은 https를 처리해서 넘겨주고, 이 서버는 사이트를 보여 주는 역할을 나눠 맡습니다.
- 문제가 되는 건 **한 서버에 이런 패널을 두 개** 설치할 때입니다. 둘이 같은 자리(80·443 포트)를 차지하려고 싸웁니다.

**프로그램이 시작되지 않아요.**
- **서비스 로그** 탭에서 오류 메시지를 확인하세요.
- 프로그램이 `PORT` 환경변수의 포트로 열고 있는지 확인하세요.
- `exec format error`가 보이면 리눅스용으로 빌드되지 않은 파일입니다(4단계 참고).
- `GLIBC_2.xx not found`가 보이면 내 컴퓨터의 리눅스가 서버보다 최신이라 생긴 문제입니다. `cross`로 빌드하거나 Git 자동 배포를 쓰세요.

**PHP 사이트(WordPress 등)도 같이 돌리고 싶어요.**
이 패널은 PHP를 지원하지 않습니다. PHP를 지원하는 다른 호스팅 패널을 **다른 서버**에 설치해서 쓰세요. 필요하면 이 패널의 "연결(리버스 프록시)" 사이트로 그 서버를 연결할 수 있습니다.

**관리자 비밀번호를 잊어버렸어요.**
1. 서버에 SSH로 접속해서 아래를 실행합니다. 패널 관리자 계정만 지워지고, 사이트·데이터는 그대로입니다.
   ```bash
   sudo mariadb rpanel -e "DELETE FROM users"
   ```
2. 다시 `https://서버IP:8443`에 접속하면 관리자 계정을 새로 만드는 화면이 나옵니다.

**사이트 파일 접속 비밀번호를 잊어버렸어요.**
사이트 → **SSH · SFTP** 탭에서 새 비밀번호로 바꿀 수 있습니다.

**패널 주소를 `https://panel.내도메인.com`처럼 쓰고 싶어요.**
1. 도메인 DNS에 `panel` A 레코드를 서버 IP로 추가합니다.
2. 서버에서 `sudo rpanel-update --force --panel-domain=panel.내도메인.com --email=내이메일`을 실행합니다. 기존 사이트와 데이터는 그대로 유지됩니다.

---

## 업데이트와 삭제

### 업데이트

서버에 접속해서 아래 한 줄을 실행합니다.

```bash
sudo rpanel-update
```

- 새 버전이 있을 때만 업데이트합니다. 새 버전이 있는지 확인만 하려면 `sudo rpanel-update --check`.
- 사이트, 데이터베이스, 설정, 비밀번호는 **그대로 유지**됩니다. 처음 설치할 때 붙인 옵션(프록시 모드 등)도 그대로 적용됩니다.
- 업데이트 전에 패널 데이터를 자동으로 백업하고, 새 버전이 제대로 시작되지 않으면 **이전 버전으로 자동으로 되돌립니다.**
- 업데이트하는 동안 몇 분간 패널 화면이 안 열릴 수 있지만, 사이트는 계속 접속됩니다.

### 삭제

```bash
sudo rpanel-uninstall
```

실행하면 두 가지 중 하나를 고르라고 물어봅니다.

| 선택 | 지우는 것 | 남기는 것 |
|---|---|---|
| **1) 패널만 삭제** | 패널 프로그램, 사이트 연결 설정 (사이트 접속이 멈춤) | 사이트 파일, 데이터베이스, 백업, 설정 — 다시 설치하면 이어서 쓸 수 있습니다 |
| **2) 전부 삭제** | 위의 것 + 사이트 파일과 계정, 패널이 만든 데이터베이스, 백업, 인증서 | 없음 (되돌릴 수 없습니다) |

- 어느 쪽이든 Nginx·MariaDB·Redis 같은 기본 프로그램은 지우지 않습니다. 다른 곳에서 쓰고 있을 수 있기 때문입니다.
- `rpanel-uninstall` 명령이 없어졌다면(설치가 중간에 실패한 경우 등) 이렇게 실행하세요:
  ```bash
  curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/uninstall.sh | sudo bash
  ```

---

## 문제가 생겼을 때

| 증상 | 확인할 것 |
|---|---|
| 패널(`:8443`)에 접속이 안 돼요 | 클라우드 업체 방화벽에서 8443 포트를 열었는지 확인하세요. 주소가 `https://`로 시작하는지도 확인하세요. |
| 사이트가 안 열려요 | 도메인 DNS가 서버 IP를 가리키는지 확인하세요(반영에 시간이 걸릴 수 있음). 클라우드 방화벽에서 80·443 포트도 열어야 합니다. |
| "502 Bad Gateway"가 나와요 | 프로그램이 꺼져 있다는 뜻입니다. `app` 파일을 올리고 **적용**을 눌렀는지, **서비스 로그**에 오류가 없는지 확인하세요. |
| https 발급이 실패해요 | 위의 DNS와 80 포트가 대부분의 원인입니다. 프록시 모드라면 앞단 프록시에서 발급해야 합니다. |
| HTML 사이트에 "403"이나 "404"가 나와요 | 파일 탭에서 `index.html`이 사이트 폴더 **바로 안**에 있는지 확인하세요. zip을 풀면 폴더가 한 겹 더 생기는 경우가 많습니다. |
| 그 밖의 오류 | 사이트 → **Nginx 로그** 탭의 오류 기록을 확인하세요. 해결이 안 되면 [이슈](https://github.com/sinji0117/Rpanel/issues)에 남겨 주세요. |

---

## 더 알아보기

- 설치 옵션, 구조, 보안 설계, 개발·테스트 방법, API는 **[기술 문서](docs/ADVANCED.md)**를 보세요.
- 라이선스: MIT ([LICENSE](LICENSE)). 개인·회사 모두 무료로 쓰고 고치고 배포할 수 있습니다.

---

## 후원하기

Rpanel이 도움이 되셨다면 커피 한 잔으로 응원해 주세요. 후원은 기능 개선과 유지보수에 큰 힘이 됩니다. ☕

<p align="center">
  <a href="https://buymeacoffee.com/bandiuri" target="_blank" rel="noopener noreferrer"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" alt="Buy Me A Coffee — 후원하기" width="320"></a>
</p>

<p align="center"><a href="https://buymeacoffee.com/bandiuri" target="_blank" rel="noopener noreferrer">buymeacoffee.com/bandiuri</a></p>

- 후원이 어렵다면 GitHub에서 ⭐ **Star**를 눌러 주는 것도 큰 응원이 됩니다.
- 버그나 아이디어는 [이슈](https://github.com/sinji0117/Rpanel/issues)로 남겨 주세요.
