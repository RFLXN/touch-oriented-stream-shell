#!/usr/bin/env bash
# 토스 프론트(PLF02WH) 단일 파티션에 이미지를 기록하고 되읽기 검증을 수행한다.
#
# 사용법: nix develop -c src/scripts/flash.sh [-y] <파티션 이름> <이미지>
#         nix develop -c src/scripts/flash.sh --enter      # Loader 모드에서 MaskROM으로 전환 후 로더 적재
#         nix develop -c src/scripts/flash.sh --reboot     # 일반 부팅으로 재부팅
#
# 기기 내장 Loader는 읽기 응답을 신뢰할 수 없으므로(0xCC 반환), 모든 기록 작업은
# MaskROM에 로더를 올린 상태(Read LBA 가능)에서만 수행한다. 미충족 시 거절하므로 먼저 `--enter`를 실행해야 한다.
# 로더 바이너리(`local/loader/`)는 `src/scripts/fetch-loader.sh`로 내려받는다.
#
# 기록 전 기기 파티션 테이블을 읽어 시작 LBA가 예상 값(`src/scripts/lib.sh` 테이블)과 다르면 기록하지 않으며,
# 기록 후에는 항상 동일 길이만큼 되읽어 바이트 단위로 대조한다.
# Android sparse 이미지(매직 `3aff26ed`)는 raw로 변환해 기록하고, 크기가 512바이트 배수가 아니면 0으로 패딩한다
# (섹터 단위 되읽기 검증을 위해). 심볼릭 링크를 넘기면 실제 대상 파일을 추적해 기록한다
# (`dist/images/`의 이미지는 Nix 스토어를 가리키는 심볼릭 링크다).
#
# `boot`, `recovery`, `userdata`는 확인 없이 바로 기록하며(주 배포 대상이며 `src/scripts/deploy.sh`에서 사전에 1회 확인함),
# 그 외 파티션(`uboot`, `trust`, `security`, `cache` 등)은 기록 직전 확인 프롬프트를 띄운다(`-y`로 생략 가능).
set -euo pipefail
. "$(dirname "$0")/lib.sh"

loader=$top/local/loader/rk3399_loader_v1.30.130.bin

enter() {
  case "$(rk_mode)" in
    Loader)
      echo "[info] Loader -> MaskROM 전환"
      rkdeveloptool rd 3 >/dev/null || {
        echo "[FAIL] 기기에 명령을 보낼 수 없습니다. USB 권한 규칙(docs/02-progress.md 2.2절)을 확인하세요. 기기에는 아무것도 기록하지 않았습니다." >&2
        exit 1
      }
      for _ in $(seq 1 15); do
        sleep 1
        [ "$(rk_mode)" = "Maskrom" ] && break
      done
      [ "$(rk_mode)" = "Maskrom" ] || {
        echo "[FAIL] MaskROM 장치가 감지되지 않습니다. 초기화 버튼을 누른 채 전원을 다시 연결한 뒤 실행하세요. 기기에는 아무것도 기록하지 않았습니다." >&2
        exit 1
      }
      ;;
    Maskrom) ;;
    *)
      echo "[FAIL] 기기가 Loader 또는 MaskROM 상태가 아닙니다. 초기화 버튼을 누른 채 전원을 연결하세요." >&2
      exit 1
      ;;
  esac
  if ! rk_can_read; then
    [ -f "$loader" ] || {
      echo "[FAIL] 로더 파일이 없습니다: $loader. 먼저 src/scripts/fetch-loader.sh를 실행하세요." >&2
      exit 1
    }
    echo "[info] 로더를 RAM에 적재합니다"
    rk db "$loader" || { echo "[FAIL] 로더 적재 실패(rkdeveloptool db)" >&2; exit 1; }
    sleep 1
  fi
  rk_can_read || { echo "[FAIL] Read LBA가 동작하지 않습니다" >&2; exit 1; }
  echo "[ ok ] MaskROM + 로더 적재 상태"
}

yes=0
args=()
for a in "$@"; do
  case $a in
    -y) yes=1 ;;
    *) args+=("$a") ;;
  esac
done

case "${args[0]:-}" in
  --enter)
    enter
    exit 0
    ;;
  --reboot)
    rk rd || { echo "[FAIL] 재부팅 명령 실패(rkdeveloptool rd)" >&2; exit 1; }
    echo "[ ok ] 재부팅"
    exit 0
    ;;
  -*) usage ;;
esac
[ ${#args[@]} -eq 2 ] || usage

part=${args[0]}
file=${args[1]}

range=$(part_lookup "$part") || { echo "[FAIL] 알 수 없는 파티션: $part" >&2; exit 1; }
read -r start count <<<"$range"
[ -f "$file" ] || { echo "[FAIL] 파일 없음: $file" >&2; exit 1; }
file=$(readlink -f "$file")

tmp=$(mktemp -d)
trap 'rm -rf "${tmp:?}"' EXIT

if [ "$(xxd -l 4 -p "$file")" = "3aff26ed" ]; then
  simg2img "$file" "$tmp/raw"
  file=$tmp/raw
fi

size=$(stat -c %s "$file")
max=$((count * 512))
[ "$size" -le "$max" ] || { echo "[FAIL] 이미지 크기가 파티션을 초과합니다: $size > $max" >&2; exit 1; }
if [ $((size % 512)) -ne 0 ]; then
  cp --no-preserve=mode "$file" "$tmp/padded"
  truncate -s $(((size + 511) / 512 * 512)) "$tmp/padded"
  file=$tmp/padded
fi

rk_require_readable
rk_check_ppt "$part" "$start"

case $part in
  boot | recovery | userdata) ;;
  *) confirm "[주의] $part는 일반 플래싱 대상(boot, recovery, userdata)이 아닙니다. 계속 기록할까요?" ;;
esac

rk_write_verify "$part" "$start" "$file" "$tmp"
