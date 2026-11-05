[← 사용 설명서 목차](README.md) · [Rpanel 처음 화면](../README.md)

# 메일 서버 올리기 (낮은 포트를 쓰는 앱)

메일 서버는 웹 앱과 달리 `25`·`465`·`587`·`993` 같은 **1024 미만 포트**를 직접 열어야 합니다.
Rpanel 의 앱은 보안을 위해 일반 계정으로, 권한을 막은 채 실행되므로 기본으로는 이 포트를 열 수 없습니다.
앱 설정의 **1024 미만 포트 허용 (메일 서버)** 을 켜면 그 앱에만 낮은 포트를 여는 권한 하나(`CAP_NET_BIND_SERVICE`)를 줍니다. root 로 실행하지 않고 다른 격리 설정은 그대로입니다.

## pickmail 설치 예

[pickmail](https://github.com/sinji0117/pickmail) 은 Rust 로 만든 메일 서버(SMTP·IMAP·웹메일)입니다. 한 저장소에 백엔드와 웹메일 화면이 같이 있어서 앱 하나로 올립니다.

1. **런타임** 메뉴에서 Rust(최신 stable)와 Node.js 를 설치해 둡니다. 첫 빌드는 메모리 4GB 이상에서 10~20분 걸립니다.
2. **프로젝트** → 프로젝트 하나 → **+ 백엔드** 를 누르고 아래처럼 넣습니다.

   | 칸 | 값 |
   |---|---|
   | 앱 종류 | Rust |
   | Git 저장소 | `https://github.com/sinji0117/pickmail` (비공개면 [Deploy key](git-deploy.md#비공개private-저장소)) |
   | 루트 디렉토리 | (비움) |
   | 빌드 명령 | `cargo build --release --locked --manifest-path backend/Cargo.toml --target-dir target -p pickmail-server` |
   | 바이너리 이름 | `pickmail` |
   | 프런트엔드 폴더 | `frontend` |
   | 도메인 | `mail.example.com` |
   | 헬스체크 경로 | `/healthz` |
   | 1024 미만 포트 허용 (메일 서버) | 켬 |

3. 앱의 **데이터베이스** 탭에서 MariaDB · MongoDB · Redis 를 하나씩 만듭니다. `DATABASE_URL` · `MONGODB_URL` · `REDIS_URL` 이름 그대로 두면 pickmail 이 알아서 읽습니다.
4. **환경변수** 탭에 아래를 넣습니다.

   | 키 | 값 |
   |---|---|
   | `PICKMAIL_HOSTNAME` | `mail.example.com` |
   | `PICKMAIL_PORTS` | `standard` (25·465·587·993) |
   | `PICKMAIL_ADMIN_EMAIL` | `postmaster@example.com` |
   | `PICKMAIL_ADMIN_PASSWORD` | 관리자 처음 비밀번호 |
   | `PICKMAIL_CLOUDFLARE_TOKEN` | Cloudflare API 토큰 (영역 DNS 편집) — 메일 포트 인증서를 자동으로 받습니다 |

5. **보안** → **방화벽** 에서 `25,465,587,993` TCP 를 엽니다.
6. **배포** 를 누릅니다. 끝나면 `https://mail.example.com` 의 관리 화면 → 도메인 → DNS 레코드 → **Cloudflare 에 등록** 으로 MX·SPF·DKIM·DMARC 를 넣습니다.

자세한 내용은 pickmail 의 [Rpanel 설치 안내](https://github.com/sinji0117/pickmail/blob/main/doc/11_rpanel_integration.md) 를 보세요.

## 알아 둘 것

- 설정을 바꾸면 **다음 배포부터** 적용됩니다.
- 이 옵션은 관리자만 바꿀 수 있고, 바꾸면 감사 로그(`app.low_ports`)에 남습니다.
- 앱은 `DATA_DIR` 환경변수로 쓸 수 있는 데이터 폴더(`/var/lib/rpanel/apps/<slug>/data`)를 받습니다. 메일·인증서처럼 남겨야 할 파일은 여기에 둡니다.
- 같은 서버의 다른 프로그램(Postfix 등)이 이미 25번을 쓰고 있으면 앱이 시작하지 못합니다. `sudo ss -ltnp 'sport = :25'` 로 확인하세요.
