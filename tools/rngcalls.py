#!/usr/bin/env python3
"""Every direct CALL (E8 rel32) in SKULL.EXE's code that targets one of the
random number routines, with the function Ghidra placed it in (if any).

Works on the gitignored re/skull/flat.bin (base 0x10000) and the
decompilation listing re/skull_decomp.c for function starts.
"""
import bisect
import re
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FLAT = ROOT / 're/skull/flat.bin'
SRC = ROOT / 're/skull_decomp.c'
BASE = 0x10000
CODE_END = 0x10000 + 0x595AC
RNG = {0x1C6A1: 'rnd', 0x1C6B7: 'random(n)', 0x1C6DC: 'bit', 0x1C6F6: 'rand4'}


def main():
    code = FLAT.read_bytes()[: CODE_END - BASE]
    starts = sorted(int(m, 16) for m in re.findall(r'^//==== \S+ @ ([0-9a-f]{8})', SRC.read_text(), flags=re.M))
    sites = []
    for i in range(len(code) - 5):
        if code[i] != 0xE8:
            continue
        rel = struct.unpack_from('<i', code, i + 1)[0]
        target = BASE + i + 5 + rel
        if target in RNG:
            sites.append((BASE + i, RNG[target]))
    inside = outside = 0
    for site, name in sites:
        k = bisect.bisect_right(starts, site) - 1
        owner = starts[k] if k >= 0 else None
        # Treat a site more than 0x800 past the nearest start as unowned.
        if owner is None or site - owner > 0x800:
            outside += 1
            print(f'{site:#x}  {name:10}  outside any known function (nearest start {owner:#x})')
        else:
            inside += 1
    print(f'{len(sites)} call sites: {inside} inside known functions, {outside} outside')


if __name__ == '__main__':
    main()
