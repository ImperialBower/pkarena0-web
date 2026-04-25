#!/usr/bin/env python3
"""Generate PWA icons for pkarena0-web from icon-source.png.

Produces www/icon-192.png, www/icon-512.png, www/icon-512-maskable.png.
The maskable variant adds the safe-zone padding (Android adaptive icons
crop to ~80% of the canvas) over a dark background.
"""
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "scripts" / "icon-source.png"
OUT = ROOT / "www"

BG = (13, 13, 26, 255)  # #0d0d1a — site background


def square(img: Image.Image, size: int) -> Image.Image:
    return img.convert("RGBA").resize((size, size), Image.LANCZOS)


def maskable(img: Image.Image, size: int, safe_ratio: float = 0.8) -> Image.Image:
    canvas = Image.new("RGBA", (size, size), BG)
    inner = int(size * safe_ratio)
    scaled = img.convert("RGBA").resize((inner, inner), Image.LANCZOS)
    offset = (size - inner) // 2
    canvas.paste(scaled, (offset, offset), scaled)
    return canvas


def main() -> None:
    src = Image.open(SRC)
    OUT.mkdir(parents=True, exist_ok=True)
    square(src, 192).save(OUT / "icon-192.png", optimize=True)
    square(src, 512).save(OUT / "icon-512.png", optimize=True)
    maskable(src, 512).save(OUT / "icon-512-maskable.png", optimize=True)
    print("wrote icon-192.png, icon-512.png, icon-512-maskable.png")


if __name__ == "__main__":
    main()
