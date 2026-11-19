# 3. 유저랜드 개발 가이드

기기를 "동작"하게 만드는 작업(부팅, 커널, 화면, 터치, 네트워크)은 끝났다. 이 문서는 그 위에서 앱을 개발하고 배포하는 방법을 다룬다. 현재 기기에는 스트림덱 앱이 올라가 있다: 위젯 배치·편집 UI, 설정, PC 연동(3.16절). 위젯 종류 추가 방법은 3.17절, PC와의 통신은 [04-host-link.md](04-host-link.md)에 있다.

각 항목에 **확인됨**(이 기기에서 실제로 검증)과 **미검증**(일반적으로 통하는 방법이지만 이 기기에서는 미확인)을 표시했다. 기기 조사 내용은 [01-findings.md](01-findings.md), 기기를 현재 상태로 만드는 절차와 구성은 [02-progress.md](02-progress.md)에 있다.

## 3.1 큰 그림

```
개발 PC (NixOS)                             토스 프론트
┌──────────────────────────┐   Micro USB   ┌──────────────────────────────┐
│ nix develop               │──────────────│ Alpine Linux 3.24 (musl)      │
│  src/scripts/deploy.sh    │  플래싱       │  dropbear (SSH)               │
│  src/scripts/tf-ssh.sh    │  USB 이더넷   │  tossd (데몬, Rust)           │
│  src/scripts/app-push.sh  │  172.16.42.x │    ↕ WebSocket                │
│  src/scripts/console.py   │  USB 시리얼   │  cage → toss-shell (화면, QML)   │
└──────────────────────────┘              │  800×1280 터치 화면           │
          └────────────── LAN (선택) ───────│  eth0 / wlan0                 │
                                           └──────────────────────────────┘
```

- 필수 연결은 하부 Micro USB 케이블 하나다. 이 케이블로 플래싱, SSH, 시리얼 콘솔, 파일 전송을 모두 처리한다.
- 기기에 LAN 케이블이 꽂혀 있으면 기기가 직접 인터넷에 나가고, 같은 공유기의 PC에서 LAN으로도 SSH 접속이 된다. 없으면 필요할 때만 PC 인터넷을 공유해 쓴다(3.4절).
- 개발 PC의 명령은 모두 프로젝트 디렉터리의 `nix develop` 안에서 실행한다.
- **기기 상태는 저장소에서 만든다.** 기기에서 직접 바꾼 것은 임시 테스트이며, 남기려면 저장소에 반영해 플래싱한다(3.6절).
- 위 그림은 **개발 시**의 연결이다. 실사용 시에는 PC에서 `toss-connector`가 실행돼 덱의 `tossd`에 접속한다(USB·이더넷·Wi-Fi, 포트 7879). 이 부분은 [04-host-link.md](04-host-link.md)에 있다.
- 덱 SSH는 설정에서 꺼 둘 수 있다. 꺼져 있으면 위 스크립트들이 접속하지 못한다(3.3절).

## 3.2 PC와 기기 연결하기

### 물리 연결

1. 기기 하부 뚜껑을 열고 Micro USB 포트에 **데이터 케이블**을 꽂아 PC에 연결한다. 충전 전용 케이블은 안 된다.
2. 후면 USB-C에 전원(12V PD)을 꽂는다. 꽂으면 바로 켜진다.
3. 약 10초 뒤 PC에 `Toss Front Alpine`이라는 USB 장치가 잡힌다.

### PC 쪽 준비 (한 번)

USB 장치 권한이 필요하다. 현재는 임시 규칙을 쓰고 있어 **PC를 재부팅하면 사라진다.** 영구 적용하려면 NixOS 설정에 추가한다.

```nix
services.udev.extraRules = ''
  # Rockchip Loader/MaskROM — 커널과 이미지를 구울 때
  SUBSYSTEM=="usb", ATTR{idVendor}=="2207", MODE="0660", TAG+="uaccess"
  # 토스 프론트 USB 시리얼 콘솔
  SUBSYSTEM=="tty", ATTRS{idVendor}=="1d6b", ATTRS{idProduct}=="0104", MODE="0660", TAG+="uaccess"
'';
```

임시로 적용하려면(재부팅 전까지 유효):

```bash
sudo sh -c 'mkdir -p /run/udev/rules.d && printf "%s\n" "SUBSYSTEM==\"usb\", ATTR{idVendor}==\"2207\", MODE=\"0666\"" "SUBSYSTEM==\"tty\", ATTRS{idVendor}==\"1d6b\", ATTRS{idProduct}==\"0104\", MODE=\"0666\"" > /run/udev/rules.d/99-rockchip.rules && udevadm control --reload && udevadm trigger'
```

USB 이더넷은 별도 설정이 필요 없다. 기기가 DHCP로 PC에 `172.16.42.2`를 할당한다. 게이트웨이와 DNS는 내려주지 않으므로 PC의 인터넷 경로는 바뀌지 않는다(확인됨: NetworkManager가 자동으로 연결).

### 연결 확인

```bash
nix develop -c src/scripts/tf-ssh.sh 'uname -a'
```

`Linux toss-front 6.18.63-tossfront …`가 출력되면 정상이다.

## 3.3 기기에 접속하기

| 작업 | 명령 | 상태 |
|---|---|---|
| 셸 열기 | `src/scripts/tf-ssh.sh` | 확인됨 |
| 명령 하나 실행 | `src/scripts/tf-ssh.sh 'ls /dev/input'` | 확인됨 |
| 파일 보내기 | `src/scripts/tf-ssh.sh --put ./app /root/app` | 확인됨 |
| 파일 받기 | `src/scripts/tf-ssh.sh --get /root/log.txt ./log.txt` | 확인됨 |
| 재부팅 | `src/scripts/tf-ssh.sh --reboot` (뒤에 모드 지정 가능: `normal` 기본, `loader` 플래싱용 Loader로, `recovery` 복귀용 순정 커널로 1회) | 확인됨 |
| 화면 한 지점에 터치 이벤트 주입 | `src/scripts/tf-tap.sh 750 53` (좌표는 화면 픽셀. 여러 지점을 이어서 지정 가능) | 확인됨 |
| 시리얼 콘솔로 명령 실행 | `src/scripts/console.py 'dmesg \| tail'` | 확인됨 |
| 시리얼 콘솔 대화형 | `picocom /dev/ttyACM0` | 미검증 |

- 로그인 계정은 `root`, 비밀번호 없음. SSH는 USB 이더넷 IP뿐 아니라 유선·무선 IP에서도 수신한다(이 기기의 현재 설정. 3.15절 보안 주의 참고).
- **SSH는 덱 설정에서 끌 수 있다**(설정 → `NETWORK` → `SSH`). 꺼져 있으면 `tf-ssh.sh`와 이를 쓰는 스크립트(`app-push.sh`, `tf-tap.sh`, `deploy.sh`의 Loader 진입 단계)가 모두 동작하지 않는다. `Connection refused`가 나오면 덱 화면에서 다시 켠다. 꺼 둔 상태에서도 시리얼 콘솔(`src/scripts/console.py`)은 동작한다.
- LAN IP로 접속하려면 `TF_HOST=<기기 주소> src/scripts/tf-ssh.sh …`. 기기에 접속하는 다른 스크립트(`tf-net.sh`, `tf-tap.sh`, `tf-drag.sh`, `app-push.sh`, `deploy.sh`의 Loader 진입 단계)도 같은 변수를 따른다 — IP와 SSH 옵션을 한 곳(`src/scripts/lib.sh`)에서 가져다 쓴다. IP는 기기에서 `ip -4 addr show eth0`으로 확인한다. DHCP 할당이라 바뀔 수 있으므로, 고정하려면 공유기에서 MAC 주소에 IP를 예약한다.
- 스크립트에 잘못된 인자를 주면(예: `src/scripts/tf-ssh.sh --put` 뒤에 경로가 하나뿐일 때, `src/scripts/tf-tap.sh`에 좌표가 홀수 개일 때) 사용법을 출력하고 종료한다. 사용법은 각 스크립트 상단 주석 그대로다.
- 기기를 재플래싱하면 SSH 호스트 키가 바뀐다. 그래서 `tf-ssh.sh`는 호스트 키를 기록하지 않는다.
- **시리얼 콘솔은 네트워크가 안 될 때의 비상 통로다.** 네트워크 설정이 망가져도 접속할 수 있고, `console.py --put`으로 파일도 전송할 수 있다.

직접 `ssh`를 쓰려면 `~/.ssh/config`에 추가한다.

```
Host tossfront
    HostName 172.16.42.1
    User root
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null
    ServerAliveInterval 5
```

기기에서 띄운 서비스를 PC에서 보려면 SSH 포트 포워딩을 쓴다. 예를 들어 기기의 8080 포트를 PC의 `localhost:8080`으로 연결하려면: `ssh -L 8080:127.0.0.1:8080 tossfront` (미검증)

## 3.4 패키지 설치하기

**기기에 직접 설치한 패키지는 임시 테스트용이다.** `userdata`를 다시 구우면 사라진다. 계속 쓸 패키지는 `src/os/alpine/packages`에 추가해 이미지에 넣는다(3.6절).

LAN 케이블이 꽂혀 있으면 기기에서 바로 `apk add …`가 된다(확인됨). 기기에 인터넷이 연결돼 있지 않을 때는 PC 인터넷을 공유해 쓴다(확인됨).

```bash
nix develop -c src/scripts/tf-net.sh apk add htop
```

`tf-net.sh`는 뒤에 넘긴 명령을 기기에서 실행하는 동안만 PC를 거쳐 인터넷이 되도록 연결한다. `apk`뿐 아니라 `wget`, `git`, `pip` 등 프록시 환경 변수를 따르는 도구라면 모두 동작한다. 기본은 USB IP로 접속하며 `TF_HOST=<기기 IP>`를 주면 LAN으로 접속한다(`tf-ssh.sh`와 동일).

```bash
nix develop -c src/scripts/tf-net.sh 'apk update && apk upgrade'
```

- PC에 root 권한이나 방화벽 설정이 필요 없다. PC에서 프록시를 localhost에만 띄우고 SSH 터널로 기기에 연결해 준다.
- 실행할 때마다 기기 시계를 PC 시각으로 맞춘다. 시계가 어긋나면 HTTPS 검증이 실패하기 때문이다.
- 패키지 저장소는 Alpine v3.24의 `main`과 `community`다. 패키지 이름은 [pkgs.alpinelinux.org](https://pkgs.alpinelinux.org/packages)에서 검색한다.

## 3.5 기기 환경

### 있는 것

| 항목 | 사양/상태 |
|---|---|
| 배포판 | Alpine 3.24.4. C 라이브러리는 **musl**, init은 OpenRC, 기본 도구는 busybox |
| CPU, 메모리 | 6코어 (리틀 코어 4개 최대 1.416GHz, 빅 코어 2개 최대 1.8GHz, 부하에 따라 자동 스케일링), 1,956MB. 스트림덱 UI가 뜬 상태에서 130~180MB 사용 (`free` 기준. 프로세스별로는 컴포지터 약 75MB, UI 약 165~200MB(설정과 키보드를 한 번 열어 본 뒤 증가), 데몬 약 1MB — 공유 메모리와 폰트 같은 mmap 파일이 있어 단순 합과 다름) |
| GPU | Mali-T860, Panfrost. OpenGL ES 3.1까지 지원. **Vulkan 미지원.** 유저스페이스 드라이버(Mesa)는 이미지에 포함 |
| 온도 | `/sys/class/thermal/thermal_zone0/temp`. 70°C부터 CPU가 자동으로 스로틀링된다. 방열 구조가 없어 풀로드를 오래 걸면 72°C 부근에서 유지된다 |
| 저장공간 | 약 11.4GB 중 약 10.4GB 여유 |
| 디스플레이 | `/dev/dri/card1` (DSI-1, 800×1280), `/dev/fb0` (32비트). 세로 방향, 실제 주사율 약 57Hz. `card0`은 GPU다. 번호는 커널 인식 순서이므로 앱에서 번호를 고정하지 말고 `/sys/class/drm/card*-DSI-1`로 찾는 편이 안전하다 |
| 화면 환경 | Wayland 키오스크(`cage`). 앱 하나를 전체 화면으로 띄운다 (3.7절 C) |
| UI 프레임워크 | Qt Quick 6.11 (QML, WebSocket 모듈 포함). 런처는 `qmlscene` |
| 폰트 | Noto Sans CJK와 Noto Serif CJK (Regular, Bold). 한·중·일·영 모두 지원. 고정 폭 숫자용 `Noto Sans Mono CJK` 포함. 그 밖에 DejaVu |
| 백라이트 | `/sys/class/backlight/backlight/brightness` (0~255) |
| 터치 | 입력 장치 `ilitek_ts`. 멀티터치 10점, 좌표 0~16383 (두 축) |
| 버튼 | 입력 장치 `adc-keys`. 하부 초기화 버튼이 `KEY_VOLUMEUP`. `KEY_VOLUMEDOWN`도 정의돼 있으나 물리 버튼 매핑은 미확인 |
| USB | 디바이스 모드 전용. 가젯 기능으로 시리얼, 이더넷(RNDIS) 사용 중 |
| Wi-Fi | `wlan0` (BCM43455, 2.4/5GHz). 스캔 확인, 접속은 3.15절 |
| 블루투스 | `hci0` (BR/EDR + LE). 주변 기기 스캔 확인 |
| 이더넷 | `eth0` (100Mbit). 케이블을 꽂으면 자동으로 IP를 받는다 |
| 시계 | 부팅 후 네트워크가 연결되면 NTP로 자동 동기화. 시간대는 한국(KST). RTC가 있어 재부팅 시 유지되지만, 백업 배터리가 없어 전원을 뽑으면 2013년으로 초기화된다 — 오프라인 부팅 시 시계가 맞지 않음 |
| 가상 입력 | `uinput` 사용 가능 |
| 오디오 | ALSA 카드 `Toss Front` (`hw:0,0`), 스피커 1개. 아래 "오디오" 참고 |

입력 장치 번호(`event0` 등)는 바뀔 수 있다. 이름으로 찾으려면: `grep -A4 ilitek_ts /proc/bus/input/devices`

### 오디오

- 재생: `aplay 파일.wav`. 기준 테스트음은 `aplay /usr/share/sounds/tossfront/reference.wav` (저음 500Hz → 중음 1kHz → 고음 2kHz, 각 1초)
- 볼륨: `amixer -c 0 sset DAC <값>` (0~192, 스텝당 0.5dB). 바꾼 값을 재부팅 후에도 유지하려면 `alsactl store`

볼륨 기준표. 기준 테스트음(최대 진폭의 70% 사인파)을 재생해 직접 들어 보고 정리했다.

| `DAC` 값 | 음량 | 체감 |
|---|---|---|
| 108 | -42dB | 아주 작음 |
| **120** | **-36dB** | **책상 위에서 쓰기 적당 — 기본값** |
| 132 | -30dB | |
| 144 | -24dB | 매장 키오스크 용도에 적당한 크기 |
| 156 | -18dB | 여기서부터 크게 들림 |
| 192 | 0dB | 최대. 놀랄 만큼 큼 |

앱에서 볼륨 조절을 넣는다면 108~144 범위를 쓰고 상한을 144~156 사이로 잡는 편이 무난하다. 진폭이 작은 음원은 같은 값에서 더 작게 들리므로, 알림음 파일을 기준 테스트음과 비슷한 레벨로 맞춰 두면 이 표가 그대로 들어맞는다.
- **`Playback Polarity`는 `R Invert`로 둔다.** 이 기기의 앰프는 좌·우 채널 차분을 증폭한다. `Normal`로 두면 좌우에 같은 신호가 들어가 상쇄되면서 소리가 극단적으로 작아진다. 반대로 `R Invert` 상태에서 볼륨을 최대로 올리면 상당히 크게 나온다.
- 스테레오 음원은 좌우 차이 성분만 출력되므로 모노로 믹싱해 재생하는 편이 좋다.
- 앰프가 켜져 있는 동안 화이트 노이즈가 난다. 재생이 끝나면 0.5초 뒤 앰프 전원을 내린다(`/etc/local.d/20-audio.start`).
- 스피커가 작아 저음 출력이 약하다. 알림음은 1kHz 전후가 무난하다.

### 없는 것

| 없는 것 | 영향 | 대안 |
|---|---|---|
| FUSE 등 | 커널 설정에 미포함 | 아래 "커널 옵션 추가" |
| X11 | X11 전용 앱 실행 불가 | `xwayland` 패키지 추가 (미검증) |
| Vulkan | Vulkan 요구 앱/도구 사용 불가 | 없음. GPU 드라이버 미지원 |
| 컴파일러, Python 등 | 기기 내부 빌드나 스크립트 실행 불가 | PC에서 빌드 후 배포 (3.10절) |

### 커널 옵션 추가

필요한 커널 기능이 빠졌으면 `src/os/kernel/toss-front.config`에 한 줄 추가하고 다시 올린다.

```bash
nix develop -c src/scripts/deploy.sh
```

커널 전체를 다시 빌드하므로 몇 분 걸린다(16코어 PC 기준 약 3분). 케이블이나 버튼을 만질 필요는 없다. 커널 설정이 바뀌면 루트 이미지도 다시 만들어지므로(내장 모듈 목록 포함) 셋 다 올리는 편이 맞고, 이때 기기에서 직접 수정한 내용은 초기화된다.

- 설정 조각 파일에 넣은 값이 최종 `.config`에 반영되지 않으면 빌드가 실패한다. 의존 옵션이 꺼져 있다는 뜻이다.
- 커널 부팅 인자는 `src/os/kernel/cmdline`, 디바이스 트리는 `src/os/kernel/dts/`에 있다. 이 둘만 수정했다면 `src/scripts/deploy.sh boot`로 충분하며 금방 끝난다.
- 여러 번 고쳐 가며 테스트할 때는 증분 빌드(`src/scripts/build-kernel.sh`, `src/scripts/build-boot.sh`)를 쓴다. 절차는 [02-progress.md](02-progress.md) 2.11절 참고(이 방식으로 디스플레이와 터치를 붙였다).

## 3.6 변경 사항 영속화

**기기는 언제든 초기화될 수 있다고 가정한다.** 기기 상태는 저장소에서 빌드해 낸 산출물이며, 기기에서 직접 바꾼 파일은 다음 플래싱 시 덮어씌워진다. 남길 변경은 저장소에 반영해야 한다.

| | 기기에서 직접 | 저장소에 반영 |
|---|---|---|
| 방법 | SSH로 접속해 설치·수정 | 아래 표의 파일 수정 후 `src/scripts/deploy.sh` |
| 속도 | 즉시 | 이미지를 다시 빌드해 플래싱해야 함 (1~2분) |
| 지속성 | `userdata` 재플래싱 시 **전부 초기화** | 저장소에 남아 언제든 동일 상태로 재현 가능 |
| 목적 | 실험, 시행착오 | 확정된 구성 |

기본 흐름은 기기에서 먼저 테스트해 보고, 동작이 확인되면 저장소에 옮겨 적은 뒤 이미지를 다시 구워 검증하는 것이다. 저장소에 반영되지 않은 변경은 없는 것으로 취급한다.

| 반영 대상 | 수정 위치 |
|---|---|
| 패키지 | `src/os/alpine/packages`에 한 줄 추가 → `src/scripts/alpine-lock.sh update` (버전·해시 고정 및 아카이브 저장) |
| 설정 파일, 스크립트, 서비스 | `src/os/rootfs/` 아래 동일 경로. 예: `src/os/rootfs/etc/local.d/10-usb-net.start` |
| 스트림덱 앱 | `src/app/` (3.16절, 3.17절). 데몬은 Nix로 빌드해 넣고, QML 파일은 그대로 포함 |
| 기타 직접 개발한 바이너리 | Nix로 빌드해 이미지에 포함. 데몬(`nix/build.nix`의 `tossd`)이 레퍼런스 |
| 활성화할 서비스 | `nix/build-rootfs.sh`의 `enable` 항목 |
| 호스트명, SSH 수신 대역·공개키, 시계 동기화·시간대, 키오스크 앱, 스트림덱 앱 포함 여부 | `flake.nix`의 `buildImage { … }` 인자 ([02-progress.md](02-progress.md) 2.6절 옵션 표 참고) |
| 기기 고유 값 (MAC 주소, SSH 호스트 키) | `local/device/rootfs/` 아래 동일 경로. git 비추적이므로 별도 백업 필수 |
| 사용자가 기기에서 설정한 값 (Wi-Fi 정보 등) | 저장소에 적지 않는다. 기기 `/data`에만 저장되며 재플래싱 후에도 유지됨 (3.15절 "기기에만 남는 데이터") |

- 반영 명령: `src/scripts/deploy.sh` (빌드 → Loader 진입 → 기록·검증 → 재부팅). `userdata`만 갱신할 때는 `src/scripts/deploy.sh userdata`.
- 동일 커밋에서는 바이트 단위로 똑같은 이미지가 나온다. 재현성 고정 요소는 [02-progress.md](02-progress.md) 2.6절을 참고하면 된다.
- 패키지를 추가할 때 설치 스크립트가 포함돼 있다면 이미지 빌드에서는 실행되지 않는다(같은 문서 2.6절 "빌드가 생략하는 작업"). 기기에서 `apk add`로 넣었을 때와 동작이 다르다면 이 부분을 의심해 봐야 한다.
- 재플래싱 시 사라지는 파일 중 백업할 것이 있으면 먼저 내려받는다: `src/scripts/tf-ssh.sh 'tar czf - /root /etc' > device-backup.tar.gz`. 이 아카이브에는 SSH 호스트 키 같은 기기 고유 값이 포함되므로 `*.tar.gz`는 `.gitignore`에 들어 있다(터치 덤프 같은 `*.bin`도 동일).

주의: `recovery` 슬롯의 순정 커널도 같은 루트 파일시스템을 마운트한다. 순정 커널에는 화면 콘솔과 GPU 드라이버가 없으므로, 화면에 의존하는 서비스는 순정 커널 부팅 시 동작하지 않도록 가드를 두었다(화면 앱은 뜨지 않고 대기). 새로 추가하는 서비스가 화면에 의존한다면 동일하게 분기 처리해야 한다.

## 3.7 화면 출력 방식

가벼운 방식부터 무거운 방식 순서다. **현재 기기는 C(Wayland 키오스크)로 구성돼 있으며 그 위에 스트림덱 UI가 뜬다.** A와 B는 디스플레이를 직접 점유하는 방식이라 C와 병행할 수 없다. 사용하려면 `kiosk.enable = false`로 빌드해야 한다.

### A. 프레임버퍼 직접 제어 (확인됨)

가장 단순한 방식이다. `kiosk.enable`을 끄면 화면에 뜨는 텍스트 콘솔이 여기에 해당한다.

- 콘솔 텍스트 출력: `echo hello > /dev/tty0`. ANSI 색상과 커서 제어 지원. 100열 × 80행.
- 픽셀 직접 쓰기: `/dev/fb0`은 800×1280, 픽셀당 4바이트(XRGB). 파일에 그대로 write하면 화면에 출력된다.
- 테스트: `cat /dev/urandom > /dev/fb0` (화면에 랜덤 노이즈 출력)

터치 입력은 `/dev/input/event*`를 직접 읽는다.

### B. DRM/KMS 직접 렌더링 (미검증)

컴포지터 없이 앱이 디스플레이를 직접 점유한다. 가장 가볍지만 앱을 해당 백엔드에 맞춰 작성해야 한다.

| 도구 | 메모 |
|---|---|
| SDL2 (`kmsdrm` 백엔드) | C/C++ 등. Alpine 패키지 `sdl2` |
| LVGL | 임베디드 UI 라이브러리. 프레임버퍼·DRM 백엔드 지원 |
| Qt (`eglfs`) | 현재 쓰는 Qt Quick을 컴포지터 없이 띄우는 백엔드. 컴포지터 메모리(약 75MB) 절감 가능 |
| `kmscube` | GPU 동작 확인용(확인됨). 이미지에 포함 |

GPU 가속(OpenGL ES 3.1) 사용 가능.

### C. Wayland 키오스크 (확인됨 — 현재 구성)

경량 컴포지터(`cage`) 위에 앱 하나를 전체 화면으로 띄운다. 일반 데스크톱용 툴킷(GTK, Qt, SDL 등)과 터치 입력을 그대로 쓸 수 있고, 앱이 종료되면 자동 재실행한다.

| 구성 요소 | 역할 |
|---|---|
| `cage` | 단일 앱 전체 화면 컴포지터. GPU(Panfrost) 렌더링. 메모리 약 75MB |
| `seatd` | 디스플레이와 입력 장치 권한을 컴포지터에 중계. 비루트 실행 지원 |
| `eudev` | 디바이스 관리자. 터치 입력을 읽는 libinput의 필수 의존성 |
| `/usr/sbin/tf-kiosk` | 위 구성을 묶어 띄우는 래퍼 스크립트. inittab이 호출 |
| `kiosk` 사용자 | 앱 실행 계정 (`seat`, `video` 그룹, 홈 `/home/kiosk`) |

**실행 앱 변경**: `flake.nix`의 `kiosk.command`를 수정하고 `src/scripts/deploy.sh userdata`. 현재는 스트림덱 UI(`toss-shell`)다. 터미널을 띄우려면 `"foot"`, 콘솔 로그인으로 되돌리려면 `kiosk.enable = false`.

**플래싱 전 테스트**: 실행 중인 앱을 내리고 다른 앱을 띄워 볼 수 있다. `tf-kiosk`가 종료되면 inittab이 원래 앱을 다시 띄우므로 테스트 중에는 커맨드 파일을 임시로 덮어쓴다(재플래싱 시 원래대로 복구됨).

```bash
src/scripts/tf-ssh.sh 'echo "<테스트할 명령>" > /etc/tossfront/kiosk.command && killall cage'
```

**화면 캡처** (확인됨): 현재 화면을 원격으로 캡처한다.

```bash
src/scripts/tf-ssh.sh "su -s /bin/sh kiosk -c 'XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-0 grim /tmp/shot.png'"
```

```bash
src/scripts/tf-ssh.sh --get /tmp/shot.png shot.png
```

참고 사항:

- 앱은 Wayland를 지원해야 한다. X11 전용 앱은 `xwayland` 패키지를 추가해야 뜬다(미검증). 로그에 `Cannot find Xwayland binary`가 남는 것은 해당 패키지가 없다는 뜻일 뿐 무해하다.
- 로그는 `/var/log/kiosk.log`, 직전 실행 로그는 `kiosk.log.1`이다. 앱이 안 뜨면 이 로그부터 확인한다.
- 앱에 환경 변수를 넘기려면 `kiosk.command`에 `env FOO=bar myapp` 형태로 적는다.
- GPU 이슈가 있으면 `env WLR_RENDERER=pixman cage …`로 CPU 렌더링으로 돌릴 수 있다(미검증).
- 폰트는 Noto Sans CJK가 포함돼 있어 한·중·일 글자가 정상 출력된다. 굵기는 Regular와 Bold 2종만 들어 있다(추가 굵기는 `font-noto-cjk-extra`, 208MB).
- 터치: 앱에 Wayland 터치 이벤트로 전달된다(확인됨: 네 모서리와 선 긋기가 터치 위치 그대로 찍혔다). 별도 보정이나 회전 설정은 필요 없다. 재검증이 필요하면 `weston-clients` 패키지의 `weston-simple-touch`(터치 좌표에 점을 그림)를 기기에 임시 설치해 `kiosk.command`로 띄우면 된다.
- 순정 커널(복귀용)로 부팅했을 때는 컴포지터를 띄우지 않는다.

GPU 가속 동작만 빠르게 확인하려면 `kmscube`(이미지에 포함)를 쓰면 된다. 화면에 회전하는 큐브가 출력되고 렌더러가 `Mali-T860 MC4 (Panfrost)`로 찍힌다(확인됨). `kmscube`는 디스플레이를 직접 점유하므로 컴포지터가 떠 있는 상태에서는 실행할 수 없다.

### D. 풀 데스크톱 환경 (미검증, 본 프로젝트 방향 아님)

Phosh, Plasma Mobile 같은 터치 친화적 환경이 Alpine 패키지로 제공된다(postmarketOS에서 주로 사용). GPU 가속은 되지만 2GB RAM 환경에서는 빠듯하다. 이 프로젝트는 단일 앱 전체 화면 구조이므로 채택하지 않았다.

### E. 웹 브라우저 키오스크 (미검증)

UI를 HTML로 작성해 키오스크 브라우저로 띄우는 방식이다. 개발은 가장 쉽지만 가장 무겁다. C의 컴포지터 위에 `cog`(WPE WebKit) 같은 경량 브라우저를 올리는 구성이 현실적이지만, 성능 우려로 제외했고 별도 측정도 하지 않았다.

### UI 프레임워크 선정 근거

Qt Quick과 Slint를 실기기에서 같은 화면 구성으로 비교 측정했다. 스트림덱 스타일 화면에서 Qt Quick이 주사율을 꽉 채웠고 CPU 사용률도 절반 수준이었다. 수치와 측정 조건은 [01-findings.md](01-findings.md) 1.5절 참고.

## 3.8 입력 처리

터치를 확인하는 가장 빠른 방법(확인됨: `evtest`가 기기에 설치돼 있음):

```bash
nix develop -c src/scripts/tf-ssh.sh 'evtest /dev/input/event0'
```

- 좌표 범위는 두 축 모두 0~16383이다. 화면 픽셀로 변환하려면 `x * 800 / 16384`, `y * 1280 / 16384`. libinput이나 SDL을 쓰면 자동으로 변환해 준다.
- 디스플레이 방향과 일치하도록 디바이스 트리에서 회전 보정을 적용해 두었다. 좌상단이 (0, 0)이다.
- 멀티터치 프로토콜 B(슬롯 기반)로 동작한다.
- 덤프해 두고 PC에서 분석하려면: 기기에서 `cat /dev/input/event0 > touch.bin`, PC에서 `src/scripts/parse-touch.py touch.bin`

## 3.9 PC 연동 구조 (스트림덱 핵심)

버튼을 눌렀을 때 PC에서 액션이 실행되는 구조는 **네트워크 통신으로 구현했다**. PC에서 도는 `toss-connector`가 덱에 접속하고(USB, 이더넷, Wi-Fi), 페어링과 암호화 세션을 거쳐 상태와 요청을 주고받는다. 상세 스펙은 [04-host-link.md](04-host-link.md)에 있고, 요약 사용법은 3.18절에 정리했다.

PC에 별도 프로그램을 깔지 않고 덱이 USB 키보드로 인식되게 하는 대안(미검증)은 [04-host-link.md](04-host-link.md) 4.14절에 적혀 있다.

## 3.10 앱 빌드 방법

기기는 **aarch64 + musl** 환경이다. PC(x86_64 + glibc)에서 빌드한 바이너리는 그대로 실행되지 않는다.

| 방법 | 설명 | 상태 |
|---|---|---|
| 기기에서 직접 빌드 | `tf-net.sh apk add build-base` 후 기기에서 컴파일. 가장 단순하지만 큰 프로젝트는 느림 | 미검증 |
| 스크립트 언어 | `apk add python3`, `lua` 등. 별도 빌드 불필요 | 미검증 |
| Go | PC에서 `GOOS=linux GOARCH=arm64 CGO_ENABLED=0 go build`. 정적 바이너리로 바로 동작 | 미검증 |
| Rust | 타깃 `aarch64-unknown-linux-musl`. 정적 바이너리로 바로 동작. devshell에 툴체인 포함(3.16절) | 확인됨: 데몬 `tossd`가 이 방식 |
| Nix 크로스 컴파일 | `pkgsCross.aarch64-multiplatform-musl.pkgsStatic.<패키지>`로 정적 바이너리 빌드. 동적 링크 산출물은 `/nix/store` 경로를 참조해 Alpine에서 그대로 돌지 않음 | 미검증 |
| nixpkgs aarch64 패키지 통째 복사 | 바이너리 캐시의 aarch64 빌드를 의존성과 함께 기기 `/nix/store`에 복사하면 동작(glibc 포함). 용량이 커서 테스트용 | 확인됨 (UI 프레임워크 비교 시 사용) |
| libc 없는 소형 C 바이너리 | `src/os/tools/`의 `reboot-mode`, `i2c-read`가 이 방식. `clang --target=aarch64-unknown-linux-gnu -static -nostdlib -ffreestanding` | 확인됨 |

**이미지에 포함할 바이너리는 PC에서 Nix로 빌드한다.** 기기에서 직접 빌드한 결과물은 재플래싱 시 사라지고 재현도 되지 않는다. 기기 라이브러리에 의존하지 않는 정적 바이너리(Rust, Go, nostdlib C)가 다루기 편하다. Alpine 패키지의 라이브러리(Qt 등)에 동적 링크해야 하는 프로그램은 기기 버전에 맞춰 크로스 컴파일해야 해 번거롭다. 스트림덱 UI를 QML로만 구성한 이유도 이 때문이다.

## 3.11 부팅 시 자동 실행

Alpine은 OpenRC를 사용한다.

**간단한 방법**: `/etc/local.d/`에 `.start` 확장자로 실행 스크립트를 둔다. 부팅 마지막 단계에 1회 실행된다(확인됨: USB 이더넷 DHCP가 이 방식).

**서비스 등록** (확인됨: 스트림덱 데몬이 이 방식): `src/app/rootfs/etc/init.d/tossd`

```sh
#!/sbin/openrc-run
description="TOSS daemon"
supervisor=supervise-daemon
command=/usr/bin/tossd
command_user="tossd:tossd"
output_log=/var/log/tossd.log
error_log=/var/log/tossd.log

depend() {
	need localmount
	after bootmisc tf-data
}

start_pre() {
	checkpath -f -o tossd:tossd -m 0644 /var/log/tossd.log
	for b in /sys/class/backlight/*/brightness; do
		[ -e "$b" ] && chgrp tossd "$b" && chmod 664 "$b"
	done
	return 0
}
```

(파일 상단 주석은 여기서 생략했다.)

- `supervisor=supervise-daemon`을 주면 프로세스가 죽었을 때 자동으로 재시작한다.
- `command_user`로 권한을 낮추면 **로그 파일을 미리 만들어 해당 사용자 소유로 넘겨야 한다**(`start_pre`의 `checkpath`). 그렇지 않으면 로그 파일을 열지 못해 시작 직후 종료된다.
- `after`의 `tf-data`는 데몬 데이터 디렉터리 `/data/toss`를 마운트하는 서비스이고(3.15절 "기기에만 남는 데이터"), `start_pre`의 나머지 루프는 백라이트 sysfs 파일 쓰기 권한을 데몬 그룹에 열어 주는 코드다(3.16절 "화면 밝기, 절전, 전원 끄기").
- 서비스 활성화는 기기에서 `rc-update add <이름> default`, 이미지 빌드에서는 `nix/build-rootfs.sh`의 `enable` 리스트에 넣는다.

**종료 시 재시작**: `/etc/inittab`에 `respawn` 라인을 추가한다. USB 시리얼과 화면 콘솔 getty가 이 방식이다.

**전체 화면 앱 실행**: 화면 전체를 쓰는 앱은 `kiosk` 옵션으로 올린다(3.7절 C). inittab의 `::respawn:/usr/sbin/tf-kiosk`가 컴포지터와 앱을 실행하고, 종료 시 재실행한다. `kiosk.enable`을 끄면 같은 자리에 로그인 프롬프트가 뜬다(`/usr/sbin/tf-console-getty`). 로그인 전 문구는 `/etc/issue`, 로그인 후 문구는 `/etc/motd`다.

## 3.12 개발 루프

스트림덱 앱 수정 시 작업 흐름(3.16절, 앱 추가 방법은 3.17절):

```bash
nix develop
```

devshell 안에서:

```bash
src/scripts/app-push.sh ui
```

```bash
src/scripts/app-push.sh tossd
```

앞 명령은 QML 파일을 올리고 UI를 재시작한다(수 초). 뒤 명령은 데몬을 기기용으로 크로스 빌드해 교체하고 서비스를 재시작한다. 수정이 끝나면 `src/scripts/deploy.sh userdata`로 이미지에 구워 확정한다.

다른 바이너리를 잠깐 올려 테스트할 때:

```bash
src/scripts/tf-ssh.sh --put ./myapp /root/myapp
```

```bash
src/scripts/tf-ssh.sh '/root/myapp'
```

- 화면 캡처: 3.7절 C
- 데몬 로그: `src/scripts/tf-ssh.sh 'tail -n 30 /var/log/tossd.log'`
- UI 로그(QML 에러 포함): `src/scripts/tf-ssh.sh 'grep -v "^00:" /var/log/kiosk.log'`
- 시스템 로그: `src/scripts/tf-ssh.sh 'tail -n 50 /var/log/messages'`, 커널 로그는 `dmesg`
- 화면 밝기: `src/scripts/tf-ssh.sh 'echo 120 > /sys/class/backlight/backlight/brightness'` (0~255)

## 3.13 트러블슈팅

| 증상 | 확인 및 조치 |
|---|---|
| SSH 접속 불가 | 덱 설정에서 SSH가 꺼져 있는지 확인(설정 → `SSH`). 아니면 `src/scripts/console.py 'ip addr; rc-status'`로 시리얼 콘솔에서 확인 |
| PC에 USB 장치 미인식 | 케이블(데이터 케이블 여부) 및 기기 전원 확인. `lsusb`에 `1d6b:0104`가 잡혀야 함 |
| `/dev/ttyACM0` 권한 에러 | 3.2절의 udev 규칙 확인 (PC 재부팅 후 사라졌을 가능성) |
| `apk` 인증서 에러 | 시스템 시각 문제(`date`로 확인). 전원을 뽑았다 켠 뒤 오프라인으로 부팅하면 2013년으로 초기화된다. 네트워크가 연결되면 자동 동기화되고, 아니면 `tf-net.sh`로 실행하면 PC 시각으로 맞춰 줌 |
| 루트 파일시스템 손상으로 부팅 실패 | initramfs가 USB 시리얼에 복구 셸을 띄운다. `console.py`로 들어가 복구하거나 `userdata` 재플래싱 |
| 커널 변경 후 부팅 실패 | 전원을 뽑았다가 초기화 버튼(하부 위쪽 버튼)을 누른 채 꽂으면 Loader 모드. `src/scripts/flash.sh --enter` 후 직전 정상 이미지를 `boot`에 다시 기록 (`src/scripts/flash.sh boot <이미지>`) |
| `flash.sh`(또는 `backup.sh`, `downgrade.sh`)에서 `[FAIL] MaskROM + 로더 적재 상태가 아닙니다` | 읽기와 쓰기는 로더를 올린 상태에서만 동작한다. `src/scripts/flash.sh --enter` 먼저 실행. 거기서 `[FAIL] 로더 파일이 없습니다`가 뜨면 `src/scripts/fetch-loader.sh`, `[FAIL] 기기에 명령을 보낼 수 없습니다`가 뜨면 3.2절 udev 규칙 확인 |
| `flash.sh`에서 `[FAIL] … 기록 실패` 또는 `되읽기 불일치` | 케이블과 전원을 점검하고 `src/scripts/flash.sh --enter`부터 재시도. 실패한 파티션은 불완전하게 쓰였을 수 있으므로 같은 이미지를 다시 기록. 도구 출력 말미가 `[FAIL]` 줄 위에 함께 출력됨 |
| 위 방법으로도 진입 불가 | `BOOT`–`VCC_1V8` 패드 쇼트로 MaskROM 진입 ([01-findings.md](01-findings.md) 1.2절) |
| 순정 커널로 임시 부팅 필요 | `src/scripts/tf-ssh.sh --reboot recovery` (1회성. 다음 재부팅 시 메인라인 복귀) |
| 직전 부팅 크래시 원인 확인 | 재부팅 후 `/sys/fs/pstore/`에 직전 커널 로그가 남는다. 전원을 차단하면 소실됨 |
| 부팅 후 연결한 Micro USB 미인식 | 기기가 켜진 상태에서 꽂으면 잡히지 않는 경우가 있었다. 기기를 재부팅한다. 상단바 USB 아이콘(삼지창 모양)이 초록색이면 정상 연결됨 |
| 화면에 앱이 안 뜨거나 재시작 반복 | `/var/log/kiosk.log` 및 `kiosk.log.1` 확인. `00:`로 시작하는 줄은 컴포지터 로그, 나머지가 앱 출력 |
| 화면이 흐려지고 `NO LINK TO TOSSD` 표시 (상단바 칩 아이콘 주황색 점멸) | 데몬 미실행 상태. `rc-service tossd status`, `/var/log/tossd.log`, `/var/log/messages`의 `supervise-daemon` 로그 확인 |
| 명령 위젯을 눌러도 PC에서 무반응 | 위젯이 비활성화(흐림) 상태면 해당 PC가 미연결이거나 그 명령을 제공하지 않는 상태다. 활성화 상태인데 동작하지 않으면 데몬 로그의 `host:` 줄 확인(`대상 호스트가 연결되어 있지 않음`, `액션 … 실패`). PC 측 거절 사유는 PC 로그에 남는다([04-host-link.md](04-host-link.md) 4.5절) |
| 화면 글자가 두부(□)로 깨짐 | 폰트 패키지 `font-noto-cjk`가 이미지에 포함됐는지 확인 (`fc-list \| grep -c CJK`) |
| 기기 상태가 저장소와 불일치 | `src/scripts/deploy.sh`로 재플래싱. 기기에서 직접 수정한 내용은 초기화되고 저장소 기준으로 복구됨 |

화면이 까맣고 백라이트만 켜져 있으면 커널 로그에서 "패널 전원 상태" 경고를 찾아본다. 점검 순서는 [01-findings.md](01-findings.md) 1.3절 "화면"에 있다.

## 3.14 현재 상태와 다음 단계

완료 항목:

1. **연결 및 접속**: Micro USB 단일 케이블로 SSH, 시리얼, 플래싱. LAN SSH 지원.
2. **화면 환경**: 부팅 시 컴포지터와 앱 자동 실행. GPU 가속 및 터치 정상 동작.
3. **앱 아키텍처**: Rust 데몬과 QML UI 분리 통신. 상태와 로직은 데몬이, 렌더링은 UI가 담당(3.16절).
4. **전역 스타일과 화면 구조**: 하드웨어 장비 톤을 따른 스타일, 4×6 타일 그리드 위에 랙과 복합 위젯 배치. 용어와 스타일 규칙은 3.16절 참고.
5. **위젯과 레이아웃 편집**: 미디어 제어, 리소스 미터(덱 자체 및 PC), 랙과 명령 위젯(버튼, 스위치, 노브). 레이아웃은 기기에서 직접 편집해 저장(3.16절). 위젯 추가 방법은 3.17절 참고.
6. **설정**: 화면 밝기, 테마, 글래스 효과, Wi-Fi 접속(3.15절), SSH 토글, 페어링, 절전, 전원 끄기.
7. **PC 연동**: 페어링, 암호화, 호스트 권한 제어, 재생 정보·제어, 리소스 모니터링, 등록 명령(실행, 스위치, 노브), 절전·웨이크업([04-host-link.md](04-host-link.md)).
8. **PC 커넥터**: `toss-connector`, 웹 콘솔, NixOS 모듈(설치, 자동 실행, 권한·명령 선언적 관리).
9. **빌드 및 배포**: 저장소 기준 전체 이미지 재현성 빌드 및 단일 명령 플래싱. 개발 중에는 플래싱 없이 푸시 테스트 가능.

남은 항목은 분야별로 분리해 정리했다:

| 분야 | 위치 |
|---|---|
| 앱·기기 미구현/과제 | [02-progress.md](02-progress.md) 2.15절 |
| PC 연동 미구현/과제 | [04-host-link.md](04-host-link.md) 4.13절 |
| 하드웨어 미확인 항목 | [01-findings.md](01-findings.md) 1.6절 |

## 3.15 네트워크 (Wi-Fi, 블루투스, 이더넷)

필요한 도구(`iw`, `wpa_supplicant`, `dhcpcd`, `bluez`)는 모두 이미지에 포함돼 있다.

### Wi-Fi

주변 AP 스캔 (확인됨):

```bash
nix develop -c src/scripts/tf-ssh.sh 'ip link set wlan0 up && iw dev wlan0 scan | grep SSID'
```

AP 접속: **기기 UI에서 설정한다.** 상단바 설정 버튼 → `Wi-Fi` → AP 선택 → 온스크린 키보드로 비밀번호 입력 후 `CONNECT`.

| 화면 | 표시 정보 | 동작 |
|---|---|---|
| `Wi-Fi` | 현재 상태(연결된 AP와 할당 IP, 연결 시도 중, 실패 사유) 및 스캔된 AP 목록(보안 아이콘, 신호 세기, 저장 여부) | `SCAN`으로 재스캔(약 5초간 버튼 비활성화, `SCANNING…` 표시). AP 탭 시 접속 화면으로 이동. 연결된 AP는 상단 상태 행을 탭해도 진입 가능 |
| 접속 (AP 이름이 제목) | AP 이름과 신호 세기. 미저장 AP면 비밀번호 입력란과 `Show password` | `CONNECT`로 접속(비밀번호를 8자 이상 63자 이하로 입력해야 활성화), 저장된 AP면 `FORGET`으로 삭제(확인 다이얼로그 표시) |

- 접속에 성공하면 해당 AP가 저장되고 다음 부팅부터 자동 연결된다. 실패하면(주로 비밀번호 오류) 저장하지 않고 `Could not connect. Check the password`를 표시한다. 잘못된 비밀번호는 약 20초 뒤에 실패로 판정된다(최대 30초). 연결 시도 중 다른 AP를 선택하거나 해당 AP를 삭제하면 진행 중이던 시도를 먼저 취소한다 — 검증되지 않은 비밀번호나 임시로 비활성화했던 기존 AP 상태가 설정 파일에 남지 않는다.
- 저장된 AP의 비밀번호를 바꾸려면 `FORGET`으로 삭제한 뒤 다시 접속한다.
- 기업용 인증(EAP), WPA3 전용, WEP AP는 `Not supported`로 표시되며 선택할 수 없다.
- IP 할당은 이미 상주 중인 `dhcpcd`가 담당한다(이더넷에서 검증됨). `/etc/network/interfaces`에 `wlan0`을 별도로 등록할 필요는 없다.

**Wi-Fi 멈춤 이슈 해결 내역**(2026-11-16): 5GHz AP에 붙었다가 삭제한 뒤 2.4GHz AP가 전혀 스캔되지 않았고, 그 상태에서 `wpa_supplicant`를 재시작하자 Wi-Fi가 먹통이 됐다. 확인 결과 무선 칩과 통신하는 SDIO 버스가 150MHz(순정 클럭)에서 에러를 내고 있었다.

| 조건 | 결과 |
|---|---|
| 150MHz | 커널 로그에 `brcmf_sdio_bus_sleep: error while changing bus sleep state -84`, `failed backplane access over SDIO, halting operation`. 부팅 직후에도 발생했고, 0.2초 간격 쿼리 시 350회 안에 터짐 |
| 50MHz | 동일 테스트에서 재현 안 됨 (런타임 클럭 하향 5500회, DTS 고정 후 재플래싱 뒤 3000회) |

이에 따라 DTS의 `sdio0` 클럭을 50MHz로 고정했다(`src/os/kernel/dts/rk3399-toss-front.dts`). 확인 명령: `src/scripts/tf-ssh.sh 'grep clock /sys/kernel/debug/mmc1/ios; dmesg | grep -c "bus sleep state"'` → `50000000 Hz`, `0`.

- 이 버스 에러는 해결됐지만 **처음 발견된 5GHz 이슈는 별개 문제이며 아직 남아 있다**(아래 참고).

**알려진 이슈: 5GHz AP 사용 후 2.4GHz AP 접속 불가** (2026-11-16, 2회 관찰, 원인 미확인). 5GHz AP에 붙었다가 끊긴 뒤(다른 AP 전환 또는 `FORGET`), 스캔이 5.7~5.8GHz 대역만 돌며 2.4GHz AP가 목록에 나오지 않는다. 인터페이스 채널이 161(5805MHz)에 고정돼 있다(`iw dev wlan0 info`). UI 목록에 직전 스캔의 2.4GHz AP가 잠시 남아 있을 수 있으나 선택하면 `Could not connect`로 끝난다. SDIO 클럭 하향으로 버스 에러가 0건인 상태에서도 동일하게 발생하므로 별개 이슈다. 무선 드라이버(`brcmfmac`) 또는 펌웨어 문제로 추정된다. **재부팅하면 정상 복구된다**(저장된 AP에 자동 접속). 5GHz AP에 계속 붙어 있는 동안에는 문제없다.

- Wi-Fi 통신 정상 여부를 점검할 때는 `ping`을 쓰지 않는다. 유선과 무선이 같은 서브넷에 있으면 응답이 유선으로 돌아와 결과가 왜곡된다. 대신 `arping -I wlan0 -c 3 <공유기 IP>`를 쓴다.

**검증 범위**(2026-11-16): 스캔과 목록 표시, 온스크린 키보드 입력, 잘못된 비밀번호 입력 시 실패 판정 및 미저장, 화면에서 비밀번호를 입력해 실제 접속(WPA2, 2.4GHz·5GHz), `dhcpcd` IP 수신, 루트 재플래싱 후 부팅 시 자동 재접속, 화면에서 AP 삭제(`FORGET`). 미검증 항목: 개방형(비밀번호 없음) AP, 접속 시도 중 다른 AP 선택이나 삭제 시 진행 중인 시도 취소(목 `wpa_supplicant` 단위 테스트로만 확인).

동작 구조:

| 역할 | 담당 |
|---|---|
| AP 스캔, 인증, 저장 | `wpa_supplicant`. 부팅 시 항상 실행된다(`/etc/local.d/30-wifi.start`). 저장된 AP와 비밀번호는 `/data/wifi/wpa_supplicant.conf`(권한 600, 디렉터리 700)에 기록된다. 이 경로는 `/etc/conf.d/wpa_supplicant`에서 지정한다 |
| UI 요청을 `wpa_supplicant`에 전달 | 데몬 `tossd`(`src/app/tossd/src/wifi.rs`). 제어 소켓 `/run/wpa_supplicant/wlan0`에 명령을 보낸다. 소켓이 `tossd` 그룹에 열려 있어 데몬이 비루트여도 동작한다 |
| IP 할당 | `dhcpcd` |
| UI | 설정 패널의 `WifiPage`, `WifiJoin` (3.16절) |

비밀번호는 UI → 데몬(로컬호스트 `127.0.0.1`) → `wpa_supplicant`로만 전달되며, 데몬은 비밀번호를 로그에 남기지 않는다(확인됨: 테스트 후 로그에 미기록).

**저장된 AP와 비밀번호는 기기 내부에만 남는다.** 이미지에도 PC에도 포함되지 않으며, 재플래싱 후에도 유지되는 `/data`(아래 "기기에만 남는 데이터")의 `/data/wifi/wpa_supplicant.conf`에 저장된다. 재부팅 후에도, 루트 재플래싱 후에도 설정이 남아 자동 접속되는 것을 확인했다.

UI 없이 수동으로 추가하려면 기기에서 `wpa_passphrase '<AP 이름>' >> /data/wifi/wpa_supplicant.conf`로 네트워크 블록을 추가하고(비밀번호 입력 프롬프트 표시) `rc-service wpa_supplicant restart`를 실행한다. 파일 상단 세 줄(`ctrl_interface`, `update_config`, `country`)은 부팅 때마다 맞춰지므로 손대지 않아도 된다.

Wi-Fi가 연결되면 기기가 직접 인터넷에 나가므로 `tf-net.sh` 없이 `apk`를 쓸 수 있다.

**보안 주의**: 현재 root 계정에 비밀번호가 없고, SSH는 모든 인터페이스(USB·유선·무선)에서 수신한다. 같은 공유기에 물린 기기라면 누구든 root로 접속할 수 있다는 뜻이다. **이제 화면에서 아무 Wi-Fi에나 붙을 수 있으므로 신뢰할 수 없는 네트워크(카페, 공용 Wi-Fi)에 연결하면 해당 네트워크의 누구나 root로 들어올 수 있다.** 공유기가 외부 접속을 막고 있고 내부망을 신뢰할 수 있다는 전제하에 열어 둔 것이다(사용자 결정). 전제가 달라지면(공용·공유 네트워크 접속, 공유기 포트 포워딩 등) 아래 중 하나를 먼저 적용해야 한다.

- **덱 설정에서 SSH를 끈다**: 설정 → `NETWORK` → `SSH`. SSH 서버가 내려가 모든 인터페이스에서 접속이 차단되고, 재부팅이나 재플래싱 후에도 닫힌 상태가 유지된다. 개발할 때만 열면 된다(3.16절 "SSH 켜고 끄기"). 가장 간편한 방법이다.
- USB 전용으로 제한: `/etc/conf.d/dropbear`를 `DROPBEAR_OPTS="-p 172.16.42.1:22 -B"`로 수정 후 `rc-service dropbear restart`
- 비밀번호 설정: `passwd` (이후 `-B` 제거 가능)
- 공개키 인증 전용: `/root/.ssh/authorized_keys`에 공개키 등록 후 `-B` 대신 `-s`

설정 파일은 `/etc/conf.d/dropbear` 한 곳이다. `-p <주소>:22`가 있으면 해당 IP에서만, 없으면 모든 인터페이스에서 수신한다. 위 방법은 기기에서 즉시 적용하는 임시 조치이며 재플래싱 시 초기화된다. 영구 반영하려면 `flake.nix`의 `buildImage` 인자 중 `ssh.lan`과 `ssh.authorizedKeys`를 수정하고 `src/scripts/deploy.sh userdata`로 올린다.

### 기기에만 남는 데이터 (`/data`)

루트 파일시스템은 플래싱할 때마다 통째로 교체된다. 플래싱과 무관하게 기기에 남아야 하는 데이터(사용자가 기기에서 설정한 값)는 `/data`에 둔다. 이미지에도 PC의 `local/device/rootfs/`에도 들어가지 않는다(재현성 대상 아님).

| 항목 | 내용 |
|---|---|
| 위치 | eMMC `cache` 파티션(384MB, 가용 공간 약 343MB). 순정 안드로이드가 쓰던 파티션이며 이 시스템은 덮어쓰지 않는다(`deploy.sh`는 `boot`, `recovery`, `userdata`만 기록) |
| 마운트 | 서비스 `tf-data`(`src/os/rootfs/etc/init.d/tf-data`, `boot` 런레벨). 파티션을 이름으로 찾아 fsck 후 `/data`에 마운트하고, 앱 디렉터리 `/data/toss/`를 만들어 데몬 사용자 소유로 넘긴다. 데몬 서비스(`tossd`)는 이 서비스 이후에 시작된다 |
| 파일시스템 | 기존 ext4를 **그대로** 사용한다(포맷하지 않음). 따라서 안드로이드가 남긴 디렉터리(`backup`, `customer`, `recovery` 등)가 함께 보인다. 건드리지 않으면 된다. 원본 내용은 `local/backup/cache.img`에 있다 |
| 현재 저장 데이터 | `/data/wifi/wpa_supplicant.conf`(저장된 Wi-Fi AP와 비밀번호), `/data/toss/`(화면 밝기와 SSH 활성 여부를 담은 `settings.json`, 덱 신원 키 `key`와 페어링 호스트 목록 `hosts.json` — [04-host-link.md](04-host-link.md) 4.9절 —, 사용자가 편집한 레이아웃 `layout.json` — 3.16절 "레이아웃 편집", 외부 연동 키를 암호화해 둔 금고 `vault.json`과 그 키 `vault.key`(둘 다 0600) — [04-host-link.md](04-host-link.md) 4.15절, 웹페이지 아이콘 버튼의 사이트 아이콘 `icons/`) |
| 추후 추가 대상 | UI 표시 설정(테마, 글래스 효과) — 현재는 미저장 |

- 파일시스템이 손상돼 마운트에 실패하면 서비스가 새로 포맷한다(기존 데이터 소실). 파티션 자체가 없거나 포맷 후에도 마운트에 실패하면 `/data`는 루트 위 디렉터리로 남으며, 이 경우 재플래싱 시 함께 초기화된다. 그 상태에서도 `/data/toss/`는 만들어지므로 앱은 다음 플래싱 전까지 정상적으로 읽고 쓴다.
- 확인 명령: `src/scripts/tf-ssh.sh 'mountpoint /data && df -h /data'`
- **`/data/toss/`의 파일은 원자적으로 기록한다.** 같은 디렉터리의 임시 파일(`<이름>.new`)에 쓰고 fsync까지 마친 뒤 이름을 바꾼다. 쓰는 도중 전원이 끊겨도 이전 파일이나 새 파일 중 하나가 온전히 남는다.
- **파싱할 수 없는 파일은 덮어쓰지 않고 백업한다.** 이름 뒤에 `.bad`를 붙여 치워 두고(`layout.json.bad`, `hosts.json.bad`, `settings.json.bad`, `key.bad`, `vault.json.bad`, `vault.key.bad`) 데몬 로그에 사유를 남긴 뒤, 해당 파일이 없는 것처럼 기본 상태로 기동한다(기본 레이아웃, 빈 페어링 목록, 새 키). 규칙에 어긋난 레이아웃도 동일하다. 구현은 `src/app/tossd/src/persist.rs`에 있다.
- 데몬은 시작 시 `/data/toss` 쓰기 가능 여부를 확인해 불가능하면 로그에 한 줄 남긴다(`…에 쓸 수 없습니다`). 이 경우 레이아웃, 페어링, 설정은 재부팅 시 유지되지 않는다.
- 기기를 초기 상태로 되돌리려면(저장된 AP 전체 삭제) 기기에서 `/data/wifi` 디렉터리를 지우고 재부팅한다.
- `/data` 내용은 PC에 백업되지 않는다. 기기를 분실하거나 `cache` 파티션을 덮어쓰면 소실된다.

### 블루투스

주변 기기 스캔 (확인됨). `bluetoothd` 없이 커널에 직접 질의하는 도구다.

```bash
nix develop -c src/scripts/tf-ssh.sh 'btmgmt le on; btmgmt power on; timeout 15 btmgmt find'
```

페어링과 프로파일(키보드, 오디오 등)을 쓰려면 `bluetoothd`와 D-Bus가 필요하다: `apk add dbus`, `rc-service dbus start`, `rc-service bluetooth start` 후 `bluetoothctl` (미검증).

### 이더넷

케이블을 꽂으면 자동으로 IP를 받는다(확인됨: 공유기 연결 시 100Mbps 링크, DHCP, 인터넷 통신). `dhcpcd` 서비스가 백그라운드에서 동작하므로 케이블 없이 부팅해도 대기 지연이 없고, 나중에 꽂아도 바로 잡힌다.

- 할당된 IP 확인: `ip -4 addr show eth0`
- 이더넷이 연결돼 있으면 기기가 직접 인터넷에 나가므로 `tf-net.sh` 없이 `apk add …`를 쓸 수 있다.
- `dhcpcd`는 `eth0`과 `wlan0`만 관리한다(`/etc/dhcpcd.conf`의 `allowinterfaces`). USB 이더넷은 건드리지 않는다.
- SSH는 LAN IP에서도 수신한다(확인됨: PC에서 기기 LAN IP로 접속). `TF_HOST=<기기 IP> src/scripts/tf-ssh.sh`. Wi-Fi 절의 보안 주의사항 참고.
MAC 주소는 `/etc/tossfront/eth0.mac`에 적힌 값으로 부팅 시 고정된다. 변경하려면 해당 파일을 수정하고 재부팅한다.

## 3.16 스트림덱 앱 (`src/app/`)

화면에 뜨는 스트림덱 앱은 두 프로세스로 나뉜다. 로직과 시스템 제어는 전부 데몬에 두고, UI는 전달받은 상태를 렌더링만 한다. **이 앱 하나가 화면 전체를 전담한다**(상단바, 위젯, 설정, 온스크린 키보드). 별도로 도는 패널이나 런처는 없다.

| 구성 요소 | 위치 | 기술 스택 | 기기 내 배치 |
|---|---|---|---|
| 데몬 `tossd` | `src/app/tossd/` | Rust. 기기용 정적 바이너리로 크로스 빌드 | `/usr/bin/tossd`, OpenRC 서비스 `tossd`(사용자 `tossd`), 로그 `/var/log/tossd.log` |
| UI | `src/app/ui/` | 순수 QML (네이티브 코드 없음). Qt Quick이 GPU로 렌더링 | `/usr/share/toss/ui/`, `toss-shell`이 실행 (키오스크가 구동) |
| 기기 배치 파일 | `src/app/rootfs/` | 서비스 스크립트, 런처 스크립트, root 헬퍼 `toss-helper`(전원, 절전, SSH) | 루트의 동일 경로 |
| 리소스 측정 | `src/app/meter/` | Rust (`toss-meter`). 덱과 PC가 공용 | 데몬에 내장 |
| 호스트 프로토콜, PC 커넥터 | `src/app/proto/`, `src/app/host/` | Rust | 덱에는 프로토콜만 데몬에 내장. [04-host-link.md](04-host-link.md) 참고 |

### 현재 지원 기능

| 기능 | 내용 | 상세 설명 |
|---|---|---|
| 위젯 | 미디어 제어(4×2), 리소스 미터(덱 자체 및 PC의 CPU·메모리·GPU·디스크·네트워크·온도, 크기 4종), 랙과 내부 명령 위젯 4종(버튼, 스위치, 단계 노브, 연속 노브), 웹페이지 버튼 2종(일반, 사이트 아이콘) | 아래 "앱과 위젯" |
| 레이아웃 | 페이지당 4×6 타일. 기기에서 직접 편집해 기기에 저장. 초기 레이아웃은 1페이지에 미디어 제어와 리소스 미터(덱 자체 온도, 메모리, 로드) 배치 | 아래 "레이아웃 편집" |
| 설정 | 화면 밝기, 테마, 글래스 효과, Wi-Fi, SSH 켜고 끄기, 페어링, 외부 연동 키(덱 금고) 상태와 PC별 허용, 레이아웃 편집·초기화, 절전, 전원 끄기 | 아래 "UI 구성" |
| PC 연동 | 페어링된 PC의 재생 정보 표시 및 제어. PC 리소스 사용량 표시. 등록된 명령 실행 및 스위치·노브 조작. PC 브라우저에서 웹페이지 열기. 외부 연동 키를 덱에 보관하고 PC에 토큰만 빌려주기(Discord). PC에서 덱 슬립·웨이크업 제어 | [04-host-link.md](04-host-link.md) |

### UI 구성

위에서부터 상단바, 페이지(타일 그리드), 페이지 표시 순이다. 설정과 정보 시트는 상단바 아래로 펼쳐지며 페이지를 덮는다.

- 상단바의 인터페이스 상태는 텍스트 라벨 없이 아이콘형 인디케이터로 표시한다(왼쪽부터 USB, LAN, WLAN, BT, CORE. CORE는 데몬 연결 상태이며 칩 아이콘이다). 비활성이면 흐린 회색, 활성이면 인디케이터 색상으로 점등된다. 초기엔 LED 옆에 텍스트 라벨을 붙였다가 아이콘형으로 바꿨다(2026-11-16).
- 상단바 아이콘을 탭하면 해당 인터페이스의 정보 시트(`shell/LinkInfo`)가 상단바 아래로 펼쳐진다. 같은 아이콘을 다시 누르거나 시트 바깥을 누르면 닫히고, 다른 아이콘을 누르면 내용만 전환된다. 열려 있는 아이콘 아래에는 짧은 밑줄이 표시된다. 표시 정보: USB(가젯 상태, 링크, IP, MAC), 이더넷(링크와 속도, IP, MAC), Wi-Fi(연결 AP, 신호 세기와 링크 속도, 대역과 주파수, 보안 방식, IP, MAC), 블루투스(어댑터 존재 여부만 — 미구현), 데몬(연결 상태, 버전, 소켓, 호스트명, IP).
- 상단바 오른쪽 끝 설정 버튼(정사각형, 48×48px)은 터치 영역이 버튼 외형보다 넓다(위쪽과 오른쪽은 화면 끝까지). 누르면 설정 패널이 열리고, 열려 있는 동안 버튼은 토글 버튼처럼 반쯤 눌린 상태를 유지한다. 다시 누르면 닫힌다. 데몬과 연결이 끊겨 있는 동안에는 눌러도 열리지 않는다.
- 설정 패널(`shell/SettingsPanel`)은 상단바 아래 전체를 덮는다. 랙 형태가 아니라 설정 화면 전용 레이아웃이다: 상단에 제목 `SETTINGS`, 그 아래에 섹션(`SettingsSection`: 인쇄된 라벨과 연한 배경면)이 쌓이고, 섹션 내 각 행(`SettingsRow`)은 왼쪽에 이름과 설명, 오른쪽에 컨트롤이 배치된다. 행 전체가 터치 영역이다. 초기에는 위젯 프레임(나사 포함)에 위젯 스타일 버튼을 넣었다가, 설정 화면답지 않아 개편했다(2026-11-16). 패널은 위치가 이동하지 않고 위에서 아래로 블라인드가 열리듯 펼쳐진다(`Theme.tPanel`, 220ms). 헤더와 뒤로가기 버튼, 콘텐츠 영역, 펼침 애니메이션은 추가 시트와 공용 프레임(`shell/DropPanel`, 그 베이스 `shell/DropSheet`)을 쓰며, 첫 화면 설정 목록은 `shell/SettingsMain`에 있다. 패널을 눌러도 아래 페이지로 터치가 통과하거나 스와이프되지 않는다.
- **데몬과 연결이 끊기면 열려 있던 오버레이가 모두 닫힌다**: 설정 패널, 확인 다이얼로그, 편집 모드(추가 시트와 이름 변경 다이얼로그 포함). 연결이 끊긴 동안에는 아무리 눌러도 데몬에 전달되지 않으므로 조작 가능한 것처럼 보이는 화면을 남기지 않는다. 연결 끊김 화면(`shell/LinkLost`)이 페이지를 덮고, 설정 버튼은 재연결될 때까지 동작하지 않는다. 인터페이스 정보 시트는 끊긴 상태에서도 열린다(데몬 연결 상태를 그곳에서 확인하기 때문).

현재 제공되는 설정 항목은 아래 표와 같다.

| 섹션 | 항목 | 기능 |
|---|---|---|
| `DISPLAY` | `Brightness` | 화면 밝기(슬라이더). 기기에 저장됨 |
| | `Dark mode` | UI 테마(라이트/다크). 미저장 상태라 재시작 시 라이트로 초기화 |
| | `Glass effect` | 디스플레이 글래스 반사 효과. 미저장 |
| `NETWORK` | `Wi-Fi` | AP 선택 및 비밀번호 입력 접속(3.15절) |
| | `SSH` | 덱 SSH 활성화/비활성화(토글). 기기에 저장됨 |
| `LAYOUT` | `Edit layout` | 레이아웃 편집 모드 진입 |
| | `Reset layout` | 기본 레이아웃으로 초기화(확인 다이얼로그) |
| `DEVICES` | `Paired devices` | 페어링된 PC 목록, 신규 페어링, 삭제(확인 다이얼로그. [04-host-link.md](04-host-link.md) 4.4절) |
| `INTEGRATIONS` | 서비스(현재는 `Discord`) | 덱 금고에 보관한 키의 상태(없음, 승인 전, 연결됨), 금고를 쓰도록 허용한 PC 목록과 해제(`REVOKE`), 키 삭제(`FORGET KEY`, 확인 다이얼로그). 키는 여기서 입력하지 않고 PC에서 보낸다([04-host-link.md](04-host-link.md) 4.15절) |
| `POWER` | `Sleep` | 절전 진입(확인 다이얼로그). Wi-Fi 연결 시에만 가능 |
| | `Power off` | 전원 종료(확인 다이얼로그) |

### 용어 정리

코드, 문서, 작업 전반에서 아래 용어로 통일한다.

| 용어 | 의미 | 코드 식별자 |
|---|---|---|
| **타일** | 위젯을 배치하는 기본 격자 칸. 페이지마다 4×6으로 배치됨. 한 칸이 약 178px(2.1cm) | `GRID_COLS`, `GRID_ROWS`(데몬), `tileW`, `tileH`(UI) |
| **페이지** | 타일 한 판. 좌우 스와이프로 전환. 동적 확장 가능 | `Page` |
| **앱** | 기능 그룹. 예: Media. 위젯 추가 시 먼저 앱을 선택 | `AppInfo` (`catalog`의 한 항목) |
| **위젯 종류** | 앱이 제공하는 위젯의 한 종류. 예: `media.player`. 차지하는 타일 크기가 정해져 있으며 한 앱에 여러 개가 있을 수 있음 | `Kind` (프로토콜의 `kind`) |
| **위젯** | 위젯 종류를 타일에 배치한 인스턴스. 개별 설정을 가짐(예: 대상 PC 선택) | `Widget`, `src/app/ui/widgets/` |
| **랙** | 버튼 위젯을 담는 컨테이너(그룹·폴더 역할). 타일 여러 개를 차지하며 라벨이 있음. 내부 슬롯 1칸이 타일 1칸에 해당 | `Rack`, `src/app/ui/shell/Rack.qml` |
| 버튼 위젯 | 랙 슬롯에 놓이는 소형 위젯: 버튼이나 노브 1개와 하단 라벨. **반드시 랙 내부에만 배치됨** | `form: "button"`, `CommandWidget` 등(공통 베이스 `CommandBase`) |
| 스위치 버튼 위젯 | 온/오프 상태를 갖는 버튼 위젯. 켜져 있으면 버튼이 눌린 상태를 유지하고 인디케이터가 점등됨 | `kind: "command.switch"`, `CommandSwitchWidget` |
| 노브 위젯 | 노브 1개로 구성된 버튼 위젯. 단계 노브(고정 단계 선택)와 연속 노브(범위 내 값 조절)가 있음. 라벨 아래에 현재 값 표시 | `kind: "command.stepped"`, `"command.linear"` |
| 복합 위젯 | 단일 버튼으로 표현할 수 없는 위젯. 위젯 프레임 안에 아래 컴포넌트를 조합해 구성. **자체가 랙과 동격으로 페이지에 놓이며 다른 위젯을 자식으로 담지 않음** | `MediaWidget`, `MeterWidget` |
| 버튼 (컴포넌트) | 누를 수 있는 인터랙티브 요소. 토글 상태가 있는 경우(스위치 버튼) 켜진 동안 반쯤 눌린 상태 유지(`held`) | `kit/Button`, 분리된 라벨 포함 버전은 `kit/LabeledButton` |
| 노브 (컴포넌트) | 돌려 값을 조절하는 원형 컨트롤. 터치 화면에서는 직선 드래그로 조작 | `kit/Knob` |
| 디스플레이 (컴포넌트) | 다양한 정보를 자유롭게 출력하는 표시 창 | `kit/Display` |
| 인디케이터 (컴포넌트) | 상태를 알리는 소형 램프. 아이콘 형태는 "아이콘 인디케이터"로 구분(작업하며 붙인 이름) | `kit/Indicator`, `kit/IconIndicator` |
| 위젯 프레임 | 랙과 복합 위젯의 외곽 몸체. 컴포넌트를 담는 영역 | `kit/WidgetFrame` |
| 상단바 | 화면 최상단 영역. 시각·날짜, 인터페이스 상태(아이콘 인디케이터), 설정 버튼 | `shell/TopBar` |

UI와 데몬을 합친 프로그램 전체는 "스트림덱 앱"으로 부른다. "위젯 프레임"과 "상단바"는 작업하며 붙인 이름이다. 기기는 "덱", 덱과 연결되는 PC는 "호스트"라 부른다(역할 기준 명칭. 프로그램 고유 이름은 [README.md](README.md) "이름" 참고: 앱 TOSS, 데몬 `tossd`, UI `toss-shell`, PC 커넥터 `toss-connector`).

### 앱과 위젯

사용자가 추가할 수 있는 위젯은 4개 앱의 위젯 종류 11가지다(데몬 `catalog`). 앱 선택 → 위젯 종류 선택 → 위젯 설정 순서로 추가한다.

| 앱 | 위젯 종류 | 배치 위치 | 설정 항목 | 동작 |
|---|---|---|---|---|
| `Media` | `media.player` | 페이지, 4×2 | 대상(`Source`): PC(미지정 시 연결된 첫 PC) 또는 `Spotify` | 재생 곡 앨범 아트·메타데이터, 진행 바, 7개 버튼(반복, 이전 곡, 5초 뒤로, 재생·일시정지, 5초 앞으로, 다음 곡, 셔플). `Spotify`를 고르면 덱이 Spotify 계정을 직접 조회하고 제어한다([04-host-link.md](04-host-link.md) 4.15절 "Spotify"). Spotify가 대상이면 키가 좋아요 / 셔플 / 이전 / 재생 / 다음 / 반복 / 대기열로 바뀌고(5초 탐색 대신 좋아요와 대기열), 다음 곡과 재생 기기 이름이 표시된다. 기기 줄을 누르면 기기 목록, 대기열 키를 누르면 대기열 창이 뜬다(`shell/SpotifySheet`). `Spotify theme`을 켜면 덱의 하드웨어풍 테마 대신 Spotify 앱의 재생 화면처럼 그린다(`widgets/SpotifyPlayer`, 아래 "Spotify 플레이어") |
| `Media` | `media.combo` (PC + Spotify) | 페이지, 4×2 | 대상 PC | 평소에는 그 PC의 미디어 위젯이다. 그 PC가 Spotify를 재생 중이면(덱이 조회한 Spotify에 곡이 있고, PC 플레이어 이름이 `spotify`로 시작하거나 곡 제목이 같으면) Spotify로 바뀌고 버튼도 Spotify로 보낸다. `Spotify theme`(이 위젯은 처음부터 켜짐)이 켜져 있으면 Spotify 플레이어(`widgets/SpotifyPlayer`)로, 꺼져 있으면 덱 테마의 미디어 위젯에서 Spotify 기능만 쓰는 모양으로 바뀐다. 곡이 바뀔 때의 짧은 어긋남 때문에 PC 화면으로 돌아갈 때는 6초 기다린다(`shell/WidgetSlot`). 이 위젯이 있으면 덱이 Spotify를 조회한다 |
| `Resource monitor` | `monitor.2x1`, `monitor.2x2`, `monitor.4x1`, `monitor.4x2` | 페이지, 이름과 동일한 크기 | 대상 장치(덱 자신 또는 PC), 표시 메트릭(크기별 최대 3·6·6·12개) | 리소스 사용량 미터 (아래 "리소스 미터") |
| `Command` | `command.run` | 랙 슬롯 | PC, 해당 PC에 등록된 명령, 라벨, 색상 | 버튼: 탭 시 명령 1개 실행 |
| | `command.switch` | 랙 슬롯 | 위와 동일 | 스위치 버튼: 온/오프 전환. 상태는 PC가 보고 |
| | `command.stepped` | 랙 슬롯 | 위와 동일 | 단계 노브: 지정 단계 중 선택 |
| | `command.linear` | 랙 슬롯 | 위와 동일 | 연속 노브: 범위 내 값 조절 |
| `Web page` | `web.button` | 랙 슬롯 | PC, 주소(`http://` 또는 `https://`), 라벨, 색상 | 버튼(`Simple`): 탭 시 그 PC의 기본 브라우저에서 주소를 연다 |
| | `web.icon` | 랙 슬롯 | 위와 동일 | 아이콘 버튼(`Icon`): `web.button`과 같되 버튼 윗면에 사이트 아이콘을 그린다 |

- **명령 위젯 4종은 PC에 등록된 명령 1개를 가리킨다.** 실행 내용, 단계/범위, 현재 상태는 모두 PC가 관리한다. 덱은 위젯 추가 시 골라 매핑만 한다([04-host-link.md](04-host-link.md) 4.6절 "명령").
- **Spotify 플레이어**(`widgets/SpotifyPlayer`): 사용자 요청으로 이 위젯만 덱의 디자인 규칙(위젯 틀, 나사, 디스플레이, 돌출 버튼, 직각 모서리)을 따르지 않는다. 둥근 어두운 카드에 앨범 아트의 대표 색에서 Spotify 검정(#121212)으로 이어지는 그라데이션을 깔고, 흰 굵은 제목, 회색 아티스트, 초록 재생 기기 줄, 가는 진행 막대(흰 채움과 손잡이), 틀 없는 아이콘 제어 줄(가운데 흰 원형 재생 버튼, 켜진 셔플·반복·좋아요는 초록과 아래 점)을 그린다. 기능과 보내는 액션은 Spotify 모드의 `MediaWidget`과 같다.
- 상태가 없는 실행 버튼(`command.run`)에는 인디케이터가 없고, 온/오프 상태가 있는 스위치에는 있다. 노브는 라벨 아래에 현재 값을 표시한다.
- 스위치는 추가할 때 인디케이터 색상(`Light`)을 고를 수 있다: `AUTO`(버튼 색상에 맞춤: 빨간 버튼은 빨강, 주황 버튼은 주황, 나머지는 초록), 초록, 주황, 빨강, 파랑. 위젯 설정의 `light`에 저장된다(`Theme.indicatorColors`). 이미 놓은 위젯의 설정은 바꿀 수 없으므로, 색상을 바꾸려면 위젯을 지우고 다시 추가한다.
- UI 구현은 위젯 종류별로 `src/app/ui/widgets/` 아래 파일이 대응된다: `MediaWidget`, `MeterWidget`(리소스 미터 4종 공용), `CommandWidget`, `CommandSwitchWidget`, `CommandSteppedWidget`, `CommandLinearWidget`(명령 위젯 4종 공통 베이스 `CommandBase`). 종류와 QML 파일 매핑은 `shell/WidgetSlot`의 `sources`에 정의돼 있다.
- 해당 PC가 미연결이거나 위젯이 쓰는 것을 제공하지 않는 상태(권한 해제, 명령 삭제)에서는 위젯이 비활성화(흐림)돼 눌리지 않는다. 명령 위젯이 가리키는 명령의 종류가 위젯과 어긋난 경우에도 비활성화된다(버튼은 `run`, 스위치는 `switch`, 노브는 `stepped`·`linear`).
- **웹페이지 버튼은 그 PC가 `web.open` 권한을 허용했을 때만 눌린다**(기본은 꺼짐. [04-host-link.md](04-host-link.md) 4.5절). 주소는 위젯 설정에 저장되며 데몬이 `http://`, `https://` 주소만 받는다(공백·제어 문자 없이 512바이트 이하). PC도 같은 검사를 다시 한다.
- **사이트 아이콘은 덱이 직접 가져온다**(`src/app/tossd/src/icons.rs`. 덱이 인터넷에 연결되어 있어야 한다). 페이지의 `apple-touch-icon`, `icon`, 웹 앱 매니페스트의 `icons`와 사이트 루트의 `/apple-touch-icon.png`, `/apple-touch-icon-precomposed.png`, `/favicon.ico`를 모아 크기가 큰 것부터 하나씩 받아 보고, 화면이 그릴 수 있는 이미지(PNG, JPEG, GIF, ICO)인 첫 번째 것을 쓴다. SVG와 WebP는 건너뛴다. 받은 아이콘은 `/data/toss/icons/`에 보관하고, 아직 없거나 가져오지 못했으면 지구본 모양을 그린다. 실패한 주소는 10분 뒤 다시 시도한다.
- 새 위젯 종류 추가 방법은 3.17절을 참고한다.

### 리소스 미터 (앱 Resource monitor)

덱 자체 또는 PC의 리소스 사용량을 미터로 표시한다. 대상 장치와 항목은 위젯별로 설정한다.

| 위젯 종류 | 크기 | 메트릭 수 | 배치 구조 |
|---|---|---|---|
| `monitor.2x1` | 2×1 | 1~3 | 1열 |
| `monitor.2x2` | 2×2 | 1~6 | 1열 |
| `monitor.4x1` | 4×1 | 1~6 | 2열 (3 / 3) |
| `monitor.4x2` | 4×2 | 1~12 | 2열 (6 / 6) |

- **대상 선택**: 덱 자신(`This deck`, 설정 `host`가 `@deck`) 또는 페어링된 PC(해당 PC ID). 라벨에 대상 이름이 표시된다.
- **메트릭 선택**: 해당 장치에서 수집 가능한 메트릭 목록 중 최대 수만큼 고른다. 선택한 순서대로 위에서 아래(2열이면 왼쪽 열부터)로 채워진다. 설정 `metrics`에 ID 배열로 저장된다.
- **개별 행**: 짧은 이름, 게이지 바(12칸, 끝 2칸은 경고색), 수치. 게이지는 현재 값을 메트릭 상한으로 나눈 비율이다. 온도는 30°C를 하한으로 잡으며, 상한이 없는 메트릭(네트워크 속도 등)은 관측된 최댓값에 맞춰 스케일링한다.
- **표시 중인 메트릭만 수집한다.** 데몬이 레이아웃에서 "어느 대상의 어떤 메트릭을 구독 중인지" 추출해(`Layout::watched`), 덱 자체 항목은 직접 측정하고(`src/app/meter`, 1초 주기) PC 항목은 해당 호스트에 구독 요청을 보낸다([04-host-link.md](04-host-link.md) 4.6절 "리소스 모니터링"). 위젯을 삭제하면 해당 메트릭 수집도 중단된다.
- 대상 PC가 오프라인이거나 메트릭 제공을 끈 상태면 행이 흐려지고 수치 자리에 `--`가 뜬다. 라벨 줄 끝 인디케이터로 데이터 수신 상태를 확인할 수 있다.
- 덱에서 수집 가능한 항목: CPU 사용률(전체 및 코어 6개), 온도, 클럭 주파수, 로드 애버리지, 메모리, GPU 온도, 디스크 I/O 속도, 파일시스템 사용량(`/`, `/data`), 네트워크 속도. PC 측 항목은 04번 문서 표 참고.
- 추가 후 대상이나 메트릭을 변경하는 UI는 아직 없다(삭제 후 재추가).
- 관련 코드: 데몬 `layout/mod.rs`(위젯 종류, `watched`) 및 `state.rs`(덱 리소스 측정), UI `widgets/MeterWidget.qml` 및 `shell/AddPanel.qml`(대상·메트릭 선택).

### 레이아웃 편집

홈 화면 편집과 동일한 UX다. 설정 → `Edit layout`으로 진입하고, 상단 `DONE`으로 빠져나온다.

| 동작 | 방법 |
|---|---|
| 이동 | 위젯이나 랙을 드래그해 놓는다. 누른 지점에서 8px 이상 움직여야 드래그가 시작된다(그 위의 `×`·`+`를 탭하는 것만으로는 들리지 않음). 타일 격자에 스냅되며, **빈 칸에만 배치된다**(배치 가능 영역 초록색, 불가 영역 빨간색, 기존 위젯 밀어내기 없음) |
| 다른 페이지로 이동 | 드래그한 채 화면 좌우 가장자리에 잠시 머물면 페이지가 넘어간다. 편집 중에는 맨 뒤에 빈 페이지가 하나 더 열려 있어 거기 놓으면 새 페이지가 추가된다 |
| 삭제 | 모서리 `×` 버튼. 복합 위젯과 빈 랙은 즉시 삭제되고, **위젯이 들어 있는 랙은 확인 다이얼로그가 뜬다**(`REMOVE RACK`: 랙과 내부 위젯 N개가 삭제되며 되돌릴 수 없음을 안내) |
| 랙 크기 조절 | 우하단 핸들을 드래그한다. 다른 위젯과 겹치거나 내부 버튼이 잘리는 크기로는 바뀌지 않는다. 복합 위젯은 크기가 고정돼 있다(다른 크기는 별도 위젯 종류로 분리됨) |
| 랙 이름 변경 | 라벨 영역 탭. 영문은 대문자로 입력된다. 온스크린 키보드는 현재 영문만 활성화 |
| 추가 | 상단 `+` → 앱 → 위젯 종류 → (설정 있으면) 설정 선택. 현재 페이지의 첫 빈 자리에 배치되며, 공간이 부족하면 새 페이지에 놓인다. 랙도 이 메뉴에서 추가 |
| 랙에 버튼 추가 | 랙 빈 슬롯의 `+` → 앱 → 버튼 위젯 종류 → 설정 선택. 현재는 `Command` → `Run`(명령 버튼), `Switch`(명령 스위치), `Stepped knob`(단계 노브), `Linear knob`(연속 노브) 제공: PC, 해당 PC의 명령(위젯 종류에 맞는 것만 노출), 라벨, 색상 선택 |
| 랙 내 버튼 삭제 | 해당 슬롯의 `×`(즉시 삭제). 버튼을 다른 슬롯으로 옮기는 기능은 미구현(삭제 후 재추가) |
| 초기화 | 설정 → `Reset layout`(확인 다이얼로그) |

- **편집 모드 중에는 위젯이 클릭되지 않는다.** 페이지 전환은 빈 영역을 스와이프한다. 데몬 연결이 끊기면 편집 모드는 자동 종료된다.
- **수정 시마다 즉시 저장된다.** 실행 취소(Undo)는 없다. 편집 모드를 종료할 때 빈 페이지는 자동 정리된다.
- **레이아웃의 진실 공급원(SoT)은 데몬이다.** UI는 드래그 프리뷰만 그리고, 드롭 시 데몬에 편집 요청을 보낸다. 데몬이 유효성 규칙(`Layout::check`)을 통과한 요청만 저장하고 모든 UI 클라이언트에 새 레이아웃을 브로드캐스트한다. 규칙 위반이거나 알 수 없는 편집 요청이면 현재 레이아웃을 다시 보내 UI를 원복한다.
- **저장 경로는 `/data/toss/layout.json`이다.** 재플래싱 후에도 유지된다(3.15절 "기기에만 남는 데이터"). 사용자 설정 데이터이므로 저장소에는 들어가지 않는다. 파일이 없으면 기본 레이아웃(`default_layout`)으로 시작하고, 파싱 실패나 규칙 위반 시 `layout.json.bad`로 백업한 뒤 기본 레이아웃으로 시작한다(3.15절 "기기에만 남는 데이터").
- **위젯별 설정** (위 "앱과 위젯" 표 참고). 미디어 제어 위젯은 "대상 PC"를 선택한다: 미지정(`Any connected PC`)이면 연결된 첫 PC를 따라가고, 지정하면 해당 PC만 고정한다(미연결 시 `No host`). 리소스 미터는 대상 장치와 메트릭을, 명령 위젯은 PC와 명령을 선택한다. 메트릭이 수십 개여도 `ADD` 버튼은 헤더 바에 고정돼 항상 누를 수 있다. **추가 후 설정 변경 화면은 아직 없다**(삭제 후 재추가).
- 관련 코드: 데몬 `layout/`(편집 처리 `edit.rs`의 `Edit`, 저장 `store.rs`의 `Store`, 위젯 카탈로그 `catalog.rs`의 `catalog`), UI `shell/Pager`(페이지와 타일, 배치 가이드와 드래그 프리뷰), `shell/Placed`(배치된 항목 위치와 편집 오버레이: `×`, `+`, 크기 조절 핸들), `shell/LayoutDrag`(드래그 상태와 규칙), `shell/EditBar`(편집 모드 상단바), `shell/AddPanel` 및 `shell/AddOption`(추가 시트와 옵션 컨트롤), `shell/RenameDialog`. 삭제 확인은 `Main.qml`에서 처리한다.
- 자동화 테스트: 탭은 `src/scripts/tf-tap.sh`, 드래그는 `src/scripts/tf-drag.sh`(경유 지점 유지 시간은 `HOLD`로 지정).

### 화면 밝기, 절전, 전원 끄기

설정의 `DISPLAY` → `Brightness`(슬라이더)와 `POWER` → `Sleep`, `Power off`(모두 확인 다이얼로그)다. 데몬이 비루트 사용자(`tossd`)로 동작하므로 별도 경로로 권한을 위임했다.

| | 제어 방식 | 정의 위치 |
|---|---|---|
| 밝기 | 백라이트 밝기 파일(`/sys/class/backlight/backlight/brightness`, 0~255)을 데몬 서비스 기동 시 `tossd` 그룹에 열어 둔다. 데몬이 해당 파일에 직접 write | `src/app/rootfs/etc/init.d/tossd`의 `start_pre` |
| 전원 | root 권한 헬퍼가 FIFO(`/run/toss-helper`, root와 `tossd` 그룹만 쓰기 가능)를 열고 대기한다. 데몬이 `poweroff`를 쓰면 헬퍼가 `poweroff`를 실행한다(서비스를 순차 종료하는 정상 종료). 받는 명령은 `poweroff`, `sleep`, `ssh on`, `ssh off` 4종뿐이다. 헬퍼는 FIFO를 열어 둔 채 수신한 명령을 순차 처리한다 — 앞선 작업 중 들어온 명령도 유실되지 않고 순서대로 실행된다. FIFO 생성에 실패하면 종료하고 서비스가 재실행한다 | `src/app/rootfs/usr/libexec/toss-helper`, 서비스 `toss-helper` |
| 절전 | 같은 FIFO에 `sleep`을 쓴다. 헬퍼가 Wi-Fi 매직 패킷 웨이크업을 켜고(`iw phy … wowlan enable magic-packet`) 얕은 절전(`echo freeze > /sys/power/state`)에 진입한다. 데몬은 슬립 직후 5초 안에 들어온 `sleep` 요청은 버린다 — 연타된 요청이 깨어난 직후 덱을 다시 재우는 것을 방지 | 위와 동일 |

- **밝기는 기기에 저장된다.** 슬라이더에서 손을 뗀 최종값을 `/data/toss/settings.json`에 기록해 두고 데몬 기동 시 복원한다. 재플래싱 후에도 유지된다.
- **최소 밝기 하한을 두었다**(최댓값의 4%, 현재 10). 백라이트가 완전히 꺼지면 화면을 보고 다시 켤 수 없기 때문이다.
- **절전은 얕은 절전(s2idle)만 사용한다.** 깊은 절전은 이 기기에서 깨어나지 못한다([01-findings.md](01-findings.md)의 "절전 및 네트워크 웨이크업").
- **잠든 덱을 깨우는 방법은 하나뿐이다: Wi-Fi 매직 패킷.** PC에서 `toss-connector wake`로 전송한다([04-host-link.md](04-host-link.md) 4.10절). 이더넷으로는 깨어나지 않고 화면을 터치해도 깨어나지 않는다(터치 패널이 웨이크업 소스로 미등록). 깨어나지 않을 때는 재부팅 버튼을 누른다.
- 따라서 **Wi-Fi가 연결된 상태에서만 절전에 들어간다.** 미연결 상태면 설정의 `Sleep` 행이 비활성화되고, PC에서 요청해도 덱이 거절한다(`no_wifi`).
- 절전 진입 경로는 두 곳이다: 덱 설정, PC의 `toss-connector sleep`.
- 관련 코드: 데몬 `device.rs`, UI `kit/Slider`와 `shell/SettingsMain`.
- 헬퍼(`toss-helper`)는 SSH 켜고 끄기도 담당한다(아래 참고).
- **되돌릴 수 없거나 덱을 쓸 수 없게 되는 작업은 확인 다이얼로그를 거친다**(`shell/ConfirmDialog`). 화면 중앙에 영향 범위와 `CANCEL`, 실행 버튼이 뜨고 바깥 영역을 눌러도 닫힌다. 실행 버튼은 비가역적 작업이면 빨간색, 복구 가능한 작업(절전)이면 주황색이다. 초기에는 "한 번 더 누르면 실행"(인디케이터 점멸) 방식으로 만들었다가 사용자 피드백을 반영해 다이얼로그로 변경했다(2026-11-17). 다이얼로그 호출은 `Main.qml`이 담당한다(`confirm.ask(제목, 설명, 버튼 라벨, 액션, 색상)`). 설정 화면에서 확인이 필요한 작업을 추가할 때는 `confirmAsked(제목, 설명, 버튼 라벨, 액션, 색상)` 시그널을 발생시키면 된다 — 색상은 버튼 색 이름이며 비우면 빨간색이 된다. 현재 확인 다이얼로그를 거치는 작업:

| 작업 | 다이얼로그 제목 | 안내 문구 요약 | 실행 버튼 |
|---|---|---|---|
| 전원 끄기 (설정 → `Power off`) | `POWER OFF` | 덱 전원이 꺼짐 | `POWER OFF` (빨강) |
| 절전 (설정 → `Sleep`) | `SLEEP` | 덱이 절전 모드로 들어가며 페어링된 PC에서 `toss-connector wake`로 깨워야 함 | `SLEEP` (주황) |
| 레이아웃 초기화 (설정 → `Reset layout`) | `RESET LAYOUT` | 추가·이동한 위젯, 랙, 페이지가 모두 삭제되며 되돌릴 수 없음 | `RESET` (빨강) |
| 페어링 해제 (설정 → `Paired devices` → 대상 기기 → `FORGET`) | `FORGET DEVICE` | 해당 PC 페어링이 해제되며 다시 쓰려면 재페어링 필요 | `FORGET` (빨강) |
| 저장된 AP 삭제 (설정 → `Wi-Fi` → 대상 AP → `FORGET`) | `FORGET NETWORK` | 해당 AP 정보를 삭제해 자동 접속되지 않으며, 재접속 시 비밀번호 재입력 필요(개방형 AP는 자동 접속 해제만 안내) | `FORGET` (빨강) |
| 위젯이 포함된 랙 삭제 (편집 모드 `×`) | `REMOVE RACK` | 해당 랙(라벨이 있으면 라벨명 표시)과 내부 위젯 N개가 삭제되며 되돌릴 수 없음 | `REMOVE` (빨강) |

- 확인 버튼을 누르면 해당 작업을 실행하고, 삭제 작업 2종은 목록 화면으로 복귀한다. 빈 랙, 복합 위젯, 랙 내부의 단일 버튼 위젯은 확인 없이 즉시 삭제된다.
- 전원 끄기는 사용자가 실기기에서 실행해 전원이 내려갔다가 다시 켜진 이력이 있다(데몬 로그에 요청 기록 및 이후 재부팅 확인). 재기동 시 전원 버튼을 눌렀는지 전원을 재인가했는지는 확인되지 않았다.

### SSH 켜고 끄기

설정의 `NETWORK` → `SSH`(토글)다. 덱 SSH는 root 무비밀번호 상태로 USB와 네트워크에 열려 있으므로(3.15절 보안 주의사항 참고), 쓰지 않을 때 닫아 둘 수 있게 구현했다.

| | |
|---|---|
| 끄면 | SSH 서버(dropbear)를 중지하고 기존 접속도 모두 끊는다. 22번 포트를 완전히 닫는다 |
| 차단 범위 | **USB 인터페이스까지 함께 닫힌다.** LAN/Wi-Fi만 차단하려면 방화벽이 필요한데 이 커널에는 없다(넷필터 규칙 기능 제외하고 빌드). "USB IP에서만 리슨" 방식은 같은 망에서 정적 라우트를 넣으면 우회 접근이 가능해 완전한 차단으로 보기 어렵다 |
| 저장 | `/data/toss/settings.json`의 `"ssh": false`. 재플래싱하거나 재부팅해도 닫힌 상태가 유지된다. 값이 없으면 활성 상태 |
| 부팅 시 | SSH 서버 설정(`/etc/conf.d/dropbear`)의 `start_pre` 체크가 설정 파일을 읽어 닫혀 있으면 SSH 서버를 띄우지 않는다. 부팅 도중 잠깐 열리는 구간도 없다. 정의 위치는 `nix/build-rootfs.sh` |
| 제어 흐름 | 데몬이 root 헬퍼 FIFO(`/run/toss-helper`)에 `ssh on` 또는 `ssh off`를 쓰고, 전달이 성공했을 때만 설정 파일에 기록한다(실제 제어에 실패한 값을 기록해 현재 상태와 재부팅 후 상태가 어긋나는 것을 방지). 헬퍼가 `rc-service dropbear start` / `stop`을 실행하며, 끌 때는 남아 있는 dropbear 세션도 종료한다 |
| UI 표시 | 설정 파일 값이 아니라 **실제 SSH 서버 구동 상태**를 표시한다(데몬이 1초마다 확인). 토글 후 실제 상태 전환이 반영되면 UI 토글이 넘어간다 |

- **SSH를 꺼 두면 개발 스크립트도 동작하지 않는다**: `tf-ssh.sh`, `app-push.sh`, `tf-tap.sh`, 그리고 `deploy.sh`의 Loader 진입 단계. 개발이나 플래싱 시에는 덱 설정 화면에서 먼저 켜야 한다. UI가 뜨지 않을 때는 시리얼 콘솔(`src/scripts/console.py`)로 들어가 `/data/toss/settings.json`의 `"ssh"`를 `true`로 수정하고 `rc-service dropbear start`를 실행하거나, 초기화 버튼으로 Loader에 진입해 플래싱한다.
- 호스트(PC) 통신(포트 `7879`)은 SSH와 무관하다. SSH를 닫아도 `toss-connector` 연결은 정상 유지된다.
- 인증 방식(무비밀번호 root)은 그대로다. 켜져 있는 동안의 보안 특성은 기존과 동일하다. 공개키 인증만 허용하려면 `flake.nix`의 `ssh.authorizedKeys`를 설정하면 된다.
- 관련 코드: 데몬 `device.rs`(`set_ssh`), 헬퍼 `src/app/rootfs/usr/libexec/toss-helper`, UI `shell/SettingsMain`.

### 데몬–UI 통신 프로토콜

두 프로세스는 WebSocket(`ws://127.0.0.1:7878`)으로 JSON 텍스트 메시지를 주고받는다. 스펙 정본은 `src/app/tossd/src/ui_proto.rs` 상단에 있다. 데몬은 상태가 **변경될 때만** 푸시하며, 새로 접속한 UI 클라이언트에는 현재 전체 스냅샷을 보낸다.

| 방향 | 메시지 | 의미 |
|---|---|---|
| 데몬 → UI | `hello` | 접속 직후 1회. 데몬 버전 |
| 데몬 → UI | `catalog` | 접속 직후 1회. 추가 가능한 앱과 앱별 위젯 종류(배치 형태 `form`: 페이지용 `widget` vs 랙 슬롯용 `button`, 크기, 설정 항목과 타입 `host`·`command`·`metrics`·`text`·`url`·`color`·`light`·`flag`(켜기/끄기, 처음 값은 `initial`). `when_host`가 있으면 그 대상을 골랐을 때만 보이는 설정이다. `host`에 `own`이 있으면 덱 자신 선택 가능, `command`에는 선택 가능한 명령 종류 `filter`, `metrics`에는 최대 개수 `max` 포함) |
| 데몬 → UI | `layout` | 레이아웃 상태. 접속 직후 및 변경 시마다 전송. 그리드 크기(`cols`×`rows`)와 페이지별 랙 목록(라벨, 타일 좌표·크기, 내부 버튼 위젯들) 및 복합 위젯 목록. 위젯마다 종류(`kind`), 타일 좌표·크기, 라벨, 버튼 색, 위젯 설정(`settings`) 포함 |
| UI → 데몬 | `layout_move`, `layout_resize`, `layout_rename`, `layout_add`, `layout_put`, `layout_remove`, `layout_tidy`, `layout_reset` | 레이아웃 편집 명령(편집 모드): 이동, 랙 크기 조절, 랙 이름 변경, 페이지에 추가, 랙 슬롯에 버튼 추가, 삭제, 빈 페이지 정리, 기본 레이아웃 복구. 데몬은 유효성 검증 통과 시에만 저장하고 새 `layout`을 브로드캐스트한다. 거절되거나 잘못된 `layout_*` 메시지 수신 시 현재 `layout`을 재전송해 동기화한다 |
| UI → 데몬 | `action` | 호스트 액션 실행 요청 (`host`, `name`, `args`). `host`가 비어 있으면 연결된 첫 호스트로 라우팅된다. `widget`에 위젯 ID를 넘기면 실행 결과가 `result`로 돌아온다. 전송 불가 시(대상 호스트 미연결) 즉시 실패(`no_host`) 반환 |
| 데몬 → UI | `result` | 위젯 액션 실행 결과: 요청 위젯(`widget`), 단조 증가 시퀀스(`seq`), 성공 여부(`ok`), 실패 사유(`error`). 요청을 받은 호스트가 보낸 응답만 전달한다(명령 스위치는 거절 시 인디케이터가 빨간색으로 점멸) |
| 데몬 → UI | `values` | 호스트에 등록된 스위치·노브 현재 값(호스트 ID → 명령 ID → 값). 변경 시 전체 전송 |
| 데몬 → UI | `meters` | 수집 가능한 메트릭 목록(대상 → 목록). 대상은 `@deck`(덱 자신) 또는 호스트 ID. 리소스 미터 추가 시 선택용 |
| 데몬 → UI | `meter_values` | 메트릭 현재 값(대상 → 메트릭 ID → 값). **배치된 리소스 미터가 구독 중인 항목만** 약 1초마다 변경 시 전송 |
| 데몬 → UI | `state` | 시스템 상태: 호스트명(`host`), IP(`ip`), USB·유선·무선 링크 상태(`links`), 인터페이스별 IP·MAC·속도(`net`). 변경 시만 전송 (온도, 로드, 메모리 등 리소스 수치는 `meters`·`meter_values`로 전달) |
| 데몬 → UI | `device` | 덱 하드웨어 제어 상태: 화면 밝기와 범위, 전원/절전 가능 여부(`can_power`), SSH 활성 여부(`ssh`) 및 제어 가능 여부(`can_ssh`). 변경 시만 전송 |
| UI → 데몬 | `ssh` | 덱 SSH 활성화/비활성화 요청(`on`). 실제 반영 결과는 `device`의 `ssh` 필드로 회신 |
| UI → 데몬 | `brightness` | 화면 밝기 변경(`value`). `keep`이 true면 파일에 저장해 재부팅 후에도 유지(드래그 중엔 false, 릴리스 시 true) |
| UI → 데몬 | `sleep` | 덱 절전 진입 요청. Wi-Fi 연결 상태에서만 진입 |
| UI → 데몬 | `power_off` | 덱 전원 종료 요청 |
| 데몬 → UI | `wifi` | Wi-Fi 상태: 사용 가능 여부, 단계(`idle`·`connecting`·`connected`·`failed`), 연결 AP와 IP·주파수·보안·신호 세기, 스캔 중 여부, 실패 사유, 스캔된 AP 목록(SSID, 키, 신호, 보안 방식, 저장 여부). 변경 시만 전송 |
| UI → 데몬 | `wifi_scan` | 주변 AP 재스캔 요청 |
| UI → 데몬 | `wifi_connect` | AP 접속 요청. `key`(목록 식별 키)와 필요 시 `psk`(비밀번호) |
| UI → 데몬 | `wifi_forget` | 저장된 AP 삭제 요청 |
| 데몬 → UI | `hosts` | 페어링된 호스트 목록(이름, OS, 덱에 허용된 권한, 연결 상태와 경로, 재생 정보, 앨범 아트) 및 페어링 진행 상태(페어링 모드 여부, 요청 호스트와 검증 코드). 변경 시만 전송. 호스트가 보낸 이름·OS는 데몬이 길이와 문자를 정제해 전달 |
| 데몬 → UI | `vault` | 덱 금고 상태: 서비스별 키 보관 여부(`registered`), 토큰 갱신 가능 여부(`linked`), 금고를 쓰도록 허용한 호스트(`hosts`), 화면에서 허용 여부를 묻는 중인 요청(`asking`. 없으면 `null`). 키 값은 오지 않는다 |
| UI → 데몬 | `vault_answer`, `vault_revoke`, `vault_forget` | `asking`에 답하기(`allow`), 호스트의 금고 사용 허용 해제(`service`, `host`), 서비스 키와 토큰 삭제(`service`) |
| 데몬 → UI | `icons` | 웹페이지 아이콘 버튼이 표시할 사이트 아이콘(주소 → `data:` URI). 가져오지 못한 주소는 빠진다. 변경 시 전체 전송 |
| 데몬 → UI | `spotify` | 덱이 직접 조회하는 Spotify 계정의 재생 상태: 승인 여부(`linked`), 마지막 조회 성공 여부(`live`), 재생 정보(`media`, 형식은 호스트의 것과 같음), 앨범 아트(`art`), 실패 사유(`error`), 재생 기기 이름(`device`), 좋아요 여부(`liked`), 대기열(`queue`), 기기 목록(`devices`), 앨범 대표 색(`color`). UI(`DaemonLink`)는 이것을 호스트와 같은 모양(`id: "@spotify"`)으로 바꿔 미디어 위젯에 넘긴다. 그 위젯의 액션은 `host: "@spotify"`로 보내며 덱이 처리한다 |
| UI → 데몬 | `pair_start`, `pair_stop`, `pair_accept`, `pair_reject`, `host_forget` | 페어링 모드 시작·중지, 수락·거절, 페어링 삭제. `pair_accept`에는 화면에 표시된 검증 코드(`code`)를 함께 실어 보내며 현재 후보 코드와 다르면 무시한다(탭하는 순간 후보가 다른 PC로 바뀔 수 있는 경쟁 상태 방지). `pair_stop`과 `pair_reject`는 대기 중인 PC에 `rejected` 알림 전송 |

호스트(PC) 측 프로토콜은 [04-host-link.md](04-host-link.md) 4.6절에 정리돼 있다.

### UI 스타일 가이드

스타일은 실제 하드웨어 장비의 물리적 원리만 차용한다. 텍스처나 사실적 묘사는 배제한다. 상세 용어와 규칙은 `Theme.qml` 상단 주석에도 적혀 있다.

- **광원은 상단에 둔다.** 음각 영역은 위쪽 모서리에 그림자가 지고 아래쪽 모서리에 하이라이트가 맺힌다.
- **튀어나온 요소는 버튼뿐이고, 들어간 요소는 디스플레이 내부 콘텐츠뿐이다.** 버튼은 전면 패널 컷아웃 위로 돌출돼 아래쪽 측면과 그림자가 보이며, 누르면 홈 안쪽으로 들어간다. 스위치 버튼은 켜져 있는 동안 반쯤 눌린 상태를 유지한다. 디스플레이는 커버 유리가 전면 패널과 평면으로 맞물려 있고 콘텐츠는 그 아래 깊은 면에 배치된다. 깊이감은 그림자만으로 표현한다: 창 상단과 좌측 가장자리가 콘텐츠 위로 그림자를 드리운다(컷아웃 측벽을 별도로 그려 봤으나 테두리가 두꺼워 보여 제거함).
- **화면에 배치되는 최상위 단위는 랙과 복합 위젯이며 둘 다 위젯 프레임을 몸체로 쓴다.** 프레임에는 입체 높이가 없다: 연한 면과 가는 선으로 구역만 나누고 네 귀퉁이에 작은 나사만 둔다. 프레임 자체에는 그림자를 넣지 않는다.
- **버튼 위젯은 라벨과 버튼 영역을 분리한다.** 랙 장비처럼 위에는 색상 버튼만 두고, 아래에는 패널에 인쇄된 라벨을 둔다. **인디케이터는 상태를 가진 버튼에만 둔다**: 온/오프 상태가 있는 스위치 버튼은 라벨 왼쪽에 인디케이터가 있고, 일회성 실행만 하는 명령 버튼에는 없다. 버튼은 가로로 긴 형태며 크기는 어느 랙에서나 동일하다(132×80px, `Theme.rackButtonW`·`rackButtonH`). 다행 랙은 행 높이가 더 넓어 남는 세로 공간이 행간 여백이 된다. 버튼이 작기 때문에 라벨을 포함한 슬롯 전체가 터치 영역이다.
- **노브는 원형 버튼이다.** 일반 버튼과 같은 홈에서 같은 높이로 돌출돼 있고(아래쪽 측면과 그림자 표현), 상단 지시선이 현재 값을 가리킨다. 눈금은 패널에 인쇄된 짧은 선이며 현재 값에 해당하는 눈금만 진하게 표시한다(단계 노브는 현재 단계 1개, 연속 노브는 시작부터 현재 값까지). 별도 색상은 쓰지 않는다. 노브 위젯은 버튼 위젯과 짝을 이룬다: 위에 노브, 아래에 라벨, 그 밑에 작게 현재 값(`kit/LabeledKnob`). 터치 조작은 직선 드래그를 회전에 매핑한다(오른쪽·위쪽이 증가 방향).
- **누를 수 있는 요소는 모두 버튼, 가변 데이터는 모두 디스플레이에 담는다.** 텍스트, 리스트, 이미지는 디스플레이 안에 플랫하게 그린다. 디스플레이 역시 테마를 따른다: 라이트 테마에서는 밝은 배경에 어두운 글자(반사형 액정 느낌), 다크 테마에서는 어두운 배경에 밝은 글자다.
- **상태는 인디케이터로 표시한다.** 소등, 점등, 점멸(진행 중).
- **모서리는 직각에 가깝게 깎는다.** 실제 장비 패널과 버튼처럼 2~3px만 라운딩한다. 화면 전체 외곽 라운딩은 기기 베젤에 맞춘 예외다.
- **색상은 절제한다.** 패널 바탕은 무채색으로 두고, 색상은 버튼 컬러 7종과 인디케이터 시맨틱 컬러(초록 정상, 주황 진행·경고, 빨강 에러·녹화, 파랑)에만 사용한다.

라이트 테마와 다크 테마 두 벌을 제공한다. 설정 패널(상단바 설정 버튼)의 `Dark mode` 토글로 전환한다. 디스플레이 유리 반사광 효과도 설정의 `Glass effect` 토글로 끌 수 있다(`Theme.glass`). **두 표시 설정은 아직 저장되지 않는다**: UI를 재시작(재부팅 포함)하면 기본값(라이트 테마, 글래스 효과 켜짐)으로 돌아간다.

스타일 작업은 이 기준으로 마무리했다(2026-11-16). 실기기 화면을 직접 확인하면서 수치와 비율을 조정한 결과다.

| 결정 항목 | 값 |
|---|---|
| 화면 배치 단위 | 랙(버튼 위젯 컨테이너)과 복합 위젯. 둘 다 높이 없는 위젯 프레임에 작은 나사 |
| 버튼 | 컷아웃에서 5px 돌출돼 있으며 연한 그림자. 랙 내부에서는 132×80px, 아래에 인디케이터와 인쇄된 라벨 |
| 디스플레이 | 평면 커버 유리 아래 콘텐츠. 깊이는 가장자리 그림자로만 표현하고 유리에 약한 반사광 |
| 모서리 라운딩 | 직각에 가깝게 (2~3px) |
| 콘텐츠 영역 여백 | 좌우 24px, 상단 27px |
| 상단바 시계 | 12시간 형식(AM/PM), 날짜·요일도 시각과 동일한 진하기 |

시행착오 끝에 버린 디자인안:

| 시도했던 안 | 폐기 이유 |
|---|---|
| 위젯 프레임이 돌출되고 내부 디스플레이가 파인 형태(초기안) | 올라갔다가 다시 내려가는 계단이 부자연스러움 |
| 프레임 없이 버튼만 번지는 그림자로 떠 있는 형태 | 음각 디스플레이와 플로팅 버튼이 이질적으로 보임 |
| 랙 장비풍 (위젯별 금속 패널, 브러시드 메탈, 검은 컷아웃, 유리 디스플레이) | 보기에 좋지 않았음. 재질 텍스처 묘사는 배제하기로 결정 |
| 버튼 위젯 1개가 타일(랙) 전체를 채우는 형태 | 복합 위젯과 톤이 안 맞음. 랙 내부에 소형 버튼과 라벨을 분리하는 구조로 변경 |
| 큰 라운딩 (프레임 18px, 버튼 14px) | 실제 하드웨어 장비는 직각 기반 |
| 버튼 윗면 가로 헤어라인 | 없는 편이 깔끔함 |
| 디스플레이 내부에 컷아웃 측벽 그리기 | 테두리가 두꺼워 보임. 그림자만 남김 |

### 온스크린 키보드 (입력기)

텍스트 입력은 Qt 입력기 **Qt Virtual Keyboard**(`qt6-qtvirtualkeyboard`)를 쓴다. 앱 프로세스 내부에서 동작하므로 별도 IME 데몬이 없고, 언어별 레이아웃과 조합기(한국어, 일본어, 중국어 병음·주음, 태국어)가 패키지에 포함돼 있다.

| 구성 | 위치 |
|---|---|
| 입력기–입력창 바인딩 | `src/app/rootfs/usr/bin/toss-shell`의 `QT_IM_MODULE=qtvirtualkeyboard` |
| 키보드 컨테이너 | `shell/KeyboardPanel`. 화면 하단에서 올라온다. 표시되는 동안 설정 패널과 추가 시트 콘텐츠 영역이 그만큼 줄어든다(`shell/DropPanel`의 `keyboardCover`) |
| 활성 언어 | `KeyboardPanel`의 `locales`. 현재 `["en_US"]`(영문·숫자·기호). `["en_US", "ko_KR"]` 등으로 늘리면 키보드 지구본 키로 전환 가능(**다른 언어는 미테스트**) |
| 키 스타일 | `src/app/ui/QtQuick/VirtualKeyboard/Styles/toss/style.qml`. 일반 버튼과 동일한 컴포넌트(`kit/Button`)로 렌더링. 디렉터리 이름과 depth는 Qt 규약이며, `toss-shell`의 `QML_IMPORT_PATH`와 `QT_VIRTUALKEYBOARD_STYLE=toss`로 지정한다 |
| 입력창 | `kit/TextField` |

- **버튼 클릭으로 입력창 텍스트를 읽을 때는 먼저 포커스를 해제해야 한다**(`TextField`의 `blur()`). 조합 중(preedit)이거나 단어 추천 중인 글자는 아직 `text` 프로퍼티에 확정 반영되지 않기 때문이다. `blur()`는 조합 중인 텍스트를 커밋하고 키보드를 내린다. 이름 변경 다이얼로그의 `SAVE`와 추가 시트의 `ADD`가 이 방식을 쓰며, 추가 시트는 닫힐 때도 입력을 마무리한다(`AddPanel`의 `leave()`). 키보드 엔터 키는 자체적으로 조합을 확정하고 `accepted()` 시그널을 낸다.
- 실행 시 `qtvirtualkeyboard currently is not supported at client-side` 경고가 로그에 찍힌다. Wayland에서는 컴포지터 쪽 가상 키보드를 권장한다는 의미지만, 단일 앱 키오스크 구조에서는 클라이언트 내장 키보드가 정상 동작한다(확인됨).
- 단어 추천 사전이 없다는 로그(`Hunspell dictionary is missing`)도 남는다. 자동 완성을 쓰지 않으므로 무시한다.
- Qt Virtual Keyboard 라이선스는 GPLv3(또는 상용)다. 다른 Qt 모듈(LGPL)과 다르므로 이미지를 외부 배포할 경우 라이선스 확인이 필요하다.

### 왜 이런 구조로 나눴나

- **UI는 컴파일이 필요 없다.** QML 파일을 그대로 기기에 올리면 Qt 런처가 해석해 실행한다. 기기 Qt 버전에 맞춰 C++ 크로스 컴파일 환경을 구축할 필요가 없다.
- **데몬은 기기 라이브러리에 의존하지 않는다.** 단일 정적 바이너리라 Nix로 재현 가능하게 빌드하기 쉽다.
- **UI 프로세스가 죽어도 상태가 유지된다.** 모든 상태는 데몬이 들고 있어 UI가 재실행돼 재접속하면 전체 상태를 다시 받는다. 반대로 데몬이 죽으면 UI는 안내 화면을 띄우고 자동 재접속을 시도한다(확인됨, 2초 간격 재시도). 이때 열려 있던 설정 패널, 확인 다이얼로그, 편집 모드는 모두 닫힌다(위 "UI 구성" 참고).
- **권한 분리가 깔끔하다.** UI는 비루트 `kiosk`, 데몬은 비루트 `tossd` 사용자로 실행한다. 시스템 제어가 필요한 작업만 데몬에 맡기고 최소한의 통로만 열었다(밝기는 sysfs 파일 그룹 권한, 절전·전원·SSH는 명령 4종만 받는 root 헬퍼 — 위 "화면 밝기, 절전, 전원 끄기").

향후 UI 측에 네이티브 코드가 필요해지면(직접 파일 I/O나 WebSocket 외 통신 등) Rust 객체를 QML에 노출하는 CXX-Qt를 도입할 수 있다. IPC 프로토콜을 유지하는 한 데몬은 수정할 필요가 없다.

### 수정 및 테스트

devshell에 Rust 툴체인(`rustc`, `cargo`, 타깃 musl std 포함)이 들어 있다.

| 작업 | 명령 |
|---|---|
| 데몬 단위 테스트 (PC) | `cd src/app/tossd && cargo test` (38개: 레이아웃 규칙과 편집·저장, UI 접속, 호스트와 페어링, Wi-Fi, 밝기와 전원, SSH, 상태 버전과 리소스, 원자적 파일 저장) |
| 리소스 미터 테스트·기기 수집 항목 조회 | `cd src/app/meter && cargo test` (4개), `cargo run --example show` |
| 데몬 로컬 실행 (PC) | `cd src/app/tossd && cargo run` (PC `127.0.0.1:7878` 바인딩) |
| 수정 사항 즉시 푸시 (플래싱 없음) | `src/scripts/app-push.sh ui`, `src/scripts/app-push.sh tossd`, 또는 `src/scripts/app-push.sh` (둘 다) |
| 화면 캡처 | 3.7절 C |
| 무인 화면 탭 | `src/scripts/tf-tap.sh <x> <y> …` (화면 픽셀 좌표. 터치 입력 장치에 이벤트 주입) |
| 무인 화면 드래그 | `src/scripts/tf-drag.sh <x1> <y1> <x2> <y2> …` (경유점 대기 시간 `HOLD=초`. 가장자리 페이지 넘김 테스트용) |
| PC 커넥터·프로토콜 테스트 | `cd src/app/host && cargo test`, `cd src/app/proto && cargo test` ([04-host-link.md](04-host-link.md) 4.12절) |
| 데몬 실시간 로그 | `src/scripts/tf-ssh.sh 'tail -f /var/log/tossd.log'` |
| UI 로그 (QML 에러) | `src/scripts/tf-ssh.sh 'grep -v "^00:" /var/log/kiosk.log'` |

`app-push.sh`는 이미지를 굽지 않고 기기 파일만 덮어씌우는 개발용 스크립트다. UI는 수 초 안에 재시작된다. QML 파일은 기존 디렉터리 위에 덮어쓰므로 저장소에서 삭제한 파일은 기기에 남아 있다가 다음 이미지 플래싱 시 정리된다. 푸시 시 파일 mtime을 현재 시각으로 갱신한다: Qt QML 컴파일 캐시(`/home/kiosk/.cache`)는 mtime만 보고 유효성을 판단하므로 mtime이 같고 내용만 다른 파일을 수동 복사하면 이전 캐시가 그대로 뜰 수 있기 때문이다. 변경을 영구 반영하려면 `src/scripts/deploy.sh userdata`로 플래싱한다(3.6절).

데몬에 의존 크레이트를 추가했다면 `src/app/tossd/Cargo.lock`도 함께 커밋해야 한다. Nix 빌드는 이 lock 파일의 해시로 크레이트를 검증한다. C 코드가 포함된 크레이트는 크로스 C 컴파일러 설정이 추가로 필요해지므로 가급적 피하는 편이 좋다.

**QML 코드에 에러가 있으면** UI 프로세스가 뜨자마자 종료되고 키오스크가 재실행을 반복한다. 그동안 화면은 검은 화면과 부팅 스플래시를 오가고, 화면 캡처 명령도 응답하지 않고 대기한다. UI 로그에서 `…qml:줄:칸: …` 라인을 찾으면 된다(위 표 명령 참고). 푸시 전 QML 문법 검사 도구는 devshell에 없다.

### 참고 및 주의사항

- **화면에 그리는 모든 요소는 `Main.qml`의 `screen` 내부에 배치한다.** 기기 전면 베젤 모서리가 둥글어 화면 네 귀퉁이가 조금 가려진다. 따라서 `screen`은 창 가장자리에서 2px 안쪽으로 들어가 있고(`Theme.screenInset`), 최상단을 덮는 검은 마스크(`shell/ScreenCorners`)가 모서리를 둥글게 라운딩한다(`Theme.screenRadius`, 베젤에 맞춘 28px). 창에 직접 배치하면 가장자리까지 그려져 베젤에 가려진다.
- 콘텐츠(상단바와 타일 그리드)는 화면 좌우 24px(`Theme.gridMargin`), 상단 27px(`Theme.gridMarginTop`) 여백을 둔다. 기기 베젤 모서리가 크게 둥글어 이보다 좁으면 모서리 쪽 요소가 답답해 보인다.
- 폰트는 Noto Sans CJK이며 한자는 한국어 글리프(KR) 우선으로 렌더링된다(`Theme.font`). 일본어·중국어 문자도 누락 없이 출력되지만 동일 한자의 국가별 이체자 구분은 반영되지 않는다.
- 유휴 상태에서는 리드로우하지 않는다. 점멸 인디케이터나 마키 스크롤처럼 지속 애니메이션이 켜진 동안에는 매 프레임 전체 리드로우가 발생하므로 필요한 경우에만 활성화한다.
- 아이콘(`kit/Glyph`)은 필요한 것만 직접 구현해 쓴다(미디어 제어, 설정, 인터페이스, 목록, 레이아웃 편집, 온스크린 키보드). 범용 아이콘 팩은 별도로 정하지 않았다.
- **터치 통과를 막는 오버레이는 반드시 `MouseArea`로 만든다.** 빈 `TapHandler { }`는 포인터 이벤트를 아래로 통과시켜 오버레이 밑 버튼이 함께 눌린다(정보 시트 바깥을 눌렀을 때 아래 버튼이 눌리던 버그로 확인). 상단바 아래로 펼쳐지는 패널(`shell/DropSheet`: 설정 패널, 추가 시트, 정보 시트), 다이얼로그 딤머와 본체(`shell/Dialog`), 키보드 배경, 연결 끊김 오버레이 모두 `MouseArea`로 막아 두었다.
- 런처 `qmlscene`은 Qt가 향후 제거 예정이라는 deprecated 경고를 띄운다. Alpine 런타임 패키지에는 이것만 들어 있으므로, 실제로 제거되면 소형 QML 호스트 바이너리를 직접 빌드해야 한다.
- UI 통신(`7878`)은 `127.0.0.1`에서만 수신한다. 호스트(PC) 통신(`7879`)은 모든 인터페이스에서 수신하되 페어링된 키만 수락하고 전 구간을 암호화한다([04-host-link.md](04-host-link.md)).
- 기기 내부의 다른 프로세스는 누구나 데몬에 접속할 수 있다(`127.0.0.1`). 데몬이 덱 절전·종료와 호스트 액션 요청까지 할 수 있으므로, 추후 다른 프로그램을 함께 올릴 때는 접근 제어를 추가해야 한다. 현재 기기에서 도는 것은 이 앱뿐이다.
- Qt Quick 채택 근거(실기기 Slint 비교 벤치마크)는 [01-findings.md](01-findings.md) 1.5절에 정리했다.

## 3.17 위젯·앱 확장 가이드

하나의 앱은 하나 이상의 위젯 종류를 제공한다. 위젯 종류 하나는 데몬 측 식별자(`Kind`)와 UI 측 위젯 QML 파일 1개로 대응된다. 새 위젯 종류를 추가할 때 수정 범위를 최소화하도록 모듈을 분리해 두었다.

### 코드 구조

UI (`src/app/ui/`):

| 모듈/파일 | 역할 | 앱 추가 시 |
|---|---|---|
| `Main.qml` | 루트 윈도우. 아래 컴포넌트들을 조립하고 이벤트를 연결(편집 모드 전환, 추가 시트 및 레이아웃 변경 요청 전송, 확인 다이얼로그 호출, 데몬 단절 시 오버레이 닫기) | 수정 불필요 |
| `shell/` | UI 셸 프레임워크 (아래 표 참고) | `WidgetSlot.qml`에 1줄 추가 |
| `kit/` | UI 컴포넌트 라이브러리. 토큰 `Theme`과 버튼, 디스플레이, 인디케이터 등 포함. 모든 스타일은 여기서 결정됨 (아래 "컴포넌트") | 가져다 사용 |
| `widgets/` | 위젯 종류별 QML 컴포넌트 및 공통 베이스 (아래 표 참고) | 파일 1개 추가 |

`shell/` 구성 파일:

| 파일 | 역할 |
|---|---|
| `DaemonLink` | 데몬 통신 브리지. 수신 상태를 프로퍼티로 노출하고(`pages`, `hosts`, `values`, `meters`, `wifi`, `device` …), 송신은 모두 메서드로 제공(`action`, `layoutMove`, `wifiConnect` …). 이름과 설명은 파일 상단 주석 참고 |
| `TopBar` | 상단바: 시각·날짜, 인터페이스 상태 아이콘 인디케이터, 설정 버튼 |
| `EditBar` | 편집 모드 상단바: `+` 및 `DONE` |
| `Pager` | 페이지와 타일 치수. `link`(`DaemonLink`)에서 레이아웃을 읽어 렌더링하고 편집 모드 중 배치 가이드와 드래그 프리뷰를 그림. 편집 완료 시 시그널 발생(`moved`, `resized`, `removed(target, info)`, `renameAsked`, `buttonAddAsked`) |
| `Placed` | 페이지에 배치된 요소 1개(랙 또는 복합 위젯)의 위치와 편집 모드 오버레이(드래그, `×`, 랙 핸들·라벨·슬롯별 `+` 및 `×`) |
| `LayoutDrag` | 드래그 상태 머신과 배치 판정: 이동 및 랙 크기 조절, 대상 좌표, 배치 가능 여부, 가장자리 페이지 전환. 포인터 이동이 `slop`(8px)을 넘어야 이동 시작 |
| `Rack` | 랙: 위젯 프레임 내부를 슬롯으로 분할해 버튼 위젯 배치 |
| `WidgetSlot` | 위젯 슬롯 1개. 위젯 종류에 맞는 QML 파일을 로드하고(`sources`), `link`에서 필요한 데이터를 바인딩하며, 액션 시그널을 데몬으로 전달 |
| `PageDots` | 페이지 표시 |
| `LinkLost` | 데몬 연결 끊김 오버레이 |
| `DropSheet` | 상단바 아래로 펼쳐지는 패널 베이스: 블라인드 펼침 애니메이션, 전면 패널과 일체화된 배경, 터치 통과 차단 |
| `DropPanel` | `DropSheet`에 헤더(뒤로가기 버튼, 제목, 우측 슬롯)와 온스크린 키보드 높이에 맞춰 줄어드는 콘텐츠 영역을 추가한 컨테이너. 설정 패널·추가 시트 공용 프레임 |
| `SettingsPanel` | 설정 패널: 설정 하위 페이지 전환, 페어링 모드 활성/비활성, 삭제 확인 요청 |
| `SettingsMain` | 설정 루트 페이지(목록) |
| `SettingsSection`, `SettingsRow` | 설정 섹션 및 개별 행. 제목·설명, 우측 컨트롤, 진입 화살표(`chevron`) 배치 |
| `SettingsList` | 설정 목록 컨테이너: 라벨·액션 버튼, 연한 패널 내 스크롤 목록, 빈 상태 안내. 목록 갱신 시에도 스크롤 위치와 누른 행 상태 유지 |
| `WifiPage`, `WifiJoin` | Wi-Fi 화면: 연결 상태와 AP 목록, 개별 AP 접속/삭제 |
| `DevicesPage`, `DevicePage`, `PairPage` | 기기 관리 화면: 페어링 PC 목록, 개별 PC 상세 정보/해제, 신규 페어링 |
| `VaultPage` | 연동 서비스 상세 화면(설정의 INTEGRATIONS): 덱 금고 키 상태, 금고를 쓰도록 허용한 PC와 해제, 키 삭제 |
| `KeyValueRows` | 디스플레이 내부 Key-Value 표 (정보 시트, 페어링 화면의 PC 실행 커맨드) |
| `AddPanel`, `AddOption` | 위젯 추가 시트(앱 → 위젯 종류 → 설정) 및 설정 항목 선택 컴포넌트 |
| `Dialog` | 중앙 모달 다이얼로그 베이스: 배경 딤머와 카드(`Dialog.Card`) |
| `ConfirmDialog` | 확인 다이얼로그 (`ask(제목, 설명, 버튼 라벨, 액션, 색상)`) |
| `SpotifySheet` | Spotify 창: Spotify 미디어 위젯에서 연 대기열(`UP NEXT`)이나 재생 기기(`PLAY ON`) 목록. 기기를 누르면 그 기기로 재생을 옮긴다. Spotify 플레이어(`Spotify theme`)에서 열면 Spotify 앱의 목록처럼 그린다(어두운 둥근 카드, "Now playing"·"Next in queue", 초록 지금 기기, 흰 둥근 닫기 버튼, 검은 바탕) |
| `VaultAsk` | 덱 금고 사용 허용 다이얼로그: 허용하지 않은 PC가 금고를 쓰려 하면 데몬이 묻는 요청(`vault.asking`)을 띄운다. `ALLOW` / `DENY`(바깥 영역도 거절) |
| `RenameDialog` | 랙 이름 변경 다이얼로그 |
| `LinkInfo` | 인터페이스 상세 정보 시트 |
| `KeyboardPanel` | 온스크린 키보드 컨테이너 |
| `ScreenCorners` | 화면 네 귀퉁이 베젤 라운딩 마스크 |
| `names.js` | 데몬 값을 UI 표시 문자열로 변환하는 헬퍼(`viaText`: 연결 경로 문자열) |

`widgets/` 파일:

| 파일 | 역할 |
|---|---|
| `WidgetBase` | 모든 위젯의 공통 베이스. `WidgetSlot`이 주입하는 프로퍼티와 `action` 시그널 정의 (아래 "위젯이 전달받는 프로퍼티" 참고) |
| `CompositeWidget` | 복합 위젯 베이스: `WidgetBase`에 위젯 프레임과 라벨을 결합. 라벨 우측 끝 요소는 `corner` 슬롯에 배치 |
| `CommandBase` | 명령 위젯 4종 공통 베이스: 대상 명령 활성 여부(`ready`), 버튼·노브 색상과 라벨, 값 전송 후 응답 대기 중 선택값 유지(`hold()`, `set(값)`), 거절 피드백 시그널(`refused()`) |
| `CommandWidget`, `CommandSwitchWidget`, `CommandSteppedWidget`, `CommandLinearWidget` | 명령 버튼, 명령 스위치, 단계 노브, 연속 노브 |
| `WebBase` | 웹페이지 위젯 2종 공통 베이스: 대상 주소, 활성 여부(`ready`: PC 연결과 `web.open` 권한), 색상과 라벨, `open()` |
| `WebButtonWidget`, `WebIconWidget` | 웹페이지 버튼, 사이트 아이콘을 그리는 웹페이지 버튼(아이콘은 `icons`에서 받음) |
| `MediaWidget` | 미디어 제어 위젯 |
| `SpotifyPlayer` | Spotify 플레이어 위젯: Spotify를 대상으로 `Spotify theme`을 켠 미디어 위젯. Spotify 앱의 재생 화면처럼 그린다(덱 테마를 따르지 않음) |
| `MeterWidget` | 리소스 미터 위젯 (4종 크기 공용) |

데몬 (`src/app/tossd/src/`):

| 파일 | 역할 | 앱 추가 시 |
|---|---|---|
| `ui_proto.rs` | UI와 주고받는 IPC 메시지 정의. 스펙 설명이 파일 상단에 있음 | 새 데이터 교환 필요 시 메시지 추가 |
| `server.rs` | UI WebSocket 서버: UI 요청을 각 모듈로 디스패치하고(`handle`), 변경된 상태를 푸시(`push`) | UI 신규 요청 처리 추가 시 `handle` 확장 |
| `state.rs` | 모든 UI 클라이언트가 공유하는 상태(`Shared`). 필드별 버전 번호(`Versioned`)로 변경분만 차등 전송. 백그라운드 수집 루프(시스템 상태, 덱 리소스, Wi-Fi, 호스트) 및 레이아웃 편집·슬립 등 다중 모듈 조율 | 앱 상태 및 백그라운드 태스크 추가 |
| `layout/mod.rs` | 레이아웃 모델: 페이지, 랙, 위젯, 위젯 종류(`Kind`), 유효성 규칙(`Layout::check`), 기본 레이아웃(`default_layout`) | `Kind`에 새 종류 추가 |
| `layout/catalog.rs` | 추가 가능한 위젯 카탈로그(`catalog`): 앱, 위젯 종류, 설정 스키마 | `catalog`에 항목 등록 |
| `layout/edit.rs` | 레이아웃 편집 로직(`Edit`): 이동, 랙 크기·이름 변경, 추가, 삭제. UI가 보낸 설정을 카탈로그 기준으로 필터링·검증 | 수정 불필요 |
| `layout/store.rs` | 레이아웃 파일 영속화(`Store`) | 수정 불필요 |
| `hosts/mod.rs` | 호스트(PC) 상태 관리(`Hosts`), UI 전달 모델(`HostsState`), 데몬 이벤트 콜백(`Events`), UI 액션 전달(`action`, `forget`, `set_watch`) | 호스트 신규 수신 데이터는 `HostInfo`에 필드 추가 |
| `hosts/session.rs` | 개별 호스트 세션: 헬로 핸드셰이크, 수락 판정, 메시지 수신 루프(`serve`) | 새 호스트 메시지 분기 추가(`serve`) |
| `hosts/pairing.rs` | 페어링 모드 및 후보 관리, 양측 검증 처리 | 수정 불필요 |
| `hosts/blob.rs` | 청크로 수신되는 앨범 아트 바이너리 조립 | 수정 불필요 |
| `hosts/store.rs` | 페어링 호스트 목록(`hosts.json`)과 덱 키(`key`) 파일 저장 | 수정 불필요 |
| `hosts/vault.rs` | 덱 금고: 외부 연동 키 암호화 보관(`vault.json`, `vault.key`), 금고를 쓸 수 있는 호스트 관리와 허용 질문, 토큰 발급·갱신과 대여([04-host-link.md](04-host-link.md) 4.15절) | 새 서비스를 더할 때 `SERVICES` |
| `icons.rs` | 웹페이지 아이콘 버튼의 사이트 아이콘 가져오기(큰 것부터 시도)와 `icons/` 보관 | 수정 불필요 |
| `spotify.rs` | Spotify 계정 조회와 재생 제어(Spotify Web API. 토큰은 덱 금고에서 얻음). Spotify 미디어 위젯이 배치에 있을 때만 조회 | 수정 불필요 |
| `sys.rs` | 시스템 상태 수집(상단바 및 정보 시트용: 호스트명, IP, 인터페이스 링크 상태). 리소스 사용량은 `src/app/meter`(PC 공용 크레이트)가 측정하며 `state.rs`가 1초마다 호출 | 수정 불필요 |
| `device.rs` | 덱 하드웨어 제어: 화면 밝기, 절전, 전원 종료, SSH 토글 | 수정 불필요 |
| `wifi.rs` | Wi-Fi 상태 및 접속 관리 (`wpa_supplicant` 제어 소켓 연동) | 수정 불필요 |
| `persist.rs` | 원자적 파일 저장(임시 파일 쓰기 + fsync + rename) 및 손상 파일 `.bad` 격리 | 수정 불필요 |
| `main.rs` | CLI 인자 파싱 및 데몬 진입점 | 수정 불필요 |

### 컴포넌트 (`kit/`)

| 컴포넌트 | 용도 |
|---|---|
| `Theme` | 컬러, 폰트, 애니메이션 듀레이션, 공용 치수·상수 토큰 모음(단일 컴포넌트 전용 상수는 해당 컴포넌트 내부에 둠). 스타일 용어와 규칙도 파일 상단 주석에 정리됨. 컬러: 패널(`plate`, `frame`, `ink`, `inkDim` …), 디스플레이(`display`, `displayInk`, `displayDim` …), 인디케이터(`indicatorGreen`·`Amber`·`Red`·`Blue`), 버튼 컬러(`buttonColor(이름)`), 배경 대비 텍스트 색(`inkOn(색)`), 버튼 색 대응 인디케이터 색(`indicatorFor(이름)`). 치수: 콘텐츠 여백(`gridMargin`, `gridMarginTop`, `gridMarginV`), 타일 간격(`gridGap`), 위젯 프레임 내부 여백과 라벨 행(`framePad`, `labelLine`), 상단바 높이(`barHeight`, 내부 영역 `barInner`), 랙 버튼(`rackButtonW`·`H`). 기타: 비활성 투명도(`dimmed`), 라벨 최대 길이(`labelMax`), 폰트 크기(`fLabel`, `fSmall`, `fBody`, `fTitle`), 애니메이션 시간(`tPress`, `tRelease`, `tState`, `tToggle`, `tPanel`, 페이지·테마 전환 `tPage`, 조작 직후 표시 유지 `tHold`) |
| `WidgetFrame` | 위젯 프레임. 입체 높이 없이 구역만 구획. 좌상단 라벨(`label`), 네 귀퉁이 나사, 라벨 행 우측 끝 슬롯(`corner`) |
| `Button` | 버튼. 컷아웃 위로 돌출(높이 `Theme.buttonTravel`). `text` 지정 시 상단에 작은 대문자 라벨 표시(`CONNECT` 등), 내부 아이콘·인디케이터는 상판과 함께 눌림. `held` 설정 시 반쯤 눌린 상태 유지(토글 상태, 열린 메뉴 버튼). `tapped()` |
| `Label` | 인쇄형 라벨: 패널이나 디스플레이에 작게 찍는 볼드 대문자 텍스트(위젯 프레임 라벨, 설정 섹션 제목, 표 왼쪽 열). 디스플레이 내부에서는 `color`를 `Theme.displayDim`으로 지정 |
| `LabeledButton` | 분리형 라벨 버튼. 위쪽에 단색 버튼(132×80px 고정), 아래쪽에 인디케이터와 인쇄 라벨 배치. 전체 영역이 터치 대상. `on`, `flash()`, `tapped()`. `indicator`가 false면 인디케이터 없이 라벨만 중앙 정렬. 명령 버튼(인디케이터 없음)과 명령 스위치(인디케이터 있음)가 사용 |
| `Knob` | 노브. 버튼처럼 컷아웃 위로 돌출된 원형 노브에 지시선이 있고 둘레에 눈금이 인쇄됨. 자체 상태 값은 모름: 각도(`angle`)와 눈금(`ticks`: 각도와 점등 여부)을 외부에서 주입하고, 터치 시작(`grabbed()`), 이동 거리(`turned(px)`: 우/상 방향이 +), 릴리스(`released()`), 단순 탭(`tapped()`) 시그널 발생. **원형 궤적이 아닌 직선 드래그로 조작한다**(작은 노브는 손가락에 가려 원형 드래그가 불편하기 때문). 드래그 중에는 페이지 스와이프가 잠김 |
| `LabeledKnob` | 분리형 라벨 노브. 위쪽에 노브(최대 지름 104px), 아래쪽에 인쇄 라벨, 그 밑에 작은 현재 값(`value`) 표시. `LabeledButton`과 높이가 맞아 랙 안에서 나란히 두어도 라벨 줄이 정렬된다. 노브 위젯이 사용 |
| `Display` | 디스플레이. 평면 커버 유리 아래 콘텐츠 배치, 가장자리 그림자로 깊이감 표현. 유리에 정적 반사광 표시(`Theme.glass` false 시 전체 비활성, `glass` false 시 개별 비활성) |
| `Indicator` | 인디케이터. `on`, `blink`, `flash()`, `color` |
| `Blinker` | 글로벌 점멸 타이머. 여러 인디케이터와 아이콘 인디케이터의 깜빡임 위상을 동기화하며 자체 UI 요소는 없음 |
| `Toggle` | 토글: 좌우 슬라이드 스위치. 설정 온/오프에 사용. `on`, `color`(켜짐 시 트랙 색), `tapped()`. 상태는 외부에서 바인딩 |
| `Slider` | 슬라이더: 좌우 드래그로 연속값 조절(화면 밝기). 토글과 동일한 디자인 톤. `value`(0~1, 외부 주입), `color`, 드래그 중 `moved(값)`, 릴리스 시 `released(값)` |
| `TextField` | 텍스트 입력 필드. 디스플레이 내부에 입력(가독성을 위해 유리 반사 제외). 탭 시 온스크린 키보드 호출. `text`, `placeholder`, `secret`(마스킹), `plain`(자동 대문자·단어 추천 끔), `accepted()`, `focusInput()`(키보드 표시), `blur()`(입력 확정: 조합 중인 단어를 `text`에 커밋하고 키보드 닫기. 버튼으로 `text`를 읽기 전에 호출) |
| `SignalBars` | Wi-Fi 신호 세기 막대 4칸. `signal`(dBm) |
| `IconIndicator` | 아이콘 인디케이터: 아이콘 형태 인디케이터. 라벨 없이 도형만으로 상태 표시. `name`(`Glyph` 아이콘 이름), `on`, `blink`, `color` |
| `Txt` | 기본 텍스트 컴포넌트. 화면의 모든 텍스트에 사용 (공통 폰트 적용) |
| `Marquee` | 한 줄 스크롤 텍스트. 영역을 넘치면 가로로 흐름. `running`으로 필요 시에만 활성화 |
| `Segments` | 세그먼트 게이지 바. 디스플레이 내부에서 0~1 비율을 칸 수로 표시 |
| `ProgressBar` | 프로그레스 바. 디스플레이 내부에서 진행률(0~1) 표시 |
| `Glyph` | 벡터 아이콘. 버튼 위나 아이콘 인디케이터에 사용. 현재 `play`, `pause`, `prev`, `next`, `rew`, `ffwd`, `repeat`, `repeat1`, `shuffle`, `gear`, `usb`, `lan`, `wlan`, `bt`, `core`, `chevron`, `back`, `lock`, 편집용 `close`·`plus`·`grip`, 키보드용 `shift`·`backspace`·`enter`·`hide`·`globe` 내장. 추가하려면 24×24 좌표계 경로를 `fills`나 `lines`에 정의 |
| `Screw` | 나사 머리 (장식용. `WidgetFrame`에서 사용) |

### 위젯이 전달받는 프로퍼티

위젯 QML 파일은 `widgets/WidgetBase`(또는 이를 확장한 `CompositeWidget`, 명령 위젯이면 `CommandBase`)를 루트로 사용한다. `shell/WidgetSlot`이 `DaemonLink`(`shell/DaemonLink`)에서 필요한 프로퍼티를 바인딩해 내려주고, 위젯이 발생시킨 `action` 시그널에 대상 호스트와 위젯 ID를 붙여 데몬으로 전송한다(`link.action`). 위젯 QML 파일은 `link`를 직접 참조하지 않는다.

| 프로퍼티/시그널 | 내용 |
|---|---|
| `widget` | 데몬에서 내려준 위젯 객체: `id`, `kind`, `x`, `y`, `w`, `h`, `label`, `color`, `settings`(개별 설정) |
| `sys` | 시스템 상태: `host`, `ip`, `links.usb`·`eth`·`wlan`, `net`(인터페이스별 `name`·`up`·`mac`·`addr`, `usb_state`, `eth_speed`, `bt_present`) |
| `host` | 대상 호스트(PC) 상태(위젯 설정에 지정된 PC 또는 연결된 첫 PC, `WidgetSlot`이 매칭): `id`, `name`, `os`, `caps`(덱에 허용된 권한 목록. 미허용 액션은 호출해도 거절됨), `via`, `connected`, `media`(재생 상태, 없으면 `null`). 연결된 호스트가 없으면 `null` ([04-host-link.md](04-host-link.md) 4.11절) |
| `action(name, args)` | 호스트 액션 실행 요청 시그널. 예: `action("media.next", null)` |
| `result` | 이 위젯이 요청한 액션의 마지막 결과: `seq`, `ok`, `error`, `at`(UI 수신 시각). 아직 없으면 `null`. 명령 스위치와 노브가 이 값으로 거절 여부를 판단한다(`CommandBase`: 수신 후 `Theme.tHold` 이내의 실패만 처리. 유지하던 선택값을 해제하고, 스위치는 인디케이터를 빨간색으로 점멸) |
| `command` | 이 위젯이 가리키는 명령(설정 `command`가 해당 ID). 대상 호스트가 현재 제공 중일 때만 존재: `id`, `name`, `kind`(`run`·`switch`·`stepped`·`linear`), `steps`, `min`, `max`, `step`(0인 값은 생략). 아니면 `null` — 이때 위젯은 비활성화(흐림)돼 눌리지 않아야 한다 |
| `value` | 해당 명령의 현재 값(호스트가 보고한 값): 스위치 0/1, 단계는 인덱스(0부터), 범위는 수치 값. 모르면 `undefined` |
| `metrics`, `metricValues`, `meterSource` | 리소스 미터용. 위젯 설정 `host`가 가리키는 대상(`@deck`이면 덱 자신)에서 수집 가능한 메트릭 목록, 그 현재 값(메트릭 ID → 값. 이 위젯이 선택한 항목만 전달), 대상의 이름과 값 수신 여부(`own`, `name`, `live`) |

호스트에서 오는 데이터는 `host`로 들어온다: 재생 정보(`host.media`), 앨범 아트(`host.art`), 등록된 명령 목록(`host.commands`). 자주 바뀌는 값은 따로 전달된다 — 스위치·노브 값(`value`)과 메트릭 값(`metricValues`). **호스트는 이벤트가 생겼을 때만 보낸다.** 재생 위치처럼 계속 흐르는 값은 위젯이 수신한 값을 기준으로 직접 계산한다(`MediaWidget`이 이 방식). 다른 종류의 데이터나 액션을 추가하려면 프로토콜(`src/app/proto`)에 메시지와 액션 이름을 추가하고, PC 커넥터(`src/app/host`)에 이를 수집하거나 실행하는 코드를 넣는다. 절차는 [04-host-link.md](04-host-link.md) 4.12절에 있다. **호스트는 덱에 허용한 액션만 실행하므로, 새 액션에는 권한도 함께 정의한다.** 미디어 제어 위젯(`MediaWidget`)이 레퍼런스다.

명령 위젯(호스트에 등록된 명령 1개를 가리키는 버튼 위젯)은 `CommandBase`를 루트로 사용한다. 위 프로퍼티에 더해 아래 항목이 있다.

| 이름 | 내용 |
|---|---|
| `kindWanted` | 이 위젯이 다루는 명령 종류(`run`, `switch`, `stepped`, `linear`). 위젯이 지정 |
| `fits` | 해당 명령이 이 위젯으로 다룰 수 있는 형식인지 여부(단계가 2개 이상인지, 범위가 맞는지). 위젯이 지정 |
| `ready` | 대상 호스트가 연결돼 있고, 해당 명령을 제공 중이며, 종류와 형식이 맞는지 여부. 아니면 흐려지고(`fade`) 눌리지 않아야 한다 |
| `face`, `label`, `fade` | 버튼·노브 색상, 라벨, 불투명도(위젯 설정과 `ready`에서 결정) |
| `set(값)` | 해당 명령에 값 설정을 요청(`command.set`) |
| `hold()`, `holding`, `holdMs` | 호출 시점부터 잠시(`Theme.tHold`) UI에서 선택한 값을 표시. 이후에는 다시 호스트가 보고한 값을 따름 |
| `refused()` | 요청한 액션이 거절됐음을 알리는 시그널. 이 시점에 `holding`은 이미 해제된 상태 |

### 새 앱 추가 순서

1. **데몬에 위젯 종류를 추가한다.** `src/app/tossd/src/layout/mod.rs`의 `Kind`에 이름을 추가한다(`"앱.종류"` 형식. 예: `media.player`). 복합 위젯이면 `is_composite`에도 추가한다. 복합 위젯은 페이지에 직접 배치되고, 그 외에는 랙 내부에만 배치할 수 있다. 이 규칙은 `Layout::check`가 검증한다: 규칙에 어긋나는 편집 요청은 거절하고, 규칙에 어긋난 저장 파일은 백업한 뒤 기본 레이아웃으로 시작한다.
2. **카탈로그에 등록한다.** `layout/catalog.rs`의 `catalog`에 등록한다: 소속 앱, 이름과 설명, 배치 형태(`form`: 페이지에 놓는 복합 위젯은 `widget`, 랙 슬롯에 넣는 버튼 위젯은 `button`), 크기, 새로 배치할 때의 라벨, 추가 시 선택할 설정. 등록하면 편집 모드의 `+` 메뉴에 나타난다. 기본 레이아웃에도 넣으려면 `layout/mod.rs`의 `default_layout`에 1줄을 추가한다.
3. **위젯 QML 파일을 만든다.** `src/app/ui/widgets/`에 파일을 추가한다. 복합 위젯은 `CompositeWidget`을 루트로 쓰면 위젯 프레임과 라벨, 나사가 붙는다. 버튼 위젯은 `WidgetBase`에 `kit/LabeledButton`(버튼, 그 아래 인디케이터와 라벨)이나 `kit/LabeledKnob`을 넣는다 — 프레임과 나사는 랙이 갖는다. 호스트에 등록된 명령을 가리키는 위젯이면 `WidgetBase` 대신 `CommandBase`를 루트로 쓴다.
4. **위젯 종류와 QML 파일을 매핑한다.** `src/app/ui/shell/WidgetSlot.qml`의 `sources`에 `"위젯 종류의 이름": "../widgets/파일.qml"` 1줄을 추가한다. 빠뜨리면 해당 자리는 비고 UI 로그에 `알 수 없는 위젯 종류`가 찍힌다.
5. **테스트한다.** `cd src/app/tossd && cargo test` 후 `src/scripts/app-push.sh`로 기기에 푸시한다.

최소 복합 위젯 예시(호스트명을 표시하고, 버튼을 누르면 이 위젯의 대상 PC에 다음 곡 액션을 요청한다):

```qml
import QtQuick
import "../kit"

CompositeWidget {
    id: root

    Display {
        anchors { left: parent.left; right: parent.right; top: parent.top; bottom: go.top; bottomMargin: 12 }
        Txt {
            anchors.centerIn: parent
            text: root.sys.host || ""
            color: Theme.displayInk
        }
    }
    Button {
        id: go
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: 80
        text: "GO"
        onTapped: root.action("media.next", null)
    }
}
```

실제 구현 예는 `widgets/MeterWidget.qml`(데몬 값을 표시만 함), `widgets/MediaWidget.qml`(디스플레이와 버튼을 함께 쓰며 호스트 데이터를 표시하고 액션을 요청), `widgets/CommandWidget.qml`(랙에 놓이는 버튼 위젯. `CommandBase`로 자기 설정이 가리키는 명령을 찾아 실행 요청), `widgets/CommandSwitchWidget.qml`과 `CommandSteppedWidget.qml`, `CommandLinearWidget.qml`(호스트가 보고한 값을 표시하며, 직접 조작한 값을 먼저 보여 준 뒤 호스트 값으로 복귀. 요청이 거절되면 바로 호스트 값으로 복귀)이다.

### 준수 사항

- **치수는 부모에서 가져온다.** 위젯 크기는 타일 크기에서 결정되고, 타일 크기는 여백 값에 따라 달라진다. 내부 배치는 고정 좌표 대신 `anchors`와 부모 크기에 맞춘다.
- **색상, 폰트, 간격은 `Theme`에서 가져온다.** 위젯에 색상 값을 직접 적지 않는다. 테마(라이트/다크)가 바뀔 때 함께 바뀌어야 한다.
- **누를 수 있는 요소는 버튼으로, 가변 데이터는 디스플레이 안에 둔다.** 디스플레이 안에는 돌출된 컴포넌트를 넣지 않는다.
- **텍스트는 `Txt`로 쓴다.** 디스플레이 내부에서는 색상을 `Theme.displayInk`, `Theme.displayDim`으로 지정한다. 인쇄된 작은 라벨은 `Label`, 버튼 위 라벨은 `Button`의 `text`로 쓴다.
- **비활성 요소는 같은 정도로 흐리게 한다**(`Theme.dimmed`). 대상 PC가 미연결인 위젯, 조건이 충족되지 않은 버튼이 해당한다.
- **지속 애니메이션은 필요할 때만 켠다.** 유휴 상태에서는 리드로우하지 않는다. 점멸이나 마키 스크롤은 켜져 있는 동안 매 프레임 전체 리드로우를 일으킨다.
- **상태는 데몬에 둔다.** UI는 재시작되면 모든 상태를 잃는다. 유지해야 하는 값(켜짐 상태, 설정)은 데몬이 갖고 UI는 받아서 표시한다.
- **호스트가 허용하지 않은 액션은 막아 둔다.** 호스트는 덱에 허용한 액션만 실행한다(`host.caps`). 해당 액션을 요청하는 컴포넌트는 권한이 없을 때 흐려지고 눌리지 않아야 한다([04-host-link.md](04-host-link.md) 4.5절).
- **위젯별로 다른 값은 설정으로 받는다**(`widget.settings`). 대상 PC, 실행할 명령 같은 값이다. 사용자가 위젯 추가 시 선택하게 하려면 `catalog`에 설정을 등록한다(타입: `host`, `command`, `metrics`, `text`, `color`).

## 3.18 호스트(PC) 연동

덱은 PC와 연결돼 PC 상태를 표시하고 PC에 액션을 요청한다. 연결되는 PC를 **호스트**라고 부르며, 거기서 실행되는 프로그램이 `toss-connector`(`src/app/host/`)다.

**프로토콜, 암호화, 페어링, 호스트가 덱에 허용하는 권한, PC 커넥터, 확장 방법은 모두 [04-host-link.md](04-host-link.md)에 있다.** 여기에는 사용법만 정리한다.

| 작업 | 방법 |
|---|---|
| PC 커넥터 빌드 및 실행 | `nix build .#toss-connector -o dist/toss-connector && ./dist/toss-connector/bin/toss-connector` (로그인한 사용자 세션에서) |
| PC 커넥터를 NixOS에 설치 | 시스템 flake에 이 저장소를 input으로 추가하고 `toss-front.nixosModules.default`를 넣은 뒤 `programs.toss-connector.enable = true;`. 로그인하면 자동 실행된다([04-host-link.md](04-host-link.md) 4.10절) |
| 페어링 | 덱: 설정 → `Paired devices` → `PAIR NEW DEVICE`. PC: 웹 콘솔의 "새 덱 페어링", 또는 `toss-connector pair`(USB)·`toss-connector pair --deck <주소>`(LAN). 양쪽에 표시된 6자리 숫자가 같으면 덱에서 `ACCEPT`, PC에서 "숫자 일치"(터미널이면 `y`) |
| 덱에 허용한 권한 조회·변경 | 웹 콘솔의 토글, 또는 `toss-connector trust`, `toss-connector allow <권한>`, `toss-connector deny <권한>` |
| 권한과 명령을 NixOS 설정으로 고정 | `programs.toss-connector.config = { strict, permissions, commands }`. 고정한 항목은 웹 콘솔과 CLI에서 바꿀 수 없다([04-host-link.md](04-host-link.md) 4.10절) |
| 페어링 삭제 | 덱: 설정 → `Paired devices` → 대상 기기 → `FORGET`(확인 다이얼로그). PC: `toss-connector forget <키 접두사>` |
| 명령 버튼 사용 | PC: 웹 콘솔의 "명령 → + 추가", 또는 `toss-connector command add <이름> -- <명령…>`으로 등록하고, 해당 덱에 `command.run` 권한을 허용한다(웹 콘솔의 토글이나 `toss-connector allow command.run`). 덱: 편집 모드에서 랙 빈 슬롯의 `+` → `Command` → `Run` |
| PC에서 권한과 명령을 웹 콘솔로 관리 | PC: `toss-connector`를 실행해 둔 상태에서 `toss-connector console` → 브라우저에 웹 콘솔이 열린다([04-host-link.md](04-host-link.md) 4.10절) |
| 리소스 미터 추가 | 덱: 편집 모드의 `+` → `Resource monitor` → 크기 선택 → 대상(`This deck` 또는 PC)과 메트릭 선택(선택한 순서대로 배치) → 헤더의 `ADD`. PC를 대상으로 하려면 해당 PC에서 `resource.info` 권한이 허용돼 있어야 한다(기본값은 켜짐) |
| 스위치와 노브 사용 | PC: 웹 콘솔의 "명령 → + 추가", 또는 `toss-connector command add-switch`·`add-stepped`·`add-linear`로 등록(사용법과 예시는 [04-host-link.md](04-host-link.md) 4.6절 "명령"), `toss-connector command set`·`state`로 PC에서 먼저 테스트한다. 덱: 랙 빈 슬롯의 `+` → `Command` → `Switch`, `Stepped knob`, `Linear knob`. 스위치는 탭하고, 단계 노브는 탭하거나(다음 단계) 드래그하고, 연속 노브는 드래그한다(오른쪽·위쪽이 증가 방향) |
| 덱 SSH 켜고 끄기 | 덱: 설정 → `NETWORK` → `SSH`. 꺼 두면 재플래싱하거나 재부팅해도 꺼진 상태가 유지된다. 개발이나 플래싱 시에는 먼저 켠다 |
| 덱 절전·웨이크업 | PC: `toss-connector sleep`, `toss-connector wake`. 덱: 설정 → `Sleep`. 웨이크업은 PC에서만 가능하며, 덱이 Wi-Fi에 연결돼 있어야 한다 |

- USB든 LAN이든 페어링해야 연결된다. 연결되면 미디어 제어 위젯에 PC에서 재생 중인 곡과 앨범 아트가 표시되고 버튼으로 제어한다.
- PC는 덱별로 허용한 액션만 실행한다. 재생 정보, 재생 제어, 리소스 사용량은 기본으로 켜져 있고, 명령 실행은 꺼져 있다. 권한은 웹 콘솔이나 CLI로 변경하며, NixOS에서는 설정에 선언해 고정할 수도 있다.
- 덱 키와 페어링 기록, 레이아웃, 밝기는 `/data/toss/`에 저장돼 재플래싱 후에도 유지된다(3.15절).
- 위젯에서 호스트 데이터를 쓰는 방법은 3.17절의 `host`와 `action` 참고.
