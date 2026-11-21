"""Renders the Sweet IME app icon into assets/icon/.

A strawberry-milk tile with dripping cream icing and sprinkles, and a glossy
candy "あ" with a white sticker outline.

Needs Pillow and a rounded Japanese font. The default is Zen Maru Gothic Black
(SIL OFL), from https://github.com/google/fonts/tree/main/ofl/zenmarugothic;
point SWEET_ICON_FONT at the .ttf. Run from the repository root:

    SWEET_ICON_FONT=path/to/ZenMaruGothic-Black.ttf python scripts/make-icon.py
"""

import math
import os
import random
import sys

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

OUT = os.path.join("assets", "icon")
FONT = os.environ.get("SWEET_ICON_FONT", "ZenMaruGothic-Black.ttf")
M = 2048  # master size; small sizes are drawn separately at 8x

BG_TOP = (255, 196, 219)
BG_BOTTOM = (255, 128, 172)
RIM = (236, 96, 146)
ICING = (255, 250, 244)
ICING_EDGE = (246, 222, 222)
ICING_SHADOW = (205, 70, 125)
CANDY_TOP = (255, 112, 160)
CANDY_BOTTOM = (226, 38, 112)
CANDY_SHADE = (176, 20, 86)
STICKER = (255, 255, 255)
DROP = (170, 30, 90)
SPRINKLES = [(255, 214, 92), (120, 216, 190), (128, 190, 255), (190, 150, 255), (255, 120, 120),
             (255, 160, 200)]


def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def solid(size, color):
    return Image.new("RGBA", (size, size), tuple(color) + (255,))


def gradient(size, top, bottom, y0=0.0, y1=1.0):
    col = Image.new("RGBA", (1, size))
    for y in range(size):
        t = min(max((y / (size - 1) - y0) / (y1 - y0), 0.0), 1.0)
        col.putpixel((0, y), lerp(top, bottom, t) + (255,))
    return col.resize((size, size))


def paint(img, color_img, mask):
    """Draws `color_img` onto `img` where `mask` is set."""
    return Image.composite(color_img, img, mask)


def squircle(size, inset, n=4.6):
    m = Image.new("L", (size, size), 0)
    r, c = size / 2 - inset, size / 2
    pts = []
    for i in range(1440):
        t = 2 * math.pi * i / 1440
        ct, st = math.cos(t), math.sin(t)
        pts.append((c + r * math.copysign(abs(ct) ** (2 / n), ct),
                    c + r * math.copysign(abs(st) ** (2 / n), st)))
    ImageDraw.Draw(m).polygon(pts, fill=255)
    return m


def gooey(mask, radius):
    """Blur and re-threshold: joins shapes with smooth, liquid curves."""
    return mask.filter(ImageFilter.GaussianBlur(radius)).point(lambda v: 255 if v > 128 else 0) \
        .filter(ImageFilter.GaussianBlur(1.2))


def grow(mask, radius):
    """Rounded dilation."""
    return mask.filter(ImageFilter.GaussianBlur(radius)).point(lambda v: 255 if v > 18 else 0) \
        .filter(ImageFilter.GaussianBlur(1.2))


def shift(mask, dx, dy):
    out = Image.new("L", mask.size, 0)
    out.paste(mask, (round(dx), round(dy)))
    return out


def scale_alpha(mask, f):
    return mask.point(lambda v: round(v * f))


def icing_mask(size, base, drips):
    """Cream across the top with drips hanging from it."""
    m = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(m)
    d.rectangle((0, 0, size, base * size), fill=255)
    for x, length, w in drips:
        cx, w = x * size, w * size
        bottom = (base + length) * size
        d.rectangle((cx - w / 2, base * size - w, cx + w / 2, bottom), fill=255)
        d.ellipse((cx - w / 2, bottom - w / 2, cx + w / 2, bottom + w / 2), fill=255)
    # Gentle scallops between the drips.
    for i in range(9):
        x = (i + 0.5) / 9 * size
        r = 0.045 * size
        d.ellipse((x - r, base * size - r * 0.9, x + r, base * size + r * 0.9), fill=255)
    return gooey(m, size * 0.018)


def glyph_mask(size, text, scale, dy):
    font = ImageFont.truetype(FONT, round(size * scale))
    m = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(m)
    x0, y0, x1, y1 = d.textbbox((0, 0), text, font=font)
    d.text(((size - (x1 - x0)) / 2 - x0, (size - (y1 - y0)) / 2 - y0 + dy * size), text,
           font=font, fill=255)
    return m


def sparkle(d, cx, cy, r, fill, thin=0.22):
    pts = []
    for i in range(8):
        a = math.pi / 4 * i - math.pi / 2
        rr = r if i % 2 == 0 else r * thin
        pts.append((cx + rr * math.cos(a), cy + rr * math.sin(a)))
    d.polygon(pts, fill=fill)


def heart(d, cx, cy, w, fill):
    r = w / 4
    d.ellipse((cx - 2 * r, cy - r, cx, cy + r), fill=fill)
    d.ellipse((cx, cy - r, cx + 2 * r, cy + r), fill=fill)
    d.polygon([(cx - 1.93 * r, cy + 0.35 * r), (cx + 1.93 * r, cy + 0.35 * r), (cx, cy + 2.2 * r)],
              fill=fill)


def sprinkles(size, region, count, seed=7):
    """Rounded sprinkles scattered inside `region` (a mask)."""
    layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    rnd = random.Random(seed)
    placed = []
    length, width = 0.052 * size, 0.019 * size
    tries = 0
    while len(placed) < count and tries < 5000:
        tries += 1
        x, y = rnd.uniform(0.12, 0.88) * size, rnd.uniform(0.075, 0.22) * size
        # The whole sprinkle must sit on the icing.
        if any(region.getpixel((int(x + ox * size), int(y + oy * size))) < 255
               for ox in (-0.04, 0, 0.04) for oy in (-0.04, 0, 0.04)):
            continue
        if any((x - px) ** 2 + (y - py) ** 2 < (0.075 * size) ** 2 for px, py in placed):
            continue
        # Keep the middle clear above the kana.
        if abs(x - size / 2) < 0.2 * size and y > 0.2 * size:
            continue
        placed.append((x, y))
        a = rnd.uniform(0, math.pi)
        dx, dy = math.cos(a) * length / 2, math.sin(a) * length / 2
        color = SPRINKLES[len(placed) % len(SPRINKLES)]
        d.line((x - dx, y - dy, x + dx, y + dy), fill=color + (255,), width=round(width))
        for ex, ey in ((x - dx, y - dy), (x + dx, y + dy)):
            d.ellipse((ex - width / 2, ey - width / 2, ex + width / 2, ey + width / 2),
                      fill=color + (255,))
    return layer


def render_master(size, detail=True):
    s = size
    tile = squircle(s, inset=s * (0.03 if detail else 0.0))
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    img = paint(img, gradient(s, BG_TOP, BG_BOTTOM), tile)

    # Soft light from the top left.
    glow = Image.new("L", (s, s), 0)
    ImageDraw.Draw(glow).ellipse((-0.35 * s, 0.05 * s, 0.75 * s, 1.1 * s), fill=46)
    glow = ImageChops.multiply(glow.filter(ImageFilter.GaussianBlur(s * 0.14)), tile)
    img = paint(img, solid(s, (255, 255, 255)), glow)

    # Icing with a soft shadow and a thicker, darker lower edge.
    drips = [(0.13, 0.13, 0.085), (0.31, 0.06, 0.07), (0.5, 0.17, 0.09), (0.69, 0.08, 0.075),
             (0.87, 0.15, 0.085)] if detail else [(0.2, 0.1, 0.13), (0.5, 0.16, 0.14), (0.8, 0.1, 0.13)]
    icing = ImageChops.multiply(icing_mask(s, 0.24 if detail else 0.22, drips), tile)
    sh = shift(icing, 0, s * 0.018).filter(ImageFilter.GaussianBlur(s * 0.014))
    img = paint(img, solid(s, ICING_SHADOW), scale_alpha(ImageChops.multiply(sh, tile), 0.45))
    img = paint(img, solid(s, ICING_EDGE), icing)
    top_part = ImageChops.multiply(icing, shift(icing, 0, -s * 0.014))
    img = paint(img, gradient(s, ICING, (255, 240, 236), 0.0, 0.4), top_part)
    if detail:
        # Shine along each drip.
        shine = ImageChops.subtract(top_part, shift(top_part, -s * 0.012, s * 0.012))
        shine = ImageChops.multiply(shine, shift(icing, 0, -s * 0.05))
        img = paint(img, solid(s, (255, 255, 255)), scale_alpha(shine.filter(ImageFilter.GaussianBlur(s * 0.004)), 0.9))
        img = Image.alpha_composite(img, sprinkles(s, top_part, 13))

    # Candy kana: drop shadow, white sticker edge, glossy fill.
    g = glyph_mask(s, "あ", 0.62 if detail else 0.74, 0.085 if detail else 0.07)
    edge = grow(g, s * (0.03 if detail else 0.022))
    drop = shift(edge, 0, s * 0.02).filter(ImageFilter.GaussianBlur(s * 0.02))
    img = paint(img, solid(s, DROP), scale_alpha(ImageChops.multiply(drop, tile), 0.4))
    img = paint(img, solid(s, STICKER), edge)
    img = paint(img, gradient(s, CANDY_TOP, CANDY_BOTTOM, 0.3, 0.9), g)
    if detail:
        # Darker underside and a bright rim on top edges, like a gummy candy.
        under = ImageChops.subtract(g, shift(g, 0, -s * 0.02)).filter(ImageFilter.GaussianBlur(s * 0.008))
        img = paint(img, solid(s, CANDY_SHADE), scale_alpha(ImageChops.multiply(under, g), 0.55))
        top = ImageChops.subtract(g, shift(g, 0, s * 0.012))
        top = ImageChops.multiply(top.filter(ImageFilter.GaussianBlur(s * 0.006)), g)
        img = paint(img, solid(s, (255, 214, 232)), scale_alpha(top, 0.7))
        # A glassy streak across the upper part of the kana.
        band = Image.new("L", (s, s), 0)
        ImageDraw.Draw(band).ellipse((0.12 * s, 0.18 * s, 0.95 * s, 0.56 * s), fill=255)
        inner = g.filter(ImageFilter.GaussianBlur(s * 0.012)).point(lambda v: 255 if v > 225 else 0)
        inner = inner.filter(ImageFilter.GaussianBlur(s * 0.006))
        band = ImageChops.multiply(band.filter(ImageFilter.GaussianBlur(s * 0.03)), inner)
        img = paint(img, solid(s, (255, 255, 255)), scale_alpha(band, 0.22))

        deco = Image.new("RGBA", (s, s), (0, 0, 0, 0))
        d = ImageDraw.Draw(deco)
        sparkle(d, 0.835 * s, 0.6 * s, 0.075 * s, (255, 255, 255, 255))
        sparkle(d, 0.905 * s, 0.72 * s, 0.035 * s, (255, 255, 255, 230))
        sparkle(d, 0.14 * s, 0.86 * s, 0.045 * s, (255, 230, 120, 255))
        heart(d, 0.16 * s, 0.6 * s, 0.1 * s, (255, 255, 255, 240))
        img = Image.alpha_composite(img, deco)

    # Thin rim so the tile holds its shape on light backgrounds.
    rim = ImageChops.subtract(tile, squircle(s, inset=s * (0.03 if detail else 0.0) + s * 0.006))
    img = paint(img, solid(s, RIM), scale_alpha(rim, 0.5))
    return img


def main():
    if not os.path.exists(FONT):
        sys.exit(f"font not found: {FONT} (set SWEET_ICON_FONT)")
    os.makedirs(OUT, exist_ok=True)
    big = render_master(M)
    small = render_master(512, detail=False)
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256, 512, 1024]
    imgs = {sz: (big if sz >= 32 else small).resize((sz, sz), Image.LANCZOS) for sz in sizes}
    for sz in (16, 24, 32, 48, 64, 128, 256, 512, 1024):
        imgs[sz].save(os.path.join(OUT, f"sweet-ime-{sz}.png"), optimize=True)
    ico_sizes = [16, 20, 24, 32, 40, 48, 64, 256]
    # Pillow writes each size from its own image when given them in order.
    imgs[256].save(os.path.join(OUT, "sweet-ime.ico"), sizes=[(x, x) for x in ico_sizes],
                   append_images=[imgs[x] for x in ico_sizes if x != 256])
    print("wrote", OUT, file=sys.stderr)


if __name__ == "__main__":
    main()
