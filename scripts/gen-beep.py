#!/usr/bin/env python3
"""기준 테스트음(WAV)을 생성한다: 저음 -> 중음 -> 고음, 각 1초.

    gen-beep.py <출력.wav> [주파수 목록 Hz, 쉼표 구분] [음 길이(초)]

기본값은 500, 1000, 2000Hz 각 1초다. 48kHz 16비트 스테레오(좌우 동일 신호),
최대 진폭의 70%(-3dBFS) 사인파로 생성하며, 음량 보정용 기준 음원으로 기기의
`/usr/share/sounds/tossfront/reference.wav`에 배치된다.
각 톤의 시작/끝 10ms에 페이드 인/아웃을 적용해 클릭 노이즈를 방지한다.
"""
import math
import struct
import sys
import wave

RATE = 48000
AMPLITUDE = 0.7


def tone(freq: float, secs: float) -> bytes:
    n = int(RATE * secs)
    fade = int(RATE * 0.010)
    frames = bytearray()
    for i in range(n):
        env = min(1.0, i / fade, (n - 1 - i) / fade)
        v = int(32767 * AMPLITUDE * env * math.sin(2 * math.pi * freq * i / RATE))
        frames += struct.pack("<hh", v, v)
    return bytes(frames)


def main() -> None:
    out = sys.argv[1]
    freqs = [float(f) for f in (sys.argv[2] if len(sys.argv) > 2 else "500,1000,2000").split(",")]
    secs = float(sys.argv[3]) if len(sys.argv) > 3 else 1.0
    with wave.open(out, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        for f in freqs:
            w.writeframes(tone(f, secs))


if __name__ == "__main__":
    main()
