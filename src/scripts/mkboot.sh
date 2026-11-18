#!/usr/bin/env bash
# 순정 U-Boot가 요구하는 형식(Android boot image v2 + RSCE 리소스 묶음)으로 부트 이미지를 생성한다.
# 헤더 오프셋과 버전 값은 순정 761 `boot.img`에서 추출했다(동일 입력으로 리패킹 시 바이트 단위 일치 확인됨).
#
# 사용법: nix develop -c src/scripts/mkboot.sh -k <커널 Image> -r <initramfs> -d <DTB> -o <출력> [-c <cmdline>] [-s <RSCE 묶음>] [-p <boot|recovery>]
#   -s를 생략하면 DTB만 담은 RSCE 리소스 묶음을 새로 생성한다(부트 로고 제외, 약 6MB 절감).
#   -c를 생략하면 순정 커널 커맨드라인을 사용한다.
#   -p는 대상 파티션(기본값 `boot`)이며, 생성된 이미지가 파티션 크기를 초과하면 실패한다.
#
# Nix 빌드(`nix/build.nix`)가 이 스크립트와 `src/scripts/rsce.py`만 복사해 실행하므로 `src/scripts/lib.sh`를 로드하지 않고
# 두 파티션의 섹터 수를 아래에 직접 둔다(`src/scripts/lib.sh`의 파티션 테이블과 일치해야 함).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)

usage() {
  awk 'NR > 1 { if (!/^#/) exit; sub(/^# ?/, ""); print }' "$0" >&2
  exit 2
}

stock_cmdline='console=ttyFIQ0 androidboot.baseband=N/A androidboot.wificountrycode=KR androidboot.veritymode=enforcing androidboot.hardware=rk30board androidboot.console=ttyFIQ0 androidboot.verifiedbootstate=orange firmware_class.path=/vendor/etc/firmware init=/init rootwait ro loop.max_part=7 buildvariant=user test_power.battery_capacity=100 test_power.battery_status=full'

kernel= ramdisk= dtb= out= second= part=boot cmdline=$stock_cmdline
while getopts "k:r:d:o:c:s:p:" opt; do
  case $opt in
    k) kernel=$OPTARG ;;
    r) ramdisk=$OPTARG ;;
    d) dtb=$OPTARG ;;
    o) out=$OPTARG ;;
    c) cmdline=$OPTARG ;;
    s) second=$OPTARG ;;
    p) part=$OPTARG ;;
    *) usage ;;
  esac
done
[ -n "$kernel" ] && [ -n "$ramdisk" ] && [ -n "$dtb" ] && [ -n "$out" ] || usage

case $part in
  boot) part_bytes=$((0x14000 * 512)) ;;
  recovery) part_bytes=$((0x3C000 * 512)) ;;
  *) echo "[FAIL] 알 수 없는 파티션: $part" >&2; exit 2 ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "${tmp:?}"' EXIT

if [ -z "$second" ]; then
  cp "$dtb" "$tmp/rk-kernel.dtb"
  python3 -I "$here/rsce.py" pack "$tmp/second" "$tmp/rk-kernel.dtb" >/dev/null
  second=$tmp/second
fi

mkbootimg \
  --header_version 2 \
  --os_version 11.0.0 \
  --os_patch_level 2024-09 \
  --kernel "$kernel" \
  --ramdisk "$ramdisk" \
  --second "$second" \
  --dtb "$dtb" \
  --pagesize 0x800 \
  --base 0x0 \
  --kernel_offset 0x10008000 \
  --ramdisk_offset 0x11000000 \
  --second_offset 0x10f00000 \
  --tags_offset 0x10000100 \
  --dtb_offset 0x11f00000 \
  --cmdline "$cmdline" \
  --output "$out"

size=$(stat -c %s "$out")
if [ "$size" -gt "$part_bytes" ]; then
  echo "[FAIL] 부트 이미지가 $part 파티션 크기를 초과합니다: $size > $part_bytes" >&2
  exit 1
fi
echo "[ ok ] $out: $size bytes ($part 파티션 여유 $((part_bytes - size)) bytes)"
