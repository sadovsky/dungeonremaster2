#!/usr/bin/env python3
"""DM2 audio / distribution extraction. See docs/11-audio.md, docs/14-cutscenes.md.

All output goes to re/audio/ (gitignored). Nothing here is meant for the repo.

  audio.py sfx        GRAPHICS.DAT type-2 sounds -> re/audio/sfx/*.wav
  audio.py music      GRAPHICS.DAT type-3 HMP songs -> re/audio/music/*.hmp + *.mid
  audio.py hmp FILE   convert one HMP file to FILE.mid
  audio.py songlist   print map -> song table
  audio.py banks      list MELODIC.BNK / DRUM.BNK instruments
  audio.py mve        carve the MVE movies out of FTL/INTRO/END/CREDITS (+CD .VGA variants)
  audio.py cd         list files on cd/DM2.img (MODE1/2352 ISO9660)
"""
import struct
import sys
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GAME = ROOT / 'original/dumast2'
OUT = ROOT / 're/audio'
sys.path.insert(0, str(Path(__file__).resolve().parent))

SFX_HEADER = 6          # u16 sample rate, then 4 bytes (08 01 00 00: 8-bit, mono?)
MVE_OFFSET = 1330 + 98876  # DPMI stub + embedded MVE player; the movie follows
MVE_SIG = b'Interplay MVE File\x1a\x00'


# ---------------------------------------------------------------- sound effects

def sfx():
    from gdat import Gdat
    g = Gdat()
    d = OUT / 'sfx'; d.mkdir(parents=True, exist_ok=True)
    done = {}
    for r in g.records:
        if r['D'] != 2:
            continue
        e = r['P'] & 0x7FFF
        raw = g.entry(e)
        rate = struct.unpack_from('<H', raw)[0]
        name = f"c{r['T']:02}_i{r['I']:03}_s{r['S']:03}_e{e:04}.wav"
        with wave.open(str(d / name), 'wb') as w:
            w.setnchannels(1); w.setsampwidth(1); w.setframerate(rate)
            w.writeframes(raw[SFX_HEADER:])   # stored as unsigned 8-bit
        done[e] = name
    print(f'{len(done)} unique samples, {sum(1 for r in g.records if r["D"] == 2)} keys -> {d}')


# ---------------------------------------------------------------- HMP music

def _rev_vlq(b, p):
    """HMP delta: 7-bit groups little-endian; a set high bit marks the LAST byte."""
    v = shift = 0
    while True:
        c = b[p]; p += 1
        v |= (c & 0x7F) << shift
        shift += 7
        if c & 0x80:
            return v, p


def _vlq(v):
    out = [v & 0x7F]
    v >>= 7
    while v:
        out.append(0x80 | (v & 0x7F)); v >>= 7
    return bytes(reversed(out))


def _midi_vlq_read(b, p):
    v = 0
    while True:
        c = b[p]; p += 1
        v = (v << 7) | (c & 0x7F)
        if not c & 0x80:
            return v, p


DATA_LEN = {0x80: 2, 0x90: 2, 0xA0: 2, 0xB0: 2, 0xC0: 1, 0xD0: 1, 0xE0: 2}


def hmp_to_midi(h):
    """Return (midi_bytes, info). Track 0 of an HMP is a tempo/meta track."""
    assert h[:8] == b'HMIMIDIP', 'not an HMP file'
    first = 0x388 if h[8:14] == b'013195' else 0x308
    # 0x34 is not the timing base; 0x38 is the tick rate in Hz (120 in every DM2 song,
    # verified against the song-length field at 0x3C).
    ntracks, unknown34, tick_hz, seconds = struct.unpack_from('<4I', h, 0x30)
    p, tracks = first, []
    for _ in range(ntracks):
        _idx, length, _chan = struct.unpack_from('<3I', h, p)
        body, p = h[p + 12:p + length], p + length
        out, q, status = bytearray(), 0, 0
        if not tracks:  # one quarter = one second, division = tick rate
            out += b'\x00\xff\x51\x03' + (1_000_000).to_bytes(3, 'big')
        while q < len(body):
            delta, q = _rev_vlq(body, q)
            out += _vlq(delta)
            c = body[q]
            if c == 0xFF:                          # meta: FF type len data
                t, ln = body[q + 1], body[q + 2]
                out += body[q:q + 3 + ln]; q += 3 + ln
                if t == 0x2F:
                    break
            elif c in (0xF0, 0xF7):                 # sysex
                ln, q2 = _midi_vlq_read(body, q + 1)
                out += body[q:q2 + ln]; q = q2 + ln
            else:
                if c & 0x80:
                    status = c; q += 1
                    out.append(c)
                n = DATA_LEN[status & 0xF0]
                out += body[q:q + n]; q += n
        if not out.endswith(b'\xff\x2f\x00'):
            out += b'\x00\xff\x2f\x00'
        tracks.append(bytes(out))
    mid = b'MThd' + struct.pack('>IHHH', 6, 1, len(tracks), tick_hz)
    for t in tracks:
        mid += b'MTrk' + struct.pack('>I', len(t)) + t
    return mid, dict(tracks=ntracks, field34=unknown34, tick_hz=tick_hz, seconds=seconds)


def music():
    from gdat import Gdat
    g = Gdat()
    d = OUT / 'music'; d.mkdir(parents=True, exist_ok=True)
    for r in g.records:
        if r['D'] != 3:
            continue
        raw = g.entry(r['P'] & 0x7FFF)
        stem = d / f"song{r['I']:02}"
        stem.with_suffix('.hmp').write_bytes(raw)
        mid, info = hmp_to_midi(raw)
        stem.with_suffix('.mid').write_bytes(mid)
        print(f"song {r['I']:2}: {info}")


def songlist():
    raw = (GAME / 'DATA/SONGLIST.DAT').read_bytes()
    for m, s in enumerate(raw):
        if s == 0xFF:
            break
        print(f'map {m:2} -> song {s}' + (' (silence)' if s == 0 else ''))


# ---------------------------------------------------------------- FM banks

def banks():
    for name in ('MELODIC.BNK', 'DRUM.BNK'):
        b = (GAME / name).read_bytes()
        sig = b[2:8]
        used, total, name_off, data_off = struct.unpack_from('<HHII', b, 8)
        print(f'{name}: signature {sig!r}, {used} used / {total} instruments, '
              f'names @{name_off:#x}, data @{data_off:#x}, size {len(b)}')
        for i in range(total):
            idx, flag = struct.unpack_from('<HB', b, name_off + 12 * i)
            nm = b[name_off + 12 * i + 3:name_off + 12 * i + 12].split(b'\0')[0].decode('latin1')
            rec = b[data_off + 30 * idx:data_off + 30 * idx + 30]
            print(f'  {i:3} {nm:9} perc={rec[0]} voice={rec[1]}')


# ---------------------------------------------------------------- movies / CD

def mve():
    d = OUT / 'mve'; d.mkdir(parents=True, exist_ok=True)
    srcs = [GAME / n for n in ('FTL', 'INTRO', 'END', 'CREDITS')]
    cd = ROOT / 're/cd'
    srcs += [p for p in (cd / 'FTL.VGA', cd / 'INTRO.VGA', cd / 'END.VGA') if p.exists()]
    for s in srcs:
        b = s.read_bytes()
        assert b[MVE_OFFSET:MVE_OFFSET + len(MVE_SIG)] == MVE_SIG, s
        (d / (s.name.replace('.', '_') + '.mve')).write_bytes(b[MVE_OFFSET:])
        print(f'{s.name}: {len(b) - MVE_OFFSET} bytes of MVE')


def iso_files(img=GAME / 'cd/DM2.img'):
    f = open(img, 'rb')
    sec = lambda n: (f.seek(n * 2352 + 16), f.read(2048))[1]
    read = lambda lba, size: b''.join(sec(lba + i) for i in range((size + 2047) // 2048))[:size]
    pvd = sec(16)
    assert pvd[1:6] == b'CD001'
    out = {}

    def walk(lba, size, path):
        d, o = read(lba, size), 0
        while o < len(d):
            ln = d[o]
            if ln == 0:
                o = (o // 2048 + 1) * 2048; continue
            elba, esz = struct.unpack_from('<I', d, o + 2)[0], struct.unpack_from('<I', d, o + 10)[0]
            flags, name = d[o + 25], d[o + 33:o + 33 + d[o + 32]]
            o += ln
            if name in (b'\0', b'\1'):
                continue
            n = name.decode().split(';')[0]
            if flags & 2:
                walk(elba, esz, path + n + '/')
            else:
                out[path + n] = (elba, esz)
    walk(struct.unpack_from('<I', pvd, 158)[0], struct.unpack_from('<I', pvd, 166)[0], '')
    return out, read


def cd():
    files, read = iso_files()
    dest = ROOT / 're/cd'
    for n, (lba, size) in sorted(files.items()):
        print(f'{size:10} {n}')
    if '--extract' in sys.argv:
        dest.mkdir(parents=True, exist_ok=True)
        for n, (lba, size) in files.items():
            (dest / n.replace('/', '_')).write_bytes(read(lba, size))
        print('extracted to', dest)


if __name__ == '__main__':
    cmd = sys.argv[1] if len(sys.argv) > 1 else ''
    if cmd == 'hmp':
        p = Path(sys.argv[2]); m, info = hmp_to_midi(p.read_bytes())
        p.with_suffix('.mid').write_bytes(m); print(info)
    elif cmd in ('sfx', 'music', 'songlist', 'banks', 'mve', 'cd'):
        globals()[cmd]()
    else:
        print(__doc__)
