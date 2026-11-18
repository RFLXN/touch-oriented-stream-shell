# 스크립트 공용 라이브러리. 직접 실행하지 않고 각 스크립트 상단에서 1회 소싱한다:
#   set -euo pipefail
#   . "$(dirname "$0")/lib.sh"
# Nix 빌드에서 실행하는 `src/scripts/mkboot.sh`는 이 파일을 소싱하지 않는다 — 샌드박스 내에 `mkboot.sh`와 `rsce.py`만
# 복사되기 때문(`nix/build.nix`의 `bootTools`). 따라서 `boot`/`recovery` 파티션 크기는 `mkboot.sh`에도 따로 둔다.
#
# 경로
#   top, here            저장소 루트 및 `src/scripts` 디렉터리
#   usage [종료 코드]    호출한 스크립트의 헤더 주석(2번째 줄부터 주석이 끝날 때까지)을 출력하고 종료한다(기본값 2).
#   confirm <질문>       `y` 입력을 받아야 진행하며 그 외에는 `"취소"`를 출력하고 종료한다. `yes=1`이면 확인 없이 통과(`-y`).
#
# 기기 접속(`src/scripts/tf-ssh.sh`와 이를 거치지 않는 `src/scripts/tf-net.sh`에서 사용)
#   tf_host              `root@172.16.42.1`(USB 이더넷). `TF_HOST=<주소>` 지정 시 해당 주소 사용
#   tf_ssh_opts          SSH/SCP 옵션. 재플래싱마다 호스트 키가 바뀌므로 호스트 키 검증 및 `known_hosts` 기록을 끈다.
#
# 터치 입력 주입 (src/scripts/tf-tap.sh, src/scripts/tf-drag.sh)
#   tf_touch_preamble    기기 sh에 먼저 전달하는 프리앰블. 터치 입력 장치(ilitek_ts)를 이름으로 찾아 dev에 두고
#                        (장치 번호는 바뀔 수 있음), 입력 이벤트를 기록하는 함수 ev <type> <code> <value>를 정의한다.
#                        이벤트 1건은 타임스탬프 16바이트(0 채움) + type 2 + code 2 + value 4바이트(리틀 엔디안).
#                        이벤트 배치 전체를 단일 write로 써야 한다(이벤트 1건보다 작게 쓰면 커널이 거부함) —
#                        따라서 호출 측은 이벤트를 파일에 모았다가 한 번에 dev에 쓴다.
#                        페이로드 구성: 슬롯 0 (3 47 0), 트래킹 ID (3 57 n, 뗄 때는 -1 = 4294967295), X (3 53), Y (3 54),
#                        BTN_TOUCH (1 330 1/0), SYN_REPORT (0 0 0). 터치 패널 좌표는 두 축 모두 0~16383 범위다.
#
# 플래싱 (rkdeveloptool: src/scripts/flash.sh, deploy.sh, backup.sh, downgrade.sh, flash-lineage.sh)
#   tf_parts                         이 모델(PLF02WH)의 파티션 테이블: 이름, 시작 섹터, 섹터 수 (실측값, 섹터당 512바이트)
#   part_lookup <이름>               "시작_섹터 섹터_수" 출력. 테이블에 없으면 실패
#   rk_mode                          기기 상태 출력: Maskrom, Loader, 또는 빈 줄(미감지)
#   rk_can_read                      Read LBA 지원 여부 (MaskROM에서 로더를 적재한 상태인지 확인)
#   rk_require_readable              Read LBA 가능 상태가 아니면 에러 출력 후 종료. 기기 내장 Loader는 읽은 값이 부정확해
#                                    (0xCC가 나옴) 모든 읽기와 쓰기는 이 상태에서만 수행한다
#   rk_check_ppt <이름> <시작>       기기가 보고하는 파티션 테이블의 시작 LBA 일치 여부 확인. 불일치 시 종료
#   rk <인수...>                     rkdeveloptool을 조용히 실행한다. 실패 시 출력 말미를 보여 주고 실패 반환
#   rk_write_verify <이름> <시작> <파일> <임시 디렉터리>
#                                    이미지를 기록하고 같은 길이만큼 되읽어 대조한다. 파일 크기는 512의 배수여야 한다

top=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
here=$top/src/scripts

usage() {
  awk 'NR > 1 { if (!/^#/) exit; sub(/^# ?/, ""); print }' "$0" >&2
  exit "${1:-2}"
}

confirm() {
  local ans
  [ "${yes:-0}" -eq 1 ] && return 0
  read -r -p "$1 (y/N) " ans || ans=
  [ "$ans" = y ] || [ "$ans" = Y ] || { echo "취소"; exit 1; }
}

tf_host=root@${TF_HOST:-172.16.42.1}
tf_ssh_opts=(-F /dev/null -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null
  -o ConnectTimeout=8 -o LogLevel=ERROR -o ServerAliveInterval=2 -o ServerAliveCountMax=2)

tf_touch_preamble=$(cat <<'EOF'
set -e
dev=/dev/input/$(grep -A4 ilitek_ts /proc/bus/input/devices | sed -n 's/.*Handlers=.*\(event[0-9]*\).*/\1/p')
[ -c "$dev" ] || { echo "터치 입력 장치를 찾지 못했습니다" >&2; exit 1; }
le16() { printf "\\$(printf %03o $(($1 & 255)))\\$(printf %03o $(($1 >> 8 & 255)))"; }
le32() { le16 $(($1 & 65535)); le16 $(($1 >> 16 & 65535)); }
ev() { printf '\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0'; le16 $1; le16 $2; le32 $3; }
EOF
)

tf_parts="
security      0x00002000 0x00002000
uboot         0x00004000 0x00002000
trust         0x00006000 0x00002000
misc          0x00008000 0x00002000
dtbo          0x0000A000 0x00002000
vbmeta        0x0000C000 0x00000800
boot          0x0000C800 0x00014000
recovery      0x00020800 0x0003C000
backup        0x0005C800 0x000C0000
cache         0x0011C800 0x000C0000
metadata      0x001DC800 0x00008000
baseparameter 0x001E4800 0x00000800
super         0x001E5000 0x00400000
userdata      0x005E5000 0x0173AFC0
"

part_lookup() {
  awk -v p="$1" '$1 == p { print $2, $3; found = 1 } END { exit !found }' <<<"$tf_parts"
}

rk_mode() { rkdeveloptool ld 2>/dev/null | grep -oE "Maskrom|Loader" | head -1 || true; }

rk_can_read() { timeout 5 rkdeveloptool rcb 2>/dev/null | grep -q "Read LBA"; }

rk_require_readable() {
  rk_can_read || {
    echo "[FAIL] MaskROM + 로더 적재 상태가 아닙니다(Read LBA 불가). 먼저 src/scripts/flash.sh --enter를 실행하세요." >&2
    exit 1
  }
}

rk_check_ppt() {
  local actual want
  actual=$(rkdeveloptool ppt 2>/dev/null | tr -d '\r' | awk -v p="$1" '$3 == p {print toupper($2)}')
  want=$(printf '%08X' "$2")
  [ "$actual" = "$want" ] || { echo "[FAIL] 기기의 $1 시작 LBA 불일치: ${actual:-없음} != $want" >&2; exit 1; }
}

rk() {
  local out
  out=$(rkdeveloptool "$@" 2>&1) || {
    printf '%s\n' "$out" | tr '\r' '\n' | tail -n 5 >&2
    return 1
  }
}

rk_write_verify() {
  local part=$1 start=$2 file=$3 tmp=$4 size
  size=$(stat -c %s "$file")
  echo "[write] $part <- $file ($size bytes, LBA $start)"
  rk wl "$start" "$file" || { echo "[FAIL] $part 기록 실패 (rkdeveloptool wl)" >&2; exit 1; }
  rk rl "$start" "$((size / 512))" "$tmp/readback" || { echo "[FAIL] $part 되읽기 실패 (rkdeveloptool rl)" >&2; exit 1; }
  if cmp -s "$file" "$tmp/readback"; then
    echo "[ ok ] $part 되읽기 일치"
  else
    echo "[FAIL] $part 되읽기 불일치" >&2
    exit 1
  fi
  rm -f "$tmp/readback"
}
