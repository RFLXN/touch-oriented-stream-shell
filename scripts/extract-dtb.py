#!/usr/bin/env python3
"""바이너리 이미지에서 FDT(디바이스 트리) 블롭을 검색해 추출한다(읽기 전용).

사용법: python3 -I scripts/extract-dtb.py <이미지> <출력 디렉터리>
"""
import struct
import sys
from pathlib import Path

MAGIC = b"\xd0\x0d\xfe\xed"


def main() -> None:
    src, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    data = src.read_bytes()
    pos, n = 0, 0
    while (pos := data.find(MAGIC, pos)) != -1:
        if pos + 40 > len(data):
            break
        total, off_struct, off_strings, _, version = struct.unpack_from(">5I", data, pos + 4)
        sane = (
            0x100 < total <= 4 * 1024 * 1024
            and pos + total <= len(data)
            and off_struct < total
            and off_strings < total
            and 16 <= version <= 17
        )
        if sane:
            dst = out / f"{src.stem}-{n:02d}-0x{pos:x}.dtb"
            dst.write_bytes(data[pos : pos + total])
            print(f"{dst.name}\t{total} bytes")
            n += 1
            pos += total
        else:
            pos += 4
    print(f"found {n}")


if __name__ == "__main__":
    main()
