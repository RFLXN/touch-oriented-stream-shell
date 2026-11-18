#!/usr/bin/env bash
# 토스 프론트(PLF02WH) 순정 빌드 761(3.1.42) 다운그레이드 — 7개 파티션 기록
# (`security`, `uboot`, `dtbo`, `vbmeta`, `boot`, `recovery`, `super`).
#
# 사용법: nix develop -c scripts/downgrade.sh [-y] [이미지 디렉터리]     (기본값: downgrade/image)
#         `-y` 지정 시 확인 프롬프트를 건너뛴다.
#
# 사전 조건: `scripts/flash.sh --enter`로 MaskROM + 로더 적재 상태여야 한다(미충족 시 중단).
#            `backup/`에 원본 백업을 보관해 두어야 한다(`scripts/backup.sh`).
# 절차: 이미지 7종의 크기(파티션 크기와 정확히 일치해야 함) 및 MD5 검증 -> 사용자 확인 -> 파티션별로 `scripts/flash.sh`로 기록.
# `flash.sh`는 기기 파티션 테이블을 검증한 뒤 기록하고 매번 되읽기 검증을 수행하므로 하나라도 불일치하면 즉시 중단된다.
# MD5는 커뮤니티 공개 자료 기반이며, 2026-11-15에 실기기에 기록해 정상 동작을 확인한 파일들의 해시다.
set -euo pipefail
. "$(dirname "$0")/lib.sh"

yes=0
img=$top/downgrade/image
for a in "$@"; do
  case $a in
    -y) yes=1 ;;
    -*) usage ;;
    *) img=$a ;;
  esac
done

parts=(security uboot dtbo vbmeta boot recovery super)

for name in "${parts[@]}"; do
  file=$img/$name.img
  [ -f "$file" ] || { echo "[FAIL] 파일 없음: $file" >&2; exit 1; }
  range=$(part_lookup "$name")
  read -r _ count <<<"$range"
  want=$((count * 512))
  got=$(stat -c %s "$file")
  [ "$got" -eq "$want" ] || { echo "[FAIL] $name 크기 불일치: $got != $want" >&2; exit 1; }
done
echo "[pre ] 7개 이미지 크기 확인 완료"

md5sum -c - >/dev/null <<EOF || { echo "[FAIL] MD5 불일치 — 다른 버전의 이미지입니다" >&2; exit 1; }
b5cfa9d6c8febd618f91ac2843d50a1c  $img/security.img
353cd6275777d430dbeaaefaeeb24d48  $img/uboot.img
d66e118148f2cf485b26e36af53dd3fa  $img/dtbo.img
2da4cd9de1db4094ec1dc7bee32aaf02  $img/vbmeta.img
f9a5835fb3d5170d4cc7514cefe1b17c  $img/boot.img
254782375c54810590db37af604c4b0f  $img/recovery.img
b9946795aeba47032cf28b818a9c11c2  $img/super.img
EOF
echo "[pre ] MD5 일치"

rk_require_readable

echo "기록 대상: ${parts[*]} <- $img"
confirm "기기에 순정 761 이미지를 기록할까요?"

for name in "${parts[@]}"; do
  "$here/flash.sh" -y "$name" "$img/$name.img"
done

echo "[done] 7개 파티션 기록 및 검증 완료"
