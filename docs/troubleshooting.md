[← 사용 설명서 목차](README.md) · [Rpanel 처음 화면](../README.md)

# 문제가 생겼을 때

| 증상 | 확인할 것 |
|---|---|
| 패널(`:8443`)에 접속이 안 돼요 | 클라우드 업체 방화벽에서 8443 포트를 열었는지 확인하세요. 주소가 `https://`로 시작하는지도 확인하세요. |
| 사이트가 안 열려요 | 도메인 DNS가 서버 IP를 가리키는지 확인하세요(반영에 시간이 걸릴 수 있음). 클라우드 방화벽에서 80·443 포트도 열어야 합니다. |
| "502 Bad Gateway"가 나와요 | 프로그램이 꺼져 있다는 뜻입니다. `app` 파일을 올리고 **적용**을 눌렀는지([첫 사이트 만들기](first-site.md)), **서비스 로그**에 오류가 없는지 확인하세요. |
| https 발급이 실패해요 | 위의 DNS와 80 포트가 대부분의 원인입니다. 프록시 모드라면 앞단 프록시에서 발급해야 합니다. |
| HTML 사이트에 "403"이나 "404"가 나와요 | 파일 탭에서 `index.html`이 사이트 폴더 **바로 안**에 있는지 확인하세요. zip을 풀면 폴더가 한 겹 더 생기는 경우가 많습니다. |
| MongoDB 가 시작되지 않아요 ("Linux kernel versions 6.19 and newer") | 커널 6.19 이상에서 MongoDB 8 이 시작을 거부하는 알려진 문제입니다. `sudo rpanel-update --force` 로 업데이트하면 우회 설정(`/etc/systemd/system/mongod.service.d/rpanel-kernel.conf`)이 들어갑니다. |
| 설치·업데이트가 실패해요 | 실패한 단계 아래에 오류 끝부분이 나옵니다. 전체 기록은 `sudo less /var/log/rpanel-install.log` 로 봅니다. |
| 그 밖의 오류 | 사이트 → **Nginx 로그** 탭의 오류 기록을 확인하세요. 해결이 안 되면 [이슈](https://github.com/sinji0117/Rpanel/issues)에 남겨 주세요. |

---

[← 사용 설명서 목차](README.md)
