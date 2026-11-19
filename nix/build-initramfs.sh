# initramfs 이미지를 빌드한다. `nix/build.nix`의 `initramfs`가 fakeroot 환경에서 실행한다.
# 입력(환경 변수): `src`, `busyboxApk`, `regdbApk`, `firmware`, `play`(비어 있으면 생략), `out`
# 포함 내용: 정적 busybox, `src/os/initramfs/init`, 무선 펌웨어, 무선 규제 DB(`regulatory.db` 및 서명 파일 — 누락 시 빌드 실패),
# 부팅 스플래시 애니메이션 재생기 `/bin/splash-play`(`buildImage`의 `splash.enable`일 때). 스플래시 그림 자체는 `src/scripts/make-boot.sh`가 빌드 뒤에 덧붙인다.
set -euo pipefail

r=$PWD/initramfs
mkdir -p "$r/bin" "$r/dev" "$r/lib/firmware" pkg

tar xzf "$busyboxApk" -C pkg --wildcards '*busybox.static' 2>/dev/null
install -m 755 "$(find pkg -name busybox.static)" "$r/bin/busybox"
install -m 755 "$src/src/os/initramfs/init" "$r/init"
[ -z "${play:-}" ] || install -m 755 "$play" "$r/bin/splash-play"

cp -r "$firmware/." "$r/lib/firmware/"
tar xzf "$regdbApk" -C pkg --wildcards '*regulatory.db*' 2>/dev/null
find pkg -name 'regulatory.db*' -exec cp {} "$r/lib/firmware/" \;
for f in regulatory.db regulatory.db.p7s; do
  [ -f "$r/lib/firmware/$f" ] || { echo "[FAIL] wireless-regdb 패키지에 $f 파일이 없습니다" >&2; exit 1; }
done
chmod -R u+w "$r"

mknod -m 600 "$r/dev/console" c 5 1

find "$r" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
(cd "$r" && find . | LC_ALL=C sort | cpio -o -H newc --owner=0:0 --reproducible 2>/dev/null | gzip -9n) >"$out/initramfs.cpio.gz"
