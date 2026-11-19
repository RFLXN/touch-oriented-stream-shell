#!/usr/bin/env bash
# `boot` 파티션용 부트 이미지를 만든다 -> `dist/boot.img`
#
# 사용법: nix develop -c src/scripts/make-boot.sh     (`src/scripts/build-all.sh`에서 호출)
# 입력: `dist/images/boot-inputs.json`(`nix build` 결과물) + `local/splash/`의 스플래시 그림(있는 경우)
#
# 부팅 스플래시(`flake.nix`의 `splash.enable`)가 꺼져 있으면 Nix가 만든 `boot-mainline.img`를 그대로 복사한다.
# 켜져 있으면 `src/scripts/splash.py`로 첫 화면(`splash.ppm`)과 애니메이션 프레임(`splash.frames`)을 만들고, 이 둘을 담은
# 작은 cpio 묶음을 Nix의 initramfs 뒤에 이어 붙여(커널은 이어 붙은 묶음을 차례로 모두 푼다) 같은 커널·DTB·커맨드라인으로 다시 묶는다.
# 그림은 `local/splash/splash.png`, `splash.gif`, `splash.webp` 중 처음 찾은 파일이다(애니메이션 PNG·GIF·WebP 가능).
# 그림이 없으면 배경색만 칠한다.
#
# 그림을 Nix 빌드와 분리한 이유: 저작권을 가지지 않은 그림은 공개 저장소(git)에 넣을 수 없고, flake는 git이 추적하지 않는 파일을 읽지 못한다.
# `local/`은 git 추적 제외 대상이므로 `local/device/`처럼 따로 백업해 둔다.
# 같은 입력이면 같은 이미지가 나온다(파일 시각·소유자 고정, gzip에 시각을 넣지 않음).
# 미리보기: `dist/splash/preview.png`(첫 화면), 애니메이션이면 `dist/splash/preview.gif`(절반 크기).
set -euo pipefail
. "$(dirname "$0")/lib.sh"

inputs=$top/dist/images/boot-inputs.json
out=$top/dist/boot.img
work=$top/dist/splash

[ -e "$inputs" ] || { echo "[FAIL] $inputs 파일이 없습니다. src/scripts/build-all.sh를 먼저 실행하세요." >&2; exit 1; }
field() {
  python3 -I -c 'import json, sys
v = json.load(open(sys.argv[1]))[sys.argv[2]]
print(v if isinstance(v, str) else json.dumps(v))' "$inputs" "$1"
}

rm -f "$out"
splash=$(field splash)
if [ "$splash" = null ]; then
  cp --no-preserve=mode "$top/dist/images/boot-mainline.img" "$out"
  echo "[ ok ] dist/boot.img (부팅 스플래시 꺼짐)"
  exit 0
fi

image=-
for f in "$top/local/splash/splash.png" "$top/local/splash/splash.gif" "$top/local/splash/splash.webp"; do
  if [ -f "$f" ]; then
    image=$f
    break
  fi
done

rm -rf "$work"
mkdir -p "$work/root"
printf '%s\n' "$splash" >"$work/config.json"
python3 -I "$here/splash.py" "$work/config.json" "$image" "$work"
install -m 644 "$work/splash.ppm" "$work/root/splash.ppm"
[ ! -f "$work/splash.frames" ] || install -m 644 "$work/splash.frames" "$work/root/splash.frames"
find "$work/root" -exec touch -h -d @0 {} +
(cd "$work/root" && find . -mindepth 1 | LC_ALL=C sort | cpio -o -H newc --owner=0:0 --reproducible 2>/dev/null | gzip -9n) >"$work/extra.cpio.gz"
cat "$(field initramfs)" "$work/extra.cpio.gz" >"$work/initramfs.cpio.gz"

"$here/mkboot.sh" -p boot -k "$(field kernel)" -r "$work/initramfs.cpio.gz" -d "$(field dtb)" -c "$(field cmdline)" -o "$out" >/dev/null

if [ "$image" = - ]; then
  what="배경색만(local/splash/에 그림 없음)"
elif [ -f "$work/splash.frames" ]; then
  what="${image#"$top/"}, 애니메이션 $(($(stat -c %s "$work/splash.frames") / 1024))KB"
else
  what="${image#"$top/"}"
fi
echo "[ ok ] dist/boot.img (부팅 스플래시: $what, 미리보기 dist/splash/preview.png)"
