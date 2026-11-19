# TOSS — Touch Oriented Stream Shell

토스 프론트 1.5세대(PLF02WH) 단말기를 **탁상용 스트림덱**으로 쓰기 위한 프로젝트다. 기기에 메인라인 Linux와 Alpine Linux를 올리고, 전체 화면을 담당하는 앱(TOSS)을 띄워 PC의 미디어·리소스를 보여 주고 PC에 등록한 명령을 버튼·스위치·노브로 실행한다.

토스 프론트 하드웨어에서 동작하지만 토스(비바리퍼블리카)와 무관한 개인 프로젝트다.

## 무엇을 할 수 있나

- **위젯**: 미디어 플레이어(PC에서 재생 중인 곡, 앨범 아트, 재생 제어), 리소스 미터(덱 자신 또는 PC의 CPU·메모리·GPU·디스크·네트워크·온도), 명령 위젯(실행 버튼, 스위치, 단계 노브, 연속 노브), 웹페이지 버튼(PC 브라우저에서 주소 열기. 사이트 아이콘 표시 가능)
- **레이아웃 편집**: 기기 화면에서 위젯과 랙을 추가·이동·삭제하고 페이지를 늘린다
- **PC 연동**: USB·이더넷·Wi-Fi로 연결한다. 페어링한 PC만 접속할 수 있고 전 구간을 암호화한다. PC는 덱별로 허용한 권한 범위 안에서만 동작한다
- **PC에서 실행되는 것은 PC에 등록한 명령뿐이다.** 덱은 등록된 명령의 ID와 값만 보내며 실행할 커맨드를 알지 못한다
- **외부 연동 키 보관(덱 금고)**: Discord 같은 외부 서비스의 키를 덱에 암호화해 보관한다. PC는 덱이 발급한 짧은 수명의 토큰만 빌려 쓰고, 금고를 쓸 PC는 덱 화면에서 허용한다. 이 연동으로 Discord 마이크·헤드셋 음소거를 덱의 스위치로 켜고 끈다(Discord에서 바꾼 상태도 표시된다). Spotify 계정을 연결하면 덱이 그 계정에서 재생 중인 곡을 직접 가져와 미디어 위젯에 보여 주고 제어한다(휴대폰 등 다른 기기에서 재생해도 보인다)([docs/04-host-link.md](docs/04-host-link.md) 4.15절)
- **덱 설정**: 화면 밝기, 다크/라이트 테마, Wi-Fi, SSH 켜고 끄기, 외부 연동 키 상태, 절전, 전원 종료

구성은 다음과 같다.

| 구성 요소 | 위치 | 역할 |
|---|---|---|
| 기기 이미지 | `nix/`, `src/os/`, `src/scripts/` | 메인라인 Linux 6.18 + Alpine Linux 3.24. Nix로 빌드한다 |
| `tossd` | `src/app/tossd` | 덱에서 동작하는 데몬(Rust) |
| `toss-shell` | `src/app/ui` | 덱의 UI(QML). Wayland 키오스크(`cage`)에서 전체 화면으로 실행된다 |
| `toss-connector` | `src/app/host` | PC 측 프로그램(Rust, Linux). 덱에 접속해 PC와 연결하고 웹 콘솔을 제공한다 |

저장소의 폴더는 다섯이다.

| 폴더 | 내용 |
|---|---|
| `src/` | 소스: 덱 앱과 PC 측 프로그램(`app/`), 커널·rootfs 설정(`os/`), 스크립트(`scripts/`) |
| `nix/` | 이미지 빌드 정의, `toss-connector` 패키지와 NixOS 모듈 |
| `docs/` | 문서 |
| `dist/` | 빌드 산출물(git 제외) |
| `local/` | 기기 백업, 순정 이미지 등 로컬 자료(git 제외) |

## 준비물

- 토스 프론트 1.5세대 PLF02WH (1세대와 2세대는 대상이 아니다)
- Micro USB 데이터 케이블, 12V USB-C PD 전원
- Nix(flakes 활성화)가 설치된 x86_64 리눅스 PC, 디스크 여유 공간 약 25GB
- 순정 빌드 761 이미지 — 저장소에 포함되지 않는다. 받는 곳과 체크섬은 [docs/02-progress.md](docs/02-progress.md) 2.1절

모든 작업은 Micro USB 케이블만으로 할 수 있다. 분해나 시리얼 어댑터는 필요 없다.

## 사용 방법

명령은 모두 저장소 루트에서 `nix develop` 셸 안에서(또는 `nix develop -c`를 붙여) 실행한다.

### 1. 기기에 이미지 올리기

처음 한 번은 순정 기기를 플래싱 모드로 넣고, 원본을 백업하고, 순정 761로 다운그레이드해야 한다. 절차와 주의 사항은 [docs/02-progress.md](docs/02-progress.md) 2.2~2.5절을 순서대로 따른다. 그 과정에서 쓰는 주요 명령은 다음과 같다.

```bash
nix develop -c src/scripts/fetch-loader.sh
```

```bash
nix develop -c src/scripts/backup.sh
```

```bash
nix develop -c src/scripts/downgrade.sh
```

그다음부터는 명령 하나로 빌드부터 플래싱·재부팅까지 진행한다(2.6~2.7절).

```bash
nix develop -c src/scripts/deploy.sh
```

부팅하면 약 20초 뒤 스트림덱 UI가 뜬다. 저장소를 수정한 뒤에도 같은 명령으로 다시 올린다. 저장된 Wi-Fi, 페어링 목록, 레이아웃 같은 사용자 데이터는 기기의 `/data`에 남아 재플래싱 후에도 유지된다.

### 2. PC에서 `toss-connector` 실행

바로 실행하려면:

```bash
nix build .#toss-connector -o dist/toss-connector && ./dist/toss-connector/bin/toss-connector
```

NixOS에서는 시스템 flake에 이 저장소를 입력(`toss-front`)으로 추가하고 모듈을 켜면 로그인 시 자동 실행된다. 권한과 명령을 설정에 선언해 고정할 수도 있다([docs/04-host-link.md](docs/04-host-link.md) 4.10절).

```nix
modules = [
  toss-front.nixosModules.default
  { programs.toss-connector.enable = true; }
];
```

아래의 `toss-connector` 명령은 설치된 상태를 기준으로 적었다. `nix build`로 실행했다면 `./dist/toss-connector/bin/toss-connector`로 바꿔 쓴다.

### 3. 페어링

1. 덱: 설정 → `Paired devices` → `PAIR NEW DEVICE`
2. PC: 웹 콘솔(`toss-connector console`)의 "새 덱 페어링", 또는 터미널에서 `toss-connector pair`(USB) / `toss-connector pair --deck <주소>`(LAN)
3. 양쪽에 표시된 6자리 숫자가 같으면 덱에서 `ACCEPT`, PC에서 "숫자 일치"(터미널이면 `y`)

페어링하면 미디어 플레이어 위젯에 PC에서 재생 중인 곡이 표시된다.

### 4. 명령 등록과 권한

PC에서 실행할 명령은 PC에 먼저 등록한다. 웹 콘솔의 "명령 → + 추가"를 쓰거나 터미널에서 등록한다.

```bash
toss-connector command add Terminal -- foot
```

```bash
toss-connector allow command.run
```

그다음 덱의 편집 모드에서 랙 빈 슬롯의 `+` → `Command`로 위젯을 놓는다. 스위치와 노브(`add-switch`, `add-stepped`, `add-linear`)의 사용법과 예시는 [docs/04-host-link.md](docs/04-host-link.md) 4.6절 "명령"에 있다.

덱별 권한은 웹 콘솔의 토글이나 `toss-connector trust` / `allow` / `deny`로 조회·변경한다. 재생 정보, 재생 제어, 리소스 사용량은 기본으로 허용되고 명령 실행은 기본으로 꺼져 있다.

### 5. 절전과 웨이크업

```bash
toss-connector sleep
```

```bash
toss-connector wake
```

덱의 설정 → `Sleep`으로도 절전에 넣을 수 있다. 웨이크업은 PC에서만 가능하며 덱이 Wi-Fi에 연결돼 있어야 한다.

## 주의 사항

- **기본 이미지는 SSH가 열려 있고 root에 비밀번호가 없다.** 공유기가 외부 접속을 차단하는 내부망에서 쓴다는 전제다. 쓰지 않을 때는 덱의 설정 → `NETWORK` → `SSH`에서 끈다(재플래싱·재부팅 후에도 유지된다).
- 순정 761 이미지는 익명으로 배포된 파일이다. 체크섬은 이 프로젝트에서 실제로 사용해 동작을 확인한 파일과 같다는 것만 보장한다.
- `local/backup/`, `local/downgrade/`, `local/archive/`, `local/device/`는 git에 포함되지 않는다. 기기 고유 정보가 들어 있으므로 공유하지 말고 별도로 백업한다.
- 검증한 범위와 확인하지 못한 것은 [docs/02-progress.md](docs/02-progress.md) 2.12절과 [docs/04-host-link.md](docs/04-host-link.md) 4.13절에 정리돼 있다.

## 개발

| 작업 | 방법 |
|---|---|
| 기기에 접속 | `src/scripts/tf-ssh.sh` |
| 재플래싱 없이 앱 수정 테스트 | `src/scripts/app-push.sh` (끝나면 `src/scripts/deploy.sh`로 다시 올린다) |
| 위젯 추가 | [docs/03-userland-guide.md](docs/03-userland-guide.md) 3.17절 |
| PC 연동 프로토콜 확장 | [docs/04-host-link.md](docs/04-host-link.md) 4.12절 |

기기 상태는 저장소에서 재현한다. 기기 내부에서 직접 수정한 내용은 임시 실험일 뿐이며, 유지하려면 저장소에 반영해야 한다.

## 문서

| 문서 | 내용 |
|---|---|
| [docs/README.md](docs/README.md) | 문서 안내, 현재 상태, 저장소 구조 |
| [docs/01-findings.md](docs/01-findings.md) | 조사사항: 하드웨어, 펌웨어, 부팅, 커널 지원 |
| [docs/02-progress.md](docs/02-progress.md) | 진행상황: 순정 기기를 현재 상태로 구성하는 절차, 검증 범위, 복구 방법 |
| [docs/03-userland-guide.md](docs/03-userland-guide.md) | 유저랜드 개발 가이드: 스트림덱 앱 구조, 위젯 추가 |
| [docs/04-host-link.md](docs/04-host-link.md) | 덱과 PC의 통신: 프로토콜, 페어링, 권한, `toss-connector` |
