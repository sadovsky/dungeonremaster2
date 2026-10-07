#!/usr/bin/env python3
"""Drive the original game in DOSBox through a scripted action list and save
the resulting state, so it can be compared field by field with the remake.

  state_probe.py SLOT ACTIONS...

Starts a new game (title: New), applies the actions, then saves into
SKSAVE<SLOT>.DAT through the inventory's save disk. SLOT must be 7-9.

Actions:
  key:NAME[:N[:GAP]]   press key NAME N times (default 1), GAP seconds apart
                       (default 0.6)
  wait:SECONDS         idle
  click:X,Y            click at (X, Y) in 320x200 screen coordinates

Local verification only: the saves stay in the user's own game folder.
"""
import subprocess
import sys
import time
from pathlib import Path

MAIN = Path('/home/sadovsky/code/dungeonremaster2')
import os
CONF = Path(os.environ.get('DM2_DOSBOX_CONF', MAIN / 're/dosbox/dm2.conf'))
DATA = MAIN / 'original/dumast2/DATA'


def xdo(*a):
    subprocess.run(['xdotool', *map(str, a)], check=True)


def click(w, x, y, wait=1.0):
    # One call, real pointer click: DOSBox ignores synthetic per-window clicks.
    xdo('windowactivate', '--sync', w)
    xdo('mousemove', '--window', w, x, y, 'click', 1)
    time.sleep(wait)


def key(w, name):
    xdo('windowactivate', '--sync', w)
    xdo('key', '--window', w, name)


def start(new_game=True):
    p = subprocess.Popen(['dosbox', '-conf', str(CONF)], cwd=MAIN / 're/dosbox',
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    w = None
    for _ in range(60):
        r = subprocess.run(['xdotool', 'search', '--pid', str(p.pid)], capture_output=True, text=True)
        if r.stdout.strip():
            w = r.stdout.split()[0]
            break
        time.sleep(0.5)
    if not w:
        p.kill()
        sys.exit('no DOSBox window')
    time.sleep(6)
    steady = 0
    for _ in range(40):
        g = subprocess.run(['xdotool', 'getwindowgeometry', w], capture_output=True, text=True).stdout
        if 'Geometry: 320x200' in g:
            steady += 1
            if steady >= 3:
                break
        else:
            steady = 0
            xdo('key', '--window', w, 'Escape')
        time.sleep(1.5)
    time.sleep(2)
    click(w, 110, 64, wait=6)              # title: New
    xdo('mousemove', '--window', w, 300, 195)  # park the pointer off the view
    return p, w


def save(w, slot):
    click(w, 20, 5, wait=1.5)              # champion box: open inventory
    click(w, 178, 47, wait=2)              # save disk
    click(w, 57, 114, wait=3)              # Save
    click(w, 90, 53 + 8 * slot, wait=1)    # slot row (save list: slot n at y = 53 + 8n)
    click(w, 57, 151, wait=5)              # Save


def stamps():
    return {n: (DATA / f'SKSAVE{n}.DAT').stat().st_mtime_ns
            for n in range(10) if (DATA / f'SKSAVE{n}.DAT').exists()}


def main():
    slot = int(sys.argv[1])
    assert 7 <= slot <= 9, 'slots 7-9 only'
    before = stamps()
    p, w = start()
    log = []
    t0 = time.time()
    try:
        for act in sys.argv[2:]:
            kind, _, rest = act.partition(':')
            if kind == 'key':
                parts = rest.split(':')
                n = int(parts[1]) if len(parts) > 1 else 1
                gap = float(parts[2]) if len(parts) > 2 else 0.6
                for _ in range(n):
                    key(w, parts[0])
                    log.append((round(time.time() - t0, 3), parts[0]))
                    time.sleep(gap)
            elif kind == 'wait':
                time.sleep(float(rest))
            elif kind == 'click':
                x, y = map(int, rest.split(','))
                click(w, x, y)
        save(w, slot)
    finally:
        p.kill()
    after = stamps()
    changed = sorted(n for n in after if before.get(n) != after[n])
    bad = [n for n in changed if n < 7]
    if bad:
        sys.exit(f'ERROR: protected save slots changed: {bad}')
    print('changed save slots:', changed, '(requested', slot, ')')
    print('inputs', log)


if __name__ == '__main__':
    main()
