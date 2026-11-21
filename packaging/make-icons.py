#!/usr/bin/env python3
"""Generates every app icon from packaging/icon.svg (logo on a white background).

Needs `resvg` (cargo install resvg) and Pillow.  Run:  python3 packaging/make-icons.py
"""
import os, subprocess, tempfile
from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
P = lambda *a: os.path.join(ROOT, *a)
WHITE = (255, 255, 255, 255)

with tempfile.TemporaryDirectory() as tmp:
    out = os.path.join(tmp, "logo.png")
    subprocess.run(["resvg", "-w", "2048", P("packaging", "icon.svg"), out], check=True)
    logo = Image.open(out).convert("RGBA")
    logo = logo.crop(logo.getbbox())  # trim to the artwork itself


def fit(size, frac):
    """The logo scaled so its larger side is `frac` of `size`, centred on a transparent square."""
    s = int(size * frac)
    w, h = logo.size
    k = s / max(w, h)
    img = logo.resize((max(1, round(w * k)), max(1, round(h * k))), Image.LANCZOS)
    canvas = Image.new("RGBA", (size, size))
    canvas.alpha_composite(img, ((size - img.width) // 2, (size - img.height) // 2))
    return canvas


def plate(size, shape, frac=0.72, inset=0.0, radius=0.225):
    """Logo on a white plate: 'square' (full bleed), 'rounded' or 'circle'."""
    ss = 4  # supersample the plate edge
    big = size * ss
    mask = Image.new("L", (big, big))
    d = ImageDraw.Draw(mask)
    m = int(big * inset)
    box = (m, m, big - m - 1, big - m - 1)
    if shape == "square":
        d.rectangle(box, fill=255)
    elif shape == "rounded":
        d.rounded_rectangle(box, radius=int((big - 2 * m) * radius), fill=255)
    else:
        d.ellipse(box, fill=255)
    mask = mask.resize((size, size), Image.LANCZOS)
    img = Image.new("RGBA", (size, size))
    img.paste(Image.new("RGBA", (size, size), WHITE), (0, 0), mask)
    img.alpha_composite(fit(size, frac * (1 - 2 * inset)))
    return img


def save(img, *path):
    os.makedirs(os.path.dirname(P(*path)), exist_ok=True)
    img.save(P(*path), optimize=True)
    print("wrote", os.path.join(*path))


# iOS (the system rounds the corners itself): full-bleed white square
save(plate(1024, "square").convert("RGB"), "packaging", "icon-1024.png")
# macOS: rounded plate with the standard margin (824/1024)
save(plate(1024, "rounded", inset=100 / 1024), "packaging", "macos", "icon-1024.png")
# Linux, window icon
desktop = plate(256, "rounded", inset=8 / 256)
save(desktop, "packaging", "icon-256.png")
save(desktop, "crates", "lightpdf-studio", "assets", "icon.png")
# Windows .exe icon
ico = plate(256, "rounded", inset=8 / 256)
os.makedirs(P("packaging", "windows"), exist_ok=True)
ico.save(P("packaging", "windows", "icon.ico"), sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
print("wrote packaging/windows/icon.ico")
# Android: adaptive foreground (108dp, logo inside the 66dp safe zone) + legacy icons
res = ("crates", "lightpdf-studio", "android", "res")
for name, k in [("mdpi", 1), ("hdpi", 1.5), ("xhdpi", 2), ("xxhdpi", 3), ("xxxhdpi", 4)]:
    d = f"mipmap-{name}"
    save(fit(round(108 * k), 0.5), *res, d, "ic_launcher_foreground.png")
    save(plate(round(48 * k), "rounded", inset=2 / 48, radius=0.18), *res, d, "ic_launcher.png")
    save(plate(round(48 * k), "circle", frac=0.66, inset=2 / 48), *res, d, "ic_launcher_round.png")
