#!/usr/bin/env python3
"""Short-term loudness around logged events in a WAV: sfx_env.py OUT
(reads OUT.wav and OUT.events). Prints, per event, the peak 50 ms RMS in the
1.2 s after it against the median RMS of the whole clip (dB)."""
import math, struct, subprocess, sys
out = sys.argv[1]
raw = subprocess.run(['ffmpeg', '-v', 'error', '-i', out + '.wav', '-ac', '1', '-ar', '22050',
                      '-f', 's16le', '-'], capture_output=True, check=True).stdout
s = struct.unpack(f'<{len(raw)//2}h', raw)
win = 1102
rms = [math.sqrt(sum(v*v for v in s[i:i+win]) / win) + 1e-9 for i in range(0, len(s) - win, win)]
db = [20 * math.log10(r / 32768) for r in rms]
med = sorted(db)[len(db) // 2]
print(f'clip {len(s)/22050:.1f}s  median {med:.1f} dB  max {max(db):.1f} dB')
for line in open(out + '.events'):
    name, t = line.split(); t = float(t)
    i0, i1 = int(t / 0.05), int((t + 1.2) / 0.05)
    seg = db[i0:i1]
    if seg:
        k = max(range(len(seg)), key=lambda j: seg[j])
        print(f'{name} at {t:5.2f}s: peak {seg[k]:6.1f} dB at +{k*0.05:.2f}s  ({seg[k]-med:+.1f} over median)')
