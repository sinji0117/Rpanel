[← 사용 설명서 목차](README.md) · [Rpanel 처음 화면](../README.md)

# Python 프로그램 올리기

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

## Python 버전 바꾸기

기본은 서버에 깔린 Python(Ubuntu 24.04 는 3.12)입니다. 다른 버전(3.10 ~ 3.14 등)이 필요하면:

1. 왼쪽 메뉴 **런타임** → Python 에서 버전을 고르고 **설치하고 기본으로 변경**을 누릅니다. 몇십 초면 설치됩니다.
2. 이 사이트만 다른 버전을 쓰려면 사이트 → **설정** 탭 → **Python 버전**에서 고릅니다.
3. **적용**을 누르면 가상환경을 그 버전으로 새로 만들고 패키지를 다시 설치합니다.

자세한 내용은 [언어 버전 (런타임)](runtimes.md)을 보세요.

## Git 저장소에 있는 Python 프로그램

[Git으로 자동 배포하기](git-deploy.md)에서 종류 **Python**을 고르면 됩니다.

---

[← 사용 설명서 목차](README.md)
