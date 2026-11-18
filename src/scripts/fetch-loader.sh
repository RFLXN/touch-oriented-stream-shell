#!/usr/bin/env bash
# MaskROM 상태의 기기에 적재할 RK3399 로더를 내려받는다 -> `local/loader/rk3399_loader_v1.30.130.bin`
#
# 사용법: nix develop -c src/scripts/fetch-loader.sh
# 로더는 기기 RAM에만 적재되며(`rkdeveloptool db`) 플래시 스토리지에는 기록하지 않는다.
# 이 로더가 올라간 상태여야 기기 eMMC를 정상적으로 읽고 쓸 수 있다.
# 내려받은 파일은 SHA-256 검증을 통과한 경우에만 대상 경로로 옮기며, 기존 파일의 해시가 다르면 다시 내려받는다.
set -euo pipefail
. "$(dirname "$0")/lib.sh"

out=$top/local/loader/rk3399_loader_v1.30.130.bin
url=https://raw.githubusercontent.com/ophub/u-boot/main/u-boot/rockchip/sz3390/rk3399_loader_v1.30.130.bin
sha256=e7ea515e2185c67946268ccf2f2aa85d581fc1a69eb40c0b3ec634499178963a

matches() { echo "$sha256  $1" | sha256sum -c - >/dev/null 2>&1; }

mkdir -p "$(dirname "$out")"
if [ -f "$out" ] && matches "$out"; then
  echo "[ ok ] $out (이미 있음, SHA-256 일치)"
  exit 0
fi

trap 'rm -f "$out.part"' EXIT
curl -fL --max-time 60 -o "$out.part" "$url"
matches "$out.part" || { echo "[FAIL] 내려받은 로더의 SHA-256 해시 불일치: $url" >&2; exit 1; }
mv "$out.part" "$out"
echo "[ ok ] $out (SHA-256 일치)"
