#!/usr/bin/env bash
# 커널 개발용 증분 빌드 스크립트. 정식 빌드는 `nix build .#kernel .#dtb`(`scripts/build-all.sh`)를 사용한다.
#
# 사용법: nix develop -c scripts/build-kernel.sh [config|image|dtb|all]
#   config  커널 설정 생성(`defconfig` -> 타 SoC 및 모듈 비활성화 -> `os/kernel/toss-front.config` 병합)
#   image   커널 `Image` 빌드
#   dtb     `os/kernel/dts/*.dts`를 커널 트리에 복사해 DTB 빌드
#   all     위 전체(기본값)
#
# 분리한 이유: Nix 빌드는 설정이나 드라이버를 한 줄만 수정해도 커널 전체를 처음부터 다시 컴파일한다.
# 이 스크립트는 `build/linux`에 풀어 둔 소스에서 변경된 부분만 재컴파일하므로 테스트 주기를 크게 줄여 준다.
# 실제 빌드 절차는 정식 빌드와 동일한 `os/kernel/kbuild.sh`를 사용한다.
# 출력은 `build/linux-out`에 생성되며 `scripts/build-boot.sh`로 테스트용 부트 이미지를 만들 수 있다.
# 개발이 끝나면 정식 빌드로 다시 빌드해 플래싱해야 한다 — 기기에 남는 것은 정식 빌드여야 한다.
set -euo pipefail
. "$(dirname "$0")/lib.sh"

what=${1:-all}
case $what in
  config | image | dtb | all) ;;
  *) usage ;;
esac

KSRC=$top/build/linux
KOUT=$top/build/linux-out
KDIR=$top/os/kernel
JOBS=$(nproc)
# shellcheck source=../os/kernel/kbuild.sh
source "$KDIR/kbuild.sh"

if [ ! -f "$KSRC/Makefile" ]; then
  tarball=$(nix build --inputs-from "$top" nixpkgs#linux_6_18.src --no-link --print-out-paths)
  mkdir -p "$KSRC"
  tar xf "$tarball" -C "$KSRC" --strip-components=1
fi

do_config() {
  tf_add_drivers
  tf_config
}

do_image() {
  tf_add_drivers
  mk Image
  ls -l "$KOUT/arch/arm64/boot/Image" | awk '{print "[image]", $5, "bytes"}'
}

do_dtb() {
  local f name
  tf_add_dts
  for f in "$KDIR"/dts/*.dts; do
    [ -e "$f" ] || continue
    name=$(basename "${f%.dts}")
    mk "rockchip/$name.dtb"
    ls -l "$KOUT/arch/arm64/boot/dts/rockchip/$name.dtb" | awk '{print "[dtb]", $5, "bytes", $9}'
  done
}

case $what in
  config) do_config ;;
  image) do_image ;;
  dtb) do_dtb ;;
  all)
    [ -f "$KOUT/.config" ] || do_config
    do_image
    do_dtb
    ;;
esac
