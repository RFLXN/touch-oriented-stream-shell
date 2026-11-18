# 1. 조사사항 — 토스 프론트 PLF02WH

토스 프론트 1.5세대를 개조하면서 파악한 내용을 정리한 문서다. **기기에서 직접 확인한 것**, **소스나 문서를 통해 확인한 것**, **추정**을 구분해 기록했다.

- 순정 기기를 현재 상태로 구성하는 절차와 구조는 [02-progress.md](02-progress.md)
- 그 위에서 앱을 개발하는 방법은 [03-userland-guide.md](03-userland-guide.md)

근거 표기: **기기** = 실기기에서 측정/확인, **순정 DT** = 순정 커널 디바이스 트리, **소스** = 해당 프로젝트 소스 확인, **커뮤니티** = 게시글 참고, **추정** = 미검증.

## 1.1 기기 개요

| 항목 | 값 | 근거 |
|---|---|---|
| 모델 | 토스 프론트 1.5세대 PLF02WH. 제조 SUNMI, 보드 코드명 `p681x`, boardid `dog` | 기기 |
| SoC | Rockchip RK3399 (6코어: A53 ×4 + A72 ×2) | 기기 |
| RAM | 2GB LPDDR4 (실측 1,956MB) | 기기 |
| 저장장치 | eMMC 14,912MB, 카드명 `Biwin`. HS400 Enhanced Strobe 지원 | 기기 |
| 화면 | 7인치 MIPI DSI, 800×1280 세로. 기기를 놓는 방향이 곧 패널 방향 | 기기 |
| 화면 모서리 | 패널 자체는 직사각형이지만 기기 베젤 모서리가 둥글어 네 모서리가 약간 가려진다. 앱에서 가장자리 2px을 비우고 모서리를 둥글게 마스킹해 그린다([03-userland-guide.md](03-userland-guide.md) 3.16절) | 기기(육안 확인) |
| 전원 | 후면 USB-C로 12V 2.5A (PD) | 커뮤니티, 실사용 |
| 보안 부팅 | 비활성화(`fuse.programmed=0`), verified boot 상태 `orange` | 기기 |
| 순정 OS | Android 11 (SUNMI 빌드), 커널 4.19.193 | 기기 |

### 세대 구분 (커뮤니티)

| 세대 | 모델 | 구분 방법 | SoC |
|---|---|---|---|
| 1세대 | PLF00WH | 측면 카드부 돌출, 카메라 1개 | RK3399 |
| **1.5세대** | **PLF02WH** | 측면 카드부 돌출, 카메라 2개 | RK3399 |
| 2세대 | PLF20WH/BK | 측면 슬림, 전면 투입구 테두리 두꺼움 | Qualcomm |

이 문서는 1.5세대 기준으로 작성했다. 1세대도 동일한 방법이 통한다는 이야기가 있으나 확인하지 않았고, 2세대는 SoC가 달라 해당되지 않는다.

다루지 않는 기능: 카드 리더, NFC, 카메라, 결제 보안 모듈. 스트림덱 용도에는 불필요하고 제조사 전용 드라이버라 살리기 어렵다.

## 1.2 펌웨어와 부팅

### 파티션 테이블 (기기 기준, 섹터 = 512바이트)

| 이름 | 시작 LBA | 섹터 수 | 크기 | 역할 |
|---|---|---|---|---|
| (GPT, IDB 로더) | `0x0` | `0x2000` | 4MB | 파티션 테이블, DDR 초기화 로더 |
| security | `0x2000` | `0x2000` | 4MB | 기기 고유 보안 데이터 |
| uboot | `0x4000` | `0x2000` | 4MB | U-Boot |
| trust | `0x6000` | `0x2000` | 4MB | ATF, OP-TEE |
| misc | `0x8000` | `0x2000` | 4MB | 부팅 지시(BCB) |
| dtbo | `0xA000` | `0x2000` | 4MB | DT 오버레이 |
| vbmeta | `0xC000` | `0x800` | 1MB | verified boot 메타데이터 |
| boot | `0xC800` | `0x14000` | 40MB | 커널 + 램디스크 |
| recovery | `0x20800` | `0x3C000` | 120MB | 복구용 커널 + 램디스크 |
| backup | `0x5C800` | `0xC0000` | 384MB | |
| cache | `0x11C800` | `0xC0000` | 384MB | 안드로이드 캐시 파티션(ext4). 현 커널에서도 그대로 마운트된다. 이 시스템은 `/data`에 마운트해 기기에만 남는 데이터를 저장한다 |
| metadata | `0x1DC800` | `0x8000` | 16MB | |
| baseparameter | `0x1E4800` | `0x800` | 1MB | |
| super | `0x1E5000` | `0x400000` | 2GB | 안드로이드 system/vendor (동적 파티션) |
| userdata | `0x5E5000` | `0x173AFC0` | 11.6GB | 사용자 데이터 |

### 부팅 흐름

MaskROM(SoC 내장) -> IDB 로더 -> U-Boot + ATF/OP-TEE -> `boot`(또는 `recovery`) 커널 -> 루트 파일시스템.

### 순정 U-Boot 동작

| 항목 | 내용 | 근거 |
|---|---|---|
| 버전 | U-Boot 2017.09 (Rockchip 계열). 빌드 761은 2026-03-31, 799는 2026-09-05 | 기기 |
| 부트 이미지 형식 | Android boot image v2 | 기기 |
| 커널 DTB 로드 위치 | 헤더의 DTB 영역이 **아니라** `second` 영역 RSCE 리소스 묶음 내부의 `rk-kernel.dtb` | 소스, 기기 |
| DTB 누락 시 | 부팅 실패(다른 DTB로 대체하지 않음) | 소스 |
| 커널 DTB 용도 | 커널에 전달할 뿐 아니라 U-Boot 자체 드라이버(전원 칩, 버튼, USB)에서도 참조함 | 소스 |
| 메인라인 DTB 전달 시 | **문제없이 부팅됨** | 기기 |
| 커널 형식 | 비압축 arm64 `Image`를 옮기지 않고 로드한 위치에서 그대로 실행. 메인라인 커널이 misaligned 경고를 출력하지만 자체 재배치 후 정상 동작함 | 기기 |
| cmdline | 부트 이미지 cmdline 뒤에 `storagemedia=`, `androidboot.*` 등을 추가함 | 기기 |
| 하드웨어 ID | `androidboot.hardwareid=P6X021B201F004M13SXXT0g…`를 전달한다. 순정 커널이 이 값으로 패널(`M13`), Wi-Fi(`W05`) 변종을 판별한다 | 기기 |
| verified boot | 언락 상태이므로 변조된 `boot`, `recovery`, `super`도 그대로 부팅됨 | 기기 |
| 부팅 명령 | `boot_android` -> `bootrkp` -> `run distro_bootcmd` | 기기(바이너리 문자열) |
| distro 부팅 | `boot_targets=mmc1 mmc0 usb0 pxe dhcp`, extlinux 스크립트 포함. 안드로이드 이미지가 없을 때 fallback 되는지는 **미시험** | 기기(바이너리 문자열) |
| 커널 압축 | 바이너리에 gzip, lz4, lzma 문자열이 있음. 압축 커널 해제 여부는 **미시험** (비압축 `Image`도 `boot` 파티션에 들어감) | 기기(바이너리 문자열) |
| `dtbo` 적용 실패 | 경고만 출력하고 원본 DTB로 부팅 계속 | 소스 |
| `recovery` 부팅 지시 | 재부팅 인자(아래) 외에도 `misc` 파티션에 `boot-recovery`를 기록하면 `recovery`로 부팅됨 | 소스(미시험) |

### 부팅 모드 선택

| 방법 | 결과 | 근거 |
|---|---|---|
| 일반 전원 인가 | `boot` 부팅 | 기기 |
| 재부팅 인자 `recovery` | **다음 1회만** `recovery` 부팅. 그다음 재부팅 시 다시 `boot`로 복귀 | 기기 |
| 재부팅 인자 `loader` | Loader 모드(약 5초) | 기기 |
| 초기화 버튼 + 전원, Micro USB PC 연결 | Loader 모드(빌드 761) | 기기 |
| 초기화 버튼 + 전원, USB 미연결 | 공장 초기화("지우는 중") | 기기 |
| `BOOT`–`VCC_1V8` 패드 쇼트 + 전원 | MaskROM | 기기 |
| Loader에서 `rkdeveloptool rd 3` | MaskROM(패드 쇼트 불필요) | 기기 |
| 안드로이드에서 `adb reboot loader` | Loader 모드 | 기기 |

재부팅 인자는 PMU GRF 오프셋 `0x300` 레지스터에 기록한다: normal `0x5242c300`, loader `0x5242c301`, recovery `0x5242c303` (순정 DT).
초기화 버튼은 ADC 키 "volume up"(SARADC 채널 1, 임계 1000µV)이며, U-Boot가 커널 DTB의 `adc-keys` 노드로 읽는다.

### 플래싱 모드 차이

| 모드 | USB ID | 읽기 | 쓰기 |
|---|---|---|---|
| Loader (기기 내장) | `2207:330c` Loader | **신뢰 불가.** Read LBA 기능이 없어 일부 구간이 `0xCC`로 읽힘 | 가능 |
| MaskROM + 로더 적재 | `2207:330c` Maskrom | 정상 | 정상 |

- 로더는 `rk3399_loader_v1.30.130.bin`을 `rkdeveloptool db`로 RAM에 적재하며 저장장치에는 기록하지 않는다.
- 로더를 적재하기 전의 MaskROM은 상태 질의(`rcb`)에 응답하지 않고 행이 걸린다.
- `rkdeveloptool ppt` 출력은 줄 끝에 CR이 포함된다.
- 테스트 패드 위치: 하단 커버 내부, 카드 슬롯 기판 사이의 금색 원형 패드 2개. 왼쪽이 `VCC_1V8`, 오른쪽이 `BOOT`.

### 펌웨어 버전

| | 빌드 799 | 빌드 761 (3.1.42) |
|---|---|---|
| 초기화 버튼으로 Loader 진입 | 차단됨(공장 초기화만 동작) | 가능 |
| 커스텀 OS 부팅 | 차단된다는 보고(미시험) | 가능 |

순정 펌웨어 상태에서 Wi-Fi에 연결하면 자동 업데이트로 799급 빌드가 설치된다는 보고가 있다(커뮤니티).

### 부트 이미지 구성 (기기 기준, 순정 761)

| 영역 | 크기 | 내용 |
|---|---|---|
| kernel | 32.1MB | 비압축 arm64 `Image` |
| ramdisk | 0.87MB | gzip |
| second | 6.3MB | RSCE 묶음: `rk-kernel.dtb`(SHA-1 해시 포함), 로고 BMP 2개(각 3MB), 배터리 이미지 7개 |
| dtb | 122,866바이트 | `rk-kernel.dtb`와 동일 |

RSCE 형식(블록 = 512바이트): 헤더 1블록(`RSCE`, 버전, 테이블 오프셋, 엔트리 수) + 엔트리마다 1블록(`ENTR`, 경로 220바이트, 해시 32바이트, 해시 길이, 데이터 오프셋, 데이터 크기) + 데이터. 직접 패킹한 묶음이 원본과 바이트 단위로 일치함을 확인했다.

## 1.3 하드웨어 구성

원자료: `analysis/device/`(순정 커널 런타임 덤프), `analysis/dtb/`(순정 DT), `analysis/mainline/`(메인라인 부팅 로그).

### 전원

| 레일 | 출처 | 전압 | 비고 |
|---|---|---|---|
| vdd_center | RK808 DCDC1 | 0.75~1.35V (실측 0.9V) | |
| vdd_cpu_l | RK808 DCDC2 | 0.75~1.35V (실측 0.9V) | A53 |
| vcc_ddr | RK808 DCDC3 | 고정 | |
| vcc_1v8 | RK808 DCDC4 | 1.8V | |
| vcc1v8_dvp | RK808 LDO1 | 1.8V | |
| vcc3v0_tp | RK808 LDO2 | **1.8V** | 레일명과 실제 전압이 다름 |
| vcc1v8_pmu | RK808 LDO3 | 1.8V | |
| vcc_sd | RK808 LDO4 | 1.8~3.0V (실측 3.0V) | |
| vcca3v0_codec | RK808 LDO5 | 3.0V | |
| vcc_1v5 | RK808 LDO6 | 1.5V | |
| vcca1v8_codec | RK808 LDO7 | **0.9V** | 레일명과 실제 전압이 다름 |
| vcc_3v0 | RK808 LDO8 | 3.0V | |
| vcc3v3_s3, vcc3v3_s0 | RK808 스위치 | 3.3V | |
| vdd_cpu_b | SYR827 (I2C0 `0x40`) | 0.7125~1.5V (실측 1.0V) | A72. 전압 선택 핀 gpio1-17 |
| vdd_gpu | SYR828 (I2C0 `0x41`) | 0.7125~1.5V (실측 1.0V) | 전압 선택 핀 gpio1-14 |

RK808은 I2C0 `0x1b`, 인터럽트 gpio1-9에 연결된다. 순정에서는 모든 레일이 always-on이다(순정 DT).
IO 전압 도메인: bt656 <- vcc_1v8, audio <- vcc1v8_dvp, sdmmc <- vcc_sd, gpio1830 <- vcc_3v0, pmu1830 <- vcc_1v8 (순정 DT).

### 전원 스위치 (GPIO)

순정 커널은 SUNMI 전용 드라이버(`sunmi,common-config`)를 통해 부팅 시 아래 GPIO들을 모두 활성화한다.

| 이름 | 핀 | 용도 |
|---|---|---|
| `iov5` | gpio1-18 | 5V 주변 장치 전원 (추정) |
| `vbus` | gpio1-3 | USB 5V (추정) |
| `rs232_1` | gpio2-28 | RS-232 트랜시버 1 |
| `rs232_2` | gpio1-12 | RS-232 트랜시버 2 |
| `iccard` | gpio1-24 | IC 카드 리더 (추정) |

### 화면

| 항목 | 값 | 근거 |
|---|---|---|
| 경로 | VOP big (`ff900000`) -> MIPI DSI0 (`ff960000`) -> 패널 | 순정 DT |
| 패널 구동 칩 | **ILI9881C** (ID 레지스터 `98 81 5c`) | 기기 |
| 패널 변종 | `M13` = 순정 DT의 `mipi_lcd_23` | 기기 |
| 해상도·타이밍 | 800×1280, 수평 40/20/20, 수직 30/4/16 (front/sync/back). 픽셀 클럭은 순정 761이 60MHz(약 51Hz), LineageOS 배포판이 70.224MHz(60Hz)로 높여 둔 값이다 — 두 부트 이미지의 DTB 차이는 이 값 하나뿐이다 | 순정 DT, 기기 |
| 메인라인 주사율 | 70.224MHz를 요청하지만 실제 클럭은 66.67MHz로 잡힌다. **실제 주사율 약 57Hz** — 앱 프레임 플립 횟수를 세어 57.0fps로 실측 확인 | 기기 |
| DRM 장치 번호 | `card0` = GPU(panfrost), `card1` = 디스플레이(rockchip-drm, `DSI-1`). 프로브 순서에 따라 바뀔 수 있음 | 기기 |
| DSI | 3레인, RGB888, 영상 버스트 모드, 명령은 LP 모드. 순정 링크 속도 624Mbps × 3 | 순정 DT, 기기 |
| 초기화 | 명령 199개(약 1000바이트). `FF 98 81 xx`로 페이지 전환 후 마지막에 sleep-out(150ms)·display-on 전송 | 순정 DT |
| 제어 핀 | 리셋 gpio1-23(active low), enable gpio4-21, 1.8V enable gpio4-24 | 순정 DT |
| 전원 순서 | 전원 -> enable 핀 -> 20ms -> 리셋 20ms -> 150ms -> 초기화 명령 -> 30ms | 순정 DT |
| **바이어스 전원** | **IML7525**, I2C7 `0x3e`, enable 핀 gpio1-2. ±5.8V. 레지스터 맵이 TPS65132와 호환됨(`0x00`, `0x01`에 `0x12`) | 기기 |
| 백라이트 | PWM0 (`ff420000`), 주기 40000ns, 밝기 0~255 | 순정 DT |

패널 진단 요령(기기): DCS `0x0A`(전원 상태)를 읽었을 때 `0x9c`면 정상이고, `0x0c`면 명령은 수신했으나 슬립에서 빠져나오지 못한 상태(바이어스 전원이 없을 때 발생)다. 이 경우 백라이트만 켜지고 화면은 검은색으로 나온다. 커널은 이 상태에서도 "connected, enabled"로 보고하므로 패널에서 직접 레지스터를 읽어 봐야 원인을 알 수 있다.

| 관찰 | 의미 |
|---|---|
| 리셋 직후 전원 상태 `0x08`, ID `98 81 5c` | 패널 모델 일치, 명령 경로(읽기·쓰기·페이지 전환) 정상 |
| 초기화 후 전원 상태 `0x0c` | "display on"은 적용되었으나 "sleep out"과 "부스터 활성화"가 실패함 |
| sleep out 재전송 후에도 동일 | 명령 문제가 아니라 패널 구동 전압이 인가되지 않은 상태 |

화면이 검은색으로 나올 때 점검 순서(기기):

1. 커널 로그에 `패널 전원 상태 0x..(기대값 0x9c)` 경고가 있는지 확인한다. 직접 작성한 패널 드라이버가 초기화 끝에 이 값을 검사해 출력한다.
2. `/sys/kernel/debug/gpio`에서 `regulator-lcd-bias-e` 라인이 `out hi`인지 확인한다. `in`이면 바이어스 칩 enable 핀이 구동되지 않고 있는 것이다.
3. `/sys/kernel/debug/regmap/7-003e/registers`의 `00`, `01` 레지스터가 `12`인지 확인한다(바이어스 전압 설정).

패널 드라이버 작성 시 참고한 메인라인 드라이버: `panel-boe-th101mb31ig002-28a.c`(같은 800×1280), `panel-jadard-jd9365da-h3.c`. 순정 DT 초기화 시퀀스가 "타입, 지연, 길이, 데이터" 형식이라 기계적으로 변환해 쓸 수 있었다.

### 터치

| 항목 | 값 | 근거 |
|---|---|---|
| 칩 | Ilitek ILI2130 (`ILI21300RK070P00`), 프로토콜 V6, 최대 10점 | 기기 |
| 연결 | I2C8 `0x41`, 리셋 gpio0-6, 인터럽트 gpio0-2 (rising edge) | 순정 DT, 기기 |
| 좌표 | 두 축 모두 0~16383. **화면과 180도 회전된 상태라** 양축 반전 필요 | 기기 |
| (미장착) | Silead GSL3673 @ I2C8 `0x40` — 순정 DT에만 존재 | 기기 |

부팅 직후 메인라인 드라이버가 `Wrong id: 0x80` 경고를 한 줄 출력하지만 동작에는 영향이 없다(기기).

| 확인 내용 | 결과 |
|---|---|
| 경고 발생 위치 | 인터럽트 핸들러. 64바이트를 읽어 첫 바이트가 터치 보고 헤더(`0x48`)가 아니면 출력 |
| 발생 빈도 | 부팅당 정확히 1회, 인터럽트 등록 약 6ms 후. 이후 터치 126회 동안 재발 없음 |
| 드라이버 언바인드/재바인드 3회 | 재발 없음 |
| 터치가 없을 때 칩 직접 읽기 | `80 00 40 1a 00 11 00 0a …`와 문자열 `K070P00`. 터치 패킷이 아니라 직전 초기화 명령 응답의 잔여 데이터 |

즉, 터치 입력이 없는 상태에서 인터럽트가 1회 발생하고 드라이버가 직전 초기화 응답 잔여 데이터를 터치 패킷으로 파싱하려다 낸 경고다. 첫 인터럽트가 발생하는 정확한 원인은 규명하지 못했으나, 전원 인가 후 최초 등록 시에만 발생하는 점으로 보아 인터럽트를 처음 활성화하는 순간 GPIO 컨트롤러에 래치되어 있던 신호가 한 번 전달되는 것으로 추정한다. 이를 없애려면 드라이버 코드를 수정해야 하므로 그대로 두었다.

### 통신·기타

| 항목 | 값 | 근거 |
|---|---|---|
| Wi-Fi/BT | BCM43455 (BCM4345 rev 6), SDIO `fe310000`. 순정 펌웨어 `fw_bcm43455c0_ag_k019_p681x.bin` | 기기 |
| 블루투스 | UART0, 리셋 gpio0-9, wake gpio2-27, host-wake gpio0-4 | 순정 DT |
| 이더넷 | 내장 GMAC `fe300000`, RMII, PHY는 **RTL8201F** (ID `0x001cc816`), PHY 리셋 gpio3-16. MAC이 50MHz 기준 클럭 출력 | 순정 DT, 기기 |
| Wi-Fi 전원 순서 | 32kHz 클럭(RK808 clkout2) + WL_REG_ON gpio0-10, host-wake gpio0-3. SDIO는 SDR104, 순정 클럭은 150MHz(이 시스템에서는 50MHz로 하향 — 아래 "Wi-Fi SDIO 클럭" 참고) | 순정 DT, 기기 |
| 오디오 | ES8316 @ I2C1 `0x10`, I2S1, 기준 클럭은 SoC 출력(GPIO4_A0). 스피커 앰프 enable 핀 gpio0-12. 헤드폰 감지 핀 gpio0-11(순정 DT) | 순정 DT, 기기 |
| 마이크, 헤드폰 | 외부 단자 없음. 순정 DT에는 헤드폰 감지 핀과 마이크 경로가 정의되어 있으나 실장 여부는 불명 | 기기(외관), 순정 DT |
| 스피커 | 1개(모노). **앰프가 코덱 좌·우 출력의 차동 신호를 증폭한다** — 좌우에 동상 신호를 넣으면 상쇄되어 소리가 작고, 한쪽 채널 극성을 반전해야 정상 음량이 나온다. 앰프가 켜져 있는 동안 "쉬익" 하는 노이즈가 난다. 저음역(440Hz 부근)은 재생 출력이 약하다 | 기기(청취) |
| GPU | Mali-T860 MP4 `ff9a0000`. 메인라인 드라이버(Panfrost)로 OpenGL ES 3.1까지 지원. **Vulkan 미지원** | 기기 |
| 카메라 | OV5675 ×2, VCM GT9760S (I2C2) | 기기 |
| LED | AW2015 @ I2C7 `0x64` | 기기 |
| 버튼 | 전원 키 gpio0-5, ADC 키 2개("volume up" = 초기화 버튼, "volume down" = 어느 버튼인지 미확인) | 순정 DT |
| RTC | RK808 내장. 정상 동작하지만 **백업 배터리가 없어 전원을 뽑으면 2013-01-18로 초기화된다.** 재부팅 시에는 유지됨 | 기기 |

순정 DT의 `wifi_chip_type = "ap6354"` 표기는 실제 실장 칩과 다르다.

### 외부 포트

| 포트 | 확인된 내용 | 추정 |
|---|---|---|
| 하부 Micro USB (커버 내부) | 디바이스 모드 컨트롤러 `fe800000`. 플래싱, ADB, USB 가젯 | |
| 후면 USB-C | 12V 전원 입력. Type-C PHY 비활성, Type-C 컨트롤러 없음 | 전원 전용 |
| LAN | 순정에서 `eth0`으로 인식 | |
| Serial 단자 | RS-232 트랜시버 스위치 2개 존재 | WK2124(SPI-UART)의 `ttyS5`/`ttyS6`, RS-232 레벨 |

| 시리얼 장치 (순정 커널) | 하드웨어 | 용도 |
|---|---|---|
| `ttyS0` | UART0 | 블루투스 |
| `ttyS3` | UART3 | `/dev/ttyMT1`로 연결됨. 결제 보안 모듈용으로 추정되어 건드리지 않음 |
| `ttyS5`, `ttyS6` | WK2124 SPI-UART | 외부 Serial 단자용으로 추정 |
| `ttyFIQ0` | UART2 (`0xff1a0000`) | 커널 디버그 콘솔, 1,500,000bps |

호스트용 USB 컨트롤러 3개가 활성화되어 있으나 연결된 장치가 없고 배선 위치는 미확인이다.

### CPU 클럭과 발열 (기기)

- 순정 부트로더 인계 상태: 두 클러스터 모두 816MHz, `vdd_cpu_l` 0.9V, `vdd_cpu_b` 1.0V, GPU 500MHz / 1.0V.
- 메인라인 RK3399 OPP 테이블: 리틀 코어 408MHz~1.416GHz (0.825~1.125V), 빅 코어 408MHz~1.8GHz (0.825~1.2V), GPU 200~800MHz (0.825~1.1V). 순정이 각 레일에 허용한 전압 범위 안이다.
- 온도 스로틀링(메인라인 기본): 70°C 스로틀링 시작, 75°C 강한 스로틀링, 95°C 서멀 셧다운. 순정 하드웨어 차단 설정은 모드 1(핀), 극성 1, 120°C.
- 유휴 온도 약 46~55°C. 6코어 풀로드에서 15초 만에 70°C에 도달하며 스로틀링으로 72°C에서 유지된다. 방열 구조가 없는 기기다.

### 메모리 맵

- 가용 메모리 영역: `0x00200000`~`0x08400000`, `0x0a200000`~`0x80000000`. 사이 공간은 보안 펌웨어 예약 영역(기기).
- ramoops: `0x110000`, 크기 `0xf0000` (콘솔 `0x80000`). 재부팅은 물론 Loader와 MaskROM을 거쳐도 유지되지만 전원을 차단하면 소실된다(기기).

## 1.4 커널 지원

### 순정 커널 4.19.193

- 비활성: `USER_NS`, `SYSVIPC`, `VT`, `FHANDLE`, `AUTOFS4_FS`, `DEBUG_FS`, `/dev/mem`, USB HID 가젯
- 활성: USB 가젯(ACM, RNDIS, 대용량 저장장치, MIDI, UVC), ramoops, 워치독
- GPU는 비공개 `midgard` 드라이버

### 메인라인 6.18 (소스 및 실기기 확인)

| 컴포넌트 | 드라이버 | 상태 |
|---|---|---|
| RK808, SYR827/828, eMMC, USB 디바이스 모드 | 표준 메인라인 | **실기기 동작** |
| DSI 3레인 | `dw-mipi-dsi-rockchip` (1~4레인 지원) | **실기기 동작** |
| 패널 | 범용 드라이버 없음 -> 직접 작성 | **실기기 동작** |
| 터치 ILI2130 | `ilitek_ts_i2c` (`ilitek,ili2130`, V6만 지원) | **실기기 동작** |
| 바이어스 IML7525 | `tps65132` — enable 핀을 출력으로 전환하지 않는 문제가 있어 enable 핀은 별도 제어 | **실기기 동작** |
| 재부팅 인자 | `syscon-reboot-mode` — `rk3399.dtsi`에 노드가 없어 직접 추가 | **실기기 동작** |
| Wi-Fi BCM43455 | `brcmfmac` (`brcmfmac43455-sdio.*`, 순정 펌웨어 사용) | **실기기 동작** (AP 스캔, WPA2 접속 — 2.4GHz 및 5GHz) |
| 블루투스 | `hci_bcm` (`brcm,bcm4345c5`) | **실기기 동작** (주변 기기 스캔까지 확인) |
| 이더넷 | `dwmac-rk` + RTL8201 PHY | **실기기 동작** (100Mbps, 실제 통신 확인) |
| 오디오 ES8316 | `es8316` + `simple-audio-amplifier`. 순정은 코덱 드라이버가 앰프 핀을 직접 제어했으나 메인라인에는 해당 기능 없음 | **실기기 동작** |
| GPU | Panfrost | **실기기 동작** (OpenGL ES 3.1) |
| CPU 주파수 조절 | `cpufreq-dt`, `rk3399.dtsi` 공통 OPP 테이블 | **실기기 동작** |
| 온도 센서 | `rockchip-thermal` | **실기기 동작** (과열 스로틀링 확인) |
| RTC | `rk808-rtc` | **실기기 동작** |
| USB HID 가젯 | `f_hid` | 커널 포함, 미시험 |
| LED AW2015 | **미지원** (`AW2013` 드라이버만 존재) | |

메인라인 `rk3399.dtsi`에는 `/memory` 노드와 `reboot-mode` 노드가 정의되어 있지 않다.

## 1.5 유저랜드에서 확인한 것

### 화면 스택

| 항목 | 결과 | 근거 |
|---|---|---|
| Wayland 컴포지터 | `cage`(wlroots)가 GPU 렌더링으로 동작. 메모리 약 75MB | 기기 |
| 터치 | libinput이 터치스크린으로 인식해 터치 좌표 그대로 앱에 전달. 보정 불필요 | 기기 |
| 비루트 사용자로 실행 | `seat` 그룹(seatd)만으로는 부족하고 `video` 그룹도 필요. 컴포지터가 GPU 렌더링을 초기화하며 `/dev/dri/card*`를 직접 연다 | 기기 |
| 장치 관리자 | libinput이 udev를 요구한다. eudev를 켜도 네트워크 인터페이스 이름은 바뀌지 않는다 | 기기 |
| 화면 캡처 | `grim`으로 원격 캡처 가능 | 기기 |
| Qt QML 런처 | Alpine 런타임 패키지에는 `qmlscene`만 들어 있다(deprecated 경고를 낸다). `qml` 바이너리는 없다 | 기기 |
| Qt QML 캐시 | QML을 컴파일해 사용자 디렉터리(`~/.cache`)에 캐시해 두고, mtime(초 단위)만 비교해 재사용한다. 초 단위 mtime이 같도록 다른 내용의 파일로 덮어쓰면 이전 내용이 그대로 뜬다 | 기기 |
| 한·중·일 글꼴 | Alpine의 `font-noto-cjk`(설치 크기 88MB)에 Noto Sans CJK와 Noto Serif CJK의 Regular·Bold가 포함돼 있다. 국가별 5종과 고정 폭 숫자용 Mono가 함께 인식된다(총 15종). 다른 굵기는 `font-noto-cjk-extra`(208MB)에 있다 | 기기 |
| 스트림덱 UI 부하 | 유휴 상태에서 UI 프로세스의 CPU 점유율 1.3~2.5%(코어 1개 기준. 약 1초마다 시스템 상태가 바뀔 때만 리드로우), 메모리(RSS) 165~200MB(설정과 키보드를 열어 본 뒤 증가) | 기기 |
| 터치 입력 주입 | 터치 입력 장치(`/dev/input/event*`)에 press/release 이벤트를 write하면 실제 터치처럼 앱에 전달된다. 좌표는 패널 기준(두 축 0~16383)이고, 이벤트 묶음을 단일 `write`로 넘겨야 한다(쪼개서 쓰면 거부됨). `scripts/tf-tap.sh` | 기기 |
| 앱 내 온스크린 키보드 | `QT_IM_MODULE=qtvirtualkeyboard`로 Qt Virtual Keyboard를 켜면 Wayland에서 "클라이언트 쪽에서는 지원하지 않는다"는 경고가 뜨지만, 앱 내부 `InputPanel`은 정상 동작한다. Alpine 패키지에 한국어·일본어·중국어·태국어 입력기가 함께 들어 있다(설치 크기 6.1MB, 의존성 `libhunspell` 0.7MB). 키 스타일은 커스텀 `style.qml`로 바꿀 수 있다 | 기기 |
| `wpa_supplicant` 제어 | 설정 파일의 `ctrl_interface=DIR=… GROUP=<그룹>`으로 제어 소켓을 그룹에 열면 비루트 프로세스도 스캔·연결·저장을 요청할 수 있다. 응답 수신용 소켓은 abstract unix socket을 써도 된다. 잘못된 비밀번호는 약 20초 뒤 해당 AP가 `TEMP-DISABLED`로 표시되는 것으로 판별한다 | 기기 |
| Wi-Fi SDIO 클럭 | 순정 값인 150MHz(SDR104)에서는 칩이 슬립에서 깨어날 때 버스 에러가 발생한다(`brcmf_sdio_bus_sleep: error while changing bus sleep state -84`, `halting operation`). 부팅 직후에도 발생하며, 쿼리를 0.2초 간격으로 보내면 350회 안에 터졌다. 클럭을 50MHz로 낮추면 같은 테스트에서 재현되지 않았다. 에러가 난 뒤에는 링크가 유지된 것처럼 보여도 명령이 타임아웃된다 | 기기 |
| 5GHz 사용 후 Wi-Fi | 5GHz AP에서 연결이 끊긴 뒤 스캔이 5.7~5.8GHz 대역만 돌고(인터페이스 채널이 161에 고정됨) 2.4GHz AP에 붙지 못한다. 재부팅하면 풀린다. 두 번 관찰. SDIO 버스 에러가 0건인 상태에서도 발생했으므로 별개 이슈다. 원인 미확인 | 기기 |
| 같은 대역에 유선·무선 공존 | `ping -I wlan0`은 무선이 살아 있어도 실패한다(응답이 유선으로 돌아옴). 무선만 확인하려면 `arping -I wlan0` | 기기 |
| QML 터치 통과 차단 | 빈 `TapHandler`는 포인터 이벤트를 독점하지 않아 아래 레이어의 `TapHandler`까지 함께 반응한다. 아래로 통과시키지 않으려면 `MouseArea`를 쓴다 | 기기 |
| 부팅 후 연결한 Micro USB | 기기가 켜진 상태에서 꽂으니 인식되지 않았고(USB 상태 `not attached`), 재부팅하니 잡혔다. 1회 관찰, 원인 미확인 | 기기 |

### UI 프레임워크 비교 (기기 실측)

스트림덱 UI에 쓸 프레임워크를 고르기 위해 같은 구성의 화면을 두 프레임워크로 만들어 비교했다. 주사율이 약 57Hz라 57fps가 상한이다. CPU·GPU 주파수를 최대로 고정해 두고 쟀으며, CPU는 코어 1개를 100%로 잡은 수치다.

| 테스트 | Qt Quick | Slint |
|---|---|---|
| 스트림덱 스타일 UI (버튼 48개, 그림자, 페이지 전환, 확대·축소 애니메이션) | 57.0fps, CPU 47% | 54.0fps, CPU 90% |
| 위 화면의 최대 프레임 간격 | 21ms | 33ms |
| 사각형 500개 이동 | 57.0fps | 23.2fps |
| 사각형 1000개 이동 | 26.4fps | 12.2fps |
| 사각형 500개 이동 + 회전 | 45.8fps | 21.9fps |
| 사각형 200개 이동 + 회전 + 반투명 | 57.0fps | 3.3fps |
| 정지 화면 CPU | 1.4% | 0.0% |
| 메모리 (RSS) | 121MB | 102MB |

- Slint는 회전과 반투명이 겹치면 성능이 급락했다. 둘 중 하나만 쓰면 괜찮았다(200개 기준 51~54fps).
- **Slint 측정은 불리한 조건이었다.** nixpkgs에 미리 빌드된 뷰어(UI 정의를 런타임에 해석하는 방식, FemtoVG 렌더러, glibc)를 그래픽 드라이버(Mesa 26.3)와 함께 통째로 올려 돌렸다. 네이티브 컴파일 바이너리와 Skia 렌더러는 측정하지 못했다. 메모리도 뷰어 기준이라 실제 앱보다 크게 나온다.
- Qt Quick은 Alpine 패키지(Qt 6.11)의 `qmlscene`으로 돌렸다. 이미지에 약 136MB가 추가된다.
- 고부하 테스트에서는 70°C를 넘기며 스로틀링이 걸린 상태였다. 두 프레임워크 모두 같은 조건이다.
- 이 결과를 바탕으로 Qt Quick을 채택했다. 웹 엔진은 테스트하지 않았다.

측정 방법(재측정 시):

| 측정 항목 | 방법 |
|---|---|
| 프레임 수 | 앱을 `WAYLAND_DEBUG=1`로 실행하고, 로그에서 `wl_surface`에 새 버퍼를 `attach`한 횟수를 센다. 프레임워크와 무관하게 같은 기준이 된다 |
| GPU 사용 시간 | `echo 1 > /sys/bus/platform/drivers/panfrost/ff9a0000.gpu/profiling` 실행 후 `/proc/<pid>/fdinfo/*`의 `drm-engine-*` 값. 100%를 넘게 나오는 등 불안정해 참고만 했다 |
| 주파수 고정 | CPU는 거버너 `performance`, GPU는 `/sys/class/devfreq/ff9a0000.gpu/min_freq`에 최대값 |

### Alpine 패키지

- 미러는 패키지가 갱신되면 **이전 버전 파일을 삭제한다**. 버전 번호만 기록해서는 나중에 같은 파일을 받을 수 없다.
- 패키지 설치 스크립트는 기기용 프로그램을 호출한다. PC에서 rootfs 이미지를 만들 때는 실행할 수 없다.
- Alpine 기본 busybox에는 DHCP 서버(`udhcpd`)가 없다. `busybox-extras`에 있다.
- `libinput-tools`는 Python을 통째로 끌어온다(40MB 이상).

### 절전 및 네트워크 웨이크업 (WoL)

2026-11-17에 조사하고 테스트했다. "전원을 끈 덱을 네트워크로 켤 수 있는가"를 확인했다. **전원이 꺼진 상태에서는 불가능하고, 절전 상태에서는 가능성이 있다.** 절전은 얕은 절전만 동작한다. **얕은 절전에서 Wi-Fi 매직 패킷으로는 깨어나고, 이더넷으로는 깨어나지 않는다**(아래 표).

| 항목 | 확인 내용 | 근거 |
|---|---|---|
| 전원을 끈 상태 | 설정의 `Power off`는 PMIC(RK808)가 전원을 차단한다. 이더넷 PHY와 Wi-Fi 칩도 꺼지므로 패킷을 수신할 장치가 없다. PHY나 Wi-Fi에서 PMIC의 전원 켜기로 가는 배선은 순정 DT에도 없다 | 순정 DT, 메인라인 DTS |
| 이더넷 컨트롤러 | 매직 패킷 감지 기능이 있다: 커널 로그에 `Wake-Up On Lan supported`. 웨이크업 소스로 쓸 수 있게 등록돼 있고 현재는 비활성 상태다(`/sys/class/net/eth0/device/power/wakeup` = `disabled`) | 기기 |
| 이더넷 PHY | RTL8201F(id `0x001cc816`), RMII 100Mbps. PHY의 웨이크업 핀(PMEB)은 순정 DT에도 연결돼 있지 않다. 깨우려면 PHY가 아니라 컨트롤러 기능을 써야 한다 | 기기, 순정 DT |
| Wi-Fi (BCM43455) | 칩과 드라이버가 웨이크업 지원을 보고한다(`iw phy`의 WoWLAN: 매직 패킷, 패턴, 연결 끊김). 웨이크업 라인(`host-wake`, GPIO0 A3)이 DTS에 있고, 절전 중에도 칩 전원을 유지하도록 설정돼 있다(`keep-power-in-suspend`) | 기기, 메인라인 DTS |
| 절전 | 커널이 `freeze`, `mem`(`s2idle`, `deep`), `disk`를 제공한다. `mem`의 기본값은 `deep`이다 | 기기 |
| 얕은 절전 (`freeze` = s2idle) | **동작한다** (2026-11-17 테스트). 시계 알람을 20초 뒤로 걸고 절전에 넣으니 22초 뒤에 깨어났다. 절전 중에는 SSH 접속이 되지 않았고, 깨어난 뒤 UI, 데몬, USB 가젯, 이더넷, Wi-Fi(AP에 연결된 상태 유지), 호스트 연결이 모두 복구됐다. 깨어난 직후 Wi-Fi 드라이버가 채널 설정 실패(`brcmf_set_channel … -52`)를 여러 줄 출력하지만 연결은 유지됐다. 절전 중 화면이 꺼지는지, 소비 전력이 얼마나 줄어드는지는 확인하지 못했다 | 기기 |
| 깊은 절전 (`mem` = deep) | **깨어나지 않았다** (2026-11-17 테스트). 시계 알람을 20초 뒤로 걸고 절전에 넣었는데 2분이 지나도 USB에도 LAN에도 나타나지 않아 재부팅 버튼으로 복구해야 했다. 알람이 깊은 절전에서는 깨우지 못하는 것인지, 깨어나다 멈춘 것인지는 구분하지 못했다(순정 DT에는 절전 방식과 웨이크업 수단을 펌웨어에 전달하는 `rockchip-suspend` 설정이 있으나 메인라인 DTS에는 없다). **깊은 절전은 사용하지 말 것** |
| 웨이크업 소스로 등록된 장치 | `mmc0`, `mmc1`, RK808과 그 RTC, 시리얼. 이더넷과 Wi-Fi는 활성화해야 한다 | 기기 |
| 전원 버튼 | 메인라인에서는 입력 장치로 잡히지 않는다(입력 장치는 터치와 `adc-keys`뿐). 순정 DT에는 `gpio-keys`의 전원 키가 웨이크업 키로 정의돼 있다 | 기기, 순정 DT |
| 도구 | `ethtool`이 이미지에 없다(이더넷 웨이크업을 켜는 데 사용한다) | 기기 |

**매직 패킷 웨이크업 테스트** (2026-11-17. 얕은 절전에 넣고, 안전장치로 시계 알람을 걸어 둔 상태에서 PC에서 패킷을 보냈다. 알람보다 일찍 깨어나면 패킷이 깨운 것이다)

| 테스트 | 설정 | 결과 |
|---|---|---|
| 이더넷, 패킷 없음 | `ethtool -s eth0 wol g` (지원: `ug`), 알람 30초 | 33초에 깨어남 (알람) |
| 이더넷, 매직 패킷(브로드캐스트) | 위와 동일, 알람 60초, 12초쯤에 전송 | **62초에 깨어남 (알람) — 패킷으로는 깨어나지 않음** |
| 이더넷, 기기로 직접 전송(매직 패킷 + ping) | `wol ug`, 알람 60초 | **62초 (알람) — 깨어나지 않음** |
| Wi-Fi, 매직 패킷(브로드캐스트 + 직접) | `iw phy phy0 wowlan enable magic-packet`, 알람 60초, 12초쯤에 전송 | **13초에 깨어남 — 패킷이 깨웠다** |
| Wi-Fi, 패킷 없음 | 위와 동일, 알람 40초 | 43초에 깨어남 (알람). 저절로 깨어나지 않는다 |

- 이더넷으로 깨어나지 않는 원인은 찾지 못했다. 컨트롤러는 지원한다고 보고하고 설정도 받아들이지만(`Wake-on: g`, 웨이크업 소스로 활성화됨), 절전 중 들어온 패킷이 깨우지 못한다. 절전 중 컨트롤러의 전원 도메인이나 클럭이 꺼지는 것이 아닌지 의심한다(확인하지 않았다).
- Wi-Fi로 깨어날 때 드라이버가 `brcmf_report_wowl_wakeind: Get wowl_wakeind failed, err = -5`를 출력한다(웨이크업 원인을 칩에 질의하는 데 실패). 깨어나는 동작 자체에는 지장이 없었다.
- 테스트를 위해 기기에 `ethtool`을 수동으로 설치했다(`apk add`). 이미지에는 없으므로 `userdata`를 플래싱하면 사라진다. 웨이크업 설정도 테스트 후 다시 껐다(껐다 켜면 초기화되는 설정이다).

이 결과를 바탕으로 절전 기능을 구현했다(2026-11-17): 덱 설정과 PC의 `toss-connector sleep`으로 얕은 절전에 넣고, `toss-connector wake`가 Wi-Fi로 매직 패킷을 보내 깨운다([03-userland-guide.md](03-userland-guide.md) 3.16절의 "화면 밝기, 절전, 전원 끄기"). 절전 중 화면이 꺼지는지와 소비 전력이 얼마나 줄어드는지는 아직 확인하지 못했다. 터치 웨이크업은 없다(터치 패널이 웨이크업 소스로 등록돼 있지 않다. DTS에 `wakeup-source`를 넣으면 될 수 있지만 시도하지 않았다).

반대 방향(덱이 PC를 깨우기)은 별개 사안이다: 매직 패킷은 일반 UDP 브로드캐스트라 덱에서 보내는 데 제약이 없다.

### 이더넷과 Wi-Fi가 같은 네트워크에 물렸을 때 (기기)

이더넷과 Wi-Fi가 같은 공유기에 연결되면 덱에 같은 대역의 IP가 두 개 생긴다(이더넷 IP와 Wi-Fi IP). 이때 **Wi-Fi IP로는 PC에서 접속할 수 없다**. ping도, 어떤 포트로의 접속도 응답이 없다(2026-11-17).

| 확인 항목 | 결과 |
|---|---|
| Wi-Fi 자체 | 정상. AP에 붙어 있고 IP도 할당됨 |
| PC가 아는 해당 IP의 MAC | Wi-Fi MAC (패킷은 Wi-Fi로 유입됨) |
| 덱의 라우팅 경로 | 같은 대역으로 가는 경로가 둘이고 이더넷이 우선(metric 1002 대 3004). 응답 패킷은 이더넷으로 나감 |
| `rp_filter` | 1 (strict). 유입 인터페이스와 역방향 송신 인터페이스가 다르면 드롭 |
| 드롭 카운터 | Wi-Fi IP로 ping을 3회 보내자 `IPReversePathFilter` 카운터가 47 → 50으로 증가 |

- 이더넷 IP와 USB IP로는 정상 접속된다. 이더넷을 뽑으면 Wi-Fi가 유일한 경로가 되어 Wi-Fi IP도 응답한다.
- 매직 패킷 웨이크업은 이 필터링을 거치지 않으므로 영향을 받지 않는다(이더넷이 꽂힌 상태에서도 깨어났다).
- 예전에 이 IP가 응답했던 적이 있는데, 그때는 PC가 해당 IP의 MAC을 이더넷 쪽 MAC으로 알고 있었던 것으로 보인다(리눅스는 다른 인터페이스의 IP에 대해서도 ARP 응답을 보낸다). ARP 캐시가 어느 쪽으로 학습되느냐에 따라 달라진다.
- 해결하려면 역방향 경로 검사를 느슨하게(`net.ipv4.conf.all.rp_filter=2`) 설정하면 된다. 실사용에 지장이 없어 그대로 두었다.

## 1.6 미확인 항목

- 후면 USB-C 포트에 데이터 라인이 배선돼 있는지
- 외부 Serial 단자의 핀아웃, 대응 디바이스, 보드레이트
- 보드 위 UART2 디버그 핀 위치
- 호스트 USB 포트의 라우팅 대상
- 부팅 직후 터치 인터럽트가 1회 들어오는 정확한 원인
- 패닉 후 자동 재부팅이 설정값 5초가 아니라 120초로 출력된 이유 (순정 커널)
- 5GHz AP 사용 후 2.4GHz AP에 붙지 못하는 원인 (재부팅하면 풀림, 2회 관찰)
- 부팅 후 연결한 Micro USB가 인식되지 않는 원인 (1회 관찰)
- 깊은 절전에서 깨어나지 못하는 이유, 이더넷 매직 패킷이 깨우지 못하는 이유 (위 "절전 및 네트워크 웨이크업" 참고)
- 얕은 절전 중 화면이 꺼지는지, 소비 전력은 얼마나 줄어드는지
- 전원 off 후 다시 켜는 방법 (전원 버튼이 메인라인에서 입력 장치로 잡히지 않음)
- 커널 인자 `clk_ignore_unused`, `pd_ignore_unused`를 제거해도 되는지 (초기 안전장치로 추가)
- USB HID 가젯이 실제로 PC에 키보드로 인식되는지

## 1.7 참고 자료

커뮤니티 (디시인사이드 SUNMI 마이너 갤러리):

- [토스 기기 MOD 게시글 모음](https://gall.dcinside.com/mgallery/board/view/?id=sunmitechnology&no=20&page=1)
- [기기 사양과 세대 구분](https://gall.dcinside.com/mgallery/board/view/?id=sunmitechnology&no=21&page=1)
- [PC 연결과 Loader Mode 진입](https://gall.dcinside.com/mgallery/board/view/?id=sunmitechnology&no=23&page=1)
- [파티션 백업 추출과 결과 확인](https://gall.dcinside.com/mgallery/board/view/?id=sunmitechnology&no=25&page=1)
- [프론트1 기기 펌웨어 복구 (패드 위치 그림, 761 이미지)](https://gall.dcinside.com/mgallery/board/view/?id=sunmitechnology&no=26&page=1)
- [LineageOS for Toss Devices 배포](https://gall.dcinside.com/mgallery/board/view/?id=sunmitechnology&no=39&page=1)
- [프론트 1세대 1.5세대는 업데이트된듯 (799의 진입 차단)](https://gall.dcinside.com/mgallery/board/view/?id=sunmitechnology&no=56&page=1)

저장소:

- [kyseong00/toss-front-lineageos-builder](https://github.com/kyseong00/toss-front-lineageos-builder) — 순정 `super`에 GSI를 합성하는 빌더(Windows)
- [StudentsView/toss-front-plf02wh-lineageos-guide](https://github.com/StudentsView/toss-front-plf02wh-lineageos-guide)
- [SFFworks/PC-dashboard](https://github.com/SFFworks/PC-dashboard) — 토스 기기를 PC 대시보드로 쓰는 앱(안드로이드)
- [ophub/u-boot의 RK3399 로더](https://github.com/ophub/u-boot/tree/main/u-boot/rockchip/sz3390)
- [rockchip-linux/u-boot](https://github.com/rockchip-linux/u-boot) `next-dev` — 순정 U-Boot 동작 참고용 (SUNMI 빌드는 다를 수 있음)

공식:

- [토스 프론트 제품 페이지](https://tossplace.com/product/front)
- [타사 포스, 토스 프론트 연결](https://tossplace.com/support/31499) — 구성품에 "RJ9 to RS232" 케이블

커뮤니티에 올라온 이미지 파일은 익명 업로드라 출처를 보증할 수 없다. MD5 대조는 게시글의 파일과 동일하다는 것만 확인해 준다.
