#!/usr/bin/env python3
"""For each function a given function calls, list which of its callees
(transitively) draw from the game's random number generator.

  rngtree.py ADDR

Works on the gitignored decompilation listing re/skull_decomp.c.
"""
import re
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / 're/skull_decomp.c'
RNG = {0x1C6A1, 0x1C6B7, 0x1C6DC, 0x1C6F6}


def load():
    funcs = {}
    for chunk in re.split(r'(?=^//==== )', SRC.read_text(), flags=re.M):
        m = re.match(r'//==== \S+ @ ([0-9a-f]{8})', chunk)
        if m:
            funcs[int(m.group(1), 16)] = chunk
    return funcs


def callees(body):
    return [int(x, 16) for x in re.findall(r'FUN_000([0-9a-f]{5})\(', body)]


def drawing(funcs, a, seen):
    """Functions reachable from a (inclusive) that call the RNG directly."""
    if a in seen or a not in funcs:
        return set()
    seen.add(a)
    out = set()
    for c in callees(funcs[a]):
        if c in RNG:
            out.add(a)
        else:
            out |= drawing(funcs, c, seen)
    return out


def main():
    funcs = load()
    top = int(sys.argv[1], 16)
    body = funcs[top]
    print(f'{top:#x}: {len(body.splitlines())} lines')
    for c in dict.fromkeys(callees(body)):
        if c in RNG:
            print(f'  direct RNG call {c:#x}')
            continue
        r = drawing(funcs, c, set())
        tail = f" e.g. {[hex(x) for x in sorted(r)[:8]]}" if r else ''
        print(f'  {c:#x} -> {len(r)} drawing functions{tail}')


if __name__ == '__main__':
    main()
