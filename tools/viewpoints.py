#!/usr/bin/env python3
"""Find party viewpoints that show each kind of feature, for comparing views
with the original game. Prints MAP X Y DIR per feature and per tileset.

  viewpoints.py
"""
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from dungeon import Dungeon  # noqa: E402
from gdat import Gdat  # noqa: E402

DX, DY = (0, 1, 0, -1), (-1, 0, 1, 0)


def main():
    dg, g = Dungeon(), Gdat()
    el = lambda m, x, y: dg.square(m, x, y) >> 5  # noqa: E731
    floor = lambda m, x, y: el(m, x, y) == 1  # noqa: E731

    def views(m, x, y, dist):
        for d in dist:
            for dr in range(4):
                vx, vy = x - DX[dr] * d, y - DY[dr] * d
                if floor(m, vx, vy) and all(floor(m, x - DX[dr] * k, y - DY[dr] * k) for k in range(1, d)):
                    yield (m, vx, vy, dr)

    found = {}

    def want(tag, m, x, y, dist):
        if tag not in found:
            for v in views(m, x, y, dist):
                found[tag] = v + ((x, y),)
                return

    tilesets = {}
    for m, md in enumerate(dg.maps):
        lists = dg.map_lists(m)
        for x in range(md.width):
            for y in range(md.height):
                e = el(m, x, y)
                ks = [((r >> 10) & 15, r) for r in dg.things_at(m, x, y)]
                types = {k for k, _ in ks}
                if e == 4:
                    want('door', m, x, y, (2,))
                elif e == 3:
                    want('stairs', m, x, y, (1, 2))
                elif e == 2:
                    want('pit', m, x, y, (1, 2))
                elif e == 5:
                    want('teleporter', m, x, y, (1, 2))
                elif e == 1 and 4 in types:
                    want('creature', m, x, y, (2,))
                elif e == 1 and types & {5, 6, 8, 9, 10}:
                    want('items', m, x, y, (1,))
                elif e == 0:
                    for k, r in ks:
                        rec = dg.things[k][r & 0x3FF]
                        if k == 2:
                            want('wall writing', m, x, y, (1,))
                        if k == 3:
                            slot = struct.unpack_from('<H', rec, 4)[0] >> 12
                            if slot and slot <= len(lists['wall_ornaments']):
                                orn = lists['wall_ornaments'][slot - 1]
                                kind = g.lookup(9, orn, 11, 10) or 0
                                if kind == 1:
                                    want('alcove', m, x, y, (1,))
                                if kind == 3:
                                    want('mirror', m, x, y, (1,))
                                want('wall ornament', m, x, y, (1,))
        if md.tileset not in tilesets:
            for x in range(md.width):
                for y in range(md.height):
                    for dr in range(4):
                        if md.tileset not in tilesets and floor(m, x, y) and all(
                                floor(m, x + DX[dr] * k, y + DY[dr] * k) for k in (1, 2)):
                            tilesets[md.tileset] = (m, x, y, dr)
    for k, v in sorted(found.items()):
        print(f'{k:14} {v[0]} {v[1]} {v[2]} {v[3]}   target {v[4]}')
    for k, v in sorted(tilesets.items()):
        print(f'tileset-{k:<6} {v[0]} {v[1]} {v[2]} {v[3]}')


if __name__ == '__main__':
    main()
