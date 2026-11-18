#!/usr/bin/env bash
# 공통 루트 이미지에 기기 고유 파일을 오버레이해 `userdata` 파티션용 이미지를 만든다 -> `build/userdata.img`
#
# 사용법: nix develop -c scripts/make-userdata.sh     (`scripts/build-all.sh`에서 호출)
# 입력: `build/images/alpine-rootfs.img`(`nix build` 결과물) + `device/rootfs/`(있는 경우)
#
# `device/rootfs/`는 기기의 `/`에 그대로 복사되는 디렉터리다. 주로 두는 파일:
#   `etc/tossfront/eth0.mac`                    유선 이더넷 MAC 주소(기기마다 고유)
#   `etc/dropbear/*_host_key`                   SSH 호스트 키(재플래싱마다 바뀌지 않도록 유지)
# 이미지 빌드와 분리한 이유: 이런 고유 값과 비밀 키는 git에 넣을 수 없고 Nix 스토어도 world-readable이라 부적합하다.
# 따라서 공통 이미지는 Nix로 빌드하고 기기 고유 파일은 플래싱 직전에 여기서 합성한다.
# `device/`는 git 추적 제외 대상이므로 `backup/`처럼 별도로 백업해 둔다.
#
# 파일 권한은 호스트 그대로(예: 600 및 setuid 비트 포함) 보존하고 소유자는 root로 기록한다.
# 심볼릭 링크나 공백이 포함된 경로는 지원하지 않는다(저장소 경로 자체에 공백이 있어도 마찬가지다).
set -euo pipefail
. "$(dirname "$0")/lib.sh"

src=$top/build/images/alpine-rootfs.img
img=$top/build/userdata.img
overlay=$top/device/rootfs

[ -e "$src" ] || { echo "[FAIL] $src 파일이 없습니다. scripts/build-all.sh를 먼저 실행하세요." >&2; exit 1; }
rm -f "$img"
cp --no-preserve=mode "$src" "$img"

n=0
if [ -d "$overlay" ]; then
  case "$overlay" in *[[:space:]]*) echo "[FAIL] 경로에 공백이 있으면 기기 고유 파일을 오버레이할 수 없습니다: $overlay" >&2; exit 1 ;; esac
  cmds=$(mktemp)
  trap 'rm -f "$cmds"' EXIT
  exists() { debugfs -R "stat $1" "$img" 2>/dev/null | grep -q '^Inode:'; }
  while IFS= read -r -d '' path; do
    rel=${path#"$overlay"}
    case "$rel" in *[[:space:]]*) echo "[FAIL] 공백이 포함된 파일명은 지원하지 않습니다: $rel" >&2; exit 1 ;; esac
    perm=$(printf '%04o' "0$(stat -c %a "$path")")
    if [ -L "$path" ]; then
      echo "[FAIL] 심볼릭 링크는 지원하지 않습니다: $rel" >&2
      exit 1
    elif [ -d "$path" ]; then
      [ -z "$rel" ] && continue
      if ! exists "$rel"; then
        printf 'mkdir %s\nsif %s mode 04%s\n' "$rel" "$rel" "$perm" >>"$cmds"
      fi
    else
      exists "$rel" && printf 'rm %s\n' "$rel" >>"$cmds"
      printf 'write %s %s\nsif %s mode 010%s\n' "$path" "$rel" "$rel" "$perm" >>"$cmds"
      echo "[device] $rel (${perm#0})"
      n=$((n + 1))
    fi
  done < <(find "$overlay" -print0 | LC_ALL=C sort -z)
  debugfs -w -f "$cmds" "$img" >/dev/null 2>"$cmds.err" || { cat "$cmds.err" >&2; rm -f "$cmds.err"; exit 1; }
  if grep -vE '^debugfs [0-9.]+ ' "$cmds.err" | grep -q .; then
    cat "$cmds.err" >&2
    rm -f "$cmds.err"
    exit 1
  fi
  rm -f "$cmds.err"
fi

e2fsck -fn "$img" >/dev/null 2>&1 || { echo "[FAIL] 생성된 이미지의 파일시스템 무결성 검사 실패" >&2; exit 1; }
echo "[ ok ] build/userdata.img ($(($(stat -c %s "$img") / 1048576))MB, 기기 고유 파일 $n개)"
