#!/usr/bin/env python3
"""List which functions call the game's random number generator, optionally
following callees from given start addresses (for tracing which code draws
random numbers on a path such as starting a new game).

  rngscan.py ADDR...           RNG calls in each function and its callees
  rngscan.py --reach ADDR...   every function reachable from ADDR that draws

Works on the gitignored decompilation listing re/skull_decomp.c.
"""
import re
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / 're/skull_decomp.c'
RNG = {0x1C6A1, 0x1C6B7, 0x1C6DC, 0x1C6F6}   # rnd, random(n), random bit, random 2 bits


def load():
    funcs = {}
    for chunk in re.split(r'(?=^//==== )', SRC.read_text(), flags=re.M):
        m = re.match(r'//==== \S+ @ ([0-9a-f]{8})', chunk)
        if m:
            funcs[int(m.group(1), 16)] = chunk
    return funcs


def callees(body):
    return {int(x, 16) for x in re.findall(r'FUN_000([0-9a-f]{5})\(', body)}


def rng_calls(body):
    return sum(body.count(f'FUN_000{a:05x}(') for a in RNG)


def main():
    funcs = load()
    args = sys.argv[1:]
    if args and args[0] == '--reach':
        start = [int(a, 16) for a in args[1:]]
        seen, todo = set(), list(start)
        while todo:
            f = todo.pop()
            if f in seen or f not in funcs or f in RNG:
                continue
            seen.add(f)
            todo.extend(callees(funcs[f]))
        for f in sorted(seen):
            n = rng_calls(funcs[f])
            if n:
                print(f'{f:#x} draws {n}')
        return
    for a in args:
        f = int(a, 16)
        body = funcs.get(f, '')
        print(f'{f:#x} rng={rng_calls(body)} callees=' + ' '.join(f'{c:#x}' for c in sorted(callees(body))))


if __name__ == '__main__':
    main()
