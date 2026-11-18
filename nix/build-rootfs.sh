# Alpine 루트 파일시스템 이미지를 빌드한다. `nix/build.nix`의 `rootfs`가 fakeroot 환경에서 실행한다
# (fakeroot: 파일 소유자를 root로 기록하기 위해 사용).
#
# 입력(환경 변수):
#   src            저장소 파일(`os/rootfs`, `scripts/gen-beep.py`)
#   minirootfs     Alpine 기본 루트 타르볼
#   apkRepo        고정된 패키지만 포함한 로컬 APK 저장소
#   packages       설치할 패키지 목록(`os/alpine/packages`)
#   repositories   기기의 `/etc/apk/repositories` 내용
#   firmware tools kernel   선행 빌드 결과물
#   hostname sshLan authorizedKeys ntpEnable timezone zoneinfo kioskEnable kioskCommand tossEnable   `buildImage` 옵션(`flake.nix`)
#   tossd tossFiles   스트림덱 앱 바이너리 및 파일(`tossEnable`이 true일 때만 전달)
#   out            출력 디렉터리
#
# 저장소 파일(`os/rootfs`, `tossEnable` 시 `app/rootfs` 및 `app/ui`)은 모두 동일한 규칙으로 덮어쓴다:
# 디렉터리는 755, 실행 비트가 있는 파일은 755, 나머지는 644. 실행 파일에는 git 인덱스에 실행 비트를 부여해 두어야 한다 —
# `etc/init.d`, `etc/local.d/*.start`, `usr/bin`, `usr/sbin`, `usr/libexec` 아래 파일에 실행 비트가 없으면 빌드가 실패한다.
# Alpine 패키지는 설치 스크립트를 실행하지 않으므로(`--no-scripts`) 패키지가 만들던 사용자/그룹은 여기서 직접 생성한다
# (100부터 사용하지 않는 최소 UID/GID 할당).
set -euo pipefail

r=$PWD/rootfs
mkdir -p "$r"

echo "[rootfs] 기본 rootfs 압축 해제 및 패키지 설치"
tar xzf "$minirootfs" -C "$r"

overlay() {
  local from=$1 to=$2 plain
  rm -rf staging
  cp -r "$from" staging
  chmod -R u+w staging
  plain=$(cd staging && find . -type f ! -perm /111 \
    \( -path './etc/init.d/*' -o -path './etc/local.d/*.start' -o -path './usr/bin/*' -o -path './usr/sbin/*' -o -path './usr/libexec/*' \))
  [ -z "$plain" ] || { echo "[FAIL] ${from#"$src/"}에 실행 비트가 빠진 실행 파일이 있습니다(git update-index --chmod=+x 필요): $plain" >&2; exit 1; }
  find staging -type d -exec chmod 755 {} +
  find staging -type f -perm /111 -exec chmod 755 {} +
  find staging -type f ! -perm /111 -exec chmod 644 {} +
  mkdir -p "$to"
  cp -a staging/. "$to/"
  rm -rf staging
}

free() { awk -F: '{used[$3] = 1} END {for (i = 100; i < 1000; i++) if (!used[i]) {print i; exit}}' "$1"; }
sysuser() {
  local name=$1 home=$2 uid gid
  uid=$(free "$r/etc/passwd")
  gid=$(free "$r/etc/group")
  echo "$name:x:$gid:$name" >>"$r/etc/group"
  echo "$name:x:$uid:$gid:$name:$home:/sbin/nologin" >>"$r/etc/passwd"
  echo "$name:!::0:::::" >>"$r/etc/shadow"
}
sysgroup() {
  echo "$1:x:$(free "$r/etc/group"):" >>"$r/etc/group"
}
addmember() {
  grep -q "^$1:" "$r/etc/group" || { echo "[FAIL] 존재하지 않는 그룹: $1" >&2; exit 1; }
  sed -i "/^$1:/ { s/\$/,$2/; s/:,/:/ }" "$r/etc/group"
}
sysuser messagebus /dev/null
sysuser dhcpcd /var/lib/dhcpcd
sysgroup seat
if [ "$kioskEnable" = true ]; then
  sysuser kiosk /home/kiosk
  addmember seat kiosk
  addmember video kiosk
fi
if [ "$tossEnable" = true ]; then
  sysuser tossd /dev/null
fi

# shellcheck disable=SC2086
apk --root "$r" --arch aarch64 --no-scripts --no-network --no-cache --allow-untrusted \
  --repositories-file /dev/null -X "$apkRepo" add $packages >apk.log 2>&1 || { tail -n 30 apk.log; exit 1; }
tail -n 1 apk.log

echo "[rootfs] 설정 적용"
overlay "$src/os/rootfs" "$r"

install -D -m 755 "$tools/bin/reboot-mode" "$r/usr/sbin/reboot-mode"
install -D -m 755 "$tools/bin/i2c-read" "$r/usr/sbin/i2c-read"
mkdir -p "$r/usr/share/sounds/tossfront"
python3 -I "$src/scripts/gen-beep.py" "$r/usr/share/sounds/tossfront/reference.wav"

echo "$hostname" >"$r/etc/hostname"
printf '%s\n' "$repositories" >"$r/etc/apk/repositories"
sed -i 's/^root:[^:]*:/root::/' "$r/etc/shadow"

sed -i '/^tty[0-9]*::/d' "$r/etc/inittab"
echo 'ttyGS0::respawn:/sbin/getty -L 115200 ttyGS0 vt100' >>"$r/etc/inittab"
if [ "$kioskEnable" = true ]; then
  echo '::respawn:/usr/sbin/tf-kiosk' >>"$r/etc/inittab"
  mkdir -p "$r/etc/tossfront"
  echo "$kioskCommand" >"$r/etc/tossfront/kiosk.command"
  kuid=$(awk -F: '$1 == "kiosk" {print $3}' "$r/etc/passwd")
  kgid=$(awk -F: '$1 == "kiosk" {print $4}' "$r/etc/passwd")
  install -d -m 755 -o "$kuid" -g "$kgid" "$r/home/kiosk"
else
  echo '::respawn:/usr/sbin/tf-console-getty' >>"$r/etc/inittab"
fi
echo ttyGS0 >>"$r/etc/securetty"
echo 'pstore /sys/fs/pstore pstore defaults 0 0' >>"$r/etc/fstab"

mkdir -p "$r/lib/firmware"
cp -r "$firmware/." "$r/lib/firmware/"
chmod -R u+w "$r/lib/firmware"

rel=$(cat "$kernel/kernel.release")
install -D -m 644 "$kernel/modules.builtin" "$r/lib/modules/$rel/modules.builtin"
: >"$r/lib/modules/$rel/modules.dep"

printf '\n# 유선(eth0)과 무선(wlan0)에서만 DHCP 주소를 받는다. usb0은 기기가 주소를 할당하는 쪽이다.\nallowinterfaces eth0 wlan0\n' >>"$r/etc/dhcpcd.conf"

listen='-p 172.16.42.1:22 '
[ "$sshLan" = true ] && listen=''
if [ -n "$authorizedKeys" ]; then
  install -d -m 700 "$r/root/.ssh"
  printf '%s' "$authorizedKeys" >"$r/root/.ssh/authorized_keys"
  chmod 600 "$r/root/.ssh/authorized_keys"
  echo "DROPBEAR_OPTS=\"${listen}-s\"" >"$r/etc/conf.d/dropbear"
else
  echo "DROPBEAR_OPTS=\"${listen}-B\"" >"$r/etc/conf.d/dropbear"
fi
echo "[rootfs] SSH: $(cat "$r/etc/conf.d/dropbear")"
cat >>"$r/etc/conf.d/dropbear" <<'CONF'
start_pre() {
	if grep -Eq '"ssh"[[:space:]]*:[[:space:]]*false' /data/toss/settings.json 2>/dev/null; then
		eerror "SSH is closed in the deck's settings (Settings > SSH)"
		return 1
	fi
}
CONF

enable() {
  local lvl=$1 s
  shift
  mkdir -p "$r/etc/runlevels/$lvl"
  for s in "$@"; do
    [ -e "$r/etc/init.d/$s" ] || { echo "[FAIL] 존재하지 않는 서비스: $s" >&2; exit 1; }
    ln -sf "/etc/init.d/$s" "$r/etc/runlevels/$lvl/$s"
  done
}
enable sysinit devfs dmesg
enable boot tf-data bootmisc hostname sysctl syslog networking alsa
enable default dropbear local dhcpcd

[ -f "$zoneinfo" ] || { echo "[FAIL] 알 수 없는 시간대: $timezone" >&2; exit 1; }
install -m 644 "$zoneinfo" "$r/etc/localtime"
if [ "$ntpEnable" = true ]; then
  enable default ntpd
fi
echo "[rootfs] 시간대 $timezone, NTP 자동 동기화 $ntpEnable"

if [ "$kioskEnable" = true ]; then
  enable sysinit udev udev-trigger udev-settle
  enable default udev-postmount seatd
  echo "[rootfs] 키오스크(kiosk) true: $kioskCommand"
else
  echo "[rootfs] 키오스크(kiosk) false"
fi

if [ "$tossEnable" = true ]; then
  install -D -m 755 "$tossd/bin/tossd" "$r/usr/bin/tossd"
  overlay "$tossFiles/app/rootfs" "$r"
  overlay "$tossFiles/app/ui" "$r/usr/share/toss/ui"
  enable default toss-helper tossd
fi
echo "[rootfs] 스트림덱 앱(TOSS) $tossEnable"
enable shutdown killprocs mount-ro savecache

find "$r" -type f -perm /6000 | while read -r f; do
  m=$(stat -c %a "$f")
  chmod 755 "$f"
  chmod "$m" "$f"
  echo "[rootfs] setuid 유지: ${f#"$r"} ($m)"
done

apk --root "$r" --no-network --repositories-file /dev/null info -v 2>/dev/null | LC_ALL=C sort >"$out/packages.txt"
rm -f "$r/var/log/apk.log"

used=$(du -sm "$r" | cut -f1)
size=$(((used * 5 / 4 + 64 + 15) / 16 * 16))
echo "[rootfs] ext4 이미지 생성(내용 ${used}MB -> 이미지 ${size}MB)"
mke2fs -q -t ext4 -b 4096 -L alpine -O '^orphan_file,^metadata_csum_seed' \
  -U 7a8e0f52-2b1c-4c0e-9a61-746f73736672 -E hash_seed=5f1d3c7e-8a44-4b0b-b6d2-66726f6e7431 \
  -d "$r" "$out/alpine-rootfs.img" "${size}M"
