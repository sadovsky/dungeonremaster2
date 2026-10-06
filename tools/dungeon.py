#!/usr/bin/env python3
"""DUNGEON.DAT reader (DM2 PC). See docs/03-dungeon-dat.md.

  dungeon.py summary          header, maps, section sizes, checksum
  dungeon.py map N            ASCII render of map N
  dungeon.py things TYPE      field dump of one thing type (number or name)
"""
import struct
import sys
from pathlib import Path

DEFAULT = Path(__file__).resolve().parent.parent / 'original/dumast2/DATA/DUNGEON.DAT'

# Record size per thing type, from the table at 0x71808 in SKULL.EXE.
THING_SIZES = [4, 6, 4, 8, 16, 4, 4, 4, 4, 8, 4, 0, 0, 0, 8, 4]
# Spare records the game appends at runtime (table at 0x71818).
THING_SPARES = [0, 0, 0, 0, 75, 100, 60, 0, 12, 5, 200, 0, 0, 0, 60, 50]
THING_NAMES = ['door', 'teleporter', 'text', 'actuator', 'creature', 'weapon',
               'clothing', 'scroll', 'potion', 'container', 'misc', 'type11',
               'type12', 'type13', 'missile', 'cloud']
ELEMENTS = ['wall', 'floor', 'pit', 'stairs', 'door', 'teleporter', 'trickwall', 'rock']

END = 0xFFFE   # end of a thing list
NONE = 0xFFFF  # empty slot


def u16(b, o):
    return struct.unpack_from('<H', b, o)[0]


def thing_type(ref):
    return (ref >> 10) & 0x0F


def thing_index(ref):
    return ref & 0x03FF


def thing_cell(ref):
    return ref >> 14


class MapDesc:
    def __init__(self, raw):
        self.raw = raw
        self.data_offset = u16(raw, 0)
        self.flags = u16(raw, 2)
        self.flags2 = u16(raw, 4)
        self.origin_x, self.origin_y = raw[6], raw[7]
        w8 = u16(raw, 8)
        self.depth = w8 & 0x3F
        self.width = ((w8 >> 6) & 0x1F) + 1
        self.height = (w8 >> 11) + 1
        wa, wc, we = u16(raw, 10), u16(raw, 12), u16(raw, 14)
        self.wall_orn_count = wa & 0x0F
        self.floor_orn_count = (wa >> 8) & 0x0F
        self.door_orn_count = wc & 0x0F
        self.creature_type_count = (wc >> 4) & 0x0F
        self.difficulty = wc >> 12
        self.tileset = (we >> 4) & 0x0F
        self.door_type0 = (we >> 8) & 0x0F if self.flags & 0x80 else None
        self.door_type1 = we >> 12 if self.flags & 0x100 else None
        self.unknown_e_lo = we & 0x0F

    def list_bytes(self):
        return (self.creature_type_count + self.wall_orn_count
                + self.floor_orn_count + self.door_orn_count)


class Dungeon:
    def __init__(self, path=DEFAULT):
        self.d = d = Path(path).read_bytes()
        if u16(d, 0) == 0x8104:
            raise ValueError('compressed dungeon (signature 0x8104) is not handled')
        (self.seed, self.map_data_size, self.map_count, _, self.text_words,
         self.start, self.object_list_words) = struct.unpack_from('<HHBBHHH', d, 0)
        self.counts = list(struct.unpack_from('<16H', d, 12))
        o = 44
        self.maps = [MapDesc(d[o + 16 * i:o + 16 * i + 16]) for i in range(self.map_count)]
        o += 16 * self.map_count
        ncols = sum(m.width for m in self.maps)
        self.column_first = list(struct.unpack_from(f'<{ncols}H', d, o)); o += 2 * ncols
        self.object_list = list(struct.unpack_from(f'<{self.object_list_words}H', d, o))
        o += 2 * self.object_list_words
        self.text = list(struct.unpack_from(f'<{self.text_words}H', d, o)); o += 2 * self.text_words
        self.things = []
        for t in range(16):
            size = THING_SIZES[t]
            self.things.append([d[o + size * i:o + size * (i + 1)] for i in range(self.counts[t])])
            o += size * self.counts[t]
        self.map_data = d[o:o + self.map_data_size]; o += self.map_data_size
        self.trailer_offset = o
        self.checksum = u16(d, o) if o + 2 <= len(d) else None
        self.computed_checksum = sum(d[:o]) & 0xFFFF
        assert o + 2 == len(d), (o, len(d))
        col = 0
        for m in self.maps:
            m.first_column = col
            col += m.width

    # -- squares -----------------------------------------------------------
    def square(self, m, x, y):
        md = self.maps[m]
        if not (0 <= x < md.width and 0 <= y < md.height):
            return 0xE0  # what the game returns outside the map (solid rock)
        return self.map_data[md.data_offset + x * md.height + y]

    def map_lists(self, m):
        """Per-map byte lists stored right after the squares."""
        md = self.maps[m]
        o = md.data_offset + md.width * md.height
        out = {}
        for name, n in (('creature_types', md.creature_type_count),
                        ('wall_ornaments', md.wall_orn_count),
                        ('floor_ornaments', md.floor_orn_count),
                        ('door_ornaments', md.door_orn_count)):
            out[name] = list(self.map_data[o:o + n]); o += n
        return out

    def first_thing(self, m, x, y):
        md = self.maps[m]
        sq = self.square(m, x, y)
        if not (0 <= x < md.width and 0 <= y < md.height) or not sq & 0x10:
            return END
        idx = self.column_first[md.first_column + x]
        col = md.data_offset + x * md.height
        idx += sum(1 for b in self.map_data[col:col + y] if b & 0x10)
        return self.object_list[idx]

    def things_at(self, m, x, y):
        ref, out = self.first_thing(m, x, y), []
        while ref not in (END, NONE) and len(out) < 1000:
            out.append(ref)
            rec = self.things[thing_type(ref)][thing_index(ref)]
            ref = u16(rec, 0)
        return out


def render_map(dg, m):
    md = dg.maps[m]
    glyph = {0: '#', 1: '.', 2: 'o', 3: '>', 4: '|', 5: 'T', 6: '?', 7: ' '}
    lines = []
    for y in range(md.height):
        row = ''
        for x in range(md.width):
            sq = dg.square(m, x, y)
            el = sq >> 5
            g = glyph[el]
            if el == 2 and sq & 0x08:
                g = 'O'  # open pit
            if el == 3:
                g = '<' if sq & 0x04 else '>'
            if el == 4 and sq & 0x08:
                g = '-'
            if sq & 0x10 and g == '.':
                g = '+'
            row += g
        lines.append(row)
    return '\n'.join(lines)


def main():
    dg = Dungeon()
    cmd = sys.argv[1] if len(sys.argv) > 1 else 'summary'
    if cmd == 'summary':
        sx, sy, sd = dg.start & 0x1F, (dg.start >> 5) & 0x1F, (dg.start >> 10) & 3
        print(f'seed {dg.seed}  maps {dg.map_count}  start map 0 x{sx} y{sy} dir {sd}')
        print(f'map data {dg.map_data_size} B, object list {dg.object_list_words} words '
              f'({dg.object_list.count(NONE)} spare), text {dg.text_words} words')
        print('things: ' + ', '.join(f'{THING_NAMES[t]} {c}' for t, c in enumerate(dg.counts) if c))
        ok = 'ok' if dg.checksum == dg.computed_checksum else 'MISMATCH'
        print(f'checksum {dg.checksum:#06x} (byte sum {dg.computed_checksum:#06x}) {ok}')
        print(' map depth  size   origin  tileset diff  cre wall floor door  flags')
        for i, m in enumerate(dg.maps):
            print(f'{i:4} {m.depth:5} {m.width:3}x{m.height:<3} {m.origin_x:3},{m.origin_y:<3} '
                  f'{m.tileset:6} {m.difficulty:5} {m.creature_type_count:4} {m.wall_orn_count:4} '
                  f'{m.floor_orn_count:5} {m.door_orn_count:4}  {m.flags:#06x}')
    elif cmd == 'map':
        m = int(sys.argv[2])
        md = dg.maps[m]
        print(f'map {m}: {md.width}x{md.height} depth {md.depth} tileset {md.tileset}  '
              f'lists {dg.map_lists(m)}')
        print(render_map(dg, m))
        print("legend: # wall  . floor  + floor with things  o/O pit closed/open  "
              "> < stairs  | door  - door (bit 3)  T teleporter  ? trickwall  ' ' rock")
    elif cmd == 'things':
        arg = sys.argv[2]
        t = int(arg) if arg.isdigit() else THING_NAMES.index(arg)
        for i, rec in enumerate(dg.things[t]):
            words = ' '.join(f'{u16(rec, k):04x}' for k in range(0, len(rec), 2))
            print(f'{THING_NAMES[t]} {i:4}: {words}')


if __name__ == '__main__':
    main()
