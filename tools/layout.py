#!/usr/bin/env python3
"""Screen layout table, GRAPHICS.DAT key (1,0,4,0). See docs/04-rendering.md.

  layout.py ranges            list the id ranges
  layout.py show ID [W H]     parent chain and resolved placement for a WxH object
  layout.py rects             every kind-9 rectangle with its placed position

The resolver is a transcription of SKULL.EXE 0x1936F (placement) with the
record decoding of 0x190E9; both are described in docs/04-rendering.md.
"""
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from gdat import Gdat  # noqa: E402

MAGIC = 0xFC0D
RECT, TRANSLATE = 9, 1


def load(g=None):
    """Return (ranges, {id: (kind, parent, x, y)}) from entry (1,0,4,0)."""
    g = g or Gdat()
    d = g.entry(g.lookup(1, 0, 4, 0))
    magic, n = struct.unpack_from('<HH', d, 0)
    assert magic == MAGIC, hex(magic)
    ranges = [struct.unpack_from('<hh', d, 4 + 4 * i) for i in range(n)]
    recs, off = {}, 4 + 4 * n
    for lo, hi in ranges:
        for i in range(lo, hi + 1):
            recs[i] = struct.unpack_from('<hhhh', d, off)
            off += 8
    assert off == len(d), (off, len(d))
    return ranges, recs


def box_origin(anchor, px, py, w, h):
    """Top-left of a w*h box whose anchor point `anchor` (0-8) is at (px, py).

    Anchors: 1 TL, 5 TC, 2 TR, 8 ML, 0 C, 6 MR, 4 BL, 7 BC, 3 BR."""
    hw, hh = (w + 1) >> 1, (h + 1) >> 1
    dx = {0: hw, 1: 0, 2: w - 1, 3: w - 1, 4: 0, 5: hw, 6: w - 1, 7: hw, 8: 0}[anchor]
    dy = {0: hh, 1: 0, 2: 0, 3: h - 1, 4: h - 1, 5: 0, 6: hh, 7: h - 1, 8: hh}[anchor]
    return px - dx, py - dy


def anchor_point(anchor, w, h):
    """Offset of anchor point `anchor` (0-8) inside a w*h box (kinds 10-18)."""
    hw, hh = (w + 1) >> 1, (h + 1) >> 1
    dx = {0: hw, 1: 0, 2: w - 1, 3: w - 1, 4: 0, 5: hw, 6: w - 1, 7: hw, 8: 0}[anchor]
    dy = {0: hh, 1: 0, 2: 0, 3: h - 1, 4: h - 1, 5: 0, 6: hh, 7: h - 1, 8: hh}[anchor]
    return dx, dy


class Placement(tuple):
    """(x, y, w, h, skip_x, skip_y, last_rect) — visible destination box,
    how many source pixels were clipped off the left/top, and the id of the
    last rectangle passed while climbing the chain."""


def resolve(recs, rid, w=0, h=0, img=(320, 200), anchor=None):
    """Place a w*h object at layout id `rid`. Returns a Placement or None.

    w/h of 0 mean the size of the source bitmap `img` (the game reads it from
    the bitmap header). If bit 15 of rid is set, w and h are instead extra
    x/y offsets for the reference point, and the size comes from `img`.
    `anchor` overrides the record's own kind, as the game's sixth argument does.
    """
    extra = bool(rid & 0x8000)
    rec = recs.get(rid & 0x7FFF)
    if rec is None:
        return None
    kind = rec[0] if anchor is None else anchor
    if kind < 9:
        ox, oy = rec[2], rec[3]
    elif kind == 9:
        return None
    else:
        ox, oy = 0, 0
        kind -= 10
    if extra:
        ox += w; oy += h
        w = h = 0
    cx, cy, cw, ch = -10000, -10000, 20000, 20000
    pending = False
    last = 0
    cur = rec if anchor is None else (anchor,) + tuple(rec[1:])
    while cur[1] != 0:
        ck = cur[0]
        if ck < 10 or ck > 18:
            par = recs.get(cur[1])
            if par is None:
                break
            last = cur[1]
            pk, pw, ph = par[0], par[2], par[3]
            if pk == TRANSLATE:
                ox += pw; oy += ph; cx += pw; cy += ph
                cur = par
                continue
            if pk == RECT:
                if ck <= 8:
                    if ck == TRANSLATE:
                        rx, ry = cur[2], cur[3]
                    else:
                        rx, ry = box_origin(ck, cur[2], cur[3], pw, ph)
                else:
                    rx, ry = pw, ph  # rect directly in rect: not present in this data
                if pending:
                    pending = False
                    ox += rx; oy += ry; cx += rx; cy += ry
                # intersect clip with the placed rectangle
                if cx < rx:
                    cx = rx  # (the game keeps cw; the right edge is fixed below)
                if rx + pw <= cx + cw - 1:
                    cw = pw - cx + rx
                if cy < ry:
                    cy = ry
                if ry + ph <= cy + ch - 1:
                    ch = ry + ph - cy
                cur = par
            else:
                cur = par
                if pk < 9:
                    pending = True
        else:
            par = recs.get(cur[1])
            if par is None:
                break
            rect = recs.get(par[1])
            if rect is None:
                break
            last = par[1]
            rw, rh = rect[2], rect[3]
            rx, ry = (par[2], par[3]) if par[0] == TRANSLATE else \
                box_origin(par[0], par[2], par[3], rw, rh)
            cx += rx
            if cx < rx:
                cx = rx
            if rx + rw <= cx + cw - 1:
                cw = rw - cx + rx
            cy += ry
            if cy < ry:
                cy = ry
            if ry + rh <= cy + ch - 1:
                ch = ry + rh - cy
            ax, ay = anchor_point(ck - 10, rw, rh)
            ox += rx + ax + cur[2]
            oy += ry + ay + cur[3]
            cur = rect
    if w == 0:
        w = img[0]
    if h == 0:
        h = img[1]
    if kind > 8:
        return None
    x, y = box_origin(kind, ox, oy, w, h)
    skip_x = skip_y = 0
    d = cx - x
    if d < 1:
        avail_w = cw + d
    else:
        skip_x, x, w, avail_w = d, cx, w - d, cw
    w = min(w, avail_w)
    d = cy - y
    if d < 1:
        avail_h = ch + d
    else:
        skip_y, y, h, avail_h = d, cy, h - d, ch
    h = min(h, avail_h)
    if w <= 0 or h <= 0:
        return None
    return Placement((x, y, w, h, skip_x, skip_y, last))


def rect_box(recs, rid):
    """Where kind-9 rectangle `rid` lands: resolve a full-size top-left
    placement through a record that uses it as its parent, if one exists."""
    for k, r in recs.items():
        if r[1] == rid and r[0] == TRANSLATE:
            p = resolve(recs, k, recs[rid][2], recs[rid][3], anchor=1)
            if p:
                return p
    return None


def main():
    ranges, recs = load()
    cmd = sys.argv[1] if len(sys.argv) > 1 else 'ranges'
    if cmd == 'ranges':
        print(f'{len(ranges)} ranges, {len(recs)} records')
        for lo, hi in ranges:
            print(f'{lo:6}-{hi:6} ({hi - lo + 1})')
    elif cmd == 'show':
        rid = int(sys.argv[2], 0)
        w = int(sys.argv[3]) if len(sys.argv) > 3 else 0
        h = int(sys.argv[4]) if len(sys.argv) > 4 else 0
        cur, seen = rid & 0x7FFF, 0
        while cur and cur in recs and seen < 20:
            r = recs[cur]
            print(f'  {cur:6}: kind {r[0]:3} parent {r[1]:6} x {r[2]:6} y {r[3]:6}')
            cur, seen = r[1], seen + 1
        print('  placement:', resolve(recs, rid, w, h))
    elif cmd == 'rects':
        for rid in sorted(recs):
            if recs[rid][0] == RECT:
                print(rid, recs[rid], '->', rect_box(recs, rid))


if __name__ == '__main__':
    main()
