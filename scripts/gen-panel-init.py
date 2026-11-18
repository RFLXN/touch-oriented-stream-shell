#!/usr/bin/env python3
"""순정 디바이스 트리에서 패널 초기화/종료 명령열을 추출해 C 헤더로 변환한다.

    gen-panel-init.py <순정 dts> <패널 노드 이름> <출력 헤더>
    예: gen-panel-init.py analysis/device/live-dt.dts mipi_lcd_23 os/kernel/drivers/panel-sunmi-toss-front-init.h

명령열 형식(Rockchip): `[타입, 지연(ms), 페이로드 길이, 데이터...]`의 반복.
바이트 해석 없이 원문 그대로 추출하며 드라이버가 동일한 형식으로 파싱해 전송한다.
"""
import re
import sys


def node_text(dts: str, name: str) -> str:
    start = dts.index(f"{name} {{")
    end = dts.index("display-timings", start)
    return dts[start:end]


def seq_bytes(node: str, prop: str) -> bytes:
    m = re.search(rf"{prop} = (\[[^\]]*\]|<[^>]*>);", node)
    if not m:
        raise SystemExit(f"{prop} 속성이 없습니다")
    body = m.group(1)
    if body[0] == "[":
        return bytes.fromhex(body[1:-1].replace(" ", ""))
    out = b""
    for cell in body[1:-1].split():
        out += int(cell, 16).to_bytes(4, "big")
    return out


def check(seq: bytes) -> int:
    i = n = 0
    while i < len(seq):
        if i + 3 > len(seq) or i + 3 + seq[i + 2] > len(seq):
            raise SystemExit("명령열 길이가 형식과 일치하지 않습니다")
        i += 3 + seq[i + 2]
        n += 1
    return n


def c_array(name: str, seq: bytes) -> str:
    lines, i = [], 0
    while i < len(seq):
        n = 3 + seq[i + 2]
        lines.append("\t" + " ".join(f"0x{b:02x}," for b in seq[i : i + n]))
        i += n
    return f"static const u8 {name}[] = {{\n" + "\n".join(lines) + "\n};\n"


def main() -> None:
    dts_path, node_name, out_path = sys.argv[1:4]
    node = node_text(open(dts_path).read(), node_name)
    init = seq_bytes(node, "panel-init-sequence")
    exit_ = seq_bytes(node, "panel-exit-sequence")
    n_init, n_exit = check(init), check(exit_)
    with open(out_path, "w") as f:
        f.write("/* SPDX-License-Identifier: GPL-2.0 */\n")
        f.write(f"/* 자동 생성: scripts/gen-panel-init.py — 순정 DT의 {node_name}에서 추출. 직접 수정 금지. */\n")
        f.write("/* 형식: DCS/Generic 타입, 지연(ms), 페이로드 길이, 데이터... */\n\n")
        f.write(c_array("tf_panel_init_seq", init))
        f.write("\n")
        f.write(c_array("tf_panel_exit_seq", exit_))
    print(f"{out_path}: init {n_init}개 명령({len(init)} bytes), exit {n_exit}개 명령({len(exit_)} bytes)")


if __name__ == "__main__":
    main()
