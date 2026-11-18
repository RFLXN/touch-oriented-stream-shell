#!/usr/bin/env bash
# 스트림덱 앱(`src/app/`) 개발 중 이미지 전체를 다시 굽지 않고 실행 중인 기기에 바로 반영한다.
#
# 사용법: nix develop -c src/scripts/app-push.sh [ui|tossd|all]     (기본값 all)
#   ui     `src/app/ui`의 QML 파일을 기기의 `/usr/share/toss/ui`로 전송하고 UI를 재시작한다(수 초 소요)
#   tossd  데몬을 크로스 컴파일해 `/usr/bin/tossd`를 교체하고 서비스를 재시작한다
#
# 테스트 전용이므로 여기에 반영한 변경 사항은 다음 이미지 플래싱 시 초기화된다.
# 변경을 영구 반영하려면 커밋 후 `src/scripts/deploy.sh userdata`로 정식 빌드 이미지를 구워야 한다 — 기기에 남는 것은 그렇게 구운 이미지여야 한다.
# `TF_HOST=<주소>`를 지정하면 LAN 경유로 접속한다(`src/scripts/tf-ssh.sh`와 동일).
# `APP_UI_DIR=<경로>`를 지정하면 `src/app/ui` 대신 해당 디렉터리를 전송한다(임시 사본 테스트용).
set -euo pipefail
. "$(dirname "$0")/lib.sh"

what=${1:-all}

push_ui() {
  tar c --owner=0 --group=0 --mode=u+rwX,go+rX-w -C "${APP_UI_DIR:-$top/src/app/ui}" . |
    "$here/tf-ssh.sh" 'mkdir -p /usr/share/toss/ui && tar x -C /usr/share/toss/ui && find /usr/share/toss/ui -type f -exec touch {} + && killall cage'
  echo "[ ok ] UI 반영 완료(kiosk가 자동 재시작합니다)"
}

push_tossd() {
  (cd "$top/src/app/tossd" && CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld CC_aarch64_unknown_linux_musl=clang AR_aarch64_unknown_linux_musl=llvm-ar \
    cargo build --release --target aarch64-unknown-linux-musl)
  "$here/tf-ssh.sh" --put "$top/dist/cargo/aarch64-unknown-linux-musl/release/tossd" /usr/bin/tossd.new
  "$here/tf-ssh.sh" 'chmod 755 /usr/bin/tossd.new && mv /usr/bin/tossd.new /usr/bin/tossd && rc-service tossd restart >/dev/null && tail -n 2 /var/log/tossd.log'
  echo "[ ok ] tossd 반영 완료"
}

case $what in
  ui) push_ui ;;
  tossd) push_tossd ;;
  all) push_tossd; push_ui ;;
  *) usage ;;
esac
