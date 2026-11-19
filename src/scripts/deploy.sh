#!/usr/bin/env bash
# 저장소 상태를 기기에 그대로 배포한다: 이미지 빌드 -> Loader 진입 -> 기록 및 되읽기 검증 -> 재부팅.
# 기기가 어떤 상태였든 완료 후에는 "git이 추적하는 작업 트리의 파일(커밋하지 않은 수정 포함) + local/device/rootfs/ + local/splash/" 상태로 초기화된다.
#
# 사용법: nix develop -c src/scripts/deploy.sh [-y] [boot] [recovery] [userdata]
#         대상을 생략하면 세 파티션을 모두 기록한다. `-y`는 확인 프롬프트를 건너뛴다.
#
# 주의: `userdata`를 덮어쓰면 기기에서 직접 수정한 항목(임시 설치 패키지, 설정 변경, 생성 파일)이 모두 지워진다.
# 그것이 이 스크립트의 의도이므로, 보존해야 할 변경은 반드시 저장소(`src/os/`)나 `local/device/rootfs/`에 반영해야 한다.
#
# 기기가 부팅되어 있고 SSH 접속이 가능하면 자동으로 Loader 모드로 재부팅한다.
# SSH가 불가능한 상태(부팅 불능 등)에서는 초기화 버튼을 누른 채 전원을 연결해 Loader 모드로 진입한 뒤 실행한다.
# 빌드 완료 후 1회만 확인하며 파티션마다 다시 묻지 않는다. 실제 플래싱은 `src/scripts/flash.sh`가 수행한다.
set -euo pipefail
. "$(dirname "$0")/lib.sh"

yes=0
targets=()
for a in "$@"; do
  case $a in
    -y) yes=1 ;;
    boot | recovery | userdata) targets+=("$a") ;;
    *) usage ;;
  esac
done
[ ${#targets[@]} -gt 0 ] || targets=(boot recovery userdata)

image_of() {
  case $1 in
    boot) echo "$top/dist/boot.img" ;;
    recovery) echo "$top/dist/images/boot-stage1.img" ;;
    userdata) echo "$top/dist/userdata.img" ;;
  esac
}

"$here/build-all.sh"

echo
echo "기록 대상: ${targets[*]}"
confirm "기기에 기록할까요?"

if [ -z "$(rk_mode)" ]; then
  echo "[info] 기기를 Loader 모드로 재부팅합니다"
  "$here/tf-ssh.sh" --reboot loader || {
    echo "[FAIL] SSH로 재부팅할 수 없습니다. 덱 설정에서 SSH를 꺼 두었다면 켠 뒤(Settings > SSH) 다시 실행하세요." >&2
    echo "       그래도 안 되면 초기화 버튼을 누른 채 전원을 연결해 Loader 모드로 진입하세요." >&2
    exit 1
  }
  for _ in $(seq 1 30); do
    sleep 1
    [ -n "$(rk_mode)" ] && break
  done
  [ -n "$(rk_mode)" ] || { echo "[FAIL] Loader 장치가 감지되지 않습니다(USB udev 권한 규칙 확인 필요)" >&2; exit 1; }
fi

"$here/flash.sh" --enter
for t in "${targets[@]}"; do
  "$here/flash.sh" -y "$t" "$(image_of "$t")"
done
"$here/flash.sh" --reboot
echo "[ ok ] 플래싱 완료. 부팅까지 약 20초 걸립니다. 확인: src/scripts/tf-ssh.sh 'uname -r'"
