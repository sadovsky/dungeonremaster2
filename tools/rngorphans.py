#!/usr/bin/env python3
"""Functions that draw from the random number generator (directly or via
callees) but are never called by name: candidates for code reached through
function pointers, such as interrupt or timer callbacks.

Works on the gitignored decompilation listing re/skull_decomp.c.
"""
import re
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / 're/skull_decomp.c'
RNG = {0x1C6A1, 0x1C6B7, 0x1C6DC, 0x1C6F6}


def main():
    text = SRC.read_text()
    funcs = {}
    for chunk in re.split(r'(?=^//==== )', text, flags=re.M):
        m = re.match(r'//==== \S+ @ ([0-9a-f]{8})', chunk)
        if m:
            funcs[int(m.group(1), 16)] = chunk
    called = {}
    for a, body in funcs.items():
        for c in re.findall(r'FUN_000([0-9a-f]{5})\(', body):
            c = int(c, 16)
            if c != a:
                called.setdefault(c, set()).add(a)
    # Also count references that are not calls (pointers taken).
    referenced = {int(x, 16) for x in re.findall(r'(?:PTR_)?FUN_000([0-9a-f]{5})\b(?!\()', text)}
    direct = {a for a, b in funcs.items() if any(f'FUN_000{r:05x}(' in b for r in RNG)}
    for a in sorted(direct):
        if a in RNG:
            continue
        n = len(called.get(a, ()))
        if n == 0:
            note = 'address taken' if a in referenced else 'no references'
            print(f'{a:#x}  never called by name ({note})')


if __name__ == '__main__':
    main()
