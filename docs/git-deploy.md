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

---

[← 사용 설명서 목차](README.md)
