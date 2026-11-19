"""부팅 스플래시 화면과 애니메이션 프레임을 만든다. `src/scripts/make-boot.sh`가 실행한다.

    python3 -I splash.py <설정 JSON> <그림 | -> <출력 디렉터리>

설정(JSON)은 `buildImage`의 `splash` 옵션에 화면 크기를 더한 것이다(`dist/images/boot-inputs.json`의 `splash`):
    background, width, align, offset, screen: [가로, 세로]
그림은 Pillow가 읽는 이미지다(PNG, APNG, GIF, WebP). 투명한 부분은 배경색으로 채우고, 애니메이션이면 프레임마다 재생 시간을 살린다.
`-`이면 배경색만 칠한다.

출력
    splash.ppm      첫 프레임을 화면 크기로 합성한 PPM(P6). initramfs의 `fbsplash`가 그린다
    preview.png     위 화면의 미리보기
    splash.frames   애니메이션일 때만. 프레임마다 앞 프레임과 달라진 사각형만 담는다(형식은 `src/os/tools/splash-play.c` 머리 주석)
    preview.gif     애니메이션일 때만. 화면 전체를 절반 크기로 줄인 미리보기
"""
import json
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageSequence

MAGIC = b"TFSP"
VERSION = 2
MAX_FRAMES = 4096


def fail(msg):
    raise SystemExit(f"[FAIL] splash: {msg}")


def layout(cfg, size):
    sw, sh = cfg["screen"]
    scale = cfg["width"] / size[0] if cfg["width"] > 0 else 1.0
    w, h = max(1, round(size[0] * scale)), max(1, round(size[1] * scale))
    x = (sw - w) // 2
    y = {"top": cfg["offset"], "bottom": sh - h - cfg["offset"], "center": (sh - h) // 2 + cfg["offset"]}[cfg["align"]]
    return w, h, x, y


def compose(cfg, bg, frame, place):
    sw, sh = cfg["screen"]
    screen = Image.new("RGBA", (sw, sh), bg + (255,))
    if frame is not None:
        w, h, x, y = place
        layer = Image.new("RGBA", (sw, sh), (0, 0, 0, 0))
        img = frame if frame.size == (w, h) else frame.convert("RGBa").resize((w, h), Image.LANCZOS).convert("RGBA")
        layer.paste(img, (x, y))
        screen.alpha_composite(layer)
    return screen.convert("RGB")


def frames_of(path):
    if path == "-":
        return [(None, 0)]
    src = Image.open(path)
    out = []
    for frame in ImageSequence.Iterator(src):
        delay = int(frame.info.get("duration", src.info.get("duration", 100)) or 0)
        out.append((frame.convert("RGBA"), delay if delay >= 20 else 100))
        if len(out) > MAX_FRAMES:
            fail(f"프레임이 {MAX_FRAMES}개를 넘습니다")
    return out


def records(screens, delays, stride):
    out = []
    for k, cur in enumerate(screens):
        box = ImageChops.difference(screens[k - 1], cur).getbbox()
        x0, y0, x1, y1 = box or (0, 0, 0, 0)
        head = b"".join(v.to_bytes(4, "little") for v in (x0, y0, x1 - x0, y1 - y0, delays[k]))
        body = cur.crop(box).convert("RGBX").tobytes("raw", "BGRX") if box else b""
        out.append(head + body)
    return MAGIC + b"".join(v.to_bytes(4, "little") for v in (VERSION, stride, len(screens))).ljust(60, b"\0") + b"".join(out)


def main():
    cfg = json.loads(Path(sys.argv[1]).read_text())
    out = Path(sys.argv[3])
    bg = tuple(int(cfg["background"][i : i + 2], 16) for i in (1, 3, 5))
    sw, sh = cfg["screen"]
    frames = frames_of(sys.argv[2])
    place = layout(cfg, frames[0][0].size) if frames[0][0] is not None else None
    screens = [compose(cfg, bg, f, place) for f, _ in frames]

    (out / "splash.ppm").write_bytes(f"P6\n{sw} {sh}\n255\n".encode() + screens[0].tobytes())
    screens[0].save(out / "preview.png", optimize=True)
    if len(screens) < 2:
        return
    delays = [d for _, d in frames]
    (out / "splash.frames").write_bytes(records(screens, delays, sw * 4))
    small = [s.resize((sw // 2, sh // 2), Image.LANCZOS) for s in screens]
    small[0].save(out / "preview.gif", save_all=True, append_images=small[1:], duration=delays, loop=0, optimize=False)


main()
