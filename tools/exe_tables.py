#!/usr/bin/env python3
"""Print game-mechanics tables embedded in the user's own SKULL.EXE.

Nothing is copied into the repo; the remake should load these tables the same
way at runtime. See docs/06-champions.md and docs/07-combat-magic.md.

  exe_tables.py runes     rune mana costs and power multipliers
  exe_tables.py spells    the spell table, decoded
  exe_tables.py codes     action-string code slots
"""
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from le_unpack import unpack  # noqa: E402

EXE = Path(__file__).resolve().parent.parent / 'original/dumast2/SKULL.EXE'
DATA_BASE = 0x70000

RUNE_COSTS = 0x757DC      # 4 rows x 6 bytes
POWER_MULT = 0x757F4      # 6 bytes, eighths
SPELLS = 0x757FE          # 33 x 8 bytes
SPELL_COUNT = 33
ACTION_CODES = 0x7590E    # NUL-separated two-letter names
ROWS = 'PEFA'             # power, element, form, alignment
KINDS = {1: 'potion', 2: 'projectile', 3: 'other', 4: 'summon'}


def data_object():
    objs, mem, *_ = unpack(EXE.read_bytes())
    return mem[2]


def rune_name(sym):
    i = sym - 0x60
    return f'{ROWS[i // 6]}{i % 6 + 1}'


def main():
    d = data_object()
    at = lambda a, n: d[a - DATA_BASE:a - DATA_BASE + n]
    cmd = sys.argv[1] if len(sys.argv) > 1 else 'spells'
    if cmd == 'runes':
        costs = at(RUNE_COSTS, 24)
        for r in range(4):
            print(ROWS[r], list(costs[r * 6:r * 6 + 6]))
        print('power multiplier (x/8):', list(at(POWER_MULT, 6)))
    elif cmd == 'spells':
        print(' #  runes        base skill kind        type  dur')
        for i in range(SPELL_COUNT):
            key, = struct.unpack('<I', at(SPELLS + 8 * i, 4))
            base, skill = at(SPELLS + 8 * i + 4, 2)
            w, = struct.unpack('<H', at(SPELLS + 8 * i + 6, 2))
            power = key >> 24
            runes = ' '.join(rune_name((key >> s) & 0xFF) for s in (16, 8, 0) if (key >> s) & 0xFF)
            if power:
                runes = rune_name(power) + ' ' + runes
            print(f'{i:2}  {runes:12} {base:4} {skill:5} {w & 15} {KINDS.get(w & 15, "?"):10} '
                  f'{(w >> 4) & 0x3F:#5x} {w >> 10:4}')
    elif cmd == 'codes':
        names = at(ACTION_CODES, 64).split(b'\0')
        for i, n in enumerate(names):
            if not n:
                break
            print(i, n.decode())


if __name__ == '__main__':
    main()
