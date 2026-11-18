# 커널 트리에 드라이버/DTS를 패치하고 `.config`를 생성하는 공통 빌드 절차.
# 정식 빌드(`nix/build.nix`)와 증분 개발 빌드(`scripts/build-kernel.sh`)가 동일한 스크립트를 공유해 빌드 결과 차이를 없앤다.
#
# 호출 측에서 사전에 설정해야 할 변수:
#   KSRC  커널 소스 디렉터리
#   KOUT  빌드 출력 디렉터리(`make`의 `O=`)
#   KDIR  이 스크립트가 위치한 디렉터리(`os/kernel`)
#   JOBS  병렬 빌드 작업 수

# 빌드 시각, 사용자명, 호스트명이 커널에 박히므로 고정값으로 설정해 재현 빌드를 보장한다.
export KBUILD_BUILD_USER=tossfront
export KBUILD_BUILD_HOST=build
KBUILD_BUILD_TIMESTAMP=$(date -u -d "@${SOURCE_DATE_EPOCH:-315532800}")
export KBUILD_BUILD_TIMESTAMP

mk() {
  make -C "$KSRC" O="$KOUT" ARCH=arm64 LLVM=1 HOSTCC=cc HOSTCXX=c++ HOSTLD=ld HOSTAR=ar \
    -j"$JOBS" "$@"
}

tf_add_drivers() {
  local pdir=$KSRC/drivers/gpu/drm/panel
  cp "$KDIR"/drivers/panel-sunmi-toss-front.c "$KDIR"/drivers/panel-sunmi-toss-front-init.h "$pdir/"
  if ! grep -q DRM_PANEL_SUNMI_TOSS_FRONT "$pdir/Makefile"; then
    echo 'obj-$(CONFIG_DRM_PANEL_SUNMI_TOSS_FRONT) += panel-sunmi-toss-front.o' >>"$pdir/Makefile"
    python3 -I - "$pdir/Kconfig" <<'PY'
import sys
p = sys.argv[1]
s = open(p).read()
entry = '''config DRM_PANEL_SUNMI_TOSS_FRONT
	tristate "SUNMI Toss Front M13 800x1280 DSI panel"
	depends on OF
	depends on DRM_MIPI_DSI
	depends on BACKLIGHT_CLASS_DEVICE
	help
	  Panel of the Toss Front 1.5 (SUNMI p681x), variant M13.

'''
i = s.rindex("endmenu")
open(p, "w").write(s[:i] + entry + s[i:])
PY
  fi
}

tf_add_dts() {
  local f
  for f in "$KDIR"/dts/*.dts "$KDIR"/dts/*.dtsi; do
    [ -e "$f" ] || continue
    cp "$f" "$KSRC/arch/arm64/boot/dts/rockchip/"
  done
}

tf_config() {
  local frag=$KDIR/toss-front.config
  mk defconfig >/dev/null
  grep -oE '^CONFIG_ARCH_[A-Z0-9_]+=y' "$KOUT/.config" | cut -d= -f1 | grep -v '^CONFIG_ARCH_ROCKCHIP$' |
    while read -r sym; do "$KSRC/scripts/config" --file "$KOUT/.config" --disable "${sym#CONFIG_}"; done
  mk olddefconfig >/dev/null
  mk mod2noconfig >/dev/null
  "$KSRC/scripts/kconfig/merge_config.sh" -m -O "$KOUT" "$KOUT/.config" "$frag" >/dev/null
  mk olddefconfig >/dev/null
  local bad=0 line sym
  while read -r line; do
    case "$line" in
      CONFIG_*=*)
        grep -qxF "$line" "$KOUT/.config" || { echo "[warn] 반영 안 됨: $line"; bad=1; }
        ;;
      "# CONFIG_"*" is not set")
        sym=${line#\# }
        sym=${sym% is not set}
        if grep -qE "^$sym=" "$KOUT/.config"; then echo "[warn] 꺼지지 않음: $sym"; bad=1; fi
        ;;
    esac
  done <"$frag"
  echo "[config] 완료 (경고 ${bad})"
}
