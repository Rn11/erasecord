"""Draws assets/icon.png (the design of assets/icon.svg) with a transparent
background and generates the app icons from it.

    pip install pillow && python3 assets/make-icon.py   # run in app/
"""
import subprocess
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

SIZE = 1024
SCALE = 4  # drawn larger, then scaled down for smooth edges
S = SIZE * SCALE


def s(v: float) -> int:
    return round(v * SCALE)


def gradient() -> Image.Image:
    """#7d6fff at the top left to #4536c4 at the bottom right."""
    a, b = (0x7D, 0x6F, 0xFF), (0x45, 0x36, 0xC4)
    small = Image.new("RGB", (256, 256))
    px = small.load()
    for y in range(256):
        for x in range(256):
            t = (x + y) / 510
            px[x, y] = tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))
    return small.resize((S, S), Image.BILINEAR).convert("RGBA")


icon = Image.new("RGBA", (S, S), (0, 0, 0, 0))
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).rounded_rectangle((s(64), s(64), s(960), s(960)), radius=s(208), fill=255)
icon.paste(gradient(), (0, 0), mask)

draw = ImageDraw.Draw(icon)
# The speech bubble with its tail.
draw.rounded_rectangle((s(212), s(300), s(812), s(680)), radius=s(80), fill="white")
draw.polygon([(s(352), s(670)), (s(482), s(670)), (s(352), s(790))], fill="white")
# The cross, with round ends.
width = s(58)
for (x1, y1), (x2, y2) in [((436, 414), (588, 566)), ((588, 414), (436, 566))]:
    draw.line((s(x1), s(y1), s(x2), s(y2)), fill="#5747dc", width=width)
    for x, y in ((x1, y1), (x2, y2)):
        r = width / 2
        draw.ellipse((s(x) - r, s(y) - r, s(x) + r, s(y) + r), fill="#5747dc")

out = icon.resize((SIZE, SIZE), Image.LANCZOS)
# Nothing outside the rounded square may be visible.
alpha = out.getchannel("A")
assert alpha.getpixel((0, 0)) == 0 and alpha.getpixel((SIZE - 1, SIZE - 1)) == 0
path = Path(__file__).with_name("icon.png")
out.save(path)
print(f"wrote {path}")

subprocess.run(["npm", "run", "tauri", "icon", str(path)], check=True)
for extra in ("android", "ios"):
    subprocess.run(["rm", "-rf", f"src-tauri/icons/{extra}"], check=True)
