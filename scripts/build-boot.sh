#!/usr/bin/env bash
# 커널 개발용 테스트 부트 이미지를 생성한다 -> `build/boot-dev.img`
# 정식 이미지는 `scripts/build-all.sh`(`nix build`)가 생성한다.
#
# 사용법: nix develop -c scripts/build-boot.sh
# 입력: `scripts/build-kernel.sh`의 증분 빌드 결과(`build/linux-out`) + 정식 initramfs(`nix build .#initramfs`)
# 커널 커맨드라인은 정식 빌드와 동일한 `os/kernel/cmdline`에서 읽는다.
#
# 테스트 방법: `recovery` 파티션에만 기록해 1회 부팅으로 확인한다(`boot` 파티션은 건드리지 않음).
#   scripts/flash.sh --enter  ->  scripts/flash.sh recovery build/boot-dev.img  ->  scripts/flash.sh --reboot
#   ->  scripts/tf-ssh.sh --reboot recovery
set -euo pipefail
. "$(dirname "$0")/lib.sh"

build=$top/build

cmdline=$(grep -v '^[[:space:]]*#' "$top/os/kernel/cmdline" | xargs)
initramfs=$(nix build "$top#initramfs" --no-link --print-out-paths)

"$here/mkboot.sh" -p boot \
  -k "$build/linux-out/arch/arm64/boot/Image" \
  -r "$initramfs/initramfs.cpio.gz" \
  -d "$build/linux-out/arch/arm64/boot/dts/rockchip/rk3399-toss-front.dtb" \
  -c "$cmdline" \
  -o "$build/boot-dev.img"
