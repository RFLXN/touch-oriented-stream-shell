#!/usr/bin/env bash
# Alpine 패키지 버전과 해시를 고정한다(`src/os/alpine/lock.json`).
#
# 사용법: nix develop -c src/scripts/alpine-lock.sh update    # 현재 미러 기준으로 다시 고정(패키지 목록 변경/업데이트 시)
#         nix develop -c src/scripts/alpine-lock.sh restore   # 아카이브해 둔 패키지 파일을 Nix 스토어에 재등록
#         nix develop -c src/scripts/alpine-lock.sh check     # 아카이브에 락파일의 모든 패키지가 있는지 검증
#
# 이유: Alpine 미러는 패키지가 업데이트되면 이전 버전 파일을 삭제하므로, 버전만 적어 두면
# 몇 달 뒤 동일 이미지를 재현할 수 없다. 따라서
#   1) 락파일에 의존 패키지 전체의 파일명과 sha256을 기록해 git에 커밋하고
#   2) 실제 `.apk`와 타르볼은 `local/archive/alpine/`에 보관한다(git 추적 제외 — 별도 백업 대상).
# Nix 빌드는 락파일 해시 기준으로만 패키지를 찾으며, 미러에서 내려간 경우 `restore`로 아카이브에서 등록한다.
# 락파일의 저장소 구분(`main`, `community`)은 fetch URL에서 추출한다.
# `restore`와 `check`는 아카이브에 누락된 파일이 있으면 실패로 종료한다(`restore`는 존재하는 파일을 모두 등록한 뒤 종료).
set -euo pipefail
. "$(dirname "$0")/lib.sh"

list=$top/src/os/alpine/packages
lock=$top/src/os/alpine/lock.json
pool=$top/local/archive/alpine

mirror=https://dl-cdn.alpinelinux.org/alpine
branch=v3.24
release=3.24.4
tarball=alpine-minirootfs-$release-aarch64.tar.gz
tarball_sha256=4b14c18f7e90a063b61462ea1462a5732c154bf6adad594974fe6487cd98a8de
extra="busybox-static"

declared() { sed 's/#.*//' "$list" | awk 'NF {print $1}'; }

to_store() { nix store add-file --name "$(basename "$1")" "$1" >/dev/null; }

lock_files() { python3 -I -c '
import json, sys
d = json.load(open(sys.argv[1]))
print(d["minirootfs"]["file"], d["minirootfs"]["sha256"])
for p in d["packages"]:
    print(p["file"], p["sha256"])
' "$lock"; }

do_update() {
  mkdir -p "$pool"
  tmp=$(mktemp -d)
  trap 'rm -rf "${tmp:?}"' EXIT

  if [ ! -f "$pool/$tarball" ]; then
    echo "[dl] $tarball"
    curl -fL --max-time 120 -o "$pool/$tarball" "$mirror/$branch/releases/aarch64/$tarball"
  fi
  echo "$tarball_sha256  $pool/$tarball" | sha256sum -c - >/dev/null

  mkdir -p "$tmp/root"
  fakeroot tar xzf "$pool/$tarball" -C "$tmp/root"
  local apk=(apk --root "$tmp/root" --arch aarch64 --no-cache -X "$mirror/$branch/main" -X "$mirror/$branch/community")
  # shellcheck disable=SC2046
  "${apk[@]}" fetch --recursive --simulate --url $(declared) $extra | sort -u >"$tmp/urls"
  [ -s "$tmp/urls" ] || { echo "[FAIL] 의존성 해석 결과가 비어 있습니다" >&2; exit 1; }

  local url file n=0 new=0
  while read -r url; do
    file=${url##*/}
    n=$((n + 1))
    if [ ! -f "$pool/$file" ]; then
      curl -fsSL --max-time 300 -o "$pool/$file.part" "$url"
      mv "$pool/$file.part" "$pool/$file"
      new=$((new + 1))
    fi
  done <"$tmp/urls"

  python3 -I - "$tmp/urls" "$pool" "$lock" "$mirror" "$branch" "$tarball" "$tarball_sha256" <<'PY'
import hashlib, json, os, sys
urls, pool, lock, mirror, branch, tarball, tarball_sha = sys.argv[1:]
pkgs = []
for url in open(urls).read().split():
    file = url.rsplit("/", 1)[1]
    repo = url.rsplit("/", 3)[1]
    data = open(os.path.join(pool, file), "rb").read()
    pkgs.append({"file": file, "repo": repo, "sha256": hashlib.sha256(data).hexdigest()})
pkgs.sort(key=lambda p: p["file"])
out = {
    "mirror": mirror,
    "branch": branch,
    "minirootfs": {"file": tarball, "sha256": tarball_sha},
    "packages": pkgs,
}
with open(lock, "w") as f:
    json.dump(out, f, indent=1)
    f.write("\n")
PY
  echo "[lock] 패키지 $n개(신규 다운로드 $new개) -> ${lock#"$top"/}"
  do_restore
}

do_restore() {
  local file sha missing=0 n=0
  while read -r file sha; do
    if [ ! -f "$pool/$file" ]; then
      echo "[없음] $file"
      missing=$((missing + 1))
      continue
    fi
    echo "$sha  $pool/$file" | sha256sum -c - >/dev/null || { echo "[FAIL] 해시 불일치: $file" >&2; exit 1; }
    to_store "$pool/$file"
    n=$((n + 1))
  done < <(lock_files)
  echo "[store] $n개를 Nix 스토어에 등록(아카이브 누락 $missing개)"
  [ "$missing" -eq 0 ]
}

do_check() {
  local file sha bad=0
  while read -r file sha; do
    if [ ! -f "$pool/$file" ]; then
      echo "[없음] $file"
      bad=$((bad + 1))
    elif ! echo "$sha  $pool/$file" | sha256sum -c - >/dev/null 2>&1; then
      echo "[불일치] $file"
      bad=$((bad + 1))
    fi
  done < <(lock_files)
  echo "[check] 문제 $bad개"
  [ "$bad" -eq 0 ]
}

case "${1:-}" in
  update) do_update ;;
  restore) do_restore ;;
  check) do_check ;;
  *) usage ;;
esac
