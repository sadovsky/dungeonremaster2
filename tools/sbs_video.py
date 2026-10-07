#!/usr/bin/env python3
"""Record the original (DOSBox) and the remake playing the same route, side
by side, as one MP4. Run it yourself on your own copy of the game; the
output stays under re/video/ (gitignored) and is for your own viewing.

  sbs_video.py [SEGMENT ...]        default: every segment in SEGMENTS
  sbs_video.py --compose-test       check the ffmpeg layout with test patterns

Per segment:
  1. examples/posave writes a save with the party at the segment's start
     into slot 7 (slots 0-6 are never written);
  2. DOSBox loads it via title Resume; ffmpeg's x11grab records the DOSBox
     window while the route's keys and clicks are sent, and the time of each
     input after the "Game loaded" OK click is logged;
  3. the log becomes a replay script (133 ms per tick) and
     `dm2 --replay SCRIPT --frames DIR --load SAVE` renders the remake
     tick by tick;
  4. ffmpeg scales both 3x (nearest neighbour), labels them and puts them
     side by side; segments are concatenated into re/video/side_by_side.mp4.

Random things (creature moves, rain, teleporter noise) can drift: the
original's random state depends on when inputs land within a tick.
Needs: dosbox, xdotool, ffmpeg (with x11grab and drawtext), and release
builds of dm2 and examples/posave.
"""
import json
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MAIN = ROOT if (ROOT / 'original').exists() else Path('/home/sadovsky/code/dungeonremaster2')
DATA = MAIN / 'original/dumast2/DATA'
CONF = MAIN / 're/dosbox/dm2.conf'
OUT = MAIN / 're/video'
BASE = DATA / 'SKSAVE0.DAT'          # the original's own save: only read
SLOT = 7
SLOT_FILE = DATA / f'SKSAVE{SLOT}.DAT'
TICK = 8 / 60                        # 133 ms (docs/05, measured in DOSBox)
FPS = 30
SCALE = 3

# Original input -> remake interface command (docs/10). The cursor-key
# mapping follows the original's key table: Up forward, Down back, Left and
# Right turn. Clicks carry the command the same zone issues.
KEY_CMD = {'Up': 3, 'Down': 5, 'Left': 1, 'Right': 2}

# name: (map, x, y, dir, route); route items are (seconds_after_previous,
# 'key', KEY) or (seconds, 'click', x, y, command).
SEGMENTS = {
    'cave': (0, 1, 8, 0, [(1.0, 'key', 'Up')] * 6 + [(1.0, 'key', 'Right'), (1.0, 'key', 'Right'),
                                                      (1.0, 'key', 'Up'), (2.0, 'key', 'Up')]),
    'inventory': (0, 1, 8, 0, [(1.5, 'click', 20, 5, 0x07), (4.0, 'click', 20, 5, 0x07),
                               (2.0, 'key', 'Up'), (2.0, 'key', 'Up')]),
    'outdoor': (1, 2, 9, 0, [(1.0, 'key', 'Up')] * 4 + [(1.0, 'key', 'Left'), (1.0, 'key', 'Up'),
                                                         (1.0, 'key', 'Up'), (2.0, 'key', 'Right')]),
    'door': (2, 19, 12, 0, [(1.5, 'key', 'Up'), (1.5, 'key', 'Up'), (2.0, 'key', 'Left'),
                            (2.0, 'key', 'Right'), (3.0, 'key', 'Down')]),
    'stairs': (8, 12, 2, 0, [(1.5, 'key', 'Up'), (2.0, 'key', 'Up'), (3.0, 'key', 'Left'),
                             (2.0, 'key', 'Up')]),
}


def run(*a, **k):
    return subprocess.run([str(x) for x in a], check=True, **k)


def xdo(*a):
    run('xdotool', *a)


def click(w, x, y):
    # One call, real pointer click: DOSBox ignores synthetic per-window clicks.
    xdo('windowactivate', '--sync', w)
    xdo('mousemove', '--window', w, x, y, 'click', 1)


def open_original(save_slot):
    """Start DOSBox, skip the intro and load `save_slot`. Returns (proc, window)."""
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
    click(w, 45, 78)                       # title: Resume
    time.sleep(4)
    click(w, 90, 45 + 8 * save_slot)       # slot row
    time.sleep(1)
    click(w, 95, 143)                      # Load
    time.sleep(6)
    return p, w


def record_segment(name, seg, work):
    m, x, y, d, route = seg
    save = work / f'{name}.DAT'
    run(ROOT / 'target/release/examples/posave', BASE, SLOT_FILE, m, x, y, d, name.upper()[:20],
        stdout=subprocess.DEVNULL)
    save.write_bytes(SLOT_FILE.read_bytes())
    duration = sum(r[0] for r in route) + 2.0
    p, w = open_original(SLOT)
    try:
        xdo('windowactivate', '--sync', w)
        xdo('mousemove', '--window', w, 300, 195)
        click(w, 160, 143)                 # OK on "Game loaded": the game runs from here
        t0 = time.monotonic()
        rec = subprocess.Popen(['ffmpeg', '-y', '-loglevel', 'error', '-f', 'x11grab',
                                '-window_id', str(int(w)), '-framerate', str(FPS), '-i', ':0',
                                '-t', f'{duration:.2f}', '-c:v', 'libx264', '-qp', '0',
                                str(work / f'{name}_orig.mkv')])
        rec_start = time.monotonic() - t0
        log = []
        for item in route:
            time.sleep(item[0])
            if item[1] == 'key':
                xdo('key', '--window', w, item[2])
                log.append((time.monotonic() - t0, KEY_CMD[item[2]]))
            else:
                click(w, item[2], item[3])
                xdo('mousemove', '--window', w, 300, 195)
                log.append((time.monotonic() - t0, item[4]))
        rec.wait()
    finally:
        p.kill()
        p.wait()
    ticks = int(duration / TICK) + 1
    script = '\n'.join(f'{round(t / TICK)} {c:#x}' for t, c in log) + f'\nend {ticks}\n'
    (work / f'{name}.replay').write_text(script)
    frames = work / f'{name}_frames'
    run(ROOT / 'target/release/dm2', '--replay', work / f'{name}.replay', '--frames', frames,
        '--load', save, stdout=subprocess.DEVNULL)
    json.dump({'log': log, 'rec_start': rec_start}, open(work / f'{name}.json', 'w'))
    return work / f'{name}_orig.mkv', frames, rec_start


def compose(orig, remake_input, out, offset=0.0, test=False):
    """Original | remake, 3x nearest neighbour, labelled, one MP4."""
    w, h, gap, head = 320 * SCALE, 200 * SCALE, 12, 48
    label = ("drawtext=text='{t}':fontcolor=white:fontsize=28:x=(w-text_w)/2:y=10")
    inputs = ['-itsoffset', f'{offset:.3f}', '-i', str(orig)]
    inputs += remake_input
    graph = (
        f"[0:v]fps={FPS},scale={w}:{h}:flags=neighbor,pad={w}:{h + head}:0:{head}:black,"
        f"{label.format(t='Original (DOSBox)')}[a];"
        f"[1:v]fps={FPS},scale={w}:{h}:flags=neighbor,pad={w + gap}:{h + head}:{gap}:{head}:black,"
        f"{label.format(t='Remake')}[b];"
        "[a][b]hstack=inputs=2,format=yuv420p[v]")
    run('ffmpeg', '-y', '-loglevel', 'error', *inputs, '-filter_complex', graph, '-map', '[v]',
        '-shortest', '-c:v', 'libx264', '-crf', '16', '-r', FPS, out)


def concat(parts, out):
    lst = out.with_suffix('.txt')
    lst.write_text(''.join(f"file '{p}'\n" for p in parts))
    run('ffmpeg', '-y', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i', lst, '-c', 'copy', out)


def compose_test():
    """Exercise the layout with ffmpeg test patterns (no game footage)."""
    work = OUT / 'test'
    work.mkdir(parents=True, exist_ok=True)
    a, b = work / 'a.mkv', work / 'b.mkv'
    run('ffmpeg', '-y', '-loglevel', 'error', '-f', 'lavfi', '-i', 'testsrc=size=320x200:rate=30',
        '-t', '3', a)
    run('ffmpeg', '-y', '-loglevel', 'error', '-f', 'lavfi', '-i', 'smptebars=size=320x200:rate=7.5',
        '-t', '3', b)
    out = work / 'layout_test.mp4'
    compose(a, ['-i', str(b)], out)
    concat([out, out], work / 'concat_test.mp4')
    print(work / 'concat_test.mp4')


def main():
    if sys.argv[1:] == ['--compose-test']:
        compose_test()
        return
    names = sys.argv[1:] or list(SEGMENTS)
    work = OUT / 'work'
    work.mkdir(parents=True, exist_ok=True)
    parts = []
    try:
        for name in names:
            orig, frames, rec_start = record_segment(name, SEGMENTS[name], work)
            part = work / f'{name}.mp4'
            # Remake frame N shows the state after tick N; both start at the OK click.
            compose(orig, ['-framerate', str(1 / TICK), '-i', str(frames / '%05d.png')], part,
                    offset=rec_start)
            parts.append(part)
            print(f'{name}: {part}')
    finally:
        if SLOT_FILE.exists():
            SLOT_FILE.unlink()             # only the slot this script wrote
    out = OUT / 'side_by_side.mp4'
    concat(parts, out)
    print(out)


if __name__ == '__main__':
    main()
