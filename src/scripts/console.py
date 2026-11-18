#!/usr/bin/env python3
"""USB 시리얼 콘솔(`/dev/ttyACM0`)로 명령을 보내고 출력을 읽는다(비대화형 일회성 실행).

    console.py [-t 초] [-d 장치] '<셸 명령>'
    console.py [-d 장치] --put <로컬 파일> <원격 경로>    # base64로 전송 후 sha256 검증

`-t`는 출력 대기 타임아웃(기본 15초)이다.
로그인 프롬프트가 뜨면 root로 자동 로그인한 뒤 명령을 실행한다.
`--put`은 네트워크 연결이 없을 때 쓰는 전송 수단이다(네트워크가 되면 `src/scripts/tf-ssh.sh --put`이 훨씬 빠르다).
직접 대화형으로 접속할 때는 `picocom /dev/ttyACM0`을 쓰는 편이 낫다.
"""
import argparse
import os
import select
import sys
import termios
import time
import tty

MARK = "__TF_DONE__"


def read_until(fd: int, needle: bytes, timeout: float) -> bytes:
    buf = b""
    end = time.time() + timeout
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.2)
        if r:
            chunk = os.read(fd, 4096)
            if not chunk:
                break
            buf += chunk
            if needle and needle in buf:
                break
    return buf


def put(fd: int, local: str, remote: str) -> None:
    """네트워크 없이 시리얼 포트를 통해 base64 인코딩으로 기기에 파일을 전송한다."""
    import base64
    import hashlib

    data = open(local, "rb").read()
    b64 = base64.encodebytes(data)
    def drain() -> None:
        while select.select([fd], [], [], 0)[0]:
            os.read(fd, 65536)

    os.write(fd, f"stty -echo; base64 -d > {remote} <<'TF_EOF'\n".encode())
    time.sleep(0.5)
    drain()
    for line in b64.splitlines(keepends=True):
        os.write(fd, line)
        drain()
    os.write(fd, b"TF_EOF\nstty echo\n")
    read_until(fd, b"", 1.5)
    os.write(fd, f"sha256sum {remote}\n".encode())
    out = read_until(fd, b"\n", 10) + read_until(fd, b"", 1)
    want = hashlib.sha256(data).hexdigest()
    ok = want in out.decode(errors="replace")
    print(f"{local} -> {remote}: {len(data)} bytes, sha256 {'일치' if ok else '불일치'}")
    if not ok:
        raise SystemExit(1)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("-t", type=float, default=15)
    ap.add_argument("-d", default="/dev/ttyACM0")
    ap.add_argument("--put", nargs=2, metavar=("LOCAL", "REMOTE"))
    ap.add_argument("command", nargs="?", default="true")
    args = ap.parse_args()

    fd = os.open(args.d, os.O_RDWR | os.O_NOCTTY)
    tty.setraw(fd)
    attrs = termios.tcgetattr(fd)
    attrs[2] |= termios.CLOCAL
    termios.tcsetattr(fd, termios.TCSANOW, attrs)

    os.write(fd, b"\n")
    banner = read_until(fd, b"", 1.5)
    if b"login:" in banner:
        os.write(fd, b"root\n")
        read_until(fd, b"#", 5)

    if args.put:
        put(fd, args.put[0], args.put[1])
        os.close(fd)
        return

    os.write(fd, f"{args.command}; echo {MARK[:5]}''{MARK[5:]} $?\n".encode())
    out = read_until(fd, (MARK + " ").encode(), args.t)
    out += read_until(fd, b"\n", 1)
    os.close(fd)

    text = out.decode(errors="replace").replace("\r", "")
    lines = text.split("\n")
    if lines and args.command[:20] in lines[0]:
        lines = lines[1:]
    sys.stdout.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
