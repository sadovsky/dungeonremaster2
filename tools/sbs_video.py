#!/usr/bin/env python3
"""Record the original (DOSBox) and the remake playing the same route, side
by side, as one MP4. Run it yourself on your own copy of the game; the
output stays under re/video/ (gitignored) and is for your own viewing.

  sbs_video.py [SEGMENT ...]        default: every segment in SEGMENTS
  sbs_video.py --realign [SEG ...]  redo input alignment and replay from the
                                    recordings in re/video/work (no DOSBox)
  sbs_video.py --compose-only [SEG ...]  only rebuild the video from work files
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
PULSE_SOURCE = 'RDPSink.monitor'     # WSLg's output sink; DOSBox plays into it
CHANGE_PX = 600                      # pixels that must change to count as a view change (pointer ~320)

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
        # Video and the DOSBox mix (PulseAudio monitor of the WSLg sink) in one
        # process, so both share the recording's timebase.
        rec = subprocess.Popen(['ffmpeg', '-y', '-loglevel', 'error', '-thread_queue_size', '512',
                                '-f', 'x11grab', '-window_id', str(int(w)), '-framerate', str(FPS),
                                '-i', ':0', '-thread_queue_size', '512', '-f', 'pulse',
                                '-i', PULSE_SOURCE, '-t', f'{duration:.2f}', '-c:v', 'libx264',
                                '-qp', '0', '-c:a', 'pcm_s16le', str(work / f'{name}_orig.mkv')])
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
    json.dump({'log': log, 'rec_start': rec_start}, open(work / f'{name}.json', 'w'))
    return align_segment(name, work)


def align_segment(name, work):
    """Place every logged input at the tick whose remake frames best match the
    original recording (`dm2 --align`), then render the remake's frames and
    audio with that schedule. Works from the files a recording left in `work`,
    so it can be rerun without DOSBox (--realign).

    Every input is kept. The original queues commands it can't act on yet, so
    a press with no visible effect for a while still happens later; dropping
    such inputs left the remake a square behind."""
    info = json.load(open(work / f'{name}.json'))
    orig = work / f'{name}_orig.mkv'
    frames = work / f'{name}_frames'
    save = work / f'{name}.DAT'
    rgb = work / f'{name}_orig.rgb'
    # The original decoded to one 320x200 frame per remake tick, in the
    # recording's timebase (frame N at N * 133 ms).
    with open(rgb, 'wb') as f:
        run('ffmpeg', '-loglevel', 'error', '-i', orig, '-vf',
            f'fps={1 / TICK},scale=320:200:flags=neighbor', '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-',
            stdout=f)
    ticks = rgb.stat().st_size // (320 * 200 * 3)
    logged = [((t - info['rec_start']) / TICK, c) for t, c in info['log']]
    inputs = work / f'{name}.inputs'
    inputs.write_text(''.join(f'{t:.3f} {c:#x}\n' for t, c in logged) + f'end {ticks}\n')
    report = run(ROOT / 'target/release/dm2', '--align', inputs, '--orig', rgb, '--load', save,
                 '--out', work / f'{name}.replay', capture_output=True, text=True).stdout
    schedule = []
    for line in (work / f'{name}.replay').read_text().splitlines():
        a, b = line.split()
        if a != 'end':
            schedule.append((int(a), int(b, 16)))
    if frames.exists():
        for f in frames.glob('*.png'):
            f.unlink()
    run(ROOT / 'target/release/dm2', '--replay', work / f'{name}.replay', '--frames', frames,
        '--load', save, '--audio', work / f'{name}_remake.wav', stdout=subprocess.DEVNULL)
    info.update(logged_ticks=logged, schedule=schedule, align_report=report.splitlines())
    json.dump(info, open(work / f'{name}.json', 'w'))
    rgb.unlink()
    return orig, frames, work / f'{name}_remake.wav'


def change_times(inp, rate):
    """Times (s) of frames whose 320x200 picture differs from the previous one
    by more than CHANGE_PX pixels (mouse-pointer moves stay below that)."""
    raw = subprocess.run(['ffmpeg', '-loglevel', 'error', *inp, '-vf', 'scale=320:200:flags=neighbor',
                          '-f', 'rawvideo', '-pix_fmt', 'gray', '-'], capture_output=True, check=True).stdout
    n = len(raw) // 64000
    out, prev = [], None
    for i in range(n):
        f = raw[i * 64000:(i + 1) * 64000]
        if prev is not None:
            d = sum(1 for a, b in zip(f[::2], prev[::2]) if abs(a - b) > 12) * 2
            if d > CHANGE_PX:
                out.append(i / rate)
        prev = f
    return out


def compose(orig, remake_input, out, remake_wav=None):
    """Original | remake, 3x nearest neighbour, labelled, one MP4. Both start
    at recording time 0. With audio: track 1 the original, track 2 the
    remake; <out>_mixdown.mp4 gets one stereo track, original left, remake right."""
    w, h, gap, head = 320 * SCALE, 200 * SCALE, 12, 48
    label = ("drawtext=text='{t}':fontcolor=white:fontsize=28:x=(w-text_w)/2:y=10")
    inputs = ['-i', str(orig), *remake_input]
    graph = (
        f"[0:v]fps={FPS},scale={w}:{h}:flags=neighbor,pad={w}:{h + head}:0:{head}:black,"
        f"{label.format(t='Original (DOSBox)')}[a];"
        f"[1:v]fps={FPS},scale={w}:{h}:flags=neighbor,pad={w + gap}:{h + head}:{gap}:{head}:black,"
        f"{label.format(t='Remake')}[b];"
        "[a][b]hstack=inputs=2,format=yuv420p[v]")
    venc = ['-c:v', 'libx264', '-crf', '16', '-r', str(FPS)]
    if remake_wav is None:
        run('ffmpeg', '-y', '-loglevel', 'error', *inputs, '-filter_complex', graph, '-map', '[v]',
            '-shortest', *venc, out)
        return
    inputs += ['-i', str(remake_wav)]
    aenc = ['-c:a', 'aac', '-b:a', '192k', '-ar', '44100']
    run('ffmpeg', '-y', '-loglevel', 'error', *inputs, '-filter_complex',
        graph + ';[0:a]aresample=44100,aformat=channel_layouts=stereo,loudnorm=I=-20:TP=-1.5:LRA=11,aresample=44100,aformat=channel_layouts=stereo[oa];'
        '[2:a]aresample=44100,aformat=channel_layouts=stereo,loudnorm=I=-20:TP=-1.5:LRA=11,aresample=44100,aformat=channel_layouts=stereo[ra]',
        '-map', '[v]', '-map', '[oa]', '-map', '[ra]', '-shortest', *venc, *aenc,
        '-metadata:s:a:0', 'title=Original (DOSBox)', '-metadata:s:a:1', 'title=Remake',
        '-disposition:a:0', 'default', '-disposition:a:1', '0', out)
    mix = out.with_name(out.stem + '_mixdown.mp4')
    run('ffmpeg', '-y', '-loglevel', 'error', *inputs, '-filter_complex',
        graph + ';[0:a]aresample=44100,pan=mono|c0=0.5*c0+0.5*c1,loudnorm=I=-20:TP=-1.5:LRA=11,aresample=44100,'
        'aformat=sample_fmts=fltp:sample_rates=44100:channel_layouts=mono[l];'
        '[2:a]aresample=44100,pan=mono|c0=0.5*c0+0.5*c1,loudnorm=I=-20:TP=-1.5:LRA=11,aresample=44100,'
        'aformat=sample_fmts=fltp:sample_rates=44100:channel_layouts=mono[r];[l][r]amerge=inputs=2[m]',
        '-map', '[v]', '-map', '[m]', '-shortest', *venc, *aenc, mix)


def concat(parts, out):
    lst = out.with_suffix('.txt')
    lst.write_text(''.join(f"file '{p}'\n" for p in parts))
    # -map 0 keeps every stream; without it ffmpeg copies only the first audio track.
    run('ffmpeg', '-y', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i', lst, '-map', '0', '-c', 'copy', out)


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
    args = sys.argv[1:]
    compose_only = '--compose-only' in args
    realign = '--realign' in args
    names = [a for a in args if a not in ('--compose-only', '--realign')] or list(SEGMENTS)
    work = OUT / 'work'
    work.mkdir(parents=True, exist_ok=True)
    parts = []
    try:
        for name in names:
            if compose_only:
                # Reuse the recording and replay from an earlier run.
                orig, frames, wav = (work / f'{name}_orig.mkv', work / f'{name}_frames',
                                     work / f'{name}_remake.wav')
            elif realign:
                # Reuse the recording; redo the input alignment and the replay.
                orig, frames, wav = align_segment(name, work)
            else:
                orig, frames, wav = record_segment(name, SEGMENTS[name], work)
            part = work / f'{name}.mp4'
            # Remake frame N shows the state after tick N, at N * 133 ms.
            compose(orig, ['-framerate', str(1 / TICK), '-i', str(frames / '%05d.png')], part,
                    remake_wav=wav)
            parts.append(part)
            print(f'{name}: {part}')
    finally:
        if SLOT_FILE.exists():
            SLOT_FILE.unlink()             # only the slot this script wrote
    out = OUT / 'side_by_side.mp4'
    concat(parts, out)
    concat([p.with_name(p.stem + '_mixdown.mp4') for p in parts], OUT / 'side_by_side_mixdown.mp4')
    print(out)


if __name__ == '__main__':
    main()
