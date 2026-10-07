#!/usr/bin/env python3
"""Compare the original's draw log (tools/dosbox-rnghook.patch output) with the
remake's ordered draws (examples/rngseq), draw by draw (docs/05, "Draw log").

  rngcmp.py ORIGINAL_LOG REMAKE_SEQ [START_TICK]

Both streams are reduced to (tick, creature) per draw; the first draw where
they part is printed with the surrounding context, plus per-tick counts.
"""
import re
import sys
from collections import Counter

NUM = re.compile(r'^[0-9]+$')


def original(path):
    out = []
    for line in open(path):
        f = line.split()
        if len(f) < 8 or not NUM.match(f[0]):
            continue                       # H/F hook lines
        out.append((int(f[0]), int(f[7], 16) & 0x3FFF, line.strip()))
    return out


def remake(path):
    out = []
    for line in open(path):
        f = line.split()
        if len(f) < 3 or not NUM.match(f[0]):
            continue
        out.append((int(f[0]), int(f[1], 16) & 0x3FFF, line.strip()))
    return out


def main():
    o, r = original(sys.argv[1]), remake(sys.argv[2])
    # Optional START: compare from that tick on (a loaded save's first tick;
    # the original's log also holds its title screen's ticks).
    start = int(sys.argv[3]) if len(sys.argv) > 3 else 0
    o = [x for x in o if x[0] >= start]
    r = [x for x in r if x[0] >= start]
    end = min(o[-1][0], r[-1][0])
    o = [x for x in o if x[0] < end]
    r = [x for x in r if x[0] < end]
    # Outside creature processing the remake logs creature 0, while the
    # original's field still holds the last creature it loaded.
    # Weather and champion draws carry whichever creature was loaded last on
    # both sides, so only their tick is compared.
    def same(a, b):
        site = b[2].split()[2]
        if site.startswith(('weather.rs', 'champions.rs')):
            return a[0] == b[0]
        return a[0] == b[0] and (b[1] == 0 or a[1] == b[1])
    n = next((i for i, (a, b) in enumerate(zip(o, r)) if not same(a, b)), min(len(o), len(r)))
    print(f'compared ticks 0..{end - 1}: original {len(o)} draws, remake {len(r)}')
    if n == min(len(o), len(r)) and len(o) == len(r):
        print('identical (tick, creature) streams')
    else:
        t = o[n][0] if n < len(o) else r[n][0]
        print(f'first difference at draw {n}, tick {t}')
        for i in range(max(0, n - 4), n + 4):
            a = o[i][2] if i < len(o) else '-'
            b = r[i][2] if i < len(r) else '-'
            print(f'  {i:6}  O: {a[:70]:70}  R: {b}')
    co, cr = Counter(x[0] for x in o), Counter(x[0] for x in r)
    diff = [t for t in sorted(set(co) | set(cr)) if co[t] != cr[t]]
    print('ticks with different counts (first 10):', [(t, co[t], cr[t]) for t in diff[:10]])


if __name__ == '__main__':
    main()
