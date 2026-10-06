#!/usr/bin/env python3
"""GRAPHICS.DAT reader (DM2 PC, archive version 5). See docs/02-graphics-dat.md.

  gdat.py stats            category/type histogram of the index
  gdat.py keys [cat]       list index records
  gdat.py raw ENTRY out    dump one raw archive entry
"""
import collections
import struct
import sys
from pathlib import Path

DEFAULT = Path(__file__).resolve().parent.parent / 'original/dumast2/DATA/GRAPHICS.DAT'

# Types whose value is a plain number rather than an entry number.
VALUE_TYPES = {0x0B, 0x0C}

# Working labels; see "Categories and types" in docs/02-graphics-dat.md.
CATEGORY_NAMES = {
    0: 'archive info', 1: 'interface / global', 3: 'wall writing',
    4: 'music', 5: 'title screen', 6: 'full screen (other)',
    7: 'interface strings and panels', 8: 'map graphics set (walls/floor/ceiling)',
    9: 'wall ornaments', 10: 'floor ornaments', 11: 'door ornaments?',
    12: 'unknown (sound-linked)', 13: 'doors', 14: 'missiles / spell effects',
    15: 'creatures', 16: 'weapons', 17: 'clothing', 18: 'scrolls',
    19: 'potions', 20: 'containers', 21: 'misc items (21/254 = fallback image)',
    22: 'champions', 23: 'map environment set', 24: 'unknown (24)',
    26: 'dialogs and system messages',
}
TYPE_NAMES = {
    0: 'archive tag string', 1: 'image', 2: 'digital sound', 3: 'HMP music',
    4: 'offset-indexed table', 5: 'text', 7: 'raw data (palette/remap/anim)',
    8: 'u16 table', 9: 'data (1024 bytes)', 11: 'number', 12: 'number',
    13: 'data (16 bytes)', 14: 'short string table',
}
LANGUAGES = {0x10: 'en', 0x30: 'de', 0x40: 'fr', 0xF0: 'editor label'}


def decode_text(raw, obfuscated=True):
    """Undo the text obfuscation (flags bit 0x08): byte i -> (~b - i) & 0xFF."""
    if obfuscated:
        raw = bytes(((b ^ 0xFF) - i) & 0xFF for i, b in enumerate(raw))
    return raw.split(b'\0', 1)[0]


def show_escapes(s):
    """Render escape sequences: 0x01 + (code + 0x20), or '.Z' + 3 digits."""
    out, i = [], 0
    while i < len(s):
        if s[i] == 1 and i + 1 < len(s):
            out.append(f'{{{s[i + 1] - 0x20}}}'); i += 2
        elif s[i:i + 2] == b'.Z' and s[i + 2:i + 5].isdigit():
            out.append(f'{{{int(s[i + 2:i + 5])}}}'); i += 5
        else:
            out.append(chr(s[i]) if s[i] != 10 else '\\n'); i += 1
    return ''.join(out)


class Gdat:
    def __init__(self, path=DEFAULT):
        self.d = d = Path(path).read_bytes()
        sig, self.count = struct.unpack_from('<HH', d, 0)
        assert sig & 0x8000, 'missing signature bit'
        self.version = sig & 0x7FFF
        assert self.version >= 3, 'only the v3+ layout (u32 metadata length) is handled'
        meta_len = struct.unpack_from('<I', d, 4)[0]
        # Entry 0 is the metadata block; entries 1..n-1 have u16 sizes.
        self.sizes = [meta_len] + list(struct.unpack_from(f'<{self.count - 1}H', d, 8))
        self.offsets = []
        off = 8 + 2 * (self.count - 1)
        for s in self.sizes:
            self.offsets.append(off)
            off += s
        assert off == len(d), (off, len(d))
        self._parse_meta(self.entry(0))

    def entry(self, i):
        o = self.offsets[i]
        return self.d[o:o + self.sizes[i]]

    def _parse_meta(self, m):
        marker, nrec, nfld = struct.unpack_from('>HHH', m, 0)
        assert marker == 0x8001, hex(marker)
        fields, off = {}, 0
        for k in range(nfld):
            letter, width = chr(m[6 + 2 * k]), m[7 + 2 * k]
            fields[letter] = (off, width)
            off += width
        self.fields, self.rec_size = fields, off
        base = 6 + 2 * nfld
        self.records = []
        for r in range(nrec):
            row = m[base + r * off: base + (r + 1) * off]
            rec = {}
            for letter, (fo, w) in fields.items():
                rec[letter] = int.from_bytes(row[fo:fo + w], 'big')
            self.records.append(rec)
        self.index = {(r['T'], r['I'], r['D'], r['S']): r for r in self.records}

    def lookup(self, cat, idx, typ, sub):
        """Mirror of the game's key lookup: value, or None."""
        r = self.index.get((cat, idx, typ, sub))
        if r is None:
            return None
        return r['P'] if typ in VALUE_TYPES else r['P'] & 0x7FFF


def main():
    g = Gdat()
    cmd = sys.argv[1] if len(sys.argv) > 1 else 'stats'
    if cmd == 'stats':
        print(f'version {g.version}, {g.count} entries, {len(g.records)} records, '
              f'fields {g.fields}')
        by = collections.Counter((r['T'], r['D']) for r in g.records)
        cats = sorted({c for c, _ in by})
        types = sorted({t for _, t in by})
        print('cat\\type ' + ''.join(f'{t:>6}' for t in types))
        for c in cats:
            print(f'{c:>8} ' + ''.join(f'{by.get((c, t), 0) or "":>6}' for t in types))
        print('F values', collections.Counter(r['F'] for r in g.records).most_common(8))
        print('G values', collections.Counter(r['G'] for r in g.records).most_common(8))
    elif cmd == 'keys':
        cat = int(sys.argv[2], 0) if len(sys.argv) > 2 else None
        for r in g.records:
            if cat is None or r['T'] == cat:
                print(f"cat {r['T']:3} idx {r['I']:3} type {r['D']:2} sub {r['S']:3} "
                      f"F {r['F']:3} G {r['G']:3} value {r['P']:#06x}")
    elif cmd == 'categories':
        by = collections.defaultdict(collections.Counter)
        for r in g.records:
            by[r['T']][r['D']] += 1
        for c in sorted(by):
            types = ', '.join(f"{TYPE_NAMES.get(t, t)}:{n}" for t, n in sorted(by[c].items()))
            print(f"{c:3} {CATEGORY_NAMES.get(c, '?'):40} {types}")
    elif cmd == 'text':
        # text [lang] [cat] -- lang is en/de/fr/editor/all (default en)
        lang = sys.argv[2] if len(sys.argv) > 2 else 'en'
        cat = int(sys.argv[3], 0) if len(sys.argv) > 3 else None
        obf = bool((g.lookup(0, 0, 11, 0) or 0) & 0x08)
        for r in g.records:
            if r['D'] != 5 or (cat is not None and r['T'] != cat):
                continue
            code = r['F'] & 0xF0
            if lang != 'all' and code and LANGUAGES.get(code, '').split()[0] != lang:
                continue
            s = decode_text(g.entry(r['P'] & 0x7FFF), obf)
            print(f"{r['T']:3} {r['I']:3} {r['S']:3} {LANGUAGES.get(code, 'any'):5} {show_escapes(s)}")
    elif cmd == 'raw':
        i = int(sys.argv[2], 0)
        Path(sys.argv[3]).write_bytes(g.entry(i))
    elif cmd == 'export':
        # export [out_dir] [cat]: decode every image (type 1) to PNG
        import gimg
        out = Path(sys.argv[2] if len(sys.argv) > 2 else
                   Path(__file__).resolve().parent.parent / 're/png')
        cat = int(sys.argv[3], 0) if len(sys.argv) > 3 else None
        out.mkdir(parents=True, exist_ok=True)
        pal = gimg.master_palette(g)
        done, failed = {}, []
        for r in g.records:
            if r['D'] != 1 or (cat is not None and r['T'] != cat):
                continue
            e = r['P'] & 0x7FFF
            name = f"c{r['T']:02d}_i{r['I']:03d}_s{r['S']:03d}_e{e}.png"
            try:
                img = done.get(e) or gimg.decode(g.entry(e))
            except Exception as ex:  # report, keep going
                failed.append((e, str(ex)))
                continue
            done[e] = img
            gimg.write_png(out / name, img.w, img.h, img.pixels, pal)
        print(f'{len(done)} images decoded to {out}, {len(failed)} failed')
        for e, msg in failed[:20]:
            print('  entry', e, msg)


if __name__ == '__main__':
    main()
