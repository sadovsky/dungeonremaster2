#!/usr/bin/env python3
"""Test renderer for the 3D viewport (walls, floor, ceiling). See docs/04-rendering.md.

  viewport.py MAP X Y DIR [out.png]   render one view to re/vp/ (default name)
  viewport.py walk MAP X Y DIR N      render N steps forward

Output goes under re/ (gitignored): it is built from the local game data.
This is a verification tool, not the engine; only the pieces documented as
'verified' in docs/04 are implemented (no ornaments, items or creatures).
"""
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gimg  # noqa: E402
import layout  # noqa: E402
from dungeon import Dungeon  # noqa: E402
from gdat import Gdat  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
VP_W, VP_H = 224, 136

# View cone: cell -> (lateral, forward). Lateral > 0 is to the party's right.
CELLS = [(0, 0), (-1, 0), (1, 0), (0, 1), (-1, 1), (1, 1), (0, 2), (-1, 2), (1, 2),
         (-2, 2), (2, 2), (0, 3), (-1, 3), (1, 3), (-2, 3), (2, 3), (0, 4), (-1, 4),
         (1, 4), (-2, 4), (2, 4), (-3, 4), (3, 4)]
DRAW_ORDER = [19, 20, 17, 18, 16, 14, 15, 12, 13, 11, 9, 10, 7, 8, 6, 4, 5, 3, 1, 2]
SWAP = [0, 2, 1, 3, 5, 4, 6, 8, 7, 10, 9, 11, 13, 12, 15, 14]  # left<->right partner
DX, DY = (0, 1, 0, -1), (-1, 0, 1, 0)
LAYOUT_CEILING, LAYOUT_FLOOR, LAYOUT_WALL0 = 700, 701, 702


class Src:
    """A decoded image with raw values (nibbles for 4-bit) and a colour map."""
    def __init__(self, g, cat, idx, sub):
        e = g.lookup(cat, idx, 1, sub)
        if e is None:
            raise KeyError((cat, idx, sub))
        raw = g.entry(e)
        w0, w1 = struct.unpack_from('<HH', raw, 0)
        self.w, self.h = w0 & 0x3FF, w1 & 0x3FF
        htag, wtag = w1 >> 10, w0 >> 10
        if htag in (gimg.TAG_8BPP, gimg.TAG_RAW) and not (htag == gimg.TAG_RAW and
                                                           struct.unpack_from('<H', raw, 4)[0] == 4):
            img = gimg.decode(raw)
            self.px, self.cmap = img.pixels, None
        else:
            if htag == gimg.TAG_RAW:
                img = gimg.decode(raw)  # 4-bit raw: rare, use mapped values
                self.px, self.cmap = img.pixels, None
            else:
                self.px, _ = gimg.decode_4bpp(raw, self.w, self.h)
                self.cmap = raw[-16:]
        # Drawing offset (0x3EED3): attribute (cat,idx,12,sub) when the width
        # tag is 32; signed bytes 4/5 for compressed 8-bit; else the two tags.
        def s6(v):
            return v - 64 if v >= 32 else v
        if wtag == 32:
            v = g.lookup(cat, idx, 12, sub) or 0
            self.off = (s8(v >> 8), s8(v & 0xFF))
        elif htag == gimg.TAG_8BPP:
            self.off = (s8(raw[4]), s8(raw[5]))
        elif htag == gimg.TAG_RAW:
            self.off = (0, 0)
        else:
            self.off = (s6(wtag), s6(htag))
        base = g.lookup(cat, idx, 12, 0xFE) or 0
        self.off = (self.off[0] + s8(base >> 8), self.off[1] + s8(base & 0xFF))


def s8(v):
    v &= 0xFF
    return v - 256 if v >= 128 else v


def blit(dst, dw, dh, src, recs, rid, flip=0, key=-1):
    """Place src at layout id rid (applying its drawing offset) and copy it.

    Mirrors 0x1B8E5: a non-zero drawing offset is passed by setting bit 15 of
    the id with the offsets in place of the size; the size then comes from
    the source bitmap itself."""
    ox, oy = src.off
    if ox or oy:
        p = layout.resolve(recs, rid | 0x8000, ox, oy, img=(src.w, src.h))
    else:
        p = layout.resolve(recs, rid, src.w, src.h)
    if p is None:
        return None
    x, y, w, h, sx, sy, _ = p
    if x < 0 or y < 0 or x + w > dw or y + h > dh:
        # the viewport bitmap clips through rect 3 in the layout chain
        return None
    for row in range(h):
        srow = sy + row
        if flip & 2:
            srow = src.h - 1 - srow
        for col in range(w):
            scol = sx + col
            if flip & 1:
                scol = src.w - 1 - scol
            v = src.px[srow * src.w + scol]
            if v == key:
                continue
            dst[(y + row) * dw + x + col] = src.cmap[v] if src.cmap else v
    return p


def parity(dg, m, x, y, d):
    """Wall/floor alternation bit (0x54874): map layer + map origin + x + y + facing."""
    md = dg.maps[m]
    return (md.depth + md.origin_x + md.origin_y + x + y + d) & 1


def view_type(sq):
    """Square view type for walls only: True if drawn as a wall face."""
    e = sq >> 5
    return e == 0 or (e == 6 and not sq & 4)


def render(g, dg, recs, m, px, py, d):
    md = dg.maps[m]
    tileset = md.tileset
    par = parity(dg, m, px, py, d)
    key = g.lookup(8, tileset, 11, 100) or 0
    flags = g.lookup(8, tileset, 11, 0x65) or 0
    buf = bytearray(VP_W * VP_H)
    # ceiling (mode 0x20) and floor (mode 1) flips, simplified to the
    # parity-driven cases (time-based modes not modelled here).
    ceil_flip = (0 if par else 1) if flags & 2 and not flags & 4 else 0
    floor_flip = par if flags & 8 and not flags & 0x10 else 0
    for sub, rid, fl in ((1, LAYOUT_CEILING, ceil_flip), (0, LAYOUT_FLOOR, floor_flip)):
        try:
            blit(buf, VP_W, VP_H, Src(g, 8, tileset, sub), recs, rid, fl)
        except KeyError:
            pass
    for c in DRAW_ORDER:
        lat, fwd = CELLS[c]
        x = px + DX[d] * fwd + DX[(d + 1) & 3] * lat
        y = py + DY[d] * fwd + DY[(d + 1) & 3] * lat
        if not view_type(dg.square(m, x, y)):
            continue
        side = lat
        flip = 1 if side > 0 else 0
        if c >= 16:
            if side in (-2, 2):
                flip = 0
            flip ^= par
            sub = 50
        elif par == 0:
            sub = 34 + c
        else:
            sub = 34 + SWAP[c]  # no alternate (176+) art in this archive
            if side == 0:
                flip = 1
        try:
            blit(buf, VP_W, VP_H, Src(g, 8, tileset, sub), recs, LAYOUT_WALL0 + c, flip, key)
        except KeyError:
            pass
    return buf


def main():
    g, dg = Gdat(), Dungeon()
    _, recs = layout.load(g)
    pal = gimg.master_palette(g)
    out = ROOT / 're/vp'
    out.mkdir(parents=True, exist_ok=True)
    if sys.argv[1] == 'walk':
        m, x, y, d, n = map(int, sys.argv[2:7])
        for i in range(n):
            buf = render(g, dg, recs, m, x, y, d)
            gimg.write_png(out / f'walk_{m}_{i}.png', VP_W, VP_H, buf, pal)
            x += DX[d]; y += DY[d]
        return
    m, x, y, d = map(int, sys.argv[1:5])
    name = sys.argv[5] if len(sys.argv) > 5 else f'vp_{m}_{x}_{y}_{d}.png'
    gimg.write_png(out / name, VP_W, VP_H, render(g, dg, recs, m, x, y, d), pal)
    print(out / name)


if __name__ == '__main__':
    main()
