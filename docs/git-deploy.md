[← 사용 설명서 목차](README.md) · [Rpanel 처음 화면](../README.md)

# Git으로 자동 배포하기

코드를 GitHub·GitLab에 올려 두었다면, 서버가 직접 빌드하고 실행하게 할 수 있습니다.

1. 왼쪽 메뉴 **프로젝트** → **+ 새 프로젝트**를 만들고, 그 안에서 **+ 새 앱**을 누릅니다.
2. 종류를 고르고 Git 저장소 주소를 넣습니다.
   - **HTML (React·Vue 등)**: 서버가 `npm install` → `npm run build` 로 빌드하고 결과물 폴더(기본 `dist`)를 사이트로 보여 줍니다.
   - **Rust**: **바이너리 이름**(`Cargo.toml`의 `name`)을 넣습니다.
   - **Python**: **시작 명령**을 넣습니다 ([Python 시작 명령 예](python.md) 참고). 배포할 때마다 새 가상환경에 `requirements.txt`를 설치합니다.
3. 비공개 저장소라면 앱의 **Git · 웹훅** 탭에 나오는 **Deploy key**를 GitHub/GitLab 저장소 설정에 등록합니다.
4. **배포**를 누르면 화면에 빌드 과정이 실시간으로 나옵니다.
5. 같은 탭의 **Webhook 주소**를 GitHub/GitLab에 등록하면, 앞으로는 코드를 올릴 때마다 자동으로 배포됩니다.

첫 빌드는 몇 분 걸릴 수 있습니다. 다음부터는 빌드 결과를 재사용해서 빨라집니다.

빌드에 쓰는 Rust 툴체인 · Python · Node.js 버전은 [언어 버전 (런타임)](runtimes.md)에서 고릅니다.

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
