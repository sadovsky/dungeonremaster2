#!/usr/bin/env python3
"""Record the original's random-draw log for an idle new game, using the
DOSBox build patched with tools/dosbox-rnghook.patch (docs/05, "Draw log").

  rnglog_idle.py OUT.txt [SECONDS]   (default 25 s of idle play)

Runs the hooked DOSBox by PID, skips the intro, clicks New and leaves the
party idle, then closes DOSBox. The log stays under re/ (never committed).
"""
import os
import subprocess
import sys
import time
from pathlib import Path

MAIN = Path(__file__).resolve().parent.parent
DOSBOX = Path.home() / 'tools/build/dosbox-0.74-3/src/dosbox'
CONF = Path(os.environ.get('DM2_DOSBOX_CONF', MAIN / 're/dosbox/dm2.conf'))


def xdo(*a):
    return subprocess.run(['xdotool', *map(str, a)], capture_output=True, text=True).stdout.strip()


def main():
    out = Path(sys.argv[1]).resolve()
    secs = float(sys.argv[2]) if len(sys.argv) > 2 else 25.0
    env = dict(os.environ, DM2_RNGLOG=str(out))
    p = subprocess.Popen([str(DOSBOX), '-conf', str(CONF)], cwd=MAIN / 're/dosbox', env=env,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        w = ''
        for _ in range(40):
            w = xdo('search', '--pid', p.pid).split('\n')[0]
            if w:
                break
            time.sleep(0.5)
        if not w:
            raise SystemExit('DOSBox window not found')
        time.sleep(6)
        for _ in range(2):
            xdo('key', '--window', w, 'Escape')
            time.sleep(4)
        # Title: New, clicked in the same call as the move (reliable in DOSBox).
        xdo('windowactivate', '--sync', w, 'mousemove', '--window', w, 110, 64, 'click', 1)
        time.sleep(secs)
    finally:
        p.kill()
        p.wait()
    print(out)


if __name__ == '__main__':
    main()
