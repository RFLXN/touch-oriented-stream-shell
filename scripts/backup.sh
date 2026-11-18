#!/usr/bin/env bash
# 토스 프론트(PLF02WH) 원본 파티션 백업(읽기 전용 — 기기에 쓰기 작업 없음)
#
# 사용법: nix develop -c scripts/backup.sh [출력 디렉터리]     (기본값: 저장소의 backup/)
#
# 사전 조건: `scripts/flash.sh --enter`로 MaskROM + 로더 적재 상태여야 한다(미충족 시 중단).
# 기기 내장 Loader에서 읽으면 크기만 맞고 내용은 `0xCC`로 채워진 파일이 나오기 때문이다.
# 14개 파티션(`scripts/lib.sh` 테이블)과 GPT 앞뒤 영역을 `<출력 디렉터리>/<이름>.img`로 덤프하고,
# 기기가 보고한 정보(칩, 플래시, 파티션 테이블)를 `device-info.txt`에 저장한다.
# 크기가 일치하는 파일이 이미 있으면 건너뛰며, 중간에 중단된 경우 다시 실행하면 이어서 덤프한다.
# 백업에는 기기 고유 정보가 포함되므로 출력 디렉터리는 git 추적 대상 밖이어야 한다(`backup/`은 `.gitignore`에 포함됨).
set -euo pipefail
. "$(dirname "$0")/lib.sh"

case "${1:-}" in -*) usage ;; esac
[ $# -le 1 ] || usage
out=${1:-$top/backup}

rk_require_readable
mkdir -p "$out"

mapfile -t ranges <<EOF
gpt_head      0x00000000 0x00002000
$tf_parts
gpt_tail      0x01D1FFC0 0x00000040
EOF

{
  date
  rkdeveloptool rci
  rkdeveloptool rfi
  rkdeveloptool ppt
} 2>&1 | sed 's/\x1b\[[0-9;]*[A-Za-z]//g' >"$out/device-info.txt"

for p in "${ranges[@]}"; do
  [ -n "$p" ] || continue
  read -r name start count <<<"$p"
  file="$out/$name.img"
  want=$((count * 512))
  if [ -f "$file" ] && [ "$(stat -c %s "$file")" -eq "$want" ]; then
    echo "[skip] $name (이미 있음)"
    continue
  fi
  echo "[read] $name start=$start count=$count ($want bytes)"
  rk rl "$start" "$count" "$file" || { echo "[FAIL] $name 읽기 실패 (rkdeveloptool rl)" >&2; exit 1; }
  got=$(stat -c %s "$file")
  if [ "$got" -ne "$want" ]; then
    echo "[FAIL] $name 크기 불일치: $got != $want" >&2
    exit 1
  fi
  echo "[ ok ] $name"
done

echo "[done] 모든 파티션 읽기 완료"
