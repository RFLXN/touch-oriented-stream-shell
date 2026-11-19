# 2. 진행상황 — 순정 기기에서 현재 상태까지 만드는 법

이 문서를 처음부터 따라 하면 순정 토스 프론트 1.5세대(PLF02WH)가 아래 상태가 된다.

| 항목 | 상태 |
|---|---|
| OS | Alpine Linux 3.24 |
| 커널 | 메인라인 6.18 (순정 부트로더로 부팅) |
| 화면 | 800×1280, 부팅 후 앱 하나를 전체 화면으로 띄움 (Wayland 키오스크). 스트림덱 앱(`src/app/`) |
| 터치 | 동작, 좌표가 화면과 일치 |
| Wi-Fi, 블루투스 | 동작 (국가 코드 KR) |
| 이더넷 | 동작 (100Mbps), 케이블을 꽂으면 자동으로 IP 수신, MAC 주소 고정 |
| CPU | 부하에 따라 주파수 자동 조절 (리틀 코어 최대 1.416GHz, 빅 코어 최대 1.8GHz), 과열 시 자동 스로틀링 |
| GPU | Mali-T860 가속 (Panfrost, OpenGL ES 3.1) |
| 사운드 | 스피커 출력 |
| 시계 | 부팅 시 네트워크 시각으로 자동 동기화, KST |
| 앱 | 스트림덱 앱: Rust 데몬(`tossd`) + QML UI. 부팅 시 자동으로 떠서 서로 연결된다. 위젯 배치를 기기에서 편집하고, 페어링한 PC의 미디어를 제어하며, 덱과 PC의 리소스 사용량을 미터로 표시하고, PC에 등록해 둔 명령을 버튼·스위치·노브로 실행한다 |
| PC 쪽 | `toss-connector`: PC에서 동작하며 덱에 접속한다. 브라우저 기반 웹 콘솔, NixOS 모듈(설치·자동 실행, 권한·명령 선언적 관리) |
| 전원 | 설정에서 화면 밝기, 절전(얕은 절전. PC가 Wi-Fi로 웨이크업), 전원 끄기 지원 |
| 접속 | Micro USB 케이블 하나로 SSH와 시리얼 콘솔 제공. 같은 공유기에서는 LAN으로도 SSH 접속 가능. SSH는 덱 설정에서 비활성화 가능 |
| 재현성 | 이미지 전체를 Nix로 빌드. 같은 커밋에서는 바이트 단위로 동일한 이미지 생성 |

기기 관련 사실은 [01-findings.md](01-findings.md), 이 환경 위에서 앱을 개발하는 방법은 [03-userland-guide.md](03-userland-guide.md)를 참고하면 된다.

**검증 범위는 2.12절에 정리했다.** 요약하면, 2.6절 이후(빌드 → 플래싱 → 확인)는 이 문서대로 Nix로 빌드한 이미지를 `src/scripts/deploy.sh`로 기기에 통째로 구워 검증했다. 2.3~2.5절(진입, 백업, 다운그레이드)은 작업 초기에 1회 수행했고 다시 돌리지는 않았다.

## 2.1 준비물

### 하드웨어

| 준비물 | 비고 |
|---|---|
| 토스 프론트 1.5세대 PLF02WH | 카메라 2개, 측면 카드부 돌출. 1세대와 2세대는 대상 외 |
| Micro USB **데이터** 케이블 | 충전 전용 케이블 불가 |
| 12V USB-C PD 전원 | 번들 어댑터 또는 12V 2.5A 이상 PD 충전기 |
| 금속 핀셋 또는 클립 | 펌웨어가 Loader 진입을 막고 있을 때 1회 필요 (2.3절) |

LAN 케이블, 시리얼 어댑터, 분해 공구는 필요 없다.

### PC

- Nix(flakes 활성화)가 설치된 x86_64 리눅스. NixOS에서 작업했다.
- 디스크 여유 공간 약 25GB (백업 15GB, 커널 빌드 3GB, 이미지, Alpine 패키지 아카이브 100MB 등)
- 인터넷 연결 (커널 소스, Alpine 패키지)

### 별도로 받아야 하는 파일

저장소에 포함되지 않은 파일이 두 가지 있다.

**1. RK3399 로더** — 스크립트가 내려받아 SHA-256을 검증한다.

```bash
nix develop -c src/scripts/fetch-loader.sh
```

**2. 순정 빌드 761 이미지 7개** — 커뮤니티 배포본이다. [프론트1 기기 펌웨어 복구 글](https://gall.dcinside.com/mgallery/board/view/?id=sunmitechnology&no=26&page=1)에서 받아 `local/downgrade/image/`에 푼다.

| 파일 | MD5 |
|---|---|
| `security.img` | `b5cfa9d6c8febd618f91ac2843d50a1c` |
| `uboot.img` | `353cd6275777d430dbeaaefaeeb24d48` |
| `dtbo.img` | `d66e118148f2cf485b26e36af53dd3fa` |
| `vbmeta.img` | `2da4cd9de1db4094ec1dc7bee32aaf02` |
| `boot.img` | `f9a5835fb3d5170d4cc7514cefe1b17c` |
| `recovery.img` | `254782375c54810590db37af604c4b0f` |
| `super.img` | `b9946795aeba47032cf28b818a9c11c2` |

- 익명 업로드라 출처를 보증할 수 없다. MD5 체크섬은 "이 문서를 쓸 당시 실제로 사용해 동작을 확인한 파일과 같다"는 것만 보장한다. `src/scripts/downgrade.sh`가 플래싱 전에 검증한다.
- 이 이미지는 다운그레이드에만 쓰는 것이 아니다. **순정 커널**(복귀용)과 **Wi-Fi·블루투스 펌웨어**도 여기서 추출한다.

LineageOS 이미지는 필요 없다. 작업 중 하드웨어 정보를 읽으려고 거쳤지만, 그 결과는 이미 저장소의 디바이스 트리와 드라이버에 반영돼 있다.

## 2.2 PC 준비

저장소 루트에서 도구가 들어 있는 셸을 연다. 이후 명령은 모두 이 안에서(또는 `nix develop -c`를 붙여) 실행한다.

```bash
nix develop
```

USB 장치 권한을 연다. 임시 udev 규칙이므로 PC를 재부팅하면 다시 적용해야 한다. 영구 설정은 [03-userland-guide.md](03-userland-guide.md) 3.2절을 참고하면 된다.

```bash
sudo sh -c 'mkdir -p /run/udev/rules.d && printf "%s\n" "SUBSYSTEM==\"usb\", ATTR{idVendor}==\"2207\", MODE=\"0666\"" "SUBSYSTEM==\"tty\", ATTRS{idVendor}==\"1d6b\", ATTRS{idProduct}==\"0104\", MODE=\"0666\"" > /run/udev/rules.d/99-rockchip.rules && udevadm control --reload && udevadm trigger'
```

주의: 이 저장소는 반드시 git 저장소 상태를 유지해야 한다. flake는 git이 추적하는 파일만 Nix 스토어로 복사한다. git 저장소가 아니면 `nix develop`을 실행할 때마다 디렉터리 전체(백업 포함 수십 GB)가 `/nix/store`로 복사된다. 이미지와 백업 디렉터리는 `.gitignore`에 등록돼 있다.

## 2.3 기기를 플래싱 모드로

기기 하부 뚜껑을 연다(공구 불필요). 안에 Micro USB 포트와 버튼 두 개(위가 초기화, 아래가 재부팅)가 있다. **Micro USB를 PC에 먼저 연결한다.**

### 먼저 버튼으로 시도

1. USB-C 전원을 뽑는다.
2. 초기화 버튼을 누른 채 USB-C 전원을 꽂는다.
3. 화면은 꺼져 있고 전원 버튼에 녹색 LED만 들어온다.

```bash
rkdeveloptool ld
```

`Loader` 한 줄이 뜨면 성공이다. 2.4절로 넘어간다.

### "지우는 중" 화면이 뜨면

펌웨어(빌드 799 계열)가 버튼 진입을 막고 있는 것이다. 공장 초기화가 한 번 돌 뿐 기기에 문제는 없다. 패드 쇼트로 MaskROM에 진입한다.

1. USB-C 전원을 뽑는다. Micro USB는 PC에 꽂아 둔다.
2. 카드 슬롯 기판 사이의 금색 원형 패드 두 개를 찾는다. 왼쪽이 `VCC_1V8`, 오른쪽이 `BOOT`다. 실크 표기를 확인한다.
3. 핀셋으로 **두 패드만** 쇼트한다.
4. 쇼트를 유지한 채 USB-C 전원을 꽂는다.
5. `rkdeveloptool ld`에 `Maskrom`이 뜨면 핀셋을 뗀다.

`VCC_1V8`은 전원 레일이다. 핀셋이 옆 부품에 닿지 않게 조심한다. 인식이 안 되면 전원을 뽑고 처음부터 다시 한다. 패드 위치 그림은 2.1절의 펌웨어 복구 글에 있다.

순정 상태에서는 **Wi-Fi에 연결하지 않는다.** 자동 업데이트로 진입이 막히는 펌웨어가 올라갈 수 있다.

### 로더 적재

Loader든 MaskROM이든 아래 명령 하나로 "읽고 쓸 수 있는 상태"가 된다. Loader 상태였다면 MaskROM으로 넘긴 뒤 로더를 RAM에 올린다.

```bash
src/scripts/flash.sh --enter
```

`[ ok ] MaskROM + 로더 적재 상태`가 나와야 한다. 기기 내장 Loader는 읽은 값을 신뢰할 수 없어서(일부가 `0xCC`로 나온다) 모든 읽기와 쓰기를 이 상태에서 진행한다. 기기에서 읽거나 쓰는 스크립트(`backup.sh`, `downgrade.sh`, `flash.sh`)는 이 상태가 아니면 `[FAIL] MaskROM + 로더 적재 상태가 아닙니다 …`를 출력하고 실행을 거부한다.

실패 시 나오는 에러 메시지:

| 출력 | 의미 |
|---|---|
| `[FAIL] 기기가 Loader 또는 MaskROM 상태가 아닙니다` | `rkdeveloptool ld`에 기기가 잡히지 않는다. 위 버튼이나 패드 쇼트 단계부터 다시 한다 |
| `[FAIL] 기기에 명령을 보낼 수 없습니다` | USB 권한 규칙이 빠져 있다 (2.2절) |
| `[FAIL] MaskROM 장치가 감지되지 않습니다` | Loader에서 MaskROM으로 전환 명령을 보낸 뒤 15초 안에 다시 잡히지 않았다. 초기화 버튼을 누른 채 전원을 다시 꽂고 실행한다 |
| `[FAIL] 로더 파일이 없습니다` | `src/scripts/fetch-loader.sh`를 먼저 실행한다 (2.1절) |

## 2.4 원본 백업

기기에 아무것도 쓰기 전에 전체를 백업한다. 약 15GB, 10분 정도 걸린다. 읽기 작업만 수행한다.

```bash
src/scripts/backup.sh
```

- 14개 파티션과 GPT 앞뒤를 저장소의 `local/backup/`에 저장한다(어느 디렉터리에서 실행해도 같다. 다른 곳에 받으려면 경로를 인자로 준다). 크기가 맞는 파일은 건너뛰므로 중간에 끊기면 다시 실행해 이어 받는다.
- **`flash.sh --enter`를 먼저 실행해 둔다**(2.3절). Read LBA를 쓸 수 있는 상태가 아니면 시작하지 않는다. 내장 Loader에서 읽으면 크기만 맞고 내용은 `0xCC`인 파일이 나오기 때문이다. 읽기 중 실패하면 `[FAIL] <이름> 읽기 실패`와 도구 출력을 보여 주고 중단한다.
- 파티션 주소표(`src/scripts/lib.sh`)는 이 모델의 실측값이다. `local/backup/device-info.txt`에 기기가 보고한 파티션 테이블이 같이 저장되니, 그 표와 일치하는지 확인한다. 다르면 여기서 멈춘다.
- 헤더 확인: `xxd -l 8 local/backup/boot.img`가 `ANDROID!`로 시작해야 한다.
- **백업에는 기기 고유 정보가 들어 있다.** 외부에 공유하지 말고, 가능하면 다른 디스크에도 사본을 두는 편이 좋다.

## 2.5 순정 761로 다운그레이드

목적은 두 가지다. 초기화 버튼으로 Loader에 들어가는 경로를 복구하고, 비서명 커널을 막지 않는 부트로더로 되돌리는 것이다.

```bash
src/scripts/downgrade.sh
```

7개 파티션(`security`, `uboot`, `dtbo`, `vbmeta`, `boot`, `recovery`, `super`)을 기록하고 각각 되읽어 검증한다. `[done] 7개 파티션 기록 및 검증 완료`가 나와야 한다.

- **`flash.sh --enter`를 먼저 실행해 둔다**(2.3절). 그 상태가 아니면 아무것도 쓰지 않고 멈춘다.
- 순서: `local/downgrade/image/`의 이미지 7개 크기와 MD5를 확인한다 → `기기에 순정 761 이미지를 기록할까요? (y/N)`를 한 번 묻는다(`-y`를 주면 생략) → 파티션마다 `src/scripts/flash.sh`로 쓴다.
- `flash.sh`는 파티션마다 기기의 파티션 테이블을 읽어 시작 LBA가 예상값과 같은지 검증한 뒤 쓰고, 쓴 내용을 항상 되읽어 대조한다. 하나라도 어긋나면 거기서 멈춘다(그 앞 파티션까지는 이미 기록된 상태다. 문제를 해결한 뒤 다시 실행하면 처음부터 다시 쓴다).
- 이미지가 다른 디렉터리에 있으면 경로를 인자로 준다.

확인: 재부팅 후 2.3절의 버튼 방식으로 Loader에 들어가지는지 확인한다.

```bash
src/scripts/flash.sh --reboot
```

- 순정 안드로이드의 "시작하기" 화면이 뜬다. Wi-Fi에 연결하지 말고 그대로 둔다.
- 전원을 뽑고 초기화 버튼을 누른 채 다시 꽂는다. `rkdeveloptool ld`에 `Loader`가 뜨면 다운그레이드가 적용된 것이다.
- 다시 `src/scripts/flash.sh --enter`.

참고: 배포본의 `security.img`는 전부 0으로 채워져 있다. 기기 원래의 `security` 파티션은 `local/backup/security.img`에만 남아 있다.

## 2.6 이미지 빌드

이미지는 Nix로 빌드한다. `nix/build.nix`가 `buildImage` 함수를 제공하고, `flake.nix`가 옵션을 넘겨 이 함수를 호출한다. 빌드 정의는 셋으로 나뉜다. git 외부에서 오는 재료(Alpine 패키지, 순정 추출물)는 `nix/inputs.nix`, 커널과 디바이스 트리는 `nix/kernel.nix`, 나머지(initramfs, 데몬, 루트 파일시스템, 부트 이미지 2종, 옵션)는 `nix/build.nix`에 있다. **같은 커밋에서는 바이트 단위로 동일한 이미지가 나온다.** 기기가 초기화되거나 망가져도 저장소만 있으면 똑같은 상태로 복구할 수 있게 하려는 설계다.

### 준비

별도 작업은 필요 없다. `local/downgrade/image/`에 순정 761 이미지만 있으면 된다(2.1절).

빌드에는 git에 없는 재료가 두 가지 필요하다. 순정 이미지에서 꺼낸 무선 펌웨어·순정 부트 이미지, 그리고 버전 고정한 Alpine 패키지 파일이다. Nix는 이것들을 해시로만 찾는다. Nix 스토어에 없으면 `build-all.sh`가 알아서 스토어에 넣고 다시 빌드한다. Alpine 패키지 아카이브(`local/archive/alpine/`)가 없거나 일부 빠져 있어도 멈추지 않는다. `[info] local/archive/alpine에 없는 패키지가 있습니다. 미러에서 내려받습니다.`를 출력하고 빠진 패키지는 Nix가 Alpine 미러에서 받는다(미러에서도 내려갔다면 그때 빌드가 실패한다). 수동으로 넣으려면:

```bash
src/scripts/stock-to-store.sh
```

```bash
src/scripts/alpine-lock.sh restore
```

앞의 스크립트가 다루는 재료는 제조사 소유물이라 git에 넣지 않고 `src/os/stock.json`의 SHA-256과 대조해 스토어에 넣는다. 뒤의 스크립트는 `local/archive/alpine/`에서 꺼낸다(아카이브에 빠진 패키지가 있으면 있는 것만 넣고 실패로 끝난다).

### 빌드

```bash
src/scripts/build-all.sh
```

기기가 연결돼 있지 않아도 된다. 처음에는 커널을 빌드하느라 몇 분 걸리고(16코어 PC 기준 약 3분), 이후에는 바뀐 부분만 다시 빌드한다. 결과물은 `dist/`에 생성된다.

| 산출물 | 기록할 파티션 | 내용 |
|---|---|---|
| `images/boot-mainline.img` | `boot` | 메인라인 커널 6.18 + 디바이스 트리 + initramfs |
| `images/boot-stage1.img` | `recovery` | 순정 4.19 커널 + 같은 initramfs. 복귀용 |
| `images/alpine-rootfs.img` | — | Alpine 루트 파일시스템. 기기 고유 값이 없는 공통 이미지 |
| `userdata.img` | `userdata` | 위 루트 이미지에 `local/device/rootfs/`의 파일을 얹은 이미지 (2.9절) |
| `images/manifest.txt`, `images/packages.txt`, `images/inputs.txt` | — | 커널 버전·부팅 인자·이미지 해시, 설치된 패키지 목록, 빌드 재료의 Nix 스토어 경로(이 링크가 살아 있는 동안 GC에서 제외된다) |

`dist/images`는 Nix 스토어를 가리키는 심볼릭 링크다. 이 링크가 남아 있는 동안에는 Nix 스토어를 정리(`nix-collect-garbage`)해도 빌드에 쓴 재료(커널 결과, 패키지, 펌웨어)가 지워지지 않는다. `dist/`를 지웠거나 링크가 없는 상태에서 정리했다면 재료가 사라지는데, 이때는 `build-all.sh`가 아카이브에서 다시 넣고 커널부터 재빌드한다(검증됨: 그렇게 다시 만든 이미지 셋이 이전과 바이트 단위로 동일했다).

빌드는 아래 경우에 원인을 출력하고 실패한다. 잘못된 이미지가 조용히 만들어지는 것을 막기 위한 안전장치다.

| 경우 | 출력 메시지 |
|---|---|
| 커널 설정 조각(`src/os/kernel/toss-front.config`)의 줄이 하나라도 그대로 반영되지 않았다 | `커널 설정 조각(toss-front.config)이 경고 없이 반영되지 않았습니다` |
| `src/os/rootfs/`나 `src/app/rootfs/`의 실행 파일(`etc/init.d`, `etc/local.d/*.start`, `usr/bin`, `usr/sbin`, `usr/libexec` 아래)에 실행 비트가 빠졌다 | `[FAIL] …에 실행 비트가 빠진 실행 파일이 있습니다`. `chmod +x`를 주고 git에도 실행 비트를 기록한다(`git update-index --chmod=+x <파일>`) |
| 무선 규제 DB(`regulatory.db`와 서명 파일)가 `wireless-regdb` 패키지에서 나오지 않았다 | `[FAIL] wireless-regdb 패키지에 … 파일이 없습니다` |
| 켜려는 서비스가 이미지에 없거나 시간대 이름이 잘못됐다 | `[FAIL] 존재하지 않는 서비스: …`, `[FAIL] 알 수 없는 시간대: …` |
| 부트 이미지가 파티션 크기를 넘었다 | `[FAIL] 부트 이미지가 … 파티션 크기를 초과합니다` |

### 재현성 고정 요소

| 재료 | 고정 수단 | 갱신 방법 |
|---|---|---|
| 설정, 디바이스 트리, 드라이버, 빌드 절차 (`src/os/`, `nix/`) | git 커밋 | 수정 후 커밋 |
| 커널 소스, 컴파일러(Rust 포함) 등 빌드 툴체인 | `flake.lock` | `nix flake update` |
| 데몬 Rust 의존 크레이트 | `src/app/tossd/Cargo.lock` | `cargo update` |
| Alpine 패키지 (의존성 포함 200개 — 루트용 199개 + initramfs용 `busybox-static`) | `src/os/alpine/lock.json` (파일명과 SHA-256) | `src/scripts/alpine-lock.sh update` |
| 순정에서 추출한 펌웨어·커널 | `src/os/stock.json` (SHA-256) | 바꿀 일 없음 |
| 기기 고유 값 (MAC 주소 등) | `local/device/rootfs/` — git 외부에서 직접 보관 | 2.9절 |

**Alpine 패키지 아카이브(`local/archive/alpine/`)는 지우지 말고 백업해 둔다.** Alpine 미러는 패키지가 업데이트되면 이전 버전 파일을 삭제한다. lock 파일에 기록된 버전이 미러에서 내려간 뒤에는 이 아카이브가 유일한 소스다. Nix 스토어를 정리해서 패키지가 지워졌다면 `src/scripts/alpine-lock.sh restore`로 아카이브에서 다시 넣는다. 아카이브 무결성은 `src/scripts/alpine-lock.sh check`로 확인한다.

### 설정 가능한 값

| 파일 | 내용 |
|---|---|
| `src/os/alpine/packages` | 설치할 패키지 목록. 수정 후 `src/scripts/alpine-lock.sh update`로 lock 파일을 갱신해야 반영된다 |
| `flake.nix`의 `buildImage { … }` 인자 | 아래 표의 옵션. 기본값은 `nix/build.nix`에 정의 |
| `src/os/kernel/cmdline` | 커널 부팅 인자 |
| `src/os/rootfs/` | 루트 파일시스템의 같은 경로에 그대로 덮어씌우는 파일 |

옵션은 CLI 인자가 아니라 `flake.nix` 안의 함수 인자로 넘긴다. 어떤 옵션으로 빌드했는지 커밋 이력에 남기기 위해서다. 알 수 없는 옵션을 적으면(오타 등) 빌드 전에 에러가 난다.

```nix
images = buildImage {
  hostname = "toss-front";
  ssh = {
    lan = true;
    authorizedKeys = [ ];
  };
  ntp = {
    enable = true;
    timezone = "Asia/Seoul";
  };
  kiosk = {
    enable = true;
    command = "toss-shell";
  };
  toss = {
    enable = true;
  };
};
```

| 옵션 | 기본값 | 의미 |
|---|---|---|
| `hostname` | `"toss-front"` | 호스트명 |
| `ssh.lan` | `false` | `true`면 모든 인터페이스(USB·유선·무선)에서, `false`면 USB 이더넷 IP에서만 SSH 수신 |
| `ssh.authorizedKeys` | `[ ]` | root 로그인 공개키. 하나라도 넣으면 비밀번호 로그인이 비활성화된다 |
| `ntp.enable` | `false` | `true`면 부팅 시 네트워크 시각(NTP, `pool.ntp.org`)으로 시계를 맞춘다 |
| `ntp.timezone` | `"Asia/Seoul"` | 시간대. `ntp.enable`과 무관하게 적용된다. 존재하지 않는 이름을 적으면 빌드가 실패한다 |
| `kiosk.enable` | `false` | `true`면 부팅 후 화면에 앱 하나를 전체 화면으로 띄우고, 종료되면 다시 띄운다. `false`면 화면에 로그인 프롬프트가 뜬다 |
| `kiosk.command` | `"foot"` | 실행할 명령. 비루트 사용자 `kiosk`로 실행된다. 기본값은 터미널, 스트림덱 UI는 `"toss-shell"` |
| `toss.enable` | `false` | `true`면 스트림덱 앱(`src/app/`)을 포함한다: 데몬 `tossd`를 서비스로 올리고 UI 정의를 `/usr/share/toss/ui`에 배치 |

**현재 설정은 `ssh.lan = true`, 공개키 없음, `ntp.enable = true`, `kiosk.enable = true`(스트림덱 UI), `toss.enable = true`다.** 비밀번호 없는 root를 LAN에 연 상태이므로 공유기가 외부 접속을 차단하는 내부망이라는 전제다. 다른 환경이라면 `ssh.lan = false`(기본값)로 두거나 공개키를 등록해야 한다. 옵션을 바꾸면 루트 이미지만 다시 빌드된다.

### 빌드가 생략하는 작업

Alpine 패키지 설치 스크립트는 기기용 바이너리를 호출하므로 PC에서는 실행할 수 없다. 따라서 스크립트는 건너뛰고 필요한 효과만 빌드가 직접 만든다(`nix/build-rootfs.sh`).

| 설치 스크립트가 하던 일 | 처리 방식 |
|---|---|
| 서비스 전용 사용자·그룹 생성 (`messagebus`, `dhcpcd`, `seat`) | 빌드가 직접 생성 (`kiosk`, `tossd` 계정도 동일) |
| `busybox-extras` 심볼릭 링크 (`udhcpd`, `telnet` 등) | 생성하지 않음. `busybox-extras udhcpd`처럼 전체 이름으로 호출 |
| dbus `machine-id` | 생성하지 않음. 기기마다 달라야 하는 값이다 |
| 폰트 캐시 (`fc-cache`) | 생성하지 않음. 앱 최초 실행 시 사용자 디렉터리(`/home/kiosk/.cache/fontconfig`)에 생성됨 |

패키지를 추가할 때 설치 스크립트가 포함돼 있다면 같은 방식으로 처리해야 한다.

## 2.7 기기에 기록

한 번에 빌드하고 플래싱하려면:

```bash
src/scripts/deploy.sh
```

빌드하고, 기기를 Loader로 재부팅한 뒤 세 파티션을 기록·검증하고 재부팅한다. 특정 파티션만 골라 쓸 수도 있다(`src/scripts/deploy.sh boot`). 빌드가 끝나면 `기기에 기록할까요? (y/N)`를 한 번 묻고(`-y`를 주면 생략), 이후 파티션마다 다시 묻지는 않는다. 기기가 꺼져 있거나 SSH 접속이 안 되면 2.3절대로 Loader에 진입한 뒤 실행한다.

**`userdata`를 덮어쓰면 기기에서 직접 바꾼 파일은 모두 사라진다.** 남겨야 하는 변경은 저장소(`src/os/`)나 `local/device/rootfs/`에 넣어야 한다.

수동으로 기록하려면 기기가 `MaskROM + 로더 적재 상태`여야 한다(2.3절의 `flash.sh --enter`).

```bash
src/scripts/flash.sh boot dist/images/boot-mainline.img
```

```bash
src/scripts/flash.sh recovery dist/images/boot-stage1.img
```

```bash
src/scripts/flash.sh userdata dist/userdata.img
```

```bash
src/scripts/flash.sh --reboot
```

각 명령은 기록 후 되읽어 대조하고 `되읽기 일치`를 출력한다. `flash.sh`는 기기의 파티션 테이블을 읽어 시작 LBA가 예상(`src/scripts/lib.sh`의 테이블)과 다르면 쓰기를 거부한다.

- `boot`, `recovery`, `userdata`는 확인 없이 쓴다. 그 밖의 파티션(`uboot`, `trust`, `security`, `cache` 등)을 지정하면 쓰기 직전에 `[주의] <이름>는 일반 플래싱 대상(boot, recovery, userdata)이 아닙니다. 계속 기록할까요? (y/N)`를 묻는다. `src/scripts/flash.sh -y <이름> <이미지>`로 주면 묻지 않는다.
- 실패하면 원인을 구체적으로 출력하고 멈춘다: `[FAIL] 알 수 없는 파티션`, `[FAIL] 이미지 크기가 파티션을 초과합니다`, `[FAIL] MaskROM + 로더 적재 상태가 아닙니다`, `[FAIL] 기기의 <이름> 시작 LBA 불일치`, `[FAIL] <이름> 기록 실패`·`되읽기 실패`(도구 출력 말미 포함), `[FAIL] <이름> 되읽기 불일치`.
- 인자가 잘못되면 사용법을 출력하고 종료한다. 다른 스크립트도 마찬가지다.

`super`(순정 안드로이드)는 건드리지 않는다. 부팅에는 쓰이지 않지만 그대로 남겨 둔다.

## 2.8 첫 부팅 확인

재부팅 후 약 10초가 지나면 PC에 `Toss Front Alpine`이라는 USB 장치가 잡히고, PC가 자동으로 `172.16.42.2` IP를 받는다. 첫 부팅 때 루트 파일시스템이 파티션 크기(약 11GB)에 맞춰 확장된다.

| 확인 항목 | 명령 | 기대 결과 |
|---|---|---|
| 접속 | `src/scripts/tf-ssh.sh 'uname -r'` | `6.18.…-tossfront` |
| 화면 | 직접 확인 | 부팅 로그가 지나간 뒤 스트림덱 UI가 뜬다: 상단바(시간·날짜 디스플레이, 아이콘 인디케이터 5개 — 왼쪽부터 USB, LAN, WLAN, BT, CORE —, 오른쪽 끝 설정 버튼), 아래에 미디어 제어 위젯(가로 전체, 4×2)과 리소스 미터(2×1. 덱 자체의 온도·메모리·로드). 이는 초기 레이아웃이며 사용자가 기기에서 편집했다면 편집된 레이아웃이 뜬다. 한·중·일 글자가 깨지지 않고 나온다 |
| 화면 캡처 | `src/scripts/tf-ssh.sh "su -s /bin/sh kiosk -c 'XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-0 grim /tmp/shot.png'"` 후 `src/scripts/tf-ssh.sh --get /tmp/shot.png shot.png` | 800×1280 이미지에 위와 동일한 화면. 상단바 맨 오른쪽 아이콘 인디케이터(칩 아이콘, CORE)가 초록색(데몬 연결됨), 활성 인터페이스 아이콘도 초록색 |
| 데몬 | `src/scripts/tf-ssh.sh 'rc-service tossd status; cat /var/log/tossd.log'` | `started`, 로그에 `클라이언트 접속:` 한 줄 |
| 버튼 | 상단바의 설정 버튼 탭 | 누르는 동안 홈 안으로 들어가고, 설정 패널이 상단바 아래로 펼쳐진다. 패널이 열려 있는 동안 버튼은 반쯤 눌린 상태를 유지하며, 다시 누르면 닫힌다 |
| 서비스 | `src/scripts/tf-ssh.sh 'rc-status -a \| grep -c -E "crashed\|failed"'` | `0` |
| 시계 | `src/scripts/tf-ssh.sh date` (네트워크 연결 상태에서 부팅 30초 후) | 현재 시각, `KST` |
| 화면 상태 | `src/scripts/tf-ssh.sh 'cat /sys/class/drm/card*-DSI-1/status'` | `connected` (GPU가 `card0`, 디스플레이가 `card1`로 잡힘) |
| 터치 | `src/scripts/tf-ssh.sh 'evtest /dev/input/event0'` 후 터치 | 좌표 출력. 좌상단이 (0, 0) 근처 |
| Wi-Fi | `src/scripts/tf-ssh.sh 'ip link set wlan0 up; iw dev wlan0 scan \| grep -c "^BSS"'` | 주변 AP 개수 |
| 국가 코드 | `src/scripts/tf-ssh.sh 'iw reg get \| grep country'` | `country KR` |
| 블루투스 | `src/scripts/tf-ssh.sh 'btmgmt le on; btmgmt power on; timeout 15 btmgmt find \| grep -c dev_found'` | 0보다 큼 |
| 이더넷 | `src/scripts/tf-ssh.sh 'cat /sys/class/net/eth0/address'` | 재부팅해도 같은 MAC 주소 |
| 저장공간 | `src/scripts/tf-ssh.sh 'df -h /'` | 전체 약 11.4G, 여유 약 10.4G |
| CPU 주파수 | `src/scripts/tf-ssh.sh 'cat /sys/devices/system/cpu/cpufreq/policy4/scaling_available_frequencies'` | `408000 … 1800000` |
| 온도 | `src/scripts/tf-ssh.sh 'cat /sys/class/thermal/thermal_zone0/temp'` | 유휴 시 45000~55000 (밀리도) |
| GPU | `src/scripts/tf-ssh.sh 'dmesg \| grep panfrost; ls /dev/dri'` | `mali-t860` 인식, `renderD128` |
| 사운드 | `src/scripts/tf-ssh.sh 'speaker-test -D hw:0,0 -c 2 -t sine -f 1000 -l 1'` | 스피커에서 1kHz 톤 출력 |

SSH 접속이 안 되면 시리얼 콘솔로 확인한다: `src/scripts/console.py 'rc-status; ip addr'`

부팅 로그에 남는 무해한 메시지:

- `ilitek_ts … Wrong id: 0x80` — 부팅 직후 터치가 없는데 인터럽트가 1회 들어와 발생.
- `dw-apb-uart ff180000.serial: failed to request DMA` — 블루투스용 UART가 DMA 없이 동작.
- `Direct firmware load for brcm/brcmfmac43455-sdio.sunmi,toss-front.bin failed` — 보드 전용 펌웨어 이름을 먼저 찾고 없으면 일반 이름으로 폴백.
- `[Firmware Bug]: Kernel image misaligned at boot` — 부트로더가 커널을 권장 정렬 위치에 올리지 않음. 커널이 스스로 재배치함.

## 2.9 기기 고유 값

이미지에 구워 넣을 수 없는, 기기마다 다르거나 비밀인 파일이다. PC의 `local/device/rootfs/`에 두면 `dist/userdata.img` 생성 시 기기의 같은 경로로 들어간다(`src/scripts/make-userdata.sh`). 다시 플래싱해도 값이 유지되도록 하기 위함이다.

`local/device/`는 git이 추적하지 않으며 Nix 스토어에도 복사되지 않는다. `local/backup/`처럼 직접 백업해 둔다.

Wi-Fi 접속 정보처럼 사용자가 기기에서 설정한 값은 여기에 두지 않는다. 기기의 `/data`(`cache` 파티션)에만 저장되며 다시 플래싱해도 유지된다([03-userland-guide.md](03-userland-guide.md) 3.15절 "기기에만 남는 데이터").

| 파일 (`local/device/rootfs/` 아래) | 내용 | 없을 때 동작 |
|---|---|---|
| `etc/tossfront/eth0.mac` | 이더넷 MAC 주소. 순정 안드로이드 동작 중의 `eth0` 주소를 적어 둔다 | eMMC 고유 번호에서 파생한 주소 사용 (기기마다 고정이지만 순정 MAC과는 다름) |
| `etc/dropbear/dropbear_*_host_key` | SSH 호스트 키 | 플래싱할 때마다 새로 생성 |

파일 권한은 PC에 있는 그대로 복사되고 소유자는 root가 된다.

시각은 RTC에 저장된다. 다시 플래싱하거나 재부팅해도 유지되지만 **전원을 뽑으면 초기값(2013-01-18)으로 돌아간다.** RTC 백업 배터리가 없기 때문이다(확인됨). 시각이 틀리면 HTTPS 인증서 검증이 실패해 `apk`가 동작하지 않는다. 이를 막기 위해 `ntp.enable` 옵션을 켜 두었다(2.6절). 부팅 후 네트워크가 연결되면 몇 초 안에 시각이 동기화된다. 인터넷이 없는 환경에서는 수동으로 맞춘다: `src/scripts/tf-net.sh 'hwclock -w'` (PC 시각으로 맞춘 뒤 RTC에 기록).

## 2.10 시스템 구성 상세

위 절차가 만들어 내는 시스템의 내부 구성이다. 수정이 필요하면 이 절을 참고한다.

### 주요 설계 결정

| 결정 | 이유 | 테스트해 보지 않은 대안 |
|---|---|---|
| 커널은 메인라인 6.18 (LTS) | 순정 4.19에서는 GPU 가속(비공개 드라이버), 텍스트 콘솔(`VT` 비활성), USB 키보드 에뮬레이션(HID 가젯 미지원)이 안 되고 보안 패치도 없다. LTS 버전은 패치를 얹은 채 장기 유지하기 좋다 | |
| 부트로더는 순정 U-Boot 유지 | 건드리는 곳이 `boot` 파티션 하나라 복구가 쉽다. 메인라인 커널과 디바이스 트리를 첫 시도에 부팅시켰다 | 순정 U-Boot의 extlinux 부팅 경로, 메인라인 U-Boot로 교체(nixpkgs에 RK3399용 U-Boot와 ATF가 있음). 굳이 바꿀 필요가 없어 검증하지 않음 |
| 파티션 테이블 유지, 루트는 `userdata` | 순정 복귀 경로를 남긴다. `super`(순정 안드로이드)는 건드리지 않았다 | |
| 순정 커널을 `recovery`에 보존 | 메인라인 커널 부팅에 실패해도 복귀할 수 있다. `recovery`는 재부팅 인자로 1회성 부팅이 가능해 테스트 슬롯으로도 활용한다(2.11절) | |
| 작업 인터페이스는 Micro USB 단일화 | 시리얼 어댑터나 LAN 케이블 없이 전부 처리한다. 플래싱은 Loader/MaskROM, 접속은 USB 가젯(이더넷 + 시리얼), 부팅 실패 로그는 ramoops | |
| 이미지는 PC에서 Nix로 통째로 빌드 | 기기가 초기화돼도 저장소만 있으면 동일 상태로 재구성할 수 있다. 기기 내부에서 동작하는 구성 관리 도구는 쓰지 않는다 | |
| 화면은 Wayland 키오스크(`cage`) + 단일 앱 | 가장 가볍다. 이 앱이 상태바와 버튼 화면을 모두 맡는다 | |
| 앱은 Rust 데몬 + QML UI | 이 기기에서 Qt Quick이 가장 빨랐고([01-findings.md](01-findings.md) 1.5절), QML만 쓰면 기기 Qt 버전에 맞춰 크로스 컴파일할 필요가 없다. 데몬은 단일 정적 바이너리라 재현 가능하게 빌드하기 쉽다 | 화면 쪽에 Rust를 붙이는 CXX-Qt |

ramoops 사용 요령: 전원을 뽑으면 내용이 날아가므로 커널이 멈췄을 때는 전원을 빼지 말고 하부의 재부팅 버튼으로 재부팅한다. USB 가젯이 올라오기 전(커널 초기화 초반)에 멈추면 이것이 유일한 단서다. 부트로더 단계에서 멈추면 여기에도 남지 않는다.

### 부팅 흐름

순정 U-Boot → `boot`의 메인라인 커널 → initramfs → `userdata`의 Alpine.

- 부트로더는 순정 그대로다. 761 U-Boot는 메인라인 커널과 디바이스 트리를 문제없이 부팅시킨다.
- 부트 이미지는 안드로이드 헤더 v2 형식이다. U-Boot는 헤더의 DTB 영역이 아니라 `second` 영역의 리소스 번들(RSCE)에서 `rk-kernel.dtb`를 읽는다. `src/scripts/mkboot.sh`와 `src/scripts/rsce.py`가 이 포맷을 만든다.
- 커널 부팅 인자는 `src/os/kernel/cmdline`에 있다. 무선 국가 코드(`cfg80211.ieee80211_regdom=KR`)도 여기서 지정한다.

### initramfs (`src/os/initramfs/init`)

1. USB 가젯을 올린다: USB 이더넷(RNDIS) + 가상 시리얼(ACM). 루트 파일시스템이 망가져도 Micro USB로 접속할 수 있게 가장 먼저 올린다.
2. `userdata` 파티션을 찾아 마운트한다. GPT 이름이 `userdata`인 파티션 중 첫 번째이며, 장치가 잡힐 때까지 최대 10초 기다린다.
3. 이더넷 MAC 주소를 `/etc/tossfront/eth0.mac` 값으로 설정한다.
4. Alpine init으로 제어권을 넘긴다.

마운트에 실패하거나 부팅 인자에 `tf.rescue`가 있으면 USB 시리얼에 복구 셸을 띄운다.
Wi-Fi·블루투스 펌웨어와 무선 규제 DB도 initramfs에 들어 있다. 드라이버가 커널에 내장돼 있어 루트 마운트 전에 펌웨어를 로드하기 때문이다. initramfs는 `nix/build-initramfs.sh`가 만든다.

### 커널 설정 (`src/os/kernel/toss-front.config`)

arm64 `defconfig`에서 다른 SoC 지원을 모두 끄고 모듈 빌드를 없앤 뒤 이 설정 조각을 덮어씌운다. 커널 모듈 없이 전부 내장(built-in)한다.

| 분류 | 내용 |
|---|---|
| 부팅 | initramfs, ext4, eMMC |
| 디버깅 | ramoops, 패닉 시 자동 재부팅, 행 감지, 워치독 드라이버, 재부팅 모드(`reboot-mode`) |
| 전원 | RK808, SYR82x, 고정 레귤레이터, IO 전압 도메인, TPS65132 |
| CPU·GPU | 주파수 스케일링(`cpufreq-dt`, 기본 거버너 `schedutil`), 온도 센서와 서멀 스로틀링, GPU 주파수 제어, Panfrost |
| USB | DWC3 디바이스 모드, configfs 가젯(ACM, RNDIS, NCM, ECM, Mass Storage, FunctionFS, HID) |
| 화면 | Rockchip DRM, VOP, DSI, 커스텀 패널 드라이버, PWM 백라이트, 텍스트 콘솔 |
| 입력 | evdev, Ilitek 터치, ADC 키, uinput |
| 네트워크 | 이더넷(STMMAC, RTL8201 PHY), SDIO, Wi-Fi(`brcmfmac`), 블루투스(HCI UART, BCM), IPv6, SYN 쿠키 |
| 기타 | RK808 RTC |
| 오디오 | ALSA SoC, Rockchip I2S, ES8316 코덱, `simple-audio-card`, `simple-audio-amplifier` |
| 비활성화 | 미디어, PCI, ACPI, EFI, 가상화 |

### 디바이스 트리 (`src/os/kernel/dts/rk3399-toss-front.dts`)

파라미터 출처는 순정 커널이 쓰던 디바이스 트리다([01-findings.md](01-findings.md) 1.3절).

| 노드 | 비고 |
|---|---|
| `/memory` | 메인라인 dtsi에 없어 순정과 동일한 범위를 직접 기술. 보안 펌웨어 영역 회피 |
| ramoops | 순정과 동일한 메모리 위치. 실패한 부팅 로그를 다음 부팅에서 확인 |
| `adc-keys` | 순정 U-Boot가 초기화 버튼을 읽는 데 사용. 빠지면 버튼으로 Loader에 못 들어갈 수 있음 |
| RK808 전 레일, SYR827, SYR828 | 순정 전압 범위, 전부 `always-on` |
| CPU 코어 6개 | 리틀 코어 4개는 `vdd_cpu_l`, 빅 코어 2개는 `vdd_cpu_b`에 연결. OPP 테이블은 메인라인 `rk3399.dtsi` 그대로 사용 |
| GPU | `vdd_gpu`에 연결 |
| 온도 센서 | 과열 차단은 순정과 같은 방식(핀으로 PMIC에 신호, 극성 1) |
| 오디오 | I2S1 → ES8316(`everest,es8316`, I2C1 `0x10`) → 스피커 앰프(`simple-audio-amplifier`, enable 핀 gpio0-12). 순정에서는 코덱 드라이버가 앰프 핀을 직접 제어했지만 메인라인에는 해당 기능이 없어 앰프를 별도 노드로 분리. 구성은 같은 코덱을 쓰는 Pinebook Pro 참고 |
| `iov5`, `vbus` | 순정에서 SUNMI 전용 드라이버로 켜던 전원 스위치 |
| `reboot-mode` | 메인라인 dtsi에 없어 추가. `reboot-mode loader`가 이 노드로 동작 |
| USB | 디바이스 모드 고정, USB 2.0 |
| 바이어스 전원 | `ti,tps65132`로 전압(±5.8V)만 설정하고, enable 핀은 별도 고정 레귤레이터 `lcd_bias_en`으로 구동. 드라이버가 enable 핀을 출력 모드로 전환하지 못하기 때문 |
| VOP big → DSI → 패널, 백라이트 | 백라이트는 PWM0, 주기 40000ns |
| 터치 | `ilitek,ili2130`, X축·Y축 모두 반전 |
| 이더넷 | RMII, 클럭 출력, PHY 리셋 gpio3-16 |
| Wi-Fi | SDIO, 전원 시퀀스(32kHz 클럭 + WL_REG_ON), host-wake |
| 블루투스 | UART0(흐름 제어), `brcm,bcm4345c5` |

### 패널 드라이버 (`src/os/kernel/drivers/`)

- `panel-sunmi-toss-front.c`: compatible 문자열 `sunmi,toss-front-m13`. 순정과 같은 전원 시퀀스로 켜고 초기화 명령 시퀀스를 전송한다. 마지막에 패널 전원 레지스터를 읽어 `0x9c`가 아니면 경고를 띄운다.
- `panel-sunmi-toss-front-init.h`: 순정 디바이스 트리의 `mipi_lcd_23` 초기화 명령 199개. `src/scripts/gen-panel-init.py`로 생성한다.
- 빌드 시 커널 트리에 복사하고 Kconfig와 Makefile에 등록한다(`src/os/kernel/kbuild.sh`). 커널 소스를 패치하는 부분은 여기뿐이다.

### Alpine 루트 (`nix/build-rootfs.sh`, `src/os/rootfs/`, `src/os/alpine/`)

공식 aarch64 minirootfs에 버전 고정한 패키지를 설치하고 `src/os/rootfs/` 파일을 덮어씌운다(`toss.enable`이면 `src/app/rootfs/`와 `src/app/ui/`도 포함). 복사되는 파일 권한은 디렉터리 755, 실행 비트가 있는 파일 755, 나머지 644로 맞춘다. 실행해야 하는 파일에는 git에 실행 비트가 들어가 있어야 한다.

| 항목 | 내용 |
|---|---|
| 패키지 | `src/os/alpine/packages`에 선언. 기본 시스템, SSH, 네트워크(`iw`, `wpa_supplicant`, `dhcpcd`, `bluez`), 오디오(`alsa-utils`), 그래픽(Mesa, `kmscube`), 화면(`cage`, `seatd`, `eudev`, `foot`, 폰트 — DejaVu 및 한·중·일 Noto CJK), Qt Quick 및 온스크린 키보드(스트림덱 UI용), 진단 도구(`evtest`, `grim`). lock 파일의 200개 중 199개가 루트에 설치된다(나머지 1개 `busybox-static`은 initramfs에만 들어감) |
| USB 이더넷 | 기기 `172.16.42.1`. `busybox-extras` DHCP 서버가 PC에 `172.16.42.2`를 할당한다. 게이트웨이와 DNS는 내려주지 않는다 |
| SSH | dropbear. `buildImage`의 `ssh.lan` 옵션이 `true`면 모든 인터페이스에서, `false`면 `172.16.42.1:22`에서만 수신(현재 `true`). 덱 설정에서 비활성화해 둔 경우(`/data/toss/settings.json`의 `"ssh": false`) SSH 서버가 뜨지 않는다 — `/etc/conf.d/dropbear`에 넣은 체크가 시작 전에 이를 확인한다 |
| IP 할당 | `dhcpcd`가 유선(`eth0`)과 무선(`wlan0`)에서 자동으로 받는다 |
| 로그인 프롬프트 | USB 시리얼(`ttyGS0`). `kiosk.enable`이 꺼져 있으면 화면에도 뜬다(`tf-console-getty`) |
| 화면 앱 | `kiosk.enable`이면 `/usr/sbin/tf-kiosk`가 `cage`(Wayland 키오스크 컴포지터) 위에서 `kiosk.command`를 실행한다. 사용자 `kiosk`(`seat`, `video` 그룹)로 돌고 종료되면 재시작한다(로그 `/var/log/kiosk.log`). 순정 커널로 부팅한 경우에는 아무것도 하지 않는다 |
| 스트림덱 앱 | `toss.enable`이면 데몬 `/usr/bin/tossd`(Rust 정적 바이너리, 사용자 `tossd`)와 root 헬퍼 `toss-helper`(전원, 절전, SSH 제어)를 서비스로 띄우고, QML 파일은 `/usr/share/toss/ui`에, 실행 스크립트는 `/usr/bin/toss-shell`에 둔다. 데몬 서비스는 `tf-data`(아래 "기기에만 남는 데이터") 이후에 시작한다. 구조는 [03-userland-guide.md](03-userland-guide.md) 3.16절 참고 |
| 장치 관리 | `kiosk.enable`이면 `udev`(터치 입력을 읽는 libinput이 요구)와 `seatd`(디스플레이·입력 장치를 컴포지터에 전달)를 켠다 |
| 첫 부팅 | 루트 파일시스템을 파티션 크기에 맞게 확장 |
| 사운드 | `alsa` 서비스가 믹서 상태(`/var/lib/alsa/asound.state`)를 복원. 기준 테스트음 `/usr/share/sounds/tossfront/reference.wav` |
| Wi-Fi | 부팅 시 `wpa_supplicant`를 항상 실행한다(`30-wifi.start`). 설정 파일이 없으면 빈 파일을 만들고 제어 소켓을 `tossd` 그룹에 연다. AP 선택과 비밀번호 입력은 스트림덱 설정 화면에서 처리한다. 설정 파일은 `/data/wifi/`에 저장된다 |
| 기기에만 남는 데이터 | 서비스 `tf-data`가 부팅 시 `cache` 파티션을 `/data`에 마운트한다. 플래싱 대상이 아니므로 이미지를 다시 구워도 유지된다. 앱 데이터는 `/data/toss/`, Wi-Fi 설정은 `/data/wifi/`. 파티션을 찾지 못하거나 마운트 실패 시 `/data`는 루트 파일시스템 위 디렉터리로 남는다(다시 구우면 초기화됨). 어느 경우든 `/data/toss/`는 `tf-data`가 생성해 데몬 사용자 소유로 넘긴다(데몬은 직접 만들 권한이 없다) |
| 모듈 디렉터리 | `/lib/modules/<커널 버전>/modules.builtin`. 모듈이 없는 커널이라 이 파일이 없으면 부팅 서비스가 에러를 낸다 |
| 펌웨어 | `/lib/firmware/brcm/` (순정 이미지에서 추출) |
| 유틸리티 | `reboot-mode`, `i2c-read` (`src/os/tools/`, libc 없이 빌드) |
| 시계 | `ntp.enable`이면 busybox `ntpd` 서비스를 켠다. 시간대는 `/etc/localtime`(nixpkgs tzdata에서 복사). RTC는 UTC 기준 |
| 이미지 크기 | 내용의 1.25배 + 64MB를 16MB 단위로 올림한 크기(현재 768MB). 첫 부팅 시 파티션 끝까지 확장된다 |

### 저장소 구조

| 경로 | 내용 | git |
|---|---|---|
| `flake.nix`, `flake.lock` | 이미지 옵션(`buildImage` 호출)과 개발 툴체인 devshell | 추적 |
| `.cargo/config.toml` | `cargo` 출력을 `dist/cargo/`로 보내는 설정. Nix 빌드에는 영향이 없다 | 추적 |
| `nix/` | 이미지 빌드 정의. `build.nix`가 `buildImage` 함수를 제공하고, 입력 재료는 `inputs.nix`(Alpine 패키지, 순정 추출물), 커널과 디바이스 트리는 `kernel.nix`에 정의(`build-rootfs.sh`, `build-initramfs.sh`가 루트와 initramfs 생성). PC 프로그램 패키지 `toss-connector.nix`, NixOS 모듈 `nixos-module.nix`, 모듈 테스트 `module-check.nix`도 여기 포함 | 추적 |
| `src/os/` | initramfs, 루트 설정, 패키지 목록과 lock 파일, 커널 설정·디바이스 트리·드라이버, 기기용 유틸리티 | 추적 |
| `src/app/` | 스트림덱 앱: 데몬(`tossd/`, Rust), UI(`ui/`, QML), 기기 배치 파일(`rootfs/`), 덱–PC 프로토콜(`proto/`), 리소스 측정(`meter/`), PC 커넥터(`host/`) | 추적 |
| `src/scripts/` | 준비·플래싱·접속 스크립트. 공통 유틸리티(기기 주소와 SSH 옵션, 파티션 테이블, 플래싱 도구 래퍼, usage 출력, 확인 프롬프트)는 `lib.sh`에 있음 | 추적 |
| `docs/` | 문서 | 추적 |
| `local/archive/` | 버전 고정한 Alpine 패키지 파일. **백업 대상** | 제외 |
| `local/device/` | 기기 고유 설정. **백업 대상** | 제외 |
| `local/backup/`, `local/downgrade/`, `local/loader/` | 기기 이미지 | 제외 |
| `dist/` | 빌드 결과 링크, 플래싱용 이미지, 증분 빌드용 커널 소스, `cargo` 출력. 지워도 다시 생성 가능 | 제외 |
| `local/analysis/`, `local/lineage/` | 조사 당시 원자료. 재현에는 불필요 | 제외 |

| 스크립트 | 용도 |
|---|---|
| `lib.sh` | 다른 스크립트가 source해서 쓰는 공통 라이브러리(직접 실행 안 함): 기기 주소와 SSH 옵션(`TF_HOST`), 파티션 테이블, 플래싱 도구 호출 및 되읽기 검증 함수, usage 출력, 확인 프롬프트 |
| `fetch-loader.sh` | RK3399 로더 다운로드 (SHA-256이 맞을 때만 저장. 기존 파일 해시가 다르면 다시 받음) |
| `backup.sh [디렉터리]` | 전체 파티션 백업. 기본 경로는 저장소의 `local/backup/`. Read LBA 가능한 상태에서만 시작 |
| `downgrade.sh [-y] [디렉터리]` | 순정 761 플래싱 (크기와 MD5 검증 → 1회 확인 질문 → `flash.sh`로 7개 파티션 기록). `-y`로 질문 생략 |
| `stock-to-store.sh`, `extract-firmware.sh` | 순정 이미지에서 추출한 재료를 Nix 스토어에 등록 |
| `alpine-lock.sh` | Alpine 패키지 lock 갱신(`update`), 아카이브에서 복원(`restore`), 아카이브 무결성 확인(`check`) |
| `build-all.sh` | 이미지 빌드 (`nix build` + 기기 고유 값 반영) |
| `make-userdata.sh` | 공통 루트 이미지에 `local/device/rootfs/`를 얹어 `dist/userdata.img` 생성 |
| `deploy.sh [-y] [boot] [recovery] [userdata]` | 빌드부터 플래싱·재부팅까지 일괄 실행. 1회 확인 질문(`-y`로 생략) |
| `app-push.sh` | 스트림덱 앱을 이미지를 굽지 않고 기기에 바로 올려서 테스트 |
| `tf-tap.sh`, `tf-drag.sh` | 수동 조작 없이 화면 탭·드래그 (터치 입력 장치에 이벤트 주입. UI 동작 테스트용) |
| `flash.sh [-y] <파티션> <이미지>` | 단일 파티션 기록 + 되읽기 검증. `boot`·`recovery`·`userdata` 외 파티션은 확인 질문(`-y`로 생략). `--enter`, `--reboot` |
| `mkboot.sh`, `rsce.py` | 부트 이미지와 리소스 번들 패키징. Nix 빌드가 이 둘만 복사해 실행하므로 `lib.sh`에 의존하지 않음 (`mkboot.sh -p boot\|recovery`로 대상 파티션을 주면 크기 검사 수행) |
| `build-kernel.sh`, `build-boot.sh` | 커널 수정 중 쓰는 증분 빌드와 테스트용 부트 이미지 생성 (2.11절) |
| `tf-ssh.sh`, `tf-net.sh`, `console.py` | 기기 접속(명령 실행, `--put`, `--get`, `--reboot [normal\|loader\|recovery]`), PC 인터넷 공유, 시리얼 콘솔(명령 실행, `--put`). 앞의 둘은 `TF_HOST=<주소>`를 주면 LAN으로 접속 |
| `gen-beep.py` | 기준 테스트음 생성 (빌드 중 호출) |
| `gen-panel-init.py`, `dt-show.py`, `extract-dtb.py`, `parse-touch.py` | 조사·분석용 |
| `flash-lineage.sh [-y] [디렉터리]` | LineageOS 설치 (조사 당시 사용). MD5 검증 → 1회 확인 질문 → `flash.sh`로 3개 파티션 기록. `flash.sh --enter` 상태에서만 동작하며 항상 되읽기 검증. `CHECK_ONLY=1`이면 파일 검증만 수행 |

## 2.11 수정 후 재배포

한 번 환경을 올린 뒤에는 케이블이나 버튼 조작 없이 반복 개발할 수 있다. 저장소를 수정한 뒤 `src/scripts/deploy.sh`를 돌리면 된다.

| 수정 대상 | 명령 | 비고 |
|---|---|---|
| 커널 설정, 드라이버, 디바이스 트리, 부팅 인자 | `src/scripts/deploy.sh boot` | 설정이나 드라이버를 고치면 커널 전체를 다시 빌드한다(약 3분). 디바이스 트리와 부팅 인자만 바꿨으면 금방 끝난다 |
| initramfs (`src/os/initramfs/init`) | `src/scripts/deploy.sh boot recovery` | 두 커널이 같은 initramfs를 공유한다 |
| 패키지 목록 (`src/os/alpine/packages`) | `src/scripts/alpine-lock.sh update` → `src/scripts/deploy.sh userdata` | **기기에서 직접 바꾼 파일은 모두 초기화된다** |
| 루트 설정 (`src/os/rootfs/`), 이미지 옵션 (`flake.nix`의 `buildImage` 인자), 기기 고유 값 (`local/device/rootfs/`) | `src/scripts/deploy.sh userdata` | 위와 동일 |
| 스트림덱 앱 (`src/app/`) | `src/scripts/deploy.sh userdata` | 위와 동일. 개발 중에는 `src/scripts/app-push.sh`로 플래싱 없이 테스트 가능 |

커널 설정을 바꾸면 루트 이미지도 다시 빌드된다(내장 모듈 목록이 루트에 들어가기 때문). 이때는 `src/scripts/deploy.sh`로 전체를 올리는 편이 맞다.

### 커널 수정 중 증분 빌드

커널을 조금 고칠 때마다 전체를 새로 빌드하기 부담스러우면 증분 빌드를 쓴다. 절차는 정식 빌드와 같은 스크립트(`src/os/kernel/kbuild.sh`)를 쓰고, 풀어 둔 소스(`dist/linux`)에서 변경된 부분만 다시 컴파일한다.

```bash
src/scripts/build-kernel.sh config
```

```bash
src/scripts/build-kernel.sh all
```

```bash
src/scripts/build-boot.sh
```

결과물은 `dist/boot-dev.img`다. `recovery` 파티션에 올려 1회만 부팅해 보면 `boot`를 건드리지 않고 테스트할 수 있다.

```bash
src/scripts/tf-ssh.sh --reboot loader
```

```bash
src/scripts/flash.sh --enter
```

```bash
src/scripts/flash.sh recovery dist/boot-dev.img
```

```bash
src/scripts/flash.sh --reboot
```

```bash
src/scripts/tf-ssh.sh --reboot recovery
```

테스트가 끝나면 `src/scripts/deploy.sh boot recovery`로 정식 이미지를 올린다. 기기에 최종적으로 남는 것은 정식 빌드여야 한다.

부팅에 실패하는 커널을 올렸을 때 복구 방법:

- 전원을 뽑았다가 초기화 버튼을 누른 채 꽂으면 Loader 모드로 들어간다. 직전에 잘 뜨던 이미지를 `boot`에 다시 쓴다.
- 재부팅 후 `/sys/fs/pstore/`에 직전 커널 로그가 남는다. 전원을 차단하면 날아가므로, 멈춘 기기는 하부의 재부팅 버튼으로 재부팅한다.
- 순정 커널로 한 번 부팅해서 비교하려면 `src/scripts/tf-ssh.sh --reboot recovery`를 실행한다. 다음 재부팅 때는 다시 메인라인으로 뜬다.

## 2.12 검증 범위

2026-11-15~18에 진행한 작업이며, 실제로 검증한 범위와 방법을 구분해 정리한다. "직접 확인"은 사람이 화면을 보거나 소리를 듣거나 터치해서 확인한 항목이다.

### 절차

| 단계 | 검증 방법 |
|---|---|
| 2.3 진입 | 패드 쇼트, 버튼, `flash.sh --enter` 모두 이 기기에서 실행 (작업 초기에 1회) |
| 2.4 백업 | 이 기기에서 실행. 핵심 8개 파티션은 다시 읽어 대조 (작업 초기에 1회) |
| 2.5 다운그레이드 | 이 기기에서 실행. 버튼 Loader 진입 복구 확인 (작업 초기에 1회) |
| 2.6 빌드 | Nix 스토어가 정리돼 재료와 중간 결과가 전부 사라진 상태에서 `build-all.sh` 실행 → 재료를 아카이브에서 자동 복원, 커널부터 전부 재빌드 → 이미지 셋의 SHA-256이 이전과 동일 |
| 2.7 기록 | `src/scripts/deploy.sh`로 세 파티션을 통째로 기록(빌드 → Loader 진입 → 기록·검증 → 재부팅까지 한 번에). 순정 커널로 부팅한 상태에서 실행해도 동작 |
| 2.8 확인 | 새로 플래싱한 기기에서 아래 "기능" 표의 항목을 확인 |
| 2.9 기기 고유 값 | 새 디렉터리, 600 권한 파일, 기존 파일 덮어쓰기를 이미지에 넣고 권한·소유자 확인, 파일시스템 검사 통과. 순정 MAC 파일이 플래싱 후 적용되는 것 확인 |

### 재현성

| 확인한 것 | 검증 방법 |
|---|---|
| 같은 재료에서 같은 결과 | 커널, 디바이스 트리, initramfs, 루트 이미지, 부트 이미지 2종, 데몬을 각각 두 번 빌드해 바이트 단위로 동일한지 확인 (`nix build --rebuild`) |
| 기기가 빌드 결과와 동일 | 플래싱 후 기기의 데몬·QML 파일 해시가 빌드 결과와 동일. 설치된 패키지 목록이 이미지의 목록과 동일 |
| 옵션 구조 | 알 수 없는 옵션을 주면 빌드 전에 에러. 옵션을 바꿔도 커널은 재빌드되지 않음 |
| 증분 빌드 | 증분 빌드의 디바이스 트리가 정식 빌드와 바이트 단위로 동일 |

### 기능

| 기능 | 검증 방법 |
|---|---|
| 부팅 | 메인라인 커널로 부팅, 서비스 실패 0, 부팅 에러 메시지 없음(2.8절의 무해한 메시지 제외). 루트가 첫 부팅에 약 11GB로 확장 |
| 복귀용 순정 커널 | `--reboot recovery`로 부팅(4.19.193), 서비스 실패 0, 다음 재부팅에 메인라인으로 복귀. 이때는 화면 앱을 띄우지 않음 |
| 화면 | 화면 캡처에 800×1280 전체가 그려짐. 실제 화면에 보이는 것은 직접 확인 |
| 터치 | 터치한 자리에 점을 찍는 테스트 앱으로 네 귀퉁이와 선 긋기 → 화면 캡처에 터치한 위치대로 찍힘 (직접 확인) |
| GPU | `kmscube`와 컴포지터, Qt Quick이 `Mali-T860 MC4 (Panfrost)`로 렌더링 |
| CPU 주파수 | 리틀 코어 408MHz~1.416GHz, 빅 코어 408MHz~1.8GHz, 부하에 따라 전압이 함께 변함(0.825~1.2V). 100MB 해시 계산이 0.92초(816MHz 고정) → 0.53초 |
| 발열 | 6코어 전부하 90초 동안 멈춤·에러 없음. 15초 만에 약 70°C, 이후 스로틀링하며 72°C에서 유지. 부하를 빼면 20초 뒤 55°C |
| 이더넷 | 공유기에 연결: 100Mbps 전이중, DHCP, 공유기 ping 0.8ms, 인터넷 ping 3.7ms, 손실 0, 3.8MB를 HTTPS로 0.47초, 에러 카운터 0. 재부팅해도 같은 MAC과 IP. USB 이더넷은 영향 없음 |
| SSH | USB IP와 LAN IP 양쪽으로 접속 |
| Wi-Fi | 주변 AP 검색, 국가 코드 KR. 화면에서 WPA2 AP(2.4GHz, 5GHz)에 접속해 IP를 받음(직접 확인: 비밀번호 입력). 화면에서 삭제도 동작(직접 확인) |
| 블루투스 | 주변 기기 검색 |
| 사운드 | 1kHz 테스트음이 들림(직접 확인). 믹서 설정이 부팅 시 복원됨. 기본 음량은 기준 테스트음을 다섯 단계로 듣고 정함(직접 확인) |
| 시계 | RTC가 2013년인 상태로 부팅 → 약 36초 시점에 PC와 초 단위로 일치, KST 표시. 약 2분 뒤 RTC에도 기록되고, 재부팅 시 커널이 맞는 시각을 읽어 옴. 순정 커널에서도 동작 |
| 화면 앱 실행 | `cage`와 앱이 사용자 `kiosk`로 자동 실행. 앱을 죽이면 약 5초 안에 다시 뜸 |
| 스트림덱 앱 | 데몬이 서비스로(사용자 `tossd`) 자동 실행, UI가 떠서 한 번에 접속, 데몬이 보낸 상태와 위젯이 그려짐. 데몬을 죽이면 UI에 안내가 뜨고 다시 켜면 재접속. 화면을 터치해 페이지를 넘김(직접 확인. 스타일 적용 전의 UI에서) |
| 전역 스타일과 폰트를 넣은 이미지 | `deploy.sh userdata`로 플래싱해 부팅 확인(2026-11-16): 서비스 실패 0, 데몬 실행, UI가 한 번에 접속, 한·중·일 폰트 15종 인식, 화면 캡처에 랙과 위젯이 정상 |
| 스트림덱 앱의 스타일 | 라이트 테마와 다크 테마, 랙과 복합 위젯, 한·중·일 글자를 화면 캡처로 확인. 모양(버튼 높이, 디스플레이 깊이, 모서리, 여백)은 기기에서 보고 정함(직접 확인). 유휴 시 UI 프로세스의 CPU 사용 약 1.5% (코어 하나 기준) |
| 코드를 모듈로 다시 나눈 뒤 | UI(뼈대·부품·위젯)와 데몬(모듈 넷)을 나눈 뒤 기기에 올려, 화면 캡처가 나누기 전과 같은 것을 확인. 데몬 테스트 4개 통과(규칙에 어긋난 레이아웃을 거르는 것 포함). 이어서 `deploy.sh userdata`로 플래싱해 부팅 확인: 서비스 실패 0, 데몬 실행, UI가 한 번에 접속, 기기의 UI 파일 29개가 저장소와 해시 일치 |
| 상단바의 설정 버튼 | 상단바를 눌러 테마를 바꾸던 임시 기능을 빼고 설정 버튼을 넣은 뒤 `app-push.sh ui`로 올려(플래싱하지 않음) 화면 캡처로 모양과 위치를 확인. 기기에서 버튼이 눌리면 `kiosk.log`에 로그가 남는 것을 확인(눌림 2건) |
| 상단바의 아이콘 인디케이터 | 라벨이 붙은 인디케이터 다섯 개를 아이콘 인디케이터로 바꾼 뒤 `app-push.sh ui`로 올려(플래싱하지 않음) 화면 캡처로 확인: USB·LAN이 초록, WLAN·BT가 꺼짐, CORE가 초록. 데몬을 멈추면 CORE가 주황으로 깜빡이고 다시 켜면 초록으로 돌아옴 |
| 설정 패널 | `app-push.sh ui`로 올려(플래싱하지 않음) `src/scripts/tf-tap.sh`로 터치를 주입해 확인: 설정 버튼을 누르면 패널이 열리고(화면 캡처) 버튼이 눌린 상태를 유지, 다시 누르면 닫힘. 펼쳐지는 움직임과 사람이 손으로 누르는 것은 확인하지 않음 |
| 테마 전환 | `app-push.sh ui`로 올려(플래싱하지 않음) `tf-tap.sh`로 설정 패널의 `Dark mode` 줄을 눌러 확인: 화면 전체(상단바, 아이콘 인디케이터, 설정 패널, 랙과 위젯)가 다크 테마로 바뀌고 토글 핸들이 오른쪽으로 가며 바닥에 초록색이 들어옴(화면 캡처). 다시 누르면 라이트 테마로 돌아옴. 토글이 움직이는 모습과 사람이 손으로 누르는 것은 확인하지 않음 |
| Wi-Fi 설정과 온스크린 키보드 | 기기에 테스트로 올려(플래싱하지 않음. 키보드 패키지는 아카이브의 파일을 기기에 직접 설치) `tf-tap.sh`로 눌러 확인: 설정 → `Wi-Fi`에 주변 AP 10여 개가 신호 순으로 나옴, AP를 누르면 접속 화면과 키보드가 올라옴, 글자·시프트·기호 배열이 입력되고 `Show password`로 가림/보임이 전환됨, 틀린 비밀번호로 `CONNECT` → `Connecting…` → 약 20초 뒤 `Could not connect. Check the password`, 그 AP는 저장되지 않음, 로그에 비밀번호 없음. 데몬 테스트 9개 통과(가짜 `wpa_supplicant`로 접속 흐름 확인 포함). 맞는 비밀번호로 접속하는 것은 아래 줄에서 확인 |
| 기기에만 남는 데이터 (`/data`) | 서비스와 설정을 기기에 직접 넣어(플래싱하지 않음) 확인: `cache` 파티션이 원래의 ext4 그대로 `/data`에 마운트됨, `wpa_supplicant`가 `/data/wifi/wpa_supplicant.conf`를 씀, 테스트용 AP를 저장하고 재부팅하니 `tf-data`가 `boot` 런레벨에서 `/data`를 마운트했고 그 AP가 남아 있음(확인 후 삭제), 저장할 때 파일 권한이 600으로 유지됨, 데몬이 제어 소켓에 계속 연결됨. 플래싱 후에도 남는 것은 아래 줄에서 확인 |
| Wi-Fi 접속과, 플래싱 후에도 남는 것 | 기기 화면에서 AP를 고르고 비밀번호를 입력해 접속(직접 확인. WPA2, 2.4GHz): 접속 완료, `dhcpcd`가 IP를 받음, 접속 정보가 `/data/wifi/`에 저장됨. 이어서 `deploy.sh userdata`로 플래싱해 부팅 확인(2026-11-16): 서비스 실패 0, `tf-data`가 `/data`를 마운트, 저장한 AP가 남아 있고 자동으로 재접속해 같은 IP를 받음, 상단바의 WLAN 아이콘이 켜짐, 온스크린 키보드 패키지가 이미지에 포함됨, 기기의 UI 파일 40개와 시스템 파일 4개가 저장소와 해시 일치 |
| 인터페이스 정보 시트 | `app-push.sh all`로 올려(플래싱하지 않음) `tf-tap.sh`로 상단바의 아이콘 다섯 개를 차례로 눌러 화면 캡처로 확인: USB(Configured, 172.16.42.1/24), 이더넷, Wi-Fi(AP 이름, 신호, 대역, 보안 방식, IP, MAC), 블루투스, 데몬(버전, 소켓, 호스트명). 시트 바깥을 누르면 닫히고 그 터치가 아래 버튼에 전달되지 않음(처음에는 전달됐고, 덮개를 `MouseArea`로 바꿔 수정). 데몬 테스트 9개 통과 |
| Wi-Fi의 SDIO 클럭을 낮춘 이미지 | `deploy.sh boot userdata`로 플래싱해 부팅 확인(2026-11-16): SDIO 클럭이 50MHz로 적용됨, 부팅 중 버스 에러 0건(전에는 부팅 12초쯤에 1건씩), 서비스 실패 0, Wi-Fi가 저장된 정보로 자동 접속, `arping`으로 무선 통신 확인, 기기의 UI 파일 41개가 저장소와 해시 일치. 칩에 질의를 0.2초 간격으로 3000번 보내는 테스트(약 11분)에서 버스 에러 0건 — 같은 테스트가 150MHz에서는 350번 안에 실패했다 |
| 호스트 연결과 페어링 | 기기에 테스트로 올려(플래싱하지 않음) PC의 `toss-connector`와 연동해 확인: 덱 설정에서 `PAIR NEW DEVICE` → PC에서 `toss-connector pair` → 양쪽에 같은 숫자 → 덱에서 `ACCEPT`(터치 입력), PC에서 `y` → 양쪽에 저장됨. `toss-connector`를 띄우면 연결되고, PC의 Spotify 재생 정보가 미디어 제어 위젯에 표시됨. 데몬을 재시작해도, 기기를 재부팅해도 페어링이 유지되고 PC가 스스로 재연결. 덱의 이더넷 IP로도 같은 덱으로 식별. 덱의 USB 네트워크를 내리니 PC가 이더넷으로 재연결(약 4초). 테스트: 프로토콜 5개, 데몬 10개(페어링 전체 흐름 포함) 통과. **이 줄에서 확인하지 못한 것**: Wi-Fi 경로, 여러 호스트 동시 연결. 덱의 키로 재생을 제어하는 것은 아래 "미디어 제어 위젯의 키 일곱 개" 줄에서(재생/일시정지와 곡 넘기기 키는 누르지 않았다), 플래싱 후에도 페어링이 유지되는 것은 아래 "구조 정리 후의 이미지 전체" 줄에서 확인 |
| 호스트가 허용하는 권한 | 기기에 테스트로 올려(플래싱하지 않음) 확인: PC에서 `toss-connector deny media.control` → 연결돼 있던 `toss-connector`가 재시작 없이 따라가고, 덱의 미디어 위젯 버튼이 흐려져 눌리지 않음. `deny all` → 곡 정보가 지워지고 `Not shared`. `allow`로 되돌리니 복구됨. 덱 설정 → 해당 기기의 `Allows` 줄도 함께 바뀜. 해제한 권한의 동작을 요청받으면 거부하는 것, 알 수 없는 동작을 하지 않는 것, 페어링하지 않은 상대에게 아무것도 보내지 않는 것은 테스트로 확인(호스트 8개) |
| 이벤트 기반 재생 정보와 앨범 아트 | 기기에 테스트로 올려 확인: Spotify를 재생하는 동안 PC는 재생 정보를 처음 한 번만 보냈고(45초 동안 다시 보내지 않음), 덱의 위치 표시는 스스로 흘러가며 실제 플레이어와 1초 이내로 일치했다. 곡이 바뀌자 새 정보와 앨범 아트가 왔다. 앨범 아트는 `https` 주소에서 받아 512px로 줄여 43~74KB, 덱의 미디어 위젯에 표시됐다. 일시정지·시크·앨범 아트 조각·위치를 알려 주지 않는 플레이어는 테스트로 확인(호스트 19개, 덱 데몬 12개, 프로토콜 8개) |
| 미디어 제어 위젯의 키 일곱 개 | 기기에 테스트로 올려 `tf-tap.sh`로 눌러 확인: 5초 뒤로·앞으로(Spotify의 위치가 그만큼 바뀜. 처음보다 앞이면 처음으로), 반복(누를 때마다 `None` → `Playlist` → `Track` → `None`), 셔플(켜짐·꺼짐). 켜진 키는 눌린 상태를 유지하고(인디케이터는 두지 않는다), 한 곡 반복은 아이콘에 `1`이 들어감(화면 캡처로 확인) |
| 레이아웃 편집 | 기기에 테스트로 올려 `tf-tap.sh`와 `tf-drag.sh`로 확인(화면 캡처와 데몬 로그): 설정 → `Edit layout`으로 진입, 위젯을 드래그해 옮김(드래그하는 동안 놓일 자리가 초록으로 표시됨), 가장자리에 머물러 새 페이지로 옮김, `+`로 미디어 위젯(PC를 골라서)과 랙을 추가, 랙 라벨을 눌러 이름을 변경(온스크린 키보드), 핸들로 랙을 2×1 → 4×1로 늘림, `×`로 삭제, `DONE`으로 나오면 빈 페이지가 사라짐, `Reset layout`으로 초기 레이아웃으로 돌아감. 편집할 때마다 `/data/toss/layout.json`에 저장됨. 겹치는 자리로 옮기기·잘못된 파일 치워 두기·재시작하면 저장된 레이아웃으로 시작하는 것은 테스트로 확인(덱 데몬 20개) |
| 명령 버튼 | 기기와 PC에 테스트로 올려 확인: PC에서 `toss-connector command add`로 명령을 등록 → 덱의 편집 모드에서 랙 빈 슬롯의 `+` → `Command` → `Run` → PC를 고르니, 허용하기 전에는 "No commands offered", PC에서 `toss-connector allow command.run`을 실행하자 재시작 없이 그 명령이 목록에 나옴 → 색과 라벨을 골라 추가 → 버튼을 누르니 PC에서 명령이 실행됨(파일이 생성되고 호스트 로그에 남음) → 권한을 해제하니 버튼이 흐려지고 눌러도 실행되지 않음. 등록하지 않은 id와 커맨드 그대로는 실행되지 않는 것은 테스트로 확인(호스트 22개, 덱 데몬 21개) |
| 리소스 미터 | 기기와 PC에 테스트로 올려(플래싱하지 않음) 확인: 덱 자체를 보는 리소스 미터(2×1)에 온도·메모리·로드가 표시됨. 편집 모드의 `+` → `Resource monitor`에 크기 넷이 나오고, 대상에 `This deck`과 PC가 나옴. PC를 고르니 그 PC의 리소스 41가지가 나왔고 열둘을 눌러(누른 순서가 숫자로 표시됨) 제목 줄의 `ADD`로 4×2를 배치 → 두 단에 여섯 줄씩, 1~2초 뒤 PC의 값이 들어오고 1초마다 바뀜. PC에서 `resource.info`를 해제하니 흐려지고 `--`, 다시 허용하니 복구됨 |
| 명령 스위치와 노브 | 기기와 PC에 테스트로 올려(플래싱하지 않음) `tf-tap.sh`와 `tf-drag.sh`로 확인: PC에서 `toss-connector command add-switch`·`add-stepped`(단계 4개)·`add-linear`(0~100, 단위 5)로 등록(파일에 쓰고 읽는 커맨드) → 덱의 편집 모드에서 랙 빈 슬롯의 `+` → `Command`에 위젯 종류 넷이 나오고, 종류마다 그 종류의 명령만 고를 수 있음(단계 이름들과 범위가 함께 표시됨) → 랙에 놓이자 PC의 현재 상태(꺼짐, `PASSIVE`, 20)가 그대로 표시됨. 스위치를 누르니 PC의 파일이 `on`이 되고 버튼이 눌린 채 인디케이터가 켜짐. 단계 노브를 누르니 다음 단계로, 왼쪽으로 미니 두 단계 앞으로 감. 연속 노브를 위로 미니 35 → 70까지 5씩 값이 가고 파일이 70이 됨. PC에서 파일을 직접 고치니 2초쯤 뒤 덱이 따라옴. `toss-connector deny command.run` 뒤에는 셋 다 흐려지고, 눌러도 PC의 파일이 그대로임. 다크 테마에서도 확인. 테스트 후 등록한 명령과 덱의 위젯은 삭제했다 |
| SSH 열고 닫기 | 기기에 테스트로 올려(헬퍼와 SSH 서버의 체크는 수동으로 복사) 확인: 설정의 `SSH` 토글을 눌러 닫자 몇 초 안에 USB IP와 이더넷·Wi-Fi IP 모두에서 22번 포트가 거부되고 연결돼 있던 세션이 끊김. 호스트 포트(7879)는 그대로 열려 있음. 닫힌 동안 기기 안에서 본 것(미리 예약해 둔 작업이 기록): SSH 서버 프로세스 없음, 수신 포트 없음, `settings.json`에 `"ssh": false`(밝기는 그대로), 수동으로 `rc-service dropbear start`를 해도 "SSH is closed in the deck's settings"로 거부. 토글을 다시 누르자 열림. 닫은 채 기기를 다시 켜니 30초가 지나도 SSH 서버가 `stopped`이고 포트가 닫혀 있었음(그 뒤 미리 예약해 둔 작업이 다시 엶) |
| 화면 밝기와 전원 끄기 | 기기에 테스트로 올려(서비스 파일은 수동으로 복사) 확인: 설정의 `Brightness` 슬라이더를 드래그하니 백라이트 값이 200 → 59 → 10(하한) → 234로 바뀌고 화면 밝기가 따라감, 손을 뗀 값이 `/data/toss/settings.json`에 저장되고 데몬을 재시작하니 그 값으로 돌아옴. 헬퍼(`toss-helper`)의 FIFO에는 `tossd`만 쓸 수 있고(`kiosk`는 거부됨), 알 수 없는 명령은 무시하고 계속 떠 있음. 전원 끄기는 내가 누르지 않았지만, 사용자가 기기에서 사용해 꺼졌다가 다시 켜진 것이 데몬 로그와 부팅 시각에 남아 있다. 전원 끄기와 레이아웃 초기화는 확인 다이얼로그를 거친다(다이얼로그가 뜨고 `CANCEL`로 닫히는 것을 화면 캡처로 확인. 실행 버튼은 누르지 않았다). 밝기의 범위·저장·복원, 헬퍼에 명령 전달은 테스트로 확인(덱 데몬 25개) |
| 절전과 웨이크업 | 기기에 테스트로 올려(헬퍼 파일은 수동으로 복사) 확인: PC에서 `toss-connector sleep` → 덱이 잠듦 → `toss-connector wake` → 깨어남("깨어났습니다"까지 수 초). 덱 설정 → `Sleep` → 확인 다이얼로그의 `SLEEP` → 잠듦 → PC에서 깨움. 두 번 모두 안전장치로 건 RTC 알람(120초)보다 훨씬 일찍 깨어남. 그 전에 절전 자체를 테스트: 얕은 절전은 되고 깊은 절전은 깨어나지 못함, Wi-Fi 매직 패킷은 깨우고 이더넷은 깨우지 못함([01-findings.md](01-findings.md)) |
| 글래스 효과 켜고 끄기 | `app-push.sh ui`로 올려(플래싱하지 않음) `tf-tap.sh`로 설정의 `Glass effect` 줄을 눌러 확인: 끄면 상단바의 시계, 미디어 제어 위젯, 리소스 미터의 디스플레이에서 반사광이 사라지고, 다시 누르면 돌아옴(화면 캡처) |
| 앱 올리기 스크립트 | `src/scripts/app-push.sh`의 `ui`·`tossd` 두 경로 |
| 데몬 자동 테스트 | PC에서 `cargo test`로 `src/app/tossd` 38개, `src/app/meter` 4개 통과(2026-11-17). Nix로 데몬을 빌드할 때마다 `src/app/tossd`의 테스트가 실행된다 |
| 구조 정리 후의 이미지 전체 | `deploy.sh -y`로 세 파티션을 플래싱해 확인(2026-11-18). **플래싱**: 실제 `rkdeveloptool`로 진입, 파티션 테이블 대조, 기록, 되읽기 검증이 돌았고 셋 다 일치. **부팅**: 처음 찾은 `userdata`를 고르는 initramfs로 부팅, 서비스 실패 0, 무선 국가 코드 KR, `/data/toss`의 파일 넷(덱의 키, 페어링, 레이아웃, 설정)이 플래싱 후에도 해시가 같고 새 데몬이 그대로 읽음(플래싱 전에 그 파일들을 PC의 새 데몬에 먼저 읽혀 봤다). **화면**: 플래싱 전에 찍어 둔 12장(첫 화면, 설정, Wi-Fi, 페어링된 기기, 기기 상세, 편집 모드, 추가 메뉴 셋, 인터페이스 정보, 확인 다이얼로그)을 같은 순서로 다시 찍어 픽셀 단위로 비교 — 달라진 곳은 시계, 재생 상태, 리소스 값, 검색된 AP, 기기 상세의 `Allows` 문구뿐. **새로 넣은 것**(`tf-tap.sh`, `tf-drag.sh`): 기기 상세의 `FORGET`과 위젯이 든 랙의 `×`가 확인 다이얼로그를 띄우고 `CANCEL`로 닫힘, 편집 모드에서 위젯을 두 칸 드래그해 옮겼다가 되돌림(`×`·`+`를 누를 때 타일이 들리지 않음), 끝난 뒤 레이아웃 파일이 처음과 동일. 데몬을 멈추니 열려 있던 설정이 닫히고 `NO LINK TO TOSSD`가 뜨며 설정 버튼이 동작하지 않고, 다시 띄우니 돌아옴. **헬퍼**: FIFO에 `ssh on`을 연달아 두 줄 넣으니 둘 다 처리되고 알 수 없는 명령은 버림. 설정의 `SSH` 토글(기기 안에 미리 예약해 둔 작업이 누름)을 닫자 22번 포트가 닫혀 PC의 접속이 거부되고 `"ssh": false`가 기록됐으며, 다시 누르자 열림. **`tf-net.sh`**: 기기에서 PC의 인터넷을 공유받아 파일 다운로드와 `apk`(설치 시뮬레이션)가 동작. **메시지 문구와 주석을 정리한 뒤의 이미지**도 같은 날 `deploy.sh -y`로 다시 플래싱해 확인: 세 파티션 되읽기 일치, 부팅 후 실패한 서비스 0, `/data/toss`의 파일 4개 해시가 플래싱 전과 동일, 재페어링 없이 PC가 다시 연결됨, 홈 화면 캡처 정상, 새로 빌드한 `toss-connector`로 실행 버튼과 스위치 동작(`명령 실행: …`, `명령 값 설정: …` 로그), 웹 콘솔 응답과 토큰 없는 API 요청 거부(401). **디렉터리 구조를 정리한 뒤**(`src/`, `nix/`, `dist/`, `local/`)에도 같은 날 `src/scripts/deploy.sh -y`로 다시 플래싱해 확인: 세 파티션 되읽기 일치, 부팅 후 실패한 서비스 0, `/data/toss`의 파일 4개 해시 동일, PC 재연결, 실행 버튼과 스위치 동작, 재생 중인 곡의 앨범 아트·제목·진행 막대가 미디어 제어 위젯에 표시됨 |
| 덱 금고와 웹페이지 버튼 | 단위·통합 테스트(2026-11-19): 데몬 48개(금고의 암호화 저장과 권한, 허용·거절·busy, 가짜 토큰 주소로 발급·재사용·갱신·거절, 화면 질문을 거치는 호스트 요청, 사이트 아이콘 후보 선택과 보관, 웹페이지 주소 검사), PC 70개(가짜 Discord IPC, 웹페이지 주소 검사), 프로토콜 11개 통과. Nix로 데몬을 기기 타깃으로 빌드(TLS의 C 코드를 clang으로 교차 컴파일). PC에서 덱 데몬과 커넥터를 실제로 띄워 페어링 → `toss-connector discord set`(덱 화면 질문에 `ALLOW`) → 상태 조회 → `check`(승인 전이라 `not_linked`) → 허용 해제 → `forget`, 웹페이지 아이콘 버튼 두 개의 사이트 아이콘(PNG) 가져오기를 확인. 비밀 값은 덱 파일, PC 설정 디렉터리, 로그 어디에도 평문으로 남지 않음. **이 줄에서 확인하지 못한 것**: 기기에서의 동작 전부(덱 화면의 다이얼로그·설정·위젯, 기기의 HTTPS), 실제 Discord 앱과 토큰 주소를 거친 승인·인증, PC 브라우저로 웹페이지 열기 |
| Discord 음소거 명령 | 실기기(2026-11-19): 사용자가 덱 금고에 실제 Discord 앱 키를 저장하고 승인해, 덱이 Discord 토큰 주소에서 토큰을 발급받았다(기기의 HTTPS와 덱 화면의 허용 질문 포함). 커넥터의 Discord 상주 연결로 마이크·헤드셋 음소거 명령을 만들어 덱 스위치 위젯 2개에 연결했고, Discord 단축키와 CLI로 바꾼 상태가 덱 표시등에 곧바로 반영되는 것을 화면 캡처로 확인. PC 단위 테스트 72개 통과. **확인하지 못한 것**: Discord 앱 재시작 뒤 재연결, 토큰 만료(7일) 뒤 다시 빌리는 흐름 |

### 확인하지 못한 것

- **2.3~2.5절을 이 문서의 문장대로 다시 해보지는 않았다.** 이미 다운그레이드된 기기라 반복할 수 없었다. 특히 "버튼 진입이 막힌 순정 기기"에서 시작하는 경로는 작업 초기의 1회가 전부다.
- **다른 기기.** 한 대에서만 진행했다. 패널 변종(M13)이 다른 기기에서는 패널 드라이버가 맞지 않을 수 있다.
- **저장소를 새로 받아서 진행하는 것.** 공개 저장소에서 새로 받아 처음부터 빌드해 보지는 않았다.
- **`downgrade.sh`, `flash-lineage.sh`, `backup.sh`가 파티션을 차례로 도는 부분.** 이 셋이 공유하는 진입·파티션 테이블 대조·기록·되읽기 검증(`src/scripts/lib.sh`, `flash.sh`)은 `deploy.sh`로 실제 기기에 플래싱해 확인했다. 셋 자체는 `downgrade.sh`를 실제 이미지로 크기·MD5 검증과 확인 질문까지, `backup.sh`를 가짜 도구로 16개 구간을 받는 것까지만 봤다. `flash.sh`가 일반 플래싱 대상이 아닌 파티션 앞에서 묻는 것도 가짜 도구로만 봤다.
- **`/data`를 마운트하지 못했을 때의 경로.** `cache` 파티션이 없거나 마운트되지 않을 때 `tf-data`가 루트 위에 `/data/toss/`를 만들어 주는 것은 스크립트를 읽어 확인했을 뿐, 기기에서 그 상황을 만들어 보지 않았다.
- Wi-Fi는 WPA2 AP(2.4GHz와 5GHz)에 접속해 봤고 화면에서 삭제하는 것도 됐다(직접 확인). 비밀번호 없는 AP는 해보지 않았다. USB 키보드 에뮬레이션(HID 가젯)도 해보지 않았다.
- 절전은 몇십 초씩만 재워 봤다. 오래 재운 뒤 깨어나는지, 자는 동안 화면이 꺼지는지, 전력이 얼마나 줄어드는지는 확인하지 못했다. 깊은 절전은 깨어나지 못한다([01-findings.md](01-findings.md)).
- 전원 끄기는 사용자가 기기에서 사용했다. 꺼진 뒤 다시 켜는 방법(전원 버튼인지, 전원을 다시 꽂는지)은 기록하지 못했다.
- 스트림덱 앱의 움직임(버튼이 내려갔다 올라오기, 인디케이터 점멸, 테마 전환, 흘러가는 글자)은 화면 캡처로 볼 수 없어 직접 확인으로만 검증된다. 스타일 적용 후의 UI를 사람이 터치해 본 결과(위젯이 눌리는지, 페이지가 넘어가는지)는 따로 확인받지 못했다.
- **root 헬퍼의 드문 경로.** FIFO를 만들 수 없을 때 종료하는 것은 PC의 셸에서 같은 형태의 스크립트로만 봤고, 슬립 직후 5초 안에 들어온 `sleep` 요청을 버리는 것과 SSH 명령을 전달하지 못했을 때 기록하지 않는 것은 데몬 테스트로만 봤다. (연달아 받은 명령을 순서대로 처리하는 것과 SSH를 열고 닫는 것은 기기에서 확인했다.)
- **`/data/toss`의 파일을 치워 두는 것(`.bad`)과 깨지지 않게 쓰는 것.** 데몬 테스트로만 봤다(깨진 레이아웃, 페어링 목록, 설정, 덱의 키). 기기에서 전원을 뽑아 보지 않았다.
- **저장된 AP 삭제의 확인 다이얼로그(`FORGET NETWORK`).** 기기에서 눌러 보지 않았다(잘못되면 저장된 비밀번호가 사라지기 때문이다). 같은 경로로 뜨는 `FORGET DEVICE`와 `REMOVE RACK`은 기기에서 다이얼로그가 뜨고 `CANCEL`로 닫히는 것을 봤다. 세 다이얼로그 모두 실행 버튼은 누르지 않았다.
- **키보드가 붙들고 있던 단어를 넣는 것.** `SAVE`나 `ADD`를 누를 때 입력하던 단어가 들어가는지 기기에서 보지 않았다. 지금 기기에는 단어 추천용 사전이 없어, 붙들린 단어가 실제로 생기는지부터 확인하지 못했다.

## 2.13 겪은 문제와 해결

| 문제 | 원인 | 해결 |
|---|---|---|
| 초기화 버튼을 눌러도 공장 초기화만 됨 | 펌웨어 799가 Loader 진입을 막음 | 패드 쇼트로 MaskROM → 761로 다운그레이드 |
| Loader 모드에서 읽은 값이 `0xCC`로 채워짐 | 기기 내장 Loader에 읽기 기능이 없음 | 읽기와 검증은 MaskROM에서 로더를 올린 뒤에만 |
| `nix develop`마다 디렉터리 전체가 `/nix/store`에 복사됨 | 프로젝트가 git 저장소가 아니었음 | git 저장소로 전환, 이미지·백업은 `.gitignore` |
| 부트 이미지에 DTB를 어디 넣는지 | U-Boot는 `second` 영역의 RSCE 번들을 읽음 | `src/scripts/rsce.py` |
| PC가 USB 이더넷에서 IP를 못 받음 | Alpine 기본 busybox에 DHCP 서버가 없음 | `busybox-extras` |
| 기기를 재부팅시킨 SSH가 끝나지 않음 | 연결이 끊긴 줄 모르고 대기 | 연결 확인 간격, `--reboot` 옵션 |
| `flash.sh`가 MaskROM에서 멈춤 | 로더를 올리기 전에는 상태 질의에 응답이 없음 | 타임아웃 |
| `flash.sh`의 시작 LBA 대조 실패 | `ppt` 출력의 줄 끝 CR | CR 제거 |
| 메인라인에서 Loader 재진입이 안 될 뻔함 | `rk3399.dtsi`에 `reboot-mode` 노드가 없음 | DTS에 추가 |
| **화면이 까맣고 백라이트만 켜짐** | LCD 바이어스 칩(IML7525)이 DTS에 없었고, 추가한 뒤에도 드라이버가 enable 핀을 구동하지 못함 | 칩을 추가하고, enable 핀은 별도의 고정 레귤레이터로 기술 |
| 터치 좌표가 화면과 반대 | 터치 패널이 180도 돌아가 붙어 있음 | DTS에 X축·Y축 모두 반전 |
| 부팅이 끝나도 화면에 부팅 로그가 남음 | 화면 로그인 줄을 빼 둔 상태였음 | `tf-console-getty` |
| `sysctl: tcp_syncookies is an unknown key` | 커널 설정에 SYN 쿠키가 빠져 있었음 | `CONFIG_SYN_COOKIES=y` |
| `modprobe: can't change directory to '/lib/modules'` | 모듈이 없는 커널인데 모듈 디렉터리도 없었음 | 루트에 모듈 디렉터리와 `modules.builtin` |
| `rk_gmac-dwmac: Can not read property: tx_delay` | RMII에서는 안 쓰이는 값인데 없으면 에러 레벨로 출력 | DTS에 순정 값을 기술 |
| 이더넷 MAC이 부팅마다 바뀜 | 메인라인에서는 부트로더가 MAC을 넣어 주지 않음 | initramfs가 저장된 MAC을 적용 |
| 무선 국가 코드가 기본값 | 설정한 적 없음 | 커널 부팅 인자로 KR |
| 스피커 소리가 아주 작음 | 앰프가 좌·우 출력의 차이를 증폭하는데 좌우에 같은 신호를 넣고 있었음 | 믹서에서 오른쪽 채널 극성 반전(`Playback Polarity = R Invert`) |
| 낮은 테스트음이 안 들림 | 작은 스피커라 440Hz 부근이 약함 | 테스트는 1kHz로 |
| 새로 만든 루트 이미지에 무선 펌웨어가 없음 | 빌드 스크립트가 이미지를 만든 뒤에 펌웨어를 복사 | 복사를 이미지 생성 전으로 |
| 순정 커널에서 화면 로그인이 실패를 반복 | `/dev/tty1` 노드는 있지만 열리지 않음 | 실제로 열리는지 확인하도록 수정 |
| Nix 빌드에서 루트 이미지 생성이 `Permission denied`로 실패 | Nix의 빌드 샌드박스가 setuid 비트를 실제 파일에 거는 것을 막아, `bbsuid` 같은 파일이 읽기 권한 없이 남음 | 권한을 읽을 수 있게 바꿨다가 되돌려, 실제 파일은 읽을 수 있고 기록되는 권한은 원래대로 되게 함 (`nix/build-rootfs.sh`) |
| Nix 빌드에서 일부 패키지를 `package mentioned in index not found`로 못 찾음 | 아키텍처 무관 패키지를 `noarch` 디렉터리에서 찾음 | 로컬 인덱스를 `--rewrite-arch aarch64`로 생성 |
| 루트 이미지를 두 번 빌드하면 2바이트가 다름 | apk가 남기는 `/var/log/apk.log`에 빌드 시각이 들어감 | 이미지에서 그 파일을 삭제 |
| `boot`에 512바이트만 기록됨 (되읽기는 "일치") | `dist/images/`의 이미지는 Nix 스토어를 가리키는 심볼릭 링크인데, `flash.sh`가 링크 자체의 크기를 이미지 크기로 읽음. 나머지는 직전 이미지가 남아 있어 부팅은 됐음 | `flash.sh`가 링크를 따라가 실제 파일을 쓰게 함. 기록 줄에 찍히는 바이트 수를 확인할 것 |
| 데몬 서비스가 부팅 시 `failed` (수동 실행은 됨) | 서비스가 권한을 낮춘 뒤 로그 파일을 열어서 `/var/log`에 만들지 못하고 바로 종료됨 | 서비스의 `start_pre`에서 로그 파일을 미리 만들어 데몬 사용자에게 넘김 (`src/app/rootfs/etc/init.d/tossd`) |
| 버튼을 눌러도 누른 횟수가 UI에 안 뜸 (데몬 로그에는 눌림이 찍힘) | QML에서 받은 값을 같은 객체에 고쳐 넣고 다시 대입해서, "바뀌지 않았다"고 보고 UI를 갱신하지 않음 | 새 객체로 바꿔 넣음 (현재의 `src/app/ui/shell/DaemonLink.qml`) |
| QML 파일을 올렸는데 UI가 이전 내용 그대로 뜸 | 올린 파일의 수정 시각이 이전 파일과 초 단위까지 같았다. Qt의 QML 캐시가 수정 시각만 보고 이전 컴파일 결과를 씀 | `app-push.sh`가 올린 파일의 수정 시각을 새로 찍게 함 |
| UI가 뜰 때마다 데몬에 접속했다가 끊고 다시 접속 | 데몬이 접속 초기 절차에도 읽기 주기(100ms)와 같은 타임아웃을 걸어서, UI가 막 뜨느라 요청이 늦으면 첫 접속을 거부함. 로그에는 정상 종료를 뜻하는 `클라이언트 종료:`로 찍혀 UI 쪽 문제처럼 보였음 | 접속 절차에는 5초를 주고 그 뒤에 읽기 주기를 적용. 늦게 요청하는 클라이언트를 받아 주는지 보는 테스트 추가 |

화면 문제는 커널이 "정상"이라고 보고하는데 실제로는 까만 경우라 추적이 어려웠다. 패널에 상태를 되묻는 진단을 넣어 "명령은 받았는데 절전에서 못 나온다"는 것을 확인하고, GPIO 상태를 훑어 enable 핀이 입력으로 방치된 것을 찾았다. 진단 요령과 점검 순서는 [01-findings.md](01-findings.md) 1.3절 "화면"에 있다.

## 2.14 되돌리는 방법

전부 `flash.sh --enter` 상태에서 같은 주소에 이미지를 다시 쓰는 방식이다.

| 되돌릴 상태 | 방법 | 실제로 해봤나 |
|---|---|---|
| 직전의 정상 커널 | `src/scripts/flash.sh boot <이미지>` | 예 |
| 저장소 그대로의 상태 | `src/scripts/deploy.sh` (기기에서 바꾼 것은 사라진다) | 예 |
| 순정 761 안드로이드 | `src/scripts/downgrade.sh` 후 재부팅. 초기화 버튼을 누른 채 USB 없이 켜면 공장 초기화 | 일부 (다운그레이드까지) |
| 구입 당시 상태 | `local/backup/`의 이미지를 파티션 이름대로 기록: `src/scripts/flash.sh <이름> local/backup/<이름>.img` (`boot`·`recovery`·`userdata` 외 파티션은 확인 질문이 뜬다. GPT 앞뒤는 `flash.sh`가 다루지 않는다) | **아니오** |

## 2.15 남은 일

기기 쪽:

| 일 | 메모 |
|---|---|
| 5GHz AP를 쓴 뒤 2.4GHz에 접속하지 못하는 문제 | 5GHz AP에 접속했다가 끊으면 검색이 5.7~5.8GHz 채널만 돌아 2.4GHz AP에 접속하지 못한다. 재부팅하면 복구된다. 두 번 관찰, 원인 미확인. SDIO 버스 에러(아래 줄)와는 별개다 — 그것을 고친 뒤에도 발생했다. 드라이버(`brcmfmac`)·펌웨어·국가 코드 설정 쪽을 봐야 한다 ([03-userland-guide.md](03-userland-guide.md) 3.15절) |
| Wi-Fi의 SDIO 버스 에러 (고침) | 무선 칩과의 SDIO 버스가 150MHz(순정 값)에서 에러를 냈다(`brcmf_sdio_bus_sleep … -84`, `halting operation`). 디바이스 트리의 `sdio0`을 50MHz로 제한해 플래싱했고, 플래싱 후 질의 3000번에 에러가 없었다 |
| Wi-Fi 접속 | 화면의 설정에서 AP를 고르고 비밀번호를 입력해 접속한다(동작 확인). 저장한 AP는 기기의 `/data`에 있어 플래싱해도 유지된다(확인). 남은 것: 기업용 인증·WPA3 전용 AP, 이름을 숨긴 AP, Wi-Fi 끄기는 다루지 않는다 ([03-userland-guide.md](03-userland-guide.md) 3.15절) |
| 절전의 남은 것 | 얕은 절전(s2idle)과 Wi-Fi 매직 패킷 웨이크업은 된다(설정과 `toss-connector sleep`/`wake`). 남은 것: **깊은 절전은 깨어나지 못한다**(절전 방식과 웨이크업 수단을 펌웨어에 알리는 설정이 메인라인 DTS에 없다), **이더넷으로는 깨어나지 않는다**(컨트롤러는 지원한다고 알리는데 원인 미확인), **화면을 터치해도 깨어나지 않는다**(터치를 웨이크업 소스로 등록하지 않았다. DTS의 `wakeup-source`를 테스트해 볼 만하다), 전원 버튼이 입력으로 잡히지 않는다 ([01-findings.md](01-findings.md)의 "절전 및 네트워크 웨이크업") |
| 유휴 시 화면 끄기 | 없다. 화면만 끄고 터치로 다시 켜는 것은 절전과 별개의 작업이다 |
| 이더넷과 Wi-Fi를 같은 네트워크에 함께 연결했을 때 | Wi-Fi IP로는 응답하지 않는다(ping도 접속도). 응답이 이더넷으로 나가게 돼 있어서 커널이 "들어온 경로와 나갈 경로가 다르다"며 버린다(`rp_filter`. 버린 수가 카운트되는 것으로 확인). 이더넷 IP로는 닿고, 이더넷을 빼면 Wi-Fi IP가 응답한다. 웨이크업 매직 패킷은 영향이 없다. 고치려면 그 검사를 느슨하게 한다(설정 한 줄). 그대로 두었다 ([01-findings.md](01-findings.md) 1.5절) |
| 덱의 SSH | 설정에서 SSH를 통째로 닫을 수 있다(닫아 두면 플래싱하거나 다시 켜도 닫힌 상태다). 남은 것: 열려 있는 동안에는 여전히 root에 비밀번호가 없고 LAN에서도 접속할 수 있다 — 공개키만 받게 하기(`flake.nix`의 `ssh.authorizedKeys`), 네트워크 쪽만 닫고 USB는 열어 두기(커널에 방화벽을 넣어야 한다), 닫힌 채로 플래싱하기(`deploy.sh`가 SSH 없이 Loader로 보내는 경로: 시리얼 콘솔) |
| USB 키보드 에뮬레이션 (HID 가젯) | 커널에는 넣었다. 가젯 구성(initramfs)과 테스트는 안 함 |
| 발열 | 방열 구조가 없는 기기라 전부하에서 70°C를 넘어 스로틀링한다. 평소 부하가 높아지면 최대 주파수를 낮추는 것을 검토 |
| 워치독 자동 시작 | 드라이버만 있고 켜지 않음 |
| 켜진 뒤에 꽂은 Micro USB | 기기가 켜져 있는 상태에서 Micro USB를 PC에 꽂았더니 양쪽 모두 인식하지 못했고(기기의 USB 상태가 `not attached`, 커널 로그에 기록 없음), 기기를 재부팅하자 잡혔다. 한 번 관찰한 것이고 원인은 보지 않았다. 부팅 때 꽂혀 있어야만 USB 가젯이 연결되는 것으로 보인다 |
| 픽셀 클럭 | 66.67MHz로 잡혀 약 57Hz. 60Hz에 맞추려면 클럭 설정을 봐야 한다 |
| `clk_ignore_unused`, `pd_ignore_unused` | 초기 안전장치로 넣은 부팅 인자. 빼도 되는지 확인 필요 |
| 부팅 스플래시 | 부팅 메시지 대신 이미지를 띄우는 것. initramfs에서 그리고 앱이 이어받는 방식이 가능하다(검토만 함). 급하지 않아 미뤘다 |
| 터치 드라이버 경고 | 무해. 없애려면 드라이버 패치 필요 |
| 패키지 설치 스크립트 | 빌드가 건너뛴다(2.6절). 현재 패키지에서 필요한 것은 처리했지만, 패키지를 추가할 때마다 확인이 필요하다 |
| 커널 설정 조각의 헤더 주석 | `src/os/kernel/toss-front.config`의 둘째 줄이 "디스플레이 없이 부팅"하던 때의 목표를 말하고 있다(지금은 화면·무선까지 들어 있다). 이 파일을 고치면 커널을 재빌드하므로, 다음에 커널 설정을 고칠 때 함께 고친다 |
| 커널 설정 검사의 연결 방식 | 설정 조각이 그대로 반영됐는지를 `nix/kernel.nix`가 `src/os/kernel/kbuild.sh`의 출력에서 `경고 0`이라는 문자열을 찾아 판단한다. 문자열 대신 종료 코드로 알리게 하는 것이 낫다. `kbuild.sh`를 고치면 커널을 재빌드하므로 다음 커널 변경 때 함께 한다 |
| QML 실행기 | Alpine에는 폐기 예정인 `qmlscene`뿐이다. Qt가 없애면 작은 실행기를 직접 빌드해야 한다 |

앱 쪽 ([03-userland-guide.md](03-userland-guide.md) 3.16절, 3.17절):

| 일 | 메모 |
|---|---|
| 버튼의 종류 | 랙에 넣을 수 있는 것은 앱 `Command`의 넷이다: 명령 버튼, 명령 스위치, 단계 노브, 연속 노브(모두 PC에 등록해 둔 명령을 쓴다). 남은 것: 단축키를 보내는 버튼 같은 다른 종류, 노브를 손으로 돌리는 느낌의 조정(밀어야 하는 거리 등은 사람이 써 봐야 안다), 여러 칸을 차지하는 큰 노브. |
| 레이아웃 편집의 남은 것 | 이동·추가·삭제·랙의 크기와 이름은 된다. 남은 것: 랙 안의 버튼을 다른 슬롯으로 옮기기(지금은 삭제하고 다시 넣는다), **추가한 뒤에 위젯의 설정을 바꾸는 화면**(어느 PC, 어떤 명령, 리소스 미터의 대상과 리소스 — 지금은 삭제하고 다시 놓는다), 놓인 위젯의 크기 변경(리소스 미터는 크기마다 위젯 종류가 따로다) |
| 리소스 미터의 남은 것 | 2×2와 4×1 크기를 기기에서 놓아 보지 않았다. 코어가 많은 PC에서는 선택 목록이 길다(그룹으로 접는 것을 생각해 볼 만하다). 눈금의 끝이 없는 리소스(전송 속도)는 위젯이 본 최댓값에 맞추는데, UI를 재시작하면 잊는다 |
| UI 표시 설정의 저장 | `Dark mode`(테마)와 `Glass effect`(글래스 효과)는 저장되지 않아 재시작하면 기본값으로 돌아간다. 밝기를 저장하는 `/data/toss/settings.json`에 함께 넣으면 된다 |
| 권한이 필요한 조작 | 화면 밝기, 절전, 전원 끄기, SSH 열고 닫기는 설정에 있다(데몬은 비루트 사용자로 돌고, 밝기는 파일 권한으로, 나머지는 root 헬퍼 `toss-helper`로 처리한다). 음량과 재부팅은 없다 |
| 온스크린 키보드의 언어 | 영어만 켜져 있다. 랙 이름이나 버튼 라벨에 한글을 쓰려면 한글 자판을 켜야 한다(입력기에 포함돼 있다) |
| 블루투스 | 상단바에 자리만 있다. 다루지 않는다 |
| 아이콘 | 필요한 것만 직접 그려 쓴다. 한 세트를 정하지 않았다 |
| PC와의 연결 | 남은 것은 [04-host-link.md](04-host-link.md) 4.13절: 즉시 경로 전환, 덱 자동 찾기, 윈도우·맥, 음량·키 입력 같은 내장 기능 |
| 첫 커밋 | 저장소에 아직 커밋이 없다(전부 `git add`만 돼 있다). 저장소를 새로 받아 처음부터 빌드하는 것을 확인하려면 커밋이 먼저다. **PC 쪽 NixOS 모듈을 다른 flake의 입력으로 고정해 쓰려면 커밋이 있어야 한다**([04-host-link.md](04-host-link.md) 4.10절) |
