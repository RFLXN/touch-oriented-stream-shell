#!/usr/bin/env bash
# USB 이더넷(`172.16.42.1`)을 통해 기기에 SSH/SCP로 접속한다.
#   src/scripts/tf-ssh.sh '<명령>'             # 원격 명령 실행(인자 없으면 대화형 셸)
#   src/scripts/tf-ssh.sh --put <로컬> <원격>   # 파일 업로드
#   src/scripts/tf-ssh.sh --get <원격> <로컬>   # 파일 다운로드
#   src/scripts/tf-ssh.sh --reboot [<모드>]     # 재부팅. 모드: `normal`(기본), `loader`(플래싱용 Loader), `recovery`(복구용 순정 커널)
# 재플래싱할 때마다 호스트 키가 바뀌므로 `known_hosts`를 확인/저장하지 않는다
# (단일 USB 케이블 직결이라 MITM 위험이 없다는 전제).
# 기기가 LAN SSH를 열어 둔 경우 `TF_HOST=<주소>`로 접속할 수 있으며,
# 이때도 호스트 키 검증을 생략하므로 신뢰할 수 있는 내부 네트워크에서만 사용한다.
# 접속 주소와 SSH 옵션은 `src/scripts/lib.sh`에 정의되어 있다(`tf_host`, `tf_ssh_opts`).
set -euo pipefail
. "$(dirname "$0")/lib.sh"

case "${1:-}" in
  --reboot)
    ssh "${tf_ssh_opts[@]}" "$tf_host" "PATH=\$PATH:/usr/sbin:/usr/local/sbin; (sleep 1; reboot-mode ${2:-normal}) >/dev/null 2>&1 &"
    ;;
  --put)
    [ $# -eq 3 ] || usage
    scp "${tf_ssh_opts[@]}" "$2" "$tf_host:$3"
    ;;
  --get)
    [ $# -eq 3 ] || usage
    scp "${tf_ssh_opts[@]}" "$tf_host:$2" "$3"
    ;;
  *) ssh "${tf_ssh_opts[@]}" "$tf_host" "$@" ;;
esac
