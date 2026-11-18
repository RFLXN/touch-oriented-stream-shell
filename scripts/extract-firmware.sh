#!/usr/bin/env bash
# 순정 vendor 이미지에서 Wi-Fi/블루투스 칩(BCM43455) 펌웨어를 추출해 메인라인 드라이버가 찾는 파일명으로 배치한다.
#
# 사용법: nix develop -c scripts/extract-firmware.sh
# 입력: `downgrade/image/super.img`(순정 761). 없으면 `lineage/super.img`를 사용한다(vendor 내용 동일).
# 출력: `build/firmware/brcm/`(rootfs 및 initramfs의 `/lib/firmware`에 포함됨)
#
# 펌웨어는 제조사 소유 바이너리이므로 저장소에 포함하지 않는다(`build/`는 git 추적 대상 제외).
# 4개 파일을 모두 추출한 뒤에만 `build/firmware`를 교체해 도중 실패 시 기존 파일이 손상되지 않도록 한다.
set -euo pipefail
. "$(dirname "$0")/lib.sh"

out=$top/build/firmware

super=$top/downgrade/image/super.img
[ -f "$super" ] || super=$top/lineage/super.img
[ -f "$super" ] || { echo "[FAIL] 순정 super.img가 없습니다(downgrade/image/super.img)" >&2; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "${tmp:?}" "${out:?}.part"' EXIT

lpunpack -p vendor "$super" "$tmp" >/dev/null
get() {
  debugfs -R "cat /etc/firmware/$1" "$tmp/vendor.img" 2>/dev/null >"$2"
  [ -s "$2" ] || { echo "[FAIL] vendor에 없음: $1" >&2; exit 1; }
}

rm -rf "${out:?}.part"
mkdir -p "$out.part/brcm"
get fw_bcm43455c0_ag_k019_p681x.bin "$out.part/brcm/brcmfmac43455-sdio.bin"
get nvram_ap6255_k019_p681x.txt "$out.part/brcm/brcmfmac43455-sdio.txt"
get clm_bcm43455c0_ag_k019_p681x.blob "$out.part/brcm/brcmfmac43455-sdio.clm_blob"
get BCM4345C0.hcd "$out.part/brcm/BCM4345C0.hcd"

rm -rf "${out:?}"
mv "$out.part" "$out"

(cd "$out" && find . -type f -exec ls -l {} \; | awk '{print "[firmware]", $5, $9}')
