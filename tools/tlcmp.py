#!/usr/bin/env python3
"""Compare timeline record traffic: the original's T hook lines (schedule
0x56390, pop 0x5643D, delete 0x562FF from tools/dosbox-rnghook.patch) against
the remake's (examples/rngseq with TLLOG=PATH), record by record.

  tlcmp.py ORIGINAL_LOG REMAKE_TL FIRST_TICK LAST_TICK

Each operation is (tick, op, record, type for schedules). The first
difference is printed with context (docs/05, "Timeline records").
"""
import sys


def original(path, t0, t1):
    out, pending_delete = [], None
    for line in open(path):
        f = line.split()
        if not f:
            continue
        if f[0] == 'H' and f[1] == '562ff':
            pending_delete = int(f[3], 16)       # eax: the record to delete
        elif f[0] == 'T':
            addr, tick = f[1], int(f[2])
            free, h0, et = int(f[4], 16), int(f[8], 16), int(f[10], 16)
            if not t0 <= tick <= t1:
                continue
            if addr == '56390':
                out.append((tick, 'S', free, et))
            elif addr == '5643d':
                out.append((tick, 'P', h0, None))
            elif addr == '562ff':
                # The pop routine frees its record through the delete
                # routine: fold that inner delete into the pop.
                if out and out[-1] == (tick, 'P', pending_delete, None):
                    continue
                out.append((tick, 'D', pending_delete, None))
    return out


def remake(path, t0, t1):
    out = []
    for line in open(path):
        f = line.split()
        if len(f) < 6:
            continue
        op, tick, slot, kind = f[0], int(f[1]), int(f[2]), int(f[3], 16)
        if t0 <= tick <= t1:
            out.append((tick, op, slot, kind if op == 'S' else None))
    return out


def main():
    t0, t1 = int(sys.argv[3]), int(sys.argv[4])
    a, b = original(sys.argv[1], t0, t1), remake(sys.argv[2], t0, t1)
    print(f'original {len(a)} ops, remake {len(b)} ops')
    for i, (x, y) in enumerate(zip(a, b)):
        if x != y:
            print('first difference at op', i)
            for j in range(max(0, i - 10), i + 8):
                print(f'{j:5}  O {a[j] if j < len(a) else "-"!s:28} R {b[j] if j < len(b) else "-"}')
            return
    print('identical over', min(len(a), len(b)), 'ops')


if __name__ == '__main__':
    main()
