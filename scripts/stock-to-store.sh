#!/usr/bin/env bash
# 순정 펌웨어에서 추출한 파일을 Nix 스토어에 등록한다. 이미지 빌드(`nix build`)가 이 파일들을 해시로 참조한다.
#
# 사용법: nix develop -c scripts/stock-to-store.sh
# 입력: `downgrade/image/` 아래 순정 761 이미지(`boot.img`, `super.img`)
# 등록 대상: 무선 펌웨어 4개(`super.img`의 `vendor`에서 추출), 순정 부트 이미지(복구용 커널 추출 원본)
#
# 이유: 제조사 소유 바이너리는 git에 커밋하지 않고 `os/stock.json`에 sha256만 기록한 뒤,
# 각 환경에서 순정 이미지로부터 추출해 Nix 스토어에 넣는다. 해시가 다르면 다른 펌웨어로 간주해 거절한다.
# Nix 가비지 컬렉션(`gc`)으로 스토어에서 지워졌을 때도 이 스크립트를 다시 실행하면 된다.
# `build/firmware`에 파일이 하나라도 없으면 `scripts/extract-firmware.sh`를 먼저 실행한다.
set -euo pipefail
. "$(dirname "$0")/lib.sh"

fw=$top/build/firmware
boot=$top/downgrade/image/boot.img

[ -f "$boot" ] || { echo "[FAIL] 순정 부트 이미지가 없습니다(downgrade/image/boot.img)" >&2; exit 1; }

list=$(python3 -I - "$top/os/stock.json" "$fw" "$boot" <<'PY'
import json, os, sys
stock, fw, boot = sys.argv[1:]
d = json.load(open(stock))
for path, sha in d["firmware"].items():
    print(os.path.basename(path), sha, os.path.join(fw, path))
print(d["boot"]["name"], d["boot"]["sha256"], boot)
PY
)

while read -r _ _ path; do
  if [ ! -s "$path" ]; then
    "$here/extract-firmware.sh"
    break
  fi
done <<<"$list"

while read -r name sha path; do
  echo "$sha  $path" | sha256sum -c - >/dev/null 2>&1 || { echo "[FAIL] 해시 불일치: $path" >&2; exit 1; }
  nix store add-file --name "$name" "$path" >/dev/null
  echo "[store] $name"
done <<<"$list"
