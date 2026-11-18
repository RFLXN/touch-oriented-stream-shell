#!/usr/bin/env bash
# PC의 인터넷 연결을 프록시로 빌려 기기에서 명령을 실행한다(기기 내 패키지 설치 등에 사용).
#
#   scripts/tf-net.sh apk add htop
#   scripts/tf-net.sh 'apk update && apk upgrade'
#
# USB 케이블로만 연결되어 기기에 외부 인터넷이 없을 때 사용한다. 동작 순서:
#   1. PC 로컬(127.0.0.1)에 HTTP 프록시(`tinyproxy`) 실행
#   2. SSH 역방향 포워딩으로 기기의 `127.0.0.1:8888`을 PC 프록시 포트에 연결
#   3. 기기 시스템 시각을 PC 시각과 동기화(시계가 어긋나면 TLS 인증서 검증이 실패함)
#   4. `http_proxy`/`https_proxy` 환경 변수를 설정해 지정한 명령 실행
# PC 측 root 권한이나 방화벽 변경 없이 동작하며, 명령 종료 시 프록시와 터널이 자동 정리된다.
# `TF_HOST=<주소>`를 지정하면 LAN 경유로 접속한다(`scripts/tf-ssh.sh`와 동일).
set -euo pipefail
. "$(dirname "$0")/lib.sh"

[ $# -ge 1 ] || usage

port=8888

conf=$(mktemp)
trap 'kill "${proxy_pid:-}" 2>/dev/null || true; rm -f "$conf"' EXIT
cat >"$conf" <<EOF
Port $port
Listen 127.0.0.1
Allow 127.0.0.1
Timeout 600
MaxClients 20
LogLevel Error
EOF
tinyproxy -d -c "$conf" &
proxy_pid=$!
sleep 0.5

proxy=http://127.0.0.1:$port
ssh -o ServerAliveInterval=5 -o ServerAliveCountMax=3 -o ExitOnForwardFailure=yes "${tf_ssh_opts[@]}" \
  -R "$port:127.0.0.1:$port" "$tf_host" \
  "date -s @$(date +%s) >/dev/null; export http_proxy=$proxy https_proxy=$proxy HTTP_PROXY=$proxy HTTPS_PROXY=$proxy; $*"
