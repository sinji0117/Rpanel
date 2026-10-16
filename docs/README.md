[← Rpanel 처음 화면](../README.md)

# Rpanel 사용 설명서

설치는 [처음 화면의 설치하기](../README.md#설치하기-3단계)를 보세요. 설치한 다음부터는 아래 문서를 순서대로 보면 됩니다.

## 시작하기

| 문서 | 내용 |
|---|---|
| [첫 사이트 만들기 (Rust)](first-site.md) | 도메인 연결 → 사이트 추가 → 실행 파일 올리기 → https |
| [Python 프로그램 올리기](python.md) | Flask · FastAPI · Django, `requirements.txt`, Python 버전 |
| [Git으로 자동 배포하기](git-deploy.md) | GitHub · GitLab 에서 서버가 직접 빌드 · 웹훅 자동 배포 · 프런트·백엔드·관리자 한 화면 |

## 관리하기

| 문서 | 내용 |
|---|---|
| [사이트 계정 (SSH · SFTP)](site-accounts.md) | 접속 계정 추가 · 수정 · 잠금 · 비밀번호 · 삭제 |
| [데이터베이스 쓰기](database.md) | MariaDB · MongoDB · Redis 만들기, 계정 관리, DB 웹 관리(`/db`) |
| [백업과 복원](backup.md) | 지금 백업, 백업 예약, 복원 |
| [언어 버전 (런타임)](runtimes.md) | Rust · Python · Node.js 버전 설치 · 기본 버전 · 앱별 버전 |
| [모니터링과 로그](monitoring-logs.md) | 대시보드 그래프, 가동 시간, 로그 메뉴 |
| [알림](notifications.md) | 종 아이콘, 알림 받을 일, Discord · Slack · Telegram · 웹훅 |
| [업데이트와 삭제](update-uninstall.md) | 패널 버전 확인, 업데이트, 삭제 |

## 도움말

| 문서 | 내용 |
|---|---|
| [자주 묻는 질문](faq.md) | 프록시 모드, PHP, 비밀번호 분실, 패널 도메인 … |
| [문제가 생겼을 때](troubleshooting.md) | 접속 안 됨, 502, https 발급 실패 … |
| [기술 문서](ADVANCED.md) | 설치 옵션, 구조, 보안 설계, 개발 · 테스트, API |
