[← 사용 설명서 목차](README.md) · [Rpanel 처음 화면](../README.md)

# Git으로 자동 배포하기

코드를 GitHub·GitLab에 올려 두었다면, 서버가 직접 빌드하고 실행하게 할 수 있습니다.

1. 왼쪽 메뉴 **프로젝트** → **+ 새 프로젝트**를 만들고, 그 안에서 **+ 새 앱**을 누릅니다.
2. **무엇을 만드나요?**에서 역할을 먼저 고르고 Git 저장소 주소를 넣습니다.
   - **백엔드 (API 서버)** → 언어를 고릅니다.
     - **Rust**: **바이너리 이름**(`Cargo.toml`의 `name`)을 넣습니다.
     - **Python**: **시작 명령**을 넣습니다 ([Python 시작 명령 예](python.md) 참고). 배포할 때마다 새 가상환경에 `requirements.txt`를 설치합니다.
     - **Node.js**: `npm ci`(lock 파일이 없으면 `npm install`) → 빌드 명령(기본 `npm run build --if-present`) → **시작 명령**(기본 `npm start`). [Node.js · Java](node-java.md) 참고.
     - **Java**: Maven·Gradle 로 jar 를 만들고(래퍼 `mvnw`·`gradlew` 우선, 테스트는 건너뜀) `app.jar` 로 복사해 **시작 명령**(기본 `java $JAVA_OPTS -jar app.jar`)으로 실행합니다. jar 가 여러 개면 **jar 파일**에 경로를 넣으세요.
   - **프런트엔드 (웹 화면)**: React·Vue 등. 서버가 `npm ci` → `npm run build` 로 빌드하고 결과물 폴더(기본 `dist`)를 사이트로 보여 줍니다. **API 연결**에서 같은 프로젝트의 백엔드를 고르면 `/api/` 요청이 그 백엔드로 갑니다.
     - **화면 용도**: **사용자 화면**(예: `example.com`) 또는 **관리자 화면**(예: `admin.example.com`). 빌드·서비스 방식은 같고, 프로젝트 화면에서 어느 묶음에 보일지만 정합니다. 나중에 앱 **설정** 탭에서 바꿀 수 있습니다.
3. 비공개 저장소라면 앱의 **Git · 웹훅** 탭에 나오는 **Deploy key**를 GitHub/GitLab 저장소 설정에 등록합니다.
4. **배포**를 누르면 화면에 빌드 과정이 실시간으로 나옵니다.
5. 같은 탭의 **Webhook 주소**를 GitHub/GitLab에 등록하면, 앞으로는 코드를 올릴 때마다 자동으로 배포됩니다.

첫 빌드는 몇 분 걸릴 수 있습니다. 다음부터는 빌드 결과를 재사용해서 빨라집니다.

빌드에 쓰는 Rust 툴체인 · Python · Node.js · Java 버전은 [언어 버전 (런타임)](runtimes.md)에서 고릅니다.

## 백엔드 + 프런트엔드가 한 저장소에 있을 때 (npm 빌드)

Rust · Python · Node.js · Java 프로그램과 React·Vue 화면이 같은 저장소에 있다면, 앱 설정의 **프런트엔드 폴더**에 화면 폴더(예: `frontend`)를 넣으세요.

```
저장소/
├─ Cargo.toml        ← Rust 프로그램
├─ src/
└─ frontend/         ← 프런트엔드 폴더
   ├─ package.json
   └─ dist/          ← npm run build 결과 (빌드 결과 폴더, 기본 dist)
```

배포할 때 이렇게 합니다.

1. `frontend` 폴더에서 `npm ci`(package-lock.json 이 없으면 `npm install`) → `npm run build`
2. Rust 는 `cargo build --release`, Python 은 가상환경·패키지 설치, Node.js 는 `npm ci` → 빌드, Java 는 Maven·Gradle 빌드
3. 빌드 결과(`frontend/dist`)를 프로그램 옆 같은 경로로 복사합니다. 프로그램은 작업 폴더 기준 `frontend/dist` 경로로 읽으면 됩니다.
   ```rust
   // axum 예: 화면 파일 + API 를 한 프로그램에서
   let app = Router::new()
       .nest("/api", api_routes())
       .fallback_service(tower_http::services::ServeDir::new("frontend/dist")
           .fallback(tower_http::services::ServeFile::new("frontend/dist/index.html")));
   ```

Node.js 버전은 [런타임](runtimes.md) 메뉴의 Node.js 기본 버전을 씁니다.

> 화면을 따로 서비스하고 싶다면 같은 프로젝트에 종류 **Static** 앱을 하나 더 만들고, **/api/ 프록시 포트**에 Rust 앱의 포트를 넣는 방법도 있습니다.

## 비공개(Private) 저장소

`https://github.com/계정/저장소` 주소는 **공개 저장소**에서만 됩니다. 비공개 저장소를 https 주소로 넣으면 배포할 때 이런 오류가 납니다.

```
fatal: could not read Username for 'https://github.com': terminal prompts disabled
```

이렇게 고칩니다.

1. 앱 → **Git · 웹훅** 탭 위쪽의 **`git@github.com:계정/저장소.git` 로 바꾸기**를 누릅니다. (또는 **설정** 탭에서 Git 주소를 SSH 주소로 직접 바꿉니다.)
2. 같은 탭에서 **Deploy key 만들기**를 누르고 공개키를 복사합니다.
3. GitHub 저장소 → **Settings → Deploy keys → Add deploy key** 에 붙여 넣습니다. (Allow write access 는 체크하지 않습니다.)
   - GitLab: 프로젝트 → **Settings → Repository → Deploy keys**
4. 다시 **배포**를 누릅니다.

## 프런트 · 백엔드 · 관리자 화면을 한 프로젝트로

**프로젝트** 화면은 **프런트엔드**(사용자 화면 · 관리자 화면)와 **백엔드 (API)** 두 묶음으로 나뉩니다.

| 할 일 | 방법 |
|---|---|
| 백엔드 만들기 | **백엔드 (API)** 묶음의 `+ 백엔드` → Rust · Python · Node.js · Java, 도메인은 비워 둬도 됩니다 (포트가 자동으로 정해집니다) |
| 프런트 만들기 | **프런트엔드** 묶음의 `+ 사용자 화면` → Static(React·Vite), 도메인 `example.com` |
| 프런트 ↔ 백엔드 연결 | 프런트 카드의 **/api/ 연결**에서 백엔드를 고릅니다 → `example.com/api/…` 요청이 백엔드로 갑니다 |
| 관리자 화면 | **프런트엔드** 묶음의 `+ 관리자 화면` → Static, 도메인 `admin.example.com`, **/api/ 연결**도 같은 백엔드로 |
| 한 번에 배포 | 위쪽 **전체 배포** (백엔드부터 배포합니다) |

카드 위쪽 표시(사용자 화면 · 관리자 화면 · 백엔드)로 구분됩니다. 도메인마다 https 인증서는 각 앱의 **SSL** 탭에서 받습니다.

### 검색 노출 (SEO): robots.txt · sitemap.xml

사용자 화면에 **/api/ 연결**이 있으면, 검색 로봇이 읽는 파일을 백엔드에서 받아 옵니다. 관리자 화면에서 SEO 설정을 바꾸면 프런트를 다시 빌드하지 않아도 바로 반영됩니다.

| 주소 | 실제로 응답하는 곳 |
|---|---|
| `example.com/robots.txt` | 백엔드 `/api/seo/robots.txt` |
| `example.com/sitemap.xml` (`sitemap-posts.xml` 처럼 `sitemap…xml` 모두) | 백엔드 `/api/seo/sitemap.xml` (같은 파일 이름) |

- 백엔드에 그 경로가 없거나(404) 백엔드가 꺼져 있으면, 빌드 결과(`dist`)에 있는 같은 이름의 파일을 그대로 줍니다. 그 파일도 없으면 404 입니다 (예전처럼 `index.html` 이 나가지 않습니다).
- 관리자 화면(앱 용도 **관리자**)에는 적용하지 않습니다.
- 백엔드는 `/api/seo/robots.txt`, `/api/seo/sitemap.xml` 만 만들면 됩니다. 사이트 주소는 요청의 `Host` · `X-Forwarded-Proto` 헤더로 알 수 있습니다.
- `<title>` · `description` · Open Graph 같은 `<head>` 태그는 프런트 빌드 때 `index.html` 에 들어가므로, 이 값을 바꾼 뒤에는 프런트 앱을 **다시 배포**해야 합니다.
- 패널을 업데이트한 뒤, 이미 있던 사용자 화면 앱은 한 번 **배포**해야 새 Nginx 설정이 적용됩니다.

> 프로젝트(Git)로 만든 앱은 **사이트 계정(SFTP)이 없습니다.** 코드가 Git 에서 오기 때문입니다. **사이트** 메뉴의 **계정** 칸에 `Git 배포`라고 나오고, 누르면 프로젝트 화면으로 갑니다. 파일을 직접 올려야 하면 **사이트** 메뉴에서 사이트로 만드세요.

## push 할 때 자동 배포 (웹훅)

**GitHub**: 저장소 → **Settings → Webhooks → Add webhook**

| 항목 | 넣을 값 |
|---|---|
| Payload URL | Git · 웹훅 탭의 **URL** |
| Content type | `application/json` |
| Secret | Git · 웹훅 탭의 **Secret** |
| 이벤트 | Just the push event |

**GitLab**: Settings → **Webhooks** → URL 과 Secret token 을 넣고 **Push events** 를 체크합니다.

앱에 설정한 브랜치(예: `main`)에 push 할 때만 배포합니다. GitHub·GitLab 이 이 패널 주소(`https://서버:8443`)에 접속할 수 있어야 합니다.

---

[← 사용 설명서 목차](README.md)
