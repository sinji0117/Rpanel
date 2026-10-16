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
  - **프로젝트 화면**: 프런트 · 백엔드(API) · 관리자 화면을 한 화면에서 보고, 프런트의 `/api/`를 백엔드에 연결하고, 한 번에 배포합니다. 관리자 화면은 `admin.example.com`처럼 다른 도메인으로 붙입니다.
- **파일 올리기**: 브라우저에서 끌어다 놓거나, FileZilla 같은 프로그램으로 올립니다.
- **사이트 계정 (SSH · SFTP)**: 사이트마다 접속 계정을 여러 개 만들고, 수정·잠금·비밀번호 변경·삭제를 할 수 있습니다. 외주 개발자·디자이너에게 계정을 따로 줄 때 좋습니다.
- **https 자물쇠**: 무료 인증서(Let's Encrypt)를 클릭 한 번으로 받고, 자동으로 갱신합니다.
- **데이터베이스**: MariaDB · MongoDB · Redis를 클릭 한 번으로 만들고, 접속 주소를 프로그램에 자동으로 알려 줍니다.
  - **DB 버전**: MariaDB(10.11 · 11.4 · 11.8) · MongoDB · Redis 를 더 높은 버전으로 바꿉니다 (바꾸기 전 자동 전체 백업).
  - **DB 웹 관리**: 브라우저에서 테이블 데이터를 보고 추가·수정·삭제하거나 SQL을 실행합니다. 개발자는 패널 계정 없이 `/db` 주소에서 DB 계정으로만 로그인할 수 있습니다.
  - **DB 계정 관리**: 데이터베이스마다 계정을 더 만들거나 지울 수 있습니다 (예: 통계용 읽기 전용 계정).
  - **Redis 계정 관리**: 프로그램마다 따로 쓸 Redis 계정을 만들고, 쓸 수 있는 키와 읽기 전용 여부를 정합니다.
- **백업과 복원**: 백업 예약을 원하는 만큼 추가·수정·삭제합니다 (매시간·매일·매주, 앱·DB·전체). 문제가 생기면 그때로 되돌립니다.
- **예약 작업(크론)**: 정해진 시간마다 명령을 실행합니다.
- **모니터링**: CPU·메모리·디스크·Load average 그래프(최근 30분 ~ 7일), 가동 시간, 서버 정보를 대시보드에서 봅니다.
- **로그**: 활동 기록, 배포, 서비스, 사이트 접속, 크론, 서버(패널·Nginx·SSH·DB) 로그를 **로그** 메뉴에서 종류별로 봅니다. MongoDB가 설치되어 있으면 패널 로그(활동·배포·알림·모니터링)는 MongoDB에 저장합니다.
- **패널 도메인 · 접속 IP**: 패널을 `https://panel.example.com`처럼 도메인으로 열고 (인증서 자동 발급), 정해 둔 IP 에서만 열리게 할 수 있습니다.
- **알림**: 배포·백업 실패, 사이트 응답 없음, 디스크·메모리 부족, 새 버전 등을 종 아이콘으로 알려 주고, Discord·Slack·Telegram·웹훅으로도 보냅니다. 알림 목록은 배포·백업·사이트·서버·시스템으로 나눠 봅니다.
- **관리자 여러 명**: 패널에 로그인할 관리자 계정을 추가·수정·삭제합니다.
- **언어 버전 고르기**: Rust 툴체인 · Python · Node.js 버전을 여러 개 설치해 두고, 기본 버전과 앱별 버전을 고릅니다.
- **버전 확인**: 지금 패널 버전과 GitHub 의 최신 버전을 대시보드에서 보여 주고, 새 버전이 나오면 알려 줍니다.
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
- **10~20분** 정도 걸립니다. 웹 서버(Nginx), 데이터베이스(MariaDB·MongoDB·Redis), Rust 등 필요한 프로그램을 모두 자동으로 설치합니다.
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

## 사용 설명서

설치가 끝났다면 **[사용 설명서](docs/README.md)**를 보세요.

| 처음이라면 | 관리 | 도움말 |
|---|---|---|
| [첫 사이트 만들기 (Rust)](docs/first-site.md) | [사이트 계정 (SSH · SFTP)](docs/site-accounts.md) | [자주 묻는 질문](docs/faq.md) |
| [Python 프로그램 올리기](docs/python.md) | [데이터베이스 쓰기](docs/database.md) | [문제가 생겼을 때](docs/troubleshooting.md) |
| [Git으로 자동 배포하기](docs/git-deploy.md) | [백업과 복원](docs/backup.md) | [기술 문서](docs/ADVANCED.md) |
| | [언어 버전 (런타임)](docs/runtimes.md) | |
| | [모니터링과 로그](docs/monitoring-logs.md) | |
| | [알림](docs/notifications.md) | |
| | [업데이트와 삭제](docs/update-uninstall.md) | |

---

## 라이선스

MIT ([LICENSE](LICENSE)). 개인·회사 모두 무료로 쓰고 고치고 배포할 수 있습니다.

---

## 후원하기

Rpanel이 도움이 되셨다면 커피 한 잔으로 응원해 주세요. 후원은 기능 개선과 유지보수에 큰 힘이 됩니다. ☕

<p align="center">
  <a href="https://buymeacoffee.com/bandiuri" target="_blank" rel="noopener noreferrer"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" alt="Buy Me A Coffee — 후원하기" width="320"></a>
</p>

<p align="center"><a href="https://buymeacoffee.com/bandiuri" target="_blank" rel="noopener noreferrer">buymeacoffee.com/bandiuri</a></p>

- 후원이 어렵다면 GitHub에서 ⭐ **Star**를 눌러 주는 것도 큰 응원이 됩니다.
- 버그나 아이디어는 [이슈](https://github.com/sinji0117/Rpanel/issues)로 남겨 주세요.
