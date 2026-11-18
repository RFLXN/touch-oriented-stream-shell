#!/usr/bin/env bash
# 기기에 플래싱할 이미지 3종을 빌드한다. 실제 빌드는 Nix(`nix/build.nix`)가 수행한다.
#
# 사용법: nix develop -c scripts/build-all.sh
# 생성 결과(`build/` 아래):
#   images/boot-mainline.img   -> boot      메인라인 커널 + initramfs
#   images/boot-stage1.img     -> recovery  순정 4.19 커널 + 동일 initramfs(복구용)
#   images/alpine-rootfs.img                Alpine 루트 파일시스템(기기 고유 정보가 없는 공통 이미지)
#   images/manifest.txt                     커널 버전, 커맨드라인, 이미지 해시
#   userdata.img               -> userdata  위 루트 이미지에 `device/rootfs/`를 오버레이한 최종 이미지
#
# `build/images`는 Nix 스토어를 가리키는 심볼릭 링크다. git이 추적하는 작업 트리의 파일(커밋하지 않은 수정 포함)이 같으면 같은 결과가 나온다.
# 변경된 파생물만 재빌드되므로 수정 후 언제든 다시 실행할 수 있다.
#
# 해시로 지정된 입력(순정 추출 펌웨어, 고정 Alpine 패키지)이 Nix 스토어에 없어 빌드가 실패하면,
# 아래 두 스크립트를 자동으로 실행한 뒤 1회 재시도한다(초기 빌드 시나 `nix-collect-garbage` 후).
#   scripts/stock-to-store.sh        순정 펌웨어 추출물을 Nix 스토어에 등록
#   scripts/alpine-lock.sh restore   아카이브(`archive/alpine`)의 패키지를 Nix 스토어에 등록
# 아카이브가 비어 있거나 누락된 패키지가 있어도 중단하지 않고, 재시도 시 Nix가 미러에서 내려받도록 넘긴다
# (미러에서도 사라졌으면 그때 최종 실패한다). 다른 원인으로 빌드가 실패한 경우에도 이 복구 시도를 한 번 거친 뒤 실패한다.
set -euo pipefail
. "$(dirname "$0")/lib.sh"

mkdir -p "$top/build"
build() { nix build "$top#images" --out-link "$top/build/images" -L; }
if ! build; then
  echo
  echo "[info] 빌드 실패. 외부 입력을 Nix 스토어에 등록한 뒤 재시도합니다."
  "$here/stock-to-store.sh"
  "$here/alpine-lock.sh" restore || echo "[info] archive/alpine에 없는 패키지가 있습니다. 미러에서 내려받습니다."
  build
fi
"$here/make-userdata.sh"

echo
cat "$top/build/images/manifest.txt"
echo
echo "기기 플래싱: scripts/deploy.sh   (기기의 userdata 파티션을 통째로 덮어씁니다)"
