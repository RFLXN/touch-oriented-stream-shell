# 문서

토스 프론트 1.5세대(PLF02WH)에 Alpine Linux를 올려 스트림덱처럼 활용하기 위한 프로젝트 문서다. 기기 위에서 동작하는 UI/데몬 앱의 이름은 **TOSS**(Touch Oriented Stream Shell)다.

| 문서 | 내용 |
|---|---|
| [01-findings.md](01-findings.md) | **조사사항.** 기기에 대해 파악한 사실 전체: 하드웨어, 펌웨어, 부팅, 커널 지원, 유저랜드 실측 결과, 참고 자료 |
| [02-progress.md](02-progress.md) | **진행상황.** 순정 기기를 현재 상태로 구성하는 절차. 구조 설명과 설계 이유, 검증 범위, 트러블슈팅, 복구 방법, 남은 과제 |
| [03-userland-guide.md](03-userland-guide.md) | **유저랜드 개발 가이드.** PC 연결, 변경 사항 영속화, 화면 앱 실행 방법. 스트림덱 앱(`src/app/`) 구조와 용어, 스타일 규칙은 3.16절, 위젯 추가 방법은 3.17절 |
| [04-host-link.md](04-host-link.md) | **덱과 PC의 통신.** 프로토콜(프레이밍, 메시지), 키와 암호화, 페어링, 호스트가 덱에 허용하는 권한, 명령 실행, 절전 및 웨이크업, PC 측 프로그램 `toss-connector`, 덱 측 구현, 확장 방법, 남은 과제 |

처음 읽는다면 진행상황 -> 유저랜드 가이드 순으로 보고, 세부 사실이 필요할 때 조사사항을 참고하면 된다. 덱과 PC 연동 부분을 수정할 때는 덱과 PC의 통신 문서를 보면 된다. 문서 내 절 번호는 파일 번호를 따른다(진행상황 2.x, 가이드 3.x, 덱과 PC 통신 4.x).

## 이름

| 이름 | 설명 |
|---|---|
| **TOSS** (Touch Oriented Stream Shell) | 이 프로젝트 및 덱 전체 화면을 담당하는 앱. 토스 프론트 하드웨어에서 동작하지만 토스(비바리퍼블리카)와 무관한 개인 프로젝트다 |
| `tossd` | 덱에서 동작하는 데몬(`src/app/tossd`) |
| `toss-shell` | 덱의 UI(`src/app/ui`, 기기 내 `/usr/bin/toss-shell`) |
| `toss-connector` | PC 측 프로그램(`src/app/host`). 덱에 접속해 PC와 연결한다 |
| `toss-proto` | 덱과 PC 간 통신 프로토콜 크레이트(`src/app/proto`) |
| `toss-helper` | 덱에서 root 권한이 필요한 작업(전원 종료, 절전, SSH 제어)을 대행하는 도우미 |

2026-11-17에 명칭을 확정했다.

문서에서는 "덱"은 기기를, "호스트"는 덱에 연결되는 PC를 가리킨다(제품명이 아니라 역할 기준).

## 현재 상태

| 계층 | 내용 |
|---|---|
| 부트로더 | 순정 U-Boot 그대로 사용 |
| 커널 | 메인라인 6.18. 디스플레이, 터치, GPU, Wi-Fi, 블루투스, 이더넷, 오디오, DVFS(주파수 조절) 동작 |
| OS | Alpine Linux 3.24 |
| 화면 환경 | Wayland 키오스크(`cage`). 부팅 시 앱 하나가 전체 화면으로 실행됨 |
| 앱 | 스트림덱 앱: Rust 데몬(`tossd`) + QML UI. 이 앱 하나가 전체 화면(상단바, 위젯, 설정)을 담당 |
| PC 측 | `toss-connector`(Rust). PC에서 실행되어 덱에 연결한다. 웹 콘솔로 설정을 관리하며 NixOS에서는 모듈로 설치 가능(Linux·Windows 지원, Windows는 `cargo build`로 직접 빌드) |

### 현재 지원 기능

| 구분 | 내용 | 설명 |
|---|---|---|
| 위젯 | 페이지당 4x6 그리드. 미디어 플레이어(PC 트랙 정보, 앨범 아트, 재생 제어), 리소스 미터(덱 자신 또는 PC의 CPU·메모리·GPU·디스크·네트워크·온도), 랙과 그 안의 명령 위젯(PC에 등록한 명령 사용: 실행 버튼, 스위치, 단계 노브, 연속 노브)과 웹페이지 버튼(PC 브라우저에서 주소 열기. 사이트 아이콘 표시 가능) | [03](03-userland-guide.md) 3.16절 |
| 배치 편집 | 기기에서 위젯과 랙을 이동, 추가, 삭제하고 랙 크기와 이름을 변경. 페이지 추가도 가능 | [03](03-userland-guide.md) 3.16절 |
| 덱 설정 | 화면 밝기, 다크/라이트 테마, 유리 반사 효과, Wi-Fi 연결, SSH 활성화/비활성화, 페어링, 외부 연동 키 상태와 PC별 허용, 배치 편집·초기화, 절전, 전원 종료 | [03](03-userland-guide.md) 3.16절 |
| PC 연동 | USB·이더넷·Wi-Fi 지원. 페어링된 PC만 접속 허용, 전 구간 암호화. PC에서 덱별로 허용한 권한 범위 내에서만 동작(재생 정보, 재생 제어, 리소스 조회, 등록된 명령 실행) | [04](04-host-link.md) |
| PC 측 설정 | 웹 콘솔(`toss-connector console`): 덱별 권한 설정, 명령 등록, 페어링, 절전 및 웨이크업, 외부 연동(Discord) 키 저장과 연결 | [04](04-host-link.md) 4.10절 |
| 외부 연동 | 덱 금고: Discord 같은 외부 서비스의 키를 덱에 암호화해 보관하고, 덱이 발급·갱신한 짧은 수명의 토큰만 PC에 빌려준다. 금고를 쓸 PC는 덱 화면에서 허용한다. 이 토큰으로 Discord 마이크·헤드셋 음소거를 덱의 스위치로 쓴다(상태도 Discord에서 받아 반영). Spotify는 덱이 계정의 재생 정보를 직접 가져와 미디어 위젯에 보여 주고 제어한다(로그인만 PC 브라우저) | [04](04-host-link.md) 4.15절 |
| PC 측 설치 | NixOS 모듈: flake 입력 추가 후 `programs.toss-connector.enable = true;` -> 패키지 설치 및 로그인 시 자동 실행. 모듈에서 권한과 명령을 선언적으로 고정할 수도 있음(`config`, `strict`) | [04](04-host-link.md) 4.10절 |
| 절전 및 웨이크업 | 덱이나 PC에서 절전에 넣고, PC에서 WoWLAN으로 깨움 | [04](04-host-link.md) 4.10절 |

남은 과제는 [02-progress.md](02-progress.md) 2.15절(기기와 앱) 및 [04-host-link.md](04-host-link.md) 4.13절(PC 연동)에 정리되어 있다.

## 원칙: 기기 상태는 저장소에서 재현

기기는 언제든 초기화할 수 있는 대상으로 본다. 기기에 올리는 이미지는 모두 이 저장소에서 Nix로 빌드하며(`src/scripts/build-all.sh`), 동일 커밋에서는 바이트 단위로 동일한 이미지가 생성된다. 기기 내부에서 직접 수정한 내용은 임시 실험일 뿐이며, 유지하려면 저장소에 반영해야 한다.

**예외는 사용자가 기기에서 직접 설정한 데이터다.** 저장된 Wi-Fi, 페어링 목록, 위젯 배치, 화면 밝기 등은 기기의 `/data`에 저장되어 재플래싱 후에도 유지된다. 이 데이터는 이미지나 저장소에 포함되지 않는다([03-userland-guide.md](03-userland-guide.md) 3.15절 "기기에만 남는 데이터").

| 작업 | 위치 |
|---|---|
| 저장소 -> 기기 배포 | `src/scripts/deploy.sh` ([02-progress.md](02-progress.md) 2.7절) |
| 이미지 빌드 옵션 | `flake.nix`의 `buildImage { … }` ([02-progress.md](02-progress.md) 2.6절) |
| 빌드 재현성 고정 요소 | [02-progress.md](02-progress.md) 2.6절 |
| 변경 사항 영속화 | [03-userland-guide.md](03-userland-guide.md) 3.6절 |
| 재플래싱 없이 앱 수정 테스트 | `src/scripts/app-push.sh` ([03-userland-guide.md](03-userland-guide.md) 3.16절) |
| 새 앱(위젯) 추가 | [03-userland-guide.md](03-userland-guide.md) 3.17절 |
| PC 연동 프로토콜 확장 | [04-host-link.md](04-host-link.md) 4.12절 |

## 저장소 구조

| 경로 | 내용 |
|---|---|
| `flake.nix` | 이미지 빌드 옵션, devShell, PC 측 프로그램의 패키지 및 NixOS 모듈 정의. 빌드 정의는 `nix/`, 소스는 `src/` 아래에 있다 |
| `nix/` | 이미지 빌드 정의(`build.nix`, 내부에서 호출하는 `inputs.nix`·`kernel.nix`, `build-rootfs.sh`, `build-initramfs.sh`), PC 측 프로그램 패키지(`toss-connector.nix`), NixOS 모듈(`nixos-module.nix`) 및 모듈 검증(`module-check.nix`) |
| `src/os/` | 커널 설정·디바이스 트리·드라이버, initramfs, rootfs 설정, Alpine 패키지 목록 및 락파일 |
| `src/app/` | 스트림덱 앱. `tossd/` 데몬(Rust), `ui/` 화면(QML: `shell/` 뼈대, `kit/` 컴포넌트, `widgets/` 앱 위젯), `rootfs/` 기기 배치 파일, `proto/` 덱-PC 프로토콜, `meter/` 리소스 측정(덱/PC 공용), `host/` PC 측 프로그램 `toss-connector`(`console/`은 웹 콘솔 페이지) |
| `src/scripts/` | 준비·빌드·플래싱·접속 스크립트. 공용 유틸(기기 주소, 파티션 테이블, 플래싱 헬퍼, usage 출력)은 `lib.sh`. 무인 UI 테스트용 `tf-tap.sh`(탭), `tf-drag.sh`(드래그) |
| `docs/` | 문서 디렉터리 |
| `dist/` | 빌드 산출물(git 제외): 이미지(`images`, `userdata.img`), 커널 증분 빌드, `cargo` 출력(`cargo/`) |
| `local/` | 로컬 자료(git 제외): 기기 백업, 순정 이미지, Alpine 패키지 아카이브, 기기 고유 설정, 로더, 조사 자료 |

git에 포함되지 않아 **별도로 백업해야 하는 디렉터리**: `local/backup/`(초기 기기 덤프), `local/downgrade/`(순정 761 이미지), `local/archive/`(고정된 Alpine 패키지 파일), `local/device/`(기기 고유 설정).
