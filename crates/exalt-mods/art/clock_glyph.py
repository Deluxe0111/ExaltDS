"""Draws the Recall Fate clock glyph (original art) in the style of Awakening's menu icons: a cream
glyph, lighter at the top, with a dark rim. Writes clock-32.png (for the game's 32x32 cell) and
clock-128.png (for the HD pack's 4x sheet), transparent around the glyph. tools/naga-tear puts it
on the game's own icon tile, recoloured blue.

Usage: python tools/naga-tear/art/clock_glyph.py   (writes next to this file; needs Pillow)
"""
import math
import os
from PIL import Image, ImageChops, ImageDraw, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
TOP, BOTTOM = (238, 221, 187), (221, 204, 153)  # the glyph cream of the game's icons
RIM = (34, 17, 0)


def mask(size, scale):
    """the clock's shape at size*scale: a ring with a gap at the upper left, an arrowhead turning
    it back (counter-clockwise), the hands at twelve and nine"""
    s = size * scale / 32.0
    m = Image.new('L', (size * scale,) * 2, 0)
    d = ImageDraw.Draw(m)
    cx, cy, ro, ri = 16 * s, 16.5 * s, 8.0 * s, 5.8 * s
    # the ring, open from 205 to 245 degrees (PIL angles run clockwise from 3 o'clock)
    d.pieslice((cx - ro, cy - ro, cx + ro, cy + ro), 245, 205 + 360, fill=255)
    d.ellipse((cx - ri, cy - ri, cx + ri, cy + ri), fill=0)
    # the arrowhead at the ring's end at 205 degrees, pointing on round (counter-clockwise)
    a = math.radians(205)
    rm = (ro + ri) / 2
    px, py = cx + rm * math.cos(a), cy + rm * math.sin(a)
    tx, ty = math.sin(a), -math.cos(a)  # the way round, counter-clockwise on screen
    nx, ny = math.cos(a), math.sin(a)
    w, l = 3.4 * s, 3.6 * s
    d.polygon([(px + nx * w - tx * 0.6 * s, py + ny * w - ty * 0.6 * s),
               (px - nx * w - tx * 0.6 * s, py - ny * w - ty * 0.6 * s),
               (px + tx * l, py + ty * l)], fill=255)
    # hands and the middle
    hw = 1.7 * s
    d.line((cx, cy, cx, cy - 4.2 * s), fill=255, width=round(hw))
    d.line((cx, cy, cx - 3.4 * s, cy), fill=255, width=round(hw))
    r = 1.3 * s
    d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=255)
    return m


def glyph(size, scale, rim_px):
    big = mask(size, scale)
    shape = big.resize((size, size), Image.LANCZOS)
    rim = big.filter(ImageFilter.MaxFilter(2 * round(rim_px * scale) + 1)).resize((size, size), Image.LANCZOS)
    if size > 32:
        rim = rim.filter(ImageFilter.GaussianBlur(size / 64))
    grad = Image.new('RGB', (size, size))
    for y in range(size):
        t = min(1.0, max(0.0, (y / size - 0.3) / 0.45))
        grad.paste(tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM)), (0, y, size, y + 1))
    out = Image.new('RGBA', (size, size), RIM + (0,))
    out.putalpha(rim.point(lambda v: v * 150 // 255))
    face = grad.convert('RGBA')
    face.putalpha(shape)
    out.alpha_composite(face)
    return out


if __name__ == '__main__':
    glyph(32, 16, 0.8).save(os.path.join(HERE, 'clock-32.png'))
    glyph(128, 8, 0.8).save(os.path.join(HERE, 'clock-128.png'))
