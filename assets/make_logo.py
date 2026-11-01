"""Renders the Hilal logo: a golden crescent on a midnight squircle, with a small notch
hanging from the top edge. Writes hilal.ico (PNG-compressed, one hand-rendered image per
size) and large PNGs. Small sizes drop the fine details so they stay crisp in the tray.

Run with any Python that has Pillow:  python make_logo.py
"""
import io
import math
import struct
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

HERE = Path(__file__).parent
SS = 8  # supersampling factor

MIDNIGHT_TOP = (22, 28, 66)
MIDNIGHT_BOTTOM = (6, 8, 24)
GOLD_LIGHT = (255, 231, 160)
GOLD_DEEP = (255, 170, 64)


def squircle_mask(size, n=5.0):
    """Superellipse |x|^n + |y|^n <= 1, the soft-square shape app icons use."""
    m = Image.new("L", (size, size), 0)
    px = m.load()
    r = size / 2
    for y in range(size):
        dy = abs((y + 0.5 - r) / r)
        # Solve the row's half-width once instead of testing every pixel.
        if dy >= 1:
            continue
        half = (1 - dy ** n) ** (1 / n) * r
        x0, x1 = int(round(r - half)), int(round(r + half))
        for x in range(max(0, x0), min(size, x1)):
            px[x, y] = 255
    return m


def vertical_gradient(size, top, bottom):
    g = Image.new("RGB", (1, size))
    for y in range(size):
        t = y / (size - 1)
        g.putpixel((0, y), tuple(int(a + (b - a) * t) for a, b in zip(top, bottom)))
    return g.resize((size, size))


def radial(size, cx, cy, radius, color, alpha):
    """A soft radial glow as an RGBA layer."""
    layer = Image.new("RGBA", (size, size), color + (0,))
    a = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(a)
    steps = 48
    for i in range(steps, 0, -1):
        r = radius * i / steps
        v = int(alpha * (1 - i / steps) ** 1.6)
        d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=v)
    layer.putalpha(a)
    return layer


def crescent_mask(size, cx, cy, r, bite_dx, bite_dy, bite_r):
    m = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(m)
    d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=255)
    bite = Image.new("L", (size, size), 0)
    ImageDraw.Draw(bite).ellipse(
        (cx + bite_dx - bite_r, cy + bite_dy - bite_r, cx + bite_dx + bite_r, cy + bite_dy + bite_r), fill=255
    )
    return ImageChops.subtract(m, bite)


def render(px, detailed=True):
    s = px * (SS if px <= 256 else 4)
    out = Image.new("RGBA", (s, s), (0, 0, 0, 0))

    # Midnight squircle with a cool sheen in the upper left.
    shape = squircle_mask(s)
    bg = vertical_gradient(s, MIDNIGHT_TOP, MIDNIGHT_BOTTOM).convert("RGBA")
    bg.alpha_composite(radial(s, s * 0.28, s * 0.22, s * 0.75, (70, 96, 200), 120))
    bg.putalpha(shape)
    out.alpha_composite(bg)

    # The notch: a black slab hanging from the top edge, with concave ears.
    if detailed:
        nw, nh, nr = s * 0.36, s * 0.105, s * 0.05
        x0, x1 = (s - nw) / 2, (s + nw) / 2
        notch = Image.new("L", (s, s), 0)
        d = ImageDraw.Draw(notch)
        d.rounded_rectangle((x0, -nr, x1, nh), radius=nr, fill=255)
        e = s * 0.03
        for ex, cx in ((x0 - e, x0 - e), (x1, x1 + e)):
            d.rectangle((ex, 0, ex + e, e), fill=255)
            d.ellipse((cx - e, 0, cx + e, 2 * e), fill=0)
        notch = ImageChops.multiply(notch, shape)
        black = Image.new("RGBA", (s, s), (0, 0, 0, 255))
        black.putalpha(notch)
        out.alpha_composite(black)

    # The crescent, opening to the upper right, with a warm glow behind it.
    cx, cy = s * 0.47, s * 0.56
    r = s * (0.30 if detailed else 0.34)
    moon = crescent_mask(s, cx, cy, r, r * 0.42, -r * 0.30, r * 0.86)
    glow_src = Image.new("RGBA", (s, s), GOLD_DEEP + (0,))
    glow_src.putalpha(moon.point(lambda v: int(v * 0.85)))
    glow = glow_src.filter(ImageFilter.GaussianBlur(s * 0.05))
    glow.putalpha(ImageChops.multiply(glow.getchannel("A"), shape))
    out.alpha_composite(glow)

    # Light gold at the upper left fading to deep gold at the lower right.
    ramp = Image.linear_gradient("L").rotate(45, expand=True)
    w = ramp.width
    ramp = ramp.crop((w * 0.15, w * 0.15, w * 0.85, w * 0.85)).resize((s, s))
    gold = Image.composite(Image.new("RGBA", (s, s), GOLD_DEEP + (255,)), Image.new("RGBA", (s, s), GOLD_LIGHT + (255,)), ramp)
    gold.putalpha(moon)
    out.alpha_composite(gold)

    # A small four-point sparkle in the crescent's opening.
    if detailed:
        sx, sy, sr = s * 0.70, s * 0.37, s * 0.075
        star = Image.new("L", (s, s), 0)
        d = ImageDraw.Draw(star)
        k = sr * 0.22
        d.polygon([(sx, sy - sr), (sx + k, sy - k), (sx + sr, sy), (sx + k, sy + k), (sx, sy + sr), (sx - k, sy + k), (sx - sr, sy), (sx - k, sy - k)], fill=255)
        sparkle = Image.new("RGBA", (s, s), (255, 244, 214, 255))
        sparkle.putalpha(star)
        out.alpha_composite(sparkle)

    # A thin light rim inside the squircle edge.
    rim = ImageChops.subtract(shape, shape.filter(ImageFilter.MinFilter(max(3, (s // 160) | 1))))
    rim_layer = Image.new("RGBA", (s, s), (255, 255, 255, 0))
    rim_layer.putalpha(rim.point(lambda v: int(v * 0.10)))
    out.alpha_composite(rim_layer)

    return out.resize((px, px), Image.Resampling.LANCZOS)


def write_ico(path, images):
    """ICO with PNG-compressed entries (supported since Windows Vista)."""
    blobs = []
    for im in images:
        buf = io.BytesIO()
        im.save(buf, "PNG")
        blobs.append(buf.getvalue())
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    for im, blob in zip(images, blobs):
        w = 0 if im.width >= 256 else im.width
        entries += struct.pack("<BBBBHHII", w, w, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    path.write_bytes(header + entries + b"".join(blobs))


def main():
    sizes = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
    images = [render(n, detailed=n > 24) for n in sizes]
    write_ico(HERE / "hilal.ico", images)
    for n in (512, 1024):
        render(n).save(HERE / f"logo-{n}.png")
    # A preview strip of every size, for checking by eye.
    strip = Image.new("RGBA", (sum(sizes) + 12 * len(sizes) + 12, 280), (40, 40, 46, 255))
    x = 12
    for im in images:
        strip.alpha_composite(im, (x, 280 // 2 - im.height // 2))
        x += im.width + 12
    strip.save(HERE / "preview-sizes.png")
    print("wrote", HERE / "hilal.ico")


if __name__ == "__main__":
    main()
