#!/usr/bin/env bash
# 기기 터치스크린에 드래그 입력을 주입한다(드래그 앤 드롭 등 제스처 동작을 무인으로 테스트할 때 사용).
#
# 사용법: scripts/tf-drag.sh <x1> <y1> <x2> <y2> [<x3> <y3> ...]
#   좌표는 화면 픽셀 단위(800x1280, 좌상단 0,0)다. 첫 지점을 누른 뒤 경유 지점을 순서대로 거쳐 마지막 지점에서 뗀다.
#   지점 간 이동은 여러 단계로 보간하며, 지나는 지점마다 `HOLD`초(기본 0.25) 동안 정지한다.
#   예: scripts/tf-drag.sh 200 520 600 800                     한 지점에서 다른 지점으로 드래그
#       HOLD=1 scripts/tf-drag.sh 200 520 790 520 400 300      가장자리에 1초 머물러 페이지를 넘긴 뒤 드롭
#
# 단순 탭은 `scripts/tf-tap.sh` 참고. 기기의 터치 입력 장치(`ilitek_ts`)에 evdev 이벤트를 직접 기록한다.
# 첫 지점에서 다운 프레임(슬롯, 추적 ID, X, Y, `BTN_TOUCH=1`), 이동 중 좌표 프레임,
# 마지막에 업 프레임(추적 ID -1, `BTN_TOUCH=0`)을 기록한다. 지점 사이는 14단계로 나누어 0.02초 간격으로 이동한다.
# 장치 탐색 및 이벤트 포맷은 `scripts/tf-tap.sh`와 공용이다(`scripts/lib.sh`의 `tf_touch_preamble`).
# `TF_HOST=<주소>`를 지정하면 LAN 경유로 접속한다(`scripts/tf-ssh.sh`와 동일).
set -euo pipefail
. "$(dirname "$0")/lib.sh"

if [ $# -lt 4 ] || [ $(($# % 2)) -ne 0 ]; then usage; fi
for n in "$@"; do case $n in ''|*[!0-9]*) echo "좌표는 정수여야 합니다: $n" >&2; exit 2 ;; esac; done

{
  printf '%s\n' "$tf_touch_preamble"
  cat <<'EOF'
at() { { ev 3 47 0; ev 3 53 $(($1 * 16383 / 800)); ev 3 54 $(($2 * 16383 / 1280)); ev 0 0 0; } > /tmp/tf-drag.ev; cat /tmp/tf-drag.ev > "$dev"; }
x=$1; y=$2; shift 2
{ ev 3 47 0; ev 3 57 78; ev 3 53 $((x * 16383 / 800)); ev 3 54 $((y * 16383 / 1280)); ev 1 330 1; ev 0 0 0; } > /tmp/tf-drag.ev
cat /tmp/tf-drag.ev > "$dev"
sleep "$HOLD"
while [ $# -ge 2 ]; do
  steps=14
  i=1
  while [ $i -le $steps ]; do
    at $((x + ($1 - x) * i / steps)) $((y + ($2 - y) * i / steps))
    sleep 0.02
    i=$((i + 1))
  done
  x=$1; y=$2; shift 2
  sleep "$HOLD"
done
{ ev 3 47 0; ev 3 57 4294967295; ev 1 330 0; ev 0 0 0; } > /tmp/tf-drag.ev
cat /tmp/tf-drag.ev > "$dev"
exit 0
EOF
} | "$here/tf-ssh.sh" "HOLD=${HOLD:-0.25} sh -s -- $*"
