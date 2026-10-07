#!/usr/bin/env python3
"""Capture the same game state in the original (DOSBox) and the remake, and
compose original | remake | difference. Local verification only: images are
renders of the user's own copy of the game and stay under re/ (gitignored).

  sbs_capture.py NAME MAP X Y DIR [--key KEY]... [--click X,Y]... [--cmd C]...

Writes a remake save into slot 7 with examples/posave (party moved through
the normal arrival path), loads it in the original via the title screen's
Resume, optionally sends keys/clicks, captures the window, renders the same
save with `dm2 --screenshot --load` (plus --cmd commands), and composes the
result into re/sidebyside/NAME.png. Prints the differing pixel count.
"""
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MAIN = Path('/home/sadovsky/code/dungeonremaster2')
DATA = MAIN / 'original/dumast2/DATA'
CONF = MAIN / 're/dosbox/dm2.conf'
OUT = MAIN / 're/sidebyside'
WORK = OUT / 'work'
BASE = DATA / 'SKSAVE0.DAT'   # the original's own save: never written
SLOT = 7
SLOT_FILE = DATA / f'SKSAVE{SLOT}.DAT'

sys.path.insert(0, str(MAIN / 're/dosbox'))
import sbs  # noqa: E402


def xdo(*a):
    subprocess.run(['xdotool', *map(str, a)], check=True)


def click(w, x, y, wait=1.0):
    # One call, real pointer click: DOSBox ignores synthetic per-window clicks.
    xdo('windowactivate', '--sync', w)
    xdo('mousemove', '--window', w, x, y, 'click', 1)
    time.sleep(wait)


def main():
    a = sys.argv[1:]
    name, view = a[0], a[1:5]
    keys, clicks, cmds = [], [], []
    i = 5
    while i < len(a):
        if a[i] == '--key':
            keys.append(a[i + 1])
        elif a[i] == '--click':
            clicks.append(tuple(map(int, a[i + 1].split(','))))
        elif a[i] == '--cmd':
            cmds.append(a[i + 1])
        i += 2
    WORK.mkdir(parents=True, exist_ok=True)
    assert SLOT_FILE.name not in {f'SKSAVE{n}.DAT' for n in range(7)}
    posave = ROOT / 'target/release/examples/posave'
    subprocess.run([posave, BASE, SLOT_FILE, *view, name.upper()[:20]], check=True,
                   stdout=subprocess.DEVNULL)
    shutil_copy = WORK / f'{name}.DAT'
    shutil_copy.write_bytes(SLOT_FILE.read_bytes())

    # Original: launch, skip intro, Resume, pick slot, Load, OK.
    p = subprocess.Popen(['dosbox', '-conf', str(CONF)], cwd=MAIN / 're/dosbox',
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        w = None
        for _ in range(60):
            r = subprocess.run(['xdotool', 'search', '--pid', str(p.pid)], capture_output=True, text=True)
            if r.stdout.strip():
                w = r.stdout.split()[0]
                break
            time.sleep(0.5)
        assert w, 'no DOSBox window'
        # Skip the intro movies (640x350) until the title (320x200) is steady.
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
        click(w, 45, 78, 4)                    # title: Resume
        click(w, 90, 45 + 8 * SLOT, 1)         # slot row
        click(w, 95, 143, 6)                   # Load
        click(w, 160, 143, 3)                  # OK on "Game loaded"
        for k in keys:
            xdo('key', '--window', w, k)
            time.sleep(1.5)
        for (x, y) in clicks:
            click(w, x, y, 1.5)
        xdo('mousemove', '--window', w, 300, 195)
        time.sleep(1.0)
        orig = WORK / f'{name}_orig.png'
        subprocess.run(['import', '-window', w, str(orig)], check=True)
    finally:
        p.kill()
        p.wait()

    remake = WORK / f'{name}_remake.png'
    cmd = [ROOT / 'target/release/dm2', '--screenshot', remake, '--load', shutil_copy]
    for c in cmds:
        cmd += ['--cmd', c]
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    n = sbs.compose(str(orig), str(remake), str(OUT / f'{name}.png'))
    print(f'{name}: {n} differing pixels')


if __name__ == '__main__':
    main()
