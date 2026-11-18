#!/usr/bin/env bash
# 토스 프론트(PLF02WH)에 LineageOS 20(20260928.001) 플래싱 — `boot`, `recovery`, `super` 3개 파티션 기록.
#
# 사용법: nix develop -c src/scripts/flash-lineage.sh [-y] [이미지 디렉터리]   (기본값: lineage)
#         `CHECK_ONLY=1` 지정 시 파일 검증만 수행하고 기기에는 기록하지 않는다.
#         `-y` 지정 시 확인 프롬프트를 건너뛴다.
#
# 사전 조건: `src/scripts/flash.sh --enter`로 MaskROM + 로더 적재 상태여야 한다(미충족 시 중단).
#            `local/backup/`에 원본 백업을 보관해 두어야 한다(`src/scripts/backup.sh`).
# 절차: 이미지 3종의 MD5(배포 게시글 기준 값) 검증 -> Android sparse 이미지(매직 `3aff26ed`)를 raw로 변환해
# 파티션 크기 초과 여부 확인 -> 사용자 확인 -> 파티션별로 `src/scripts/flash.sh`로 기록.
# `flash.sh`는 기기 파티션 테이블을 검증한 뒤 기록하고 매번 되읽기 검증을 수행하므로 하나라도 불일치하면 즉시 중단된다.
set -euo pipefail
. "$(dirname "$0")/lib.sh"

yes=0
img=$top/local/lineage
for a in "$@"; do
  case $a in
    -y) yes=1 ;;
    -*) usage ;;
    *) img=$a ;;
  esac
done

parts=(
  "boot     4c14ff000876bf014213774e8c7c4f24"
  "recovery d2cb9d2d21326e7fb5d4f7de4fd27d8b"
  "super    1fe85406050b73cc7823488c2d4adc2e"
)

tmp=$(mktemp -d -p "${TMPDIR:-/tmp}")
trap 'rm -rf "${tmp:?}"' EXIT

declare -A raw
for p in "${parts[@]}"; do
  read -r name md5 <<<"$p"
  file=$img/$name.img
  [ -f "$file" ] || { echo "[FAIL] 파일 없음: $file" >&2; exit 1; }
  got=$(md5sum "$file" | cut -d' ' -f1)
  [ "$got" = "$md5" ] || { echo "[FAIL] $name MD5 불일치: $got (기대값: $md5)" >&2; exit 1; }
  if [ "$(xxd -l 4 -p "$file")" = "3aff26ed" ]; then
    echo "[info] $name: sparse 이미지 -> raw 변환"
    simg2img "$file" "$tmp/$name.raw"
    raw[$name]=$tmp/$name.raw
  else
    raw[$name]=$file
  fi
  range=$(part_lookup "$name")
  read -r _ count <<<"$range"
  size=$(stat -c %s "${raw[$name]}")
  max=$((count * 512))
  [ "$size" -le "$max" ] || { echo "[FAIL] $name 크기가 파티션을 초과함: $size > $max" >&2; exit 1; }
  echo "[pre ] $name md5 ok, $size bytes (파티션 $max)"
done

if [ "${CHECK_ONLY:-0}" = "1" ]; then
  echo "[done] 검증만 수행 — 기기에는 기록하지 않음"
  exit 0
fi

rk_require_readable

echo "기록 대상: boot recovery super <- $img"
confirm "기기에 LineageOS를 기록할까요?"

for p in "${parts[@]}"; do
  read -r name _ <<<"$p"
  "$here/flash.sh" -y "$name" "${raw[$name]}"
done

echo "[done] 3개 파티션 기록 및 검증 완료"
