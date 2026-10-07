#!/usr/bin/env python3
"""Functions whose address appears as data (pointer tables, callback
registration) and that draw random numbers within a few calls: candidates
for code run from interrupts or the launcher's timer service.

Scans the relocated image re/skull/flat.bin for 32-bit values equal to a
function start, and the decompilation for which of those reach the RNG.
"""
import re
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FLAT = ROOT / 're/skull/flat.bin'
SRC = ROOT / 're/skull_decomp.c'
BASE = 0x10000
RNG = {0x1C6A1, 0x1C6B7, 0x1C6DC, 0x1C6F6}


def load():
    funcs = {}
    for chunk in re.split(r'(?=^//==== )', SRC.read_text(), flags=re.M):
        m = re.match(r'//==== \S+ @ ([0-9a-f]{8})', chunk)
        if m:
            funcs[int(m.group(1), 16)] = chunk
    return funcs


def draws_within(funcs, a, depth, seen):
    if a in seen or a not in funcs:
        return False
    seen.add(a)
    for c in re.findall(r'FUN_000([0-9a-f]{5})\(', funcs[a]):
        c = int(c, 16)
        if c in RNG:
            return True
        if depth > 0 and draws_within(funcs, c, depth - 1, seen):
            return True
    return False


def main():
    funcs = load()
    img = FLAT.read_bytes()
    starts = set(funcs)
    taken = {}
    for i in range(0, len(img) - 3):
        v = struct.unpack_from('<I', img, i)[0]
        if v in starts:
            taken.setdefault(v, []).append(BASE + i)
    for a in sorted(taken):
        if draws_within(funcs, a, 3, set()):
            refs = ', '.join(f'{r:#x}' for r in taken[a][:4])
            print(f'{a:#x}  address stored at {refs}')


if __name__ == '__main__':
    main()
