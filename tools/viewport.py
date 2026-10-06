#!/usr/bin/env python3
"""Test renderer for the 3D viewport (walls, floor, ceiling). See docs/04-rendering.md.

  viewport.py MAP X Y DIR [out.png]   render one view to re/vp/ (default name)
  viewport.py walk MAP X Y DIR N      render N steps forward

Output goes under re/ (gitignored): it is built from the local game data.
This is a verification tool, not the engine. It draws walls, floor and
ceiling, pits, ceiling holes, stairs, doors (closed, destroyed or partly open
when the panel slides vertically) and floor items. Ornaments, creatures,
missiles, split doors and teleporter fields are documented but not drawn.
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


def scale_src(src, sx, sy):
    """Nearest-neighbour scaler (0x1424B): sizes via (v*s + s/2) >> 6, then
    source column (S + 2*S*i) >> 8 with S = (w << 7) // new_w and source row
    (T//2 + j*T) >> 7 with T = (h << 7) // new_h."""
    nw, nh = (src.w * sx + sx // 2) >> 6, (src.h * sy + sy // 2) >> 6
    if nw <= 0 or nh <= 0:
        return None
    S, T = (src.w << 7) // nw, (src.h << 7) // nh
    cols = [min(src.w - 1, (S + 2 * S * i) >> 8) for i in range(nw)]
    rows = [min(src.h - 1, (T // 2 + j * T) >> 7) for j in range(nh)]
    out = Src.__new__(Src)
    out.w, out.h, out.cmap = nw, nh, src.cmap
    out.px = [src.px[r * src.w + c] for r in rows for c in cols]
    out.off = ((src.off[0] * sx + sx // 2) >> 6, (src.off[1] * sy + sy // 2) >> 6)
    return out


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


# Per-cell tables from SKULL.EXE (see docs/04 "Cell content").
DEPTH = [0, 0, 0, 1, 1, 1, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4]
DEPTH_SCALE = [96, 64, 43, 28, 19]
PIT_LAYOUT = [862, 861, 863, 859, 858, 860, 856, 855, 857, -1, -1, 853, 852, 854, 850, 851]
PIT_FLIP = [0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1]
PIT_SUB = [107, 108, 108, 110, 111, 111, 113, 114, 114, -1, -1, 118, 119, 119, 121, 121]
HOLE_LAYOUT = [871, 870, 872, 868, 867, 869, 865, 864, 866]
HOLE_SUB = [153, 154, 154, 156, 157, 157, 159, 160, 160]
HOLE_FLIP = [0, 0, 1, 0, 0, 1, 0, 0, 1]
STAIR_FRONT_SUB = [-1, -1, -1, -1, -1, -1, 79, 59, 80, 60, 81, 61, 82, 62, 83, 63, 84, 64,
                   -1, -1, -1, -1, 85, 65, 86, 66, 87, 67, 88, 68, 89, 69]
STAIR_FRONT_ALT = [-1, -1, -1, -1, -1, -1, 79, 59, 80, 60, 80, 60, 82, 62, 83, 63, 83, 63,
                   -1, -1, -1, -1, 85, 65, 86, 66, 86, 66, 88, 68, 88, 68]
STAIR_FRONT_LAYOUT = [-1, -1, -1, -1, -1, -1, 822, 809, 821, 808, 823, 810, 819, 806, 818,
                      805, 820, 807, -1, -1, -1, -1, 816, 803, 815, 802, 817, 804, 800, 800,
                      801, 801]
STAIR_SIDE_SUB = [-1, -1, 205, 199, 206, 200, -1, -1, 207, 201, 208, 202, -1, -1, 209, 203,
                  210, 204]
STAIR_SIDE_LAYOUT = [-1, -1, 832, 832, 833, 833, -1, -1, 830, 828, 831, 829, -1, -1, 826,
                     826, 827, -1]
DOOR_LAYOUT = [3810, -1, -1, 3790, 3780, 3800, 3760, 3750, 3770, -1, -1, 3730, 3720, 3740,
               3700, 3710]
DOOR_CELLS = {0, 3, 4, 5, 6, 7, 8, 11, 12, 13, 14, 15}
ITEM_SCALE = [87, 78, 71, 64, 58, 52, 47, 43, 39, 35, 31, 28, 26, 23, 21, 19, 17, 15]
QUAD_SLOT = [6, 8, 18, 16]  # item quadrant (relative to facing) -> 5x5 slot


def try_src(g, cat, idx, sub):
    try:
        return Src(g, cat, idx, sub)
    except KeyError:
        return None


def item_key(dg, ref):
    """(category, index) of a floor item, per docs/09 (types 5-10 only)."""
    t, i = ref >> 10 & 15, ref & 0x3FF
    rec = dg.things[t][i]
    w1 = struct.unpack_from('<H', rec, 2)[0]
    if t in (5, 6, 10):
        idx = w1 & 0x7F
    elif t == 7:
        idx = 0
    elif t == 8:
        idx = (w1 >> 8) & 0x7F
    else:
        w2 = struct.unpack_from('<H', rec, 4)[0]
        idx = (w2 >> 13) | ((w2 >> 1) & 3) << 3
    return 16 + t - 5, idx


def draw_cell_content(buf, g, dg, recs, m, c, x, y, d, par):
    """Pits, stairs, doors and floor items for one cell (cells 0-15)."""
    md = dg.maps[m]
    ts = md.tileset
    sq = dg.square(m, x, y)
    e = sq >> 5
    depth = DEPTH[c]
    if e == 2 and sq & 8 and PIT_SUB[c] >= 0:
        flip = PIT_FLIP[c] if c else par
        s = try_src(g, 8, ts, PIT_SUB[c])
        if s:
            blit(buf, VP_W, VP_H, s, recs, PIT_LAYOUT[c], flip)
    if e == 3:
        front = ((sq >> 3) & 1) != (d & 1)
        k = c * 2 + ((sq >> 2) & 1)
        if front and k < len(STAIR_FRONT_SUB) and STAIR_FRONT_SUB[k] >= 0:
            s, flip = try_src(g, 8, ts, STAIR_FRONT_SUB[k]), 0
            if s is None:
                s, flip = try_src(g, 8, ts, STAIR_FRONT_ALT[k]), 1
            if s:
                blit(buf, VP_W, VP_H, s, recs, STAIR_FRONT_LAYOUT[k], flip)
        elif not front and k < len(STAIR_SIDE_SUB) and STAIR_SIDE_SUB[k] >= 0:
            s = try_src(g, 8, ts, STAIR_SIDE_SUB[k])
            if s:
                blit(buf, VP_W, VP_H, s, recs, STAIR_SIDE_LAYOUT[k])
    # floor items
    for ref in dg.things_at(m, x, y):
        if not 5 <= (ref >> 10 & 15) <= 10:
            continue
        slot = QUAD_SLOT[((ref >> 14) - d) & 3]
        row = slot // 5
        if c == 0 and 4 - row < 2:
            continue  # behind the party
        cat, idx = item_key(dg, ref)
        s = try_src(g, cat, idx, 0)
        if s is None:
            continue
        sc = ITEM_SCALE[depth * 4 + 4 - row]
        s = scale_src(s, sc, sc)
        if s:
            key = g.lookup(cat, idx, 11, 4)
            blit(buf, VP_W, VP_H, s, recs, 5000 + 25 * c + slot, 0, 10 if key is None else key)
    # door across the view
    if e == 4 and c in DOOR_CELLS and ((sq >> 3) & 1) != (d & 1):
        state = sq & 7
        door = next((r for r in dg.things_at(m, x, y) if (r >> 10 & 15) == 0), None)
        if state == 0 or door is None:
            return
        w1 = struct.unpack_from('<H', dg.things[0][door & 0x3FF], 2)[0]
        we = struct.unpack_from('<H', md.raw, 14)[0]
        dtype = (we >> (12 if w1 & 1 else 8)) & 15
        key = g.lookup(14, dtype, 11, 4) or 10
        s = try_src(g, 14, dtype, depth - 1) if depth else None
        if s is None:
            s = try_src(g, 14, dtype, 0)
            if s is None:
                return
            sc = 0x71 if depth == 0 else DEPTH_SCALE[depth]
            s = scale_src(s, sc, sc)
        if s is None:
            return
        base = DOOR_LAYOUT[c]
        if state < 4 and not (w1 & 0x20):
            return  # split doors (two half panels) not modelled here
        rid = base + state if state < 4 else base
        blit(buf, VP_W, VP_H, s, recs, rid, 0, key)


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
            if c < 16:
                if c < 9 and lower_hole(dg, m, x, y):
                    s = try_src(g, 8, tileset, HOLE_SUB[c])
                    if s:
                        blit(buf, VP_W, VP_H, s, recs, HOLE_LAYOUT[c], HOLE_FLIP[c] if c else par)
                draw_cell_content(buf, g, dg, recs, m, c, x, y, d, par)
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
    lat, fwd = CELLS[0]
    draw_cell_content(buf, g, dg, recs, m, 0, px, py, d, par)
    return buf


def lower_hole(dg, m, x, y):
    """Is there an open pit in the square above (same coordinates, one layer
    up)? 0x507AB checks this only when the set's flag word has bit 0."""
    md = dg.maps[m]
    gx, gy = x + md.origin_x, y + md.origin_y
    for j, o in enumerate(dg.maps):
        if o.depth == md.depth - 1 and 0 <= gx - o.origin_x < o.width and \
                0 <= gy - o.origin_y < o.height:
            sq = dg.square(j, gx - o.origin_x, gy - o.origin_y)
            return sq >> 5 == 2 and bool(sq & 8)
    return False


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
