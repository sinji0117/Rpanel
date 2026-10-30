[← 사용 설명서 목차](README.md) · [Rpanel 처음 화면](../README.md)

# Node.js · Java 프로그램 올리기

## Node.js (Express · Next.js 등)

1. **사이트** → **+ 사이트 추가** → 종류 **Node.js**, 도메인을 넣고 만듭니다. 예시 프로그램(`server.js`)이 바로 실행됩니다.
2. 내 프로그램 파일과 `package.json`(있으면 `package-lock.json` 도)을 **파일** 탭이나 SFTP로 올립니다. `node_modules` 는 올리지 않아도 됩니다.
3. **설정** 탭에서 시작 명령을 맞춥니다. 프로그램은 `PORT` 환경변수의 포트로 `127.0.0.1`에서 열어야 합니다.

   | 프레임워크 | 시작 명령 예 | 적용 전 실행할 명령 |
   |---|---|---|
   | 그냥 Node.js · Express | `node server.js` | |
   | package.json 의 start | `npm start` | |
   | Next.js | `npx next start -H 127.0.0.1 -p $PORT` | `npm run build` |
   | Nuxt | `node .output/server/index.mjs` | `npm run build` |

4. **적용**을 누르면 `npm ci`(lock 파일이 없으면 `npm install`)로 패키지를 설치하고, 적용 전 명령을 실행한 뒤 프로그램을 시작합니다.

## Java (Spring Boot 등)

1. **사이트** → **+ 사이트 추가** → 종류 **Java**. 처음에는 예시 `App.java` 가 바로 실행됩니다.
2. 내 컴퓨터에서 실행용 jar 를 만듭니다. (`./mvnw package` → `target/<이름>.jar`, `./gradlew bootJar` → `build/libs/<이름>.jar`)
3. jar 를 **파일** 탭이나 SFTP로 `app.jar` 라는 이름으로 올립니다.
4. **설정** 탭의 시작 명령을 `java $JAVA_OPTS -jar app.jar` 로 바꾸고 **적용**합니다.

프로그램은 `PORT` 환경변수의 포트로 `127.0.0.1`에서 열어야 합니다. Spring Boot 는 `application.properties` 에 이렇게 적으면 됩니다.

```properties
server.address=127.0.0.1
server.port=${PORT}
```

메모리 등 JVM 옵션은 **환경변수** 탭에 `JAVA_OPTS`(예: `-Xmx512m`)로 넣습니다.

## 버전 바꾸기

기본은 서버에 깔린 `node` / `java` 입니다. 서버에 Java 가 없다면 먼저 설치하세요.

1. 왼쪽 메뉴 **런타임** → Node.js 또는 Java 에서 버전(Node.js `20`·`22`·`24`, Java `17`·`21`·`25` 등)을 고르고 **설치하고 기본으로 변경**을 누릅니다. Java 는 [Eclipse Temurin](https://adoptium.net) JDK 를 설치합니다.
2. 이 사이트만 다른 버전을 쓰려면 사이트 → **설정** 탭 → **버전**에서 고릅니다.
3. **적용**을 누르면 그 버전으로 실행합니다. 고른 버전의 `node`·`npm`·`java` 가 명령의 맨 앞 `PATH` 에 들어가므로 시작 명령은 그대로 두면 됩니다.

자세한 내용은 [언어 버전 (런타임)](runtimes.md)을 보세요.

## Git 저장소에 있는 프로그램

[Git으로 자동 배포하기](git-deploy.md)에서 백엔드 → **Node.js** 또는 **Java** 를 고르면 서버가 빌드합니다.

- **Node.js**: `npm ci` → `npm run build --if-present` → 시작 명령(기본 `npm start`).
- **Java**: 저장소의 `mvnw` · `pom.xml` · `gradlew` · `build.gradle` 을 보고 Maven 또는 Gradle 로 jar 를 만듭니다 (테스트는 건너뜀). 서버에 `mvn`·`gradle` 이 없어도 저장소에 래퍼(`mvnw`·`gradlew`)가 있으면 됩니다. 래퍼가 없으면 **런타임** 메뉴에서 **Maven** 또는 **Gradle** 을 설치하세요. 만든 jar 를 `app.jar` 로 복사해 `java $JAVA_OPTS -jar app.jar` 로 실행합니다. jar 가 여러 개 생기면 **jar 파일**에 경로(예: `build/libs/api.jar`)를 넣으세요.

---

[← 사용 설명서 목차](README.md)
