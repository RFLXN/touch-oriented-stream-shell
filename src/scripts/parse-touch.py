#!/usr/bin/env python3
"""`/dev/input/eventN` 원시 덤프 파일에서 터치 좌표를 추출한다.

    parse-touch.py <덤프 파일>

기기에서 `cat /dev/input/event0 > touch.bin`으로 캡처한 파일을 PC에서 파싱한다.
터치할 때마다(새 트래킹 ID가 할당될 때마다) 시작 좌표와 끝 좌표를 출력한다.
"""
import struct
import sys

EV_ABS = 3
ABS_MT_SLOT, ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_TRACKING_ID = 0x2F, 0x35, 0x36, 0x39
EVENT = struct.Struct("<qqHHi")


def main() -> None:
    data = open(sys.argv[1], "rb").read()
    touches, cur, x, y = [], None, None, None
    xs, ys = [], []
    for off in range(0, len(data) - EVENT.size + 1, EVENT.size):
        _, _, etype, code, value = EVENT.unpack_from(data, off)
        if etype != EV_ABS:
            continue
        if code == ABS_MT_TRACKING_ID:
            if value >= 0:
                cur = {"first": None, "last": None}
                touches.append(cur)
            else:
                cur = None
        elif code == ABS_MT_POSITION_X:
            x = value
            xs.append(value)
        elif code == ABS_MT_POSITION_Y:
            y = value
            ys.append(value)
        if cur is not None and code in (ABS_MT_POSITION_X, ABS_MT_POSITION_Y) and x is not None and y is not None:
            if cur["first"] is None and code == ABS_MT_POSITION_Y:
                cur["first"] = (x, y)
            cur["last"] = (x, y)

    print(f"이벤트 {len(data) // EVENT.size}개, 터치 {len(touches)}회")
    for i, t in enumerate(touches, 1):
        print(f"  {i}: 시작 {t['first']} 끝 {t['last']}")
    if xs and ys:
        print(f"X 범위 {min(xs)}~{max(xs)}, Y 범위 {min(ys)}~{max(ys)}")


if __name__ == "__main__":
    main()
