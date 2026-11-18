#!/usr/bin/env python3
"""Rockchip 리소스 번들(RSCE) 관리 도구.

순정 U-Boot는 부트 이미지의 `second` 영역에 담긴 RSCE 번들에서 `rk-kernel.dtb`를 추출해
커널 DTB로 사용한다. 이 스크립트로 번들 목록 조회, 추출, 패킹, 검증을 수행한다.

    rsce.py list   <번들>
    rsce.py unpack <번들> <출력 디렉터리>
    rsce.py pack   <출력 번들> <파일>...      # 인자 순서대로 패킹하며 파일명만 기록
    rsce.py verify <번들>                     # 각 엔트리의 SHA-1/SHA-256 해시 검증

형식(Rockchip resource_tool 기준, 블록 = 512바이트):
    헤더 1블록:   `"RSCE"`, 버전 2종(u16), 헤더 크기/테이블 오프셋/엔트리 크기(u8, 블록 단위), 엔트리 수(u32)
    엔트리 1블록: `"ENTR"`, 경로[220], 해시[32], 해시 길이(u32), 데이터 오프셋(u32, 블록 단위), 데이터 크기(u32, 바이트)
    데이터:       엔트리 순서대로 배치하며 블록 경계까지 0으로 패딩
"""
import hashlib
import struct
import sys
from pathlib import Path

BLOCK = 512
PATH_LEN = 220
HASH_LEN = 32
HEADER = struct.Struct("<4sHHBBBxI")
ENTRY = struct.Struct(f"<4s{PATH_LEN}s{HASH_LEN}sIII")


def pad(data: bytes) -> bytes:
    return data + b"\0" * (-len(data) % BLOCK)


def parse(data: bytes):
    magic, ver, tbl_ver, hdr_blks, tbl_off, ent_blks, count = HEADER.unpack_from(data, 0)
    if magic != b"RSCE":
        raise SystemExit("올바른 RSCE 번들이 아닙니다")
    entries = []
    for i in range(count):
        off = (tbl_off + i * ent_blks) * BLOCK
        tag, path, digest, hash_size, blk, size = ENTRY.unpack_from(data, off)
        if tag != b"ENTR":
            raise SystemExit(f"엔트리 {i}의 태그가 잘못되었습니다")
        name = path.split(b"\0", 1)[0].decode()
        content = data[blk * BLOCK : blk * BLOCK + size]
        entries.append((name, digest[:hash_size], blk, content))
    return (ver, tbl_ver, hdr_blks, tbl_off, ent_blks), entries


def cmd_list(src: str) -> None:
    _, entries = parse(Path(src).read_bytes())
    for name, digest, blk, content in entries:
        print(f"{name:20s} blk {blk:6d}  {len(content):9d} bytes  hash {len(digest)}B")


def cmd_verify(src: str) -> None:
    _, entries = parse(Path(src).read_bytes())
    bad = 0
    for name, digest, _, content in entries:
        if not digest:
            print(f"{name}: 해시 없음")
            continue
        algo = hashlib.sha1 if len(digest) == 20 else hashlib.sha256
        ok = algo(content).digest() == digest
        bad += not ok
        print(f"{name}: {'ok' if ok else '불일치'}")
    if bad:
        raise SystemExit(f"{bad}개 엔트리의 해시가 일치하지 않습니다")


def cmd_unpack(src: str, out: str) -> None:
    _, entries = parse(Path(src).read_bytes())
    dst = Path(out)
    dst.mkdir(parents=True, exist_ok=True)
    for name, _, _, content in entries:
        (dst / Path(name).name).write_bytes(content)
        print(name)


def cmd_pack(out: str, files: list[str]) -> None:
    contents = [(Path(f).name, Path(f).read_bytes()) for f in files]
    blk = 1 + len(contents)
    table, body = b"", b""
    for name, content in contents:
        if len(name.encode()) >= PATH_LEN:
            raise SystemExit(f"파일 이름이 너무 깁니다: {name}")
        digest = hashlib.sha1(content).digest()
        table += pad(ENTRY.pack(b"ENTR", name.encode(), digest, len(digest), blk, len(content)))
        body += pad(content)
        blk += len(pad(content)) // BLOCK
    header = pad(HEADER.pack(b"RSCE", 0, 0, 1, 1, 1, len(contents)))
    Path(out).write_bytes(header + table + body)
    print(f"{out}: {len(contents)}개 엔트리, {len(header + table + body)} bytes")


def main() -> None:
    args = sys.argv[1:]
    if len(args) == 2 and args[0] == "list":
        cmd_list(args[1])
    elif len(args) == 2 and args[0] == "verify":
        cmd_verify(args[1])
    elif len(args) == 3 and args[0] == "unpack":
        cmd_unpack(args[1], args[2])
    elif len(args) >= 3 and args[0] == "pack":
        cmd_pack(args[1], args[2:])
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
