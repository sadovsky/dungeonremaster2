#!/usr/bin/env python3
"""Drive the original game in DOSBox through a scripted action list and save
the resulting state, so it can be compared field by field with the remake.

  state_probe.py [--load SAVE] SLOT ACTIONS...

Runs the original from a scratch copy of the game under re/state/game (its
own DOSBox config mounts the copy as C:), so every save is written into the
copy and the user's install is never touched. Without --load it starts a new
game (title: New); with --load it copies SAVE into the copy's slot 7 and
loads it through title Resume. It then applies the actions and saves into
SKSAVE<SLOT>.DAT of the copy (SLOT 8 or 9 with --load, 7-9 otherwise) and
prints that file's path.

Actions:
  key:NAME[:N[:GAP]]   press key NAME N times (default 1), GAP seconds apart
                       (default 0.6)
  wait:SECONDS         idle
  click:X,Y            click at (X, Y) in 320x200 screen coordinates

Local verification only: the copy, its saves and the logs stay under re/.
"""
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

MAIN = Path('/home/sadovsky/code/dungeonremaster2')
CONF = Path(os.environ.get('DM2_DOSBOX_CONF', MAIN / 're/dosbox/dm2.conf'))
INSTALL_DATA = MAIN / 'original/dumast2/DATA'
GAME = MAIN / 're/state/game'           # mounted as C:
DATA = GAME / 'dumast2/DATA'
PROBE_CONF = MAIN / 're/state/dm2_probe.conf'
LOAD_SLOT = 7


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


def prepare_copy():
    """Scratch copy of the game without the CD image or any saves, and a
    DOSBox config that mounts it as C: (the CD image is still mounted from
    the install)."""
    (GAME / 'dumast2').mkdir(parents=True, exist_ok=True)
    subprocess.run(['rsync', '-a', '--delete', '--exclude', 'cd/', '--exclude', 'DATA/SKSAVE*',
                    str(INSTALL_DATA.parent) + '/', str(GAME / 'dumast2') + '/'], check=True)
    for f in DATA.glob('SKSAVE*'):
        f.unlink()
    conf = []
    for line in CONF.read_text().splitlines():
        if line.lower().startswith('mount c '):
            line = f'mount c {GAME}'
        conf.append(line)
    PROBE_CONF.write_text('\n'.join(conf) + '\n')


def start(load=False):
    p = subprocess.Popen(['dosbox', '-conf', str(PROBE_CONF)], cwd=MAIN / 're/dosbox',
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
    if load:
        click(w, 45, 78, wait=4)                    # title: Resume
        click(w, 90, 45 + 8 * LOAD_SLOT, wait=1)    # slot row
        click(w, 95, 143, wait=6)                   # Load
        click(w, 160, 143, wait=1)                  # OK on "Game loaded"
    else:
        click(w, 110, 64, wait=6)                   # title: New
    xdo('mousemove', '--window', w, 300, 195)       # park the pointer off the view
    return p, w


def save(w, slot):
    click(w, 20, 5, wait=1.5)              # champion box: open inventory
    click(w, 178, 47, wait=2)              # save disk
    click(w, 57, 114, wait=3)              # Save
    click(w, 90, 53 + 8 * slot, wait=1)    # slot row (save list: slot n at y = 53 + 8n)
    click(w, 57, 151, wait=5)              # Save


def stamps(data):
    return {n: (data / f'SKSAVE{n}.DAT').stat().st_mtime_ns
            for n in range(10) if (data / f'SKSAVE{n}.DAT').exists()}


def main():
    args = sys.argv[1:]
    load = None
    if args[:1] == ['--load']:
        load = Path(args[1]).resolve()
        args = args[2:]
    slot = int(args[0])
    assert 7 <= slot <= 9 and not (load and slot == LOAD_SLOT), 'slots 7-9 (8-9 with --load)'
    install_before = stamps(INSTALL_DATA)
    prepare_copy()
    if load:
        shutil.copyfile(load, DATA / f'SKSAVE{LOAD_SLOT}.DAT')
    p, w = start(load=bool(load))
    log = []
    t0 = time.time()
    try:
        for act in args[1:]:
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
    if stamps(INSTALL_DATA) != install_before:
        sys.exit('ERROR: the install\'s save files changed')
    out = DATA / f'SKSAVE{slot}.DAT'
    print('save:', out if out.exists() else 'NOT WRITTEN')
    print('inputs', log)


if __name__ == '__main__':
    main()
