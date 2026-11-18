#!/usr/bin/env bash
# 기기 터치스크린의 특정 좌표에 탭 입력을 주입한다(UI 동작을 무인으로 확인할 때 사용).
#
# 사용법: src/scripts/tf-tap.sh <x> <y> [<x> <y> ...]
#   좌표는 화면 픽셀 단위(800x1280, 좌상단 0,0)다. 여러 좌표를 주면 0.6초 간격으로 순서대로 탭한다.
#   예: src/scripts/tf-tap.sh 750 53      상단바 설정 버튼
#
# 기기의 터치 입력 장치(`ilitek_ts`)에 다운/업 evdev 이벤트를 직접 기록하며 기기에 별도 도구를 설치하지 않는다.
# 좌표마다 다운 프레임(슬롯, 추적 ID, X, Y, `BTN_TOUCH=1`)을 쓰고 0.08초 후 업 프레임(추적 ID -1, `BTN_TOUCH=0`)을 쓴다.
# 장치 탐색 및 이벤트 포맷은 `src/scripts/tf-drag.sh`와 공용이다(`src/scripts/lib.sh`의 `tf_touch_preamble`).
# `TF_HOST=<주소>`를 지정하면 LAN 경유로 접속한다(`src/scripts/tf-ssh.sh`와 동일).
set -euo pipefail
. "$(dirname "$0")/lib.sh"

if [ $# -lt 2 ] || [ $(($# % 2)) -ne 0 ]; then usage; fi
for n in "$@"; do case $n in ''|*[!0-9]*) echo "좌표는 정수여야 합니다: $n" >&2; exit 2 ;; esac; done

{
  printf '%s\n' "$tf_touch_preamble"
  cat <<'EOF'
while [ $# -ge 2 ]; do
  x=$(($1 * 16383 / 800)); y=$(($2 * 16383 / 1280)); shift 2
  { ev 3 47 0; ev 3 57 77; ev 3 53 $x; ev 3 54 $y; ev 1 330 1; ev 0 0 0; } > /tmp/tf-tap.down
  { ev 3 47 0; ev 3 57 4294967295; ev 1 330 0; ev 0 0 0; } > /tmp/tf-tap.up
  cat /tmp/tf-tap.down > "$dev"; sleep 0.08; cat /tmp/tf-tap.up > "$dev"
  [ $# -ge 2 ] && sleep 0.6
done
exit 0
EOF
} | "$here/tf-ssh.sh" "sh -s -- $*"
