[English](README.md) | **한국어** | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

# telegram

Telegram 채팅 프로바이더 `bmo-telegram`입니다. 기반 라이브러리 `bmo-chat`의 채팅 프로토콜을 통해 채팅 리시버에게
Telegram 역할을 합니다. 지원하는 읽기 모드, 할 수 있는 일, 방에서 읽는 방언(dialect)을 선언하고, 봇 계정으로 방을
읽고, 열두 가지 동사를 보내고, 방에서 오간 말을 리시버에게 넘깁니다. 봇이 볼 수 없거나 가져올 수 없는 것과 소유자
자신의 말은 읽기 전용인 소유자 본인 계정이 뒤에서 맡으며, 이는 별도 프로세스인 리더(reader)가 유지합니다.

## 구조

| 폴더 | 내용 |
|---|---|
| `src/` | 서비스 자체 패키지: 프로바이더, Bot API 연결, 라우트 |
| `dialect/` | `telegram-dialect`, 곧 `telegram-html` 방언: 마크다운을 Telegram HTML로 변환, 구조 검사, 접기, 메시지 분할 |
| `picture/` | `telegram-picture`, 그림을 읽고 다시 그리는 부분: 형식, 투명도, Telegram이 사진으로 받는 업로드, 크기 제한 안에 맞춘 그림 |
| `reader/` | 리더. MTProto로 소유자 계정을 유지하는 bun 패키지이며, 테스트는 `reader/tests/`에 있음 |
| `tools/` | 배포가 설치하는 호스티드 도구: 서비스를 통해 한 채팅의 최근 메시지를 소유자의 지시로만 가져옴 |
| `policy/` | 프로바이더의 고정 수치와 영어 문구: 한도, 거부, 자리표시자 |
| `tests/` | 저장소 전체에 걸친 검사와 서비스의 테스트 |
| `deploy/` | 배포가 연결하는 두 서비스 유닛: Rust 프로바이더와 bun 리더 |
| `docs/` | 프로바이더가 하는 일에 대한 정식 문서 |

루트에는 서비스 패키지를 겸하는 Cargo 워크스페이스 매니페스트, `testbed.toml`(이 저장소를
어떻게 테스트하고 검사하고 빌드하고 배포하는지에 대한 테스트베드 선언),
`install.toml`(새 머신에 설치할 때 필요한 것으로, `bmo install`이 읽습니다), `rustfmt.toml`, `CLAUDE.md`가 있습니다.

## 문서

| 문서 | 내용 |
|---|---|
| `docs/api.md` | 프로바이더의 고정된 공개 API: 채팅 프로토콜의 라우트, 전송 형식, 페이지. 같은 계약을 기계가 읽는 형태로 담은 `docs/openapi.json` |
| `docs/boundary-api.md` | 경계: 제공하는 라우트, 리시버에 대한 호출, 서비스와 리더 사이의 접점, 설정 |
| `docs/telegram.md` | 프로바이더: 선언, 방을 읽는 방식, 각 전송의 동작, 소유자의 목소리, `telegram-html` 방언 |
| `docs/reader.md` | 리더: 소유자 계정, 연결, 기록 탐색, 프로필 사진, 그리고 TypeScript인 이유 |
| `docs/tools.md` | 호스티드 도구: 진입점, 인자, 소유자의 지시, 프로바이더에 닿는 위치 |
| `docs/checks.md` | 저장소 검사: 의존성, 매니페스트, 구조, 버전, 한글, 개인 정보, 이름 |
| `docs/PERSONAL-DATA.md` | 저장소의 개인 정보 금지 규칙: 추적되는 파일에 들어갈 수 없는 것, 그 대신 쓰는 것, 이를 지키는 검사 |

## 버전

- **버전은 세트의 버전이며,** 빌드할 때 git 태그에서 읽습니다. 테스트베드가 이를 `BMO_VERSION`으로 빌드에 넘기고,
  헬스가 이를 답합니다. 워크스페이스 매니페스트와 리더의 `package.json`에 있는 버전 필드는 움직이지 않는 자리표시자입니다.
- **버전은 소유자가 선언한 릴리스에서만 바뀝니다.** bmo-chat의 `docs/conventions.md` 「Versions」에 규칙이
  있습니다. 커밋, 배포, 수정, 계약 변경으로는 바뀌지 않으며, 배포는 커밋으로 구분합니다.

## 빌드와 테스트

Cargo는 테스트베드의 Cargo 동사를 통해 실행되며, 하나의 공용 빌드 디렉터리에서 빌드합니다. 설치된 테스트베드에
자체 Cargo 동사가 없으면 `bmo cargo`가 같은 동사를 제공합니다.

```sh
bmo cargo check --workspace --all-targets
bmo cargo test --workspace
bmo cargo clippy --workspace --all-targets -- -D warnings
bmo cargo fmt --all --check
```

리더의 테스트는 `reader/`에서 실행합니다.

```sh
bun install
bun test
```
