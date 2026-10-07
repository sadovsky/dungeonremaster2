#!/usr/bin/env python3
"""Random draws a function makes directly, and in its callees down to a
depth, without the over-approximation of following the big dispatchers.

  rngshallow.py DEPTH ADDR...

Works on the gitignored decompilation listing re/skull_decomp.c.
"""
import re
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / 're/skull_decomp.c'
RNG = {0x1C6A1: 'rnd', 0x1C6B7: 'random(n)', 0x1C6DC: 'bit', 0x1C6F6: 'rand4'}


def load():
    funcs = {}
    for chunk in re.split(r'(?=^//==== )', SRC.read_text(), flags=re.M):
        m = re.match(r'//==== \S+ @ ([0-9a-f]{8})', chunk)
        if m:
            funcs[int(m.group(1), 16)] = chunk
    return funcs


def calls(body):
    return [int(x, 16) for x in re.findall(r'FUN_000([0-9a-f]{5})\(', body)]


def show(funcs, a, depth, indent, seen):
    if a not in funcs or a in seen:
        return
    seen.add(a)
    body = funcs[a]
    direct = [RNG[c] for c in calls(body) if c in RNG]
    sub = [c for c in dict.fromkeys(calls(body)) if c not in RNG]
    print(f"{'  ' * indent}{a:#x}: {len(body.splitlines())} lines, direct draws {direct or '-'}")
    if depth > 0:
        for c in sub:
            show(funcs, c, depth - 1, indent + 1, seen)


def main():
    funcs = load()
    depth = int(sys.argv[1])
    for a in sys.argv[2:]:
        show(funcs, int(a, 16), depth, 0, set())


if __name__ == '__main__':
    main()
