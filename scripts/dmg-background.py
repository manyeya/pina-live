"""The picture behind Pina Live's disk image window (640 by 400, and twice that for Retina): its
dark, its name and version, a glow and rings round where "Install Pina Live" sits, a plate under its
name (mid grey, so the Finder's label reads in light mode and dark), "Double-click to install",
and two decks' waveforms scrolling past a playhead along the bottom, as in Pina Live. Unsigned
builds say how to open it anyway.

    python3 scripts/dmg-background.py <folder> <version> [--unsigned]

writes background.png and background@2x.png there. Drawn four times over and scaled down, so
its edges are smooth.
"""
import math, os, sys

from PIL import Image, ImageDraw, ImageFilter, ImageFont

W, H = 640, 400
# Where the installer's icon sits (its centre) and how big: scripts/dmg-settings.py says the same.
ICON = (320, 168)
ICON_SIZE = 128
K = 4  # drawn at 4x, scaled down

BG = (12, 12, 15)
TEXT = (244, 244, 246)
TEXT_2 = (212, 212, 218)
TEXT_3 = (138, 138, 149)
ACCENT = (91, 140, 255)
CORAL = (255, 122, 89)
CURVE = (255, 209, 102)
PLATE = (110, 110, 120)

FONTS = os.path.join(os.path.dirname(__file__), "..", "crates", "app", "assets", "fonts")


def font(name, size):
    return ImageFont.truetype(os.path.join(FONTS, name), int(size * K))


def s(v):
    return v * K


def draw(version, unsigned):
    img = Image.new("RGB", (W * K, H * K), BG)
    # A soft glow round the icon, and a fainter one from the top left.
    glow = Image.new("RGBA", img.size, (0, 0, 0, 0))
    g = ImageDraw.Draw(glow)
    cx, cy = ICON
    g.ellipse([s(cx - 120), s(cy - 120), s(cx + 120), s(cy + 120)], fill=ACCENT + (70,))
    g.ellipse([s(-140), s(-180), s(220), s(120)], fill=ACCENT + (26,))
    glow = glow.filter(ImageFilter.GaussianBlur(s(60)))
    img.paste(glow, (0, 0), glow)
    d = ImageDraw.Draw(img, "RGBA")
    # Rings round it, fading out.
    for i, r in enumerate([92, 116, 142]):
        a = [42, 26, 14][i]
        d.ellipse([s(cx - r), s(cy - r), s(cx + r), s(cy + r)], outline=ACCENT + (a,), width=s(1))
    # Two decks' waveforms along the bottom, A over B, mirrored about each lane's middle, their
    # beats marked, the playhead through both.
    lanes = [(316, ACCENT, 0.0), (362, CORAL, 1.7)]
    for mid, color, seed in lanes:
        for i in range(0, W * 2):
            x = i / 2
            u = x / W
            env = 0.35 + 0.65 * abs(math.sin(u * 9.0 + seed)) ** 1.5
            hit = 0.55 + 0.45 * abs(math.sin(u * 63.0 + seed * 3)) ** 6
            a = 20 * env * hit * (0.6 + 0.4 * math.sin(u * 211 + seed * 5) ** 2)
            played = x < W / 2
            d.line(
                [(s(x), s(mid - a)), (s(x), s(mid + a))],
                fill=color + (70 if played else 160,),
                width=max(1, int(K / 2)),
            )
        for k in range(0, 17):
            bx = W * k / 16
            d.line([(s(bx), s(mid - 24)), (s(bx), s(mid - 20))], fill=(255, 255, 255, 40), width=s(1))
    # The playhead, glowing.
    layer = Image.new("L", img.size, 0)
    ImageDraw.Draw(layer).line([(s(W / 2), s(290)), (s(W / 2), s(H))], fill=255, width=s(6))
    layer = layer.filter(ImageFilter.GaussianBlur(s(6) / K))
    img.paste(Image.new("RGB", img.size, (255, 255, 255)), (0, 0), layer.point(lambda v: int(v * 0.25)))
    d = ImageDraw.Draw(img, "RGBA")
    d.line([(s(W / 2), s(290)), (s(W / 2), s(H))], fill=(255, 255, 255, 230), width=int(s(1.5)))
    # Grid lines, faint.
    for k in range(1, 8):
        x = W * k / 8
        d.line([(s(x), s(290)), (s(x), s(H))], fill=(255, 255, 255, 8), width=s(1))
    # Pina Live, and its version.
    bold = font("Geist-Bold.ttf", 20)
    d.text((s(28), s(24)), "Pina Live", font=bold, fill=TEXT)
    tw = d.textlength("Pina Live", font=bold)
    d.text((s(28) + tw + s(8), s(31)), version, font=font("GeistMono-Regular.ttf", 11), fill=TEXT_3)
    # The plate the Finder's label sits on.
    py = cy + ICON_SIZE / 2 + 12
    d.rounded_rectangle(
        [s(cx - 78), s(py - 14), s(cx + 78), s(py + 14)],
        radius=s(14),
        fill=PLATE + (255,),
        outline=(255, 255, 255, 30),
        width=s(1),
    )
    # What to do.
    say = "Double-click to install"
    f = font("Geist-Medium.ttf", 13)
    d.text((s(cx) - d.textlength(say, font=f) / 2, s(py + 26)), say, font=f, fill=TEXT_2)
    if unsigned:
        note = "If macOS won't open it: System Settings › Privacy & Security › Open Anyway"
        f = font("Geist-Regular.ttf", 10.5)
        d.text((s(W / 2) - d.textlength(note, font=f) / 2, s(H - 22)), note, font=f, fill=TEXT_3)
    return img


def main():
    out, version = sys.argv[1], sys.argv[2]
    unsigned = "--unsigned" in sys.argv
    big = draw(version, unsigned)
    big.resize((W * 2, H * 2), Image.LANCZOS).save(os.path.join(out, "background@2x.png"))
    big.resize((W, H), Image.LANCZOS).save(os.path.join(out, "background.png"))


if __name__ == "__main__":
    main()
