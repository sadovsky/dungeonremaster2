#!/usr/bin/env python3
"""Dump SKULL.EXE's mouse-zone tables (see docs/10-ui-input.md).

Reads the relocated data object produced by tools/le_unpack.py
(re/skull/obj2.bin, base 0x70000).

  uizones.py lists      every zone list: command, rect id, buttons, flags
  uizones.py screens    the 7-byte screen records that point at the lists
  uizones.py keys       keyboard lists: command and BIOS key code
"""
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA_BASE = 0x70000
ZONES = 0x72388      # 6-byte zone records
KEYS = 0x729B8       # 4-byte (command, key) records after the zone lists
NODES = 0x72B9C      # selector tree nodes
ROOTS = 0x72CCC      # selector root lists
SCREENS = 0x72D1D    # 7-byte screen records

# Command numbers handled by the dispatcher at 0x21D6C (working names).
COMMANDS = {
    1: 'turn left', 2: 'turn right', 3: 'move forward', 4: 'move right',
    5: 'move back', 6: 'move left',
    7: 'champion 1 portrait', 8: 'champion 2 portrait', 9: 'champion 3 portrait',
    10: 'champion 4 portrait', 11: 'close inventory / leader portrait',
    0x10: 'party cell front-left', 0x11: 'party cell front-right',
    0x12: 'party cell back-right', 0x13: 'party cell back-left',
    0x46: 'mouth (eat/drink)', 0x47: 'eye (inspect while held)', 0x48: 'rename champion',
    0x49: 'rename: name field', 0x4A: 'rename: title field', 0x4B: 'rename: cancel',
    0x50: 'viewport click', 0x52: 'leader command 0x52?', 0x55: 'champion name/leader',
    0x5D: 'rotate champion left', 0x5E: 'rotate champion right',
    0x6B: 'spell: delete rune', 0x6C: 'spell: cast',
    0x70: 'action menu: cancel', 0x8C: 'save game', 0x8D: 'sleep?', 0x8E: 'options/quit menu?',
    0x8F: 'unknown 0x8f', 0x90: 'pause', 0x91: 'resume',
    0xD7: 'title: start', 0xD8: 'title: start (alternate dungeon)', 0xD9: 'title: resume saved game',
    0xDA: 'title: 0xda', 0xE0: 'title: 0xe0',
}


def name(cmd):
    if cmd in COMMANDS:
        return COMMANDS[cmd]
    for lo, hi, label in ((0x14, 0x41, 'inventory slot'), (0x56, 0x59, 'action list'),
                          (0x5F, 0x62, 'leader by cell'), (0x65, 0x6A, 'spell rune'),
                          (0x71, 0x73, 'action choice'), (0x74, 0x7B, 'hand icon'),
                          (0x7D, 0x81, 'unknown 0x7d-0x81'), (0xE4, 0xE9, 'dialog button'), (0xDB, 0xDE, 'dialog choice'),
                          (0xA5, 0xD6, 'text entry key'),
                          (0xEA, 0xED, 'menu choice')):
        if lo <= cmd <= hi:
            return f'{label} {cmd - lo}'
    return ''


def load():
    return (ROOT / 're/skull/obj2.bin').read_bytes()


def u16(d, a):
    return struct.unpack_from('<H', d, a - DATA_BASE)[0]


def zone_lists(d):
    """Lists start at a record whose word 0 has bit 15 set."""
    a, lists, cur = ZONES, [], None
    while a + 6 <= KEYS - 6:  # the key table follows; the last record is a 0x8000 terminator
        w0, w1, w2 = struct.unpack_from('<HHH', d, a - DATA_BASE)
        if w0 & 0x8000:
            cur = ((a - ZONES) // 6, [])
            lists.append(cur)
        if cur is not None:
            cur[1].append((w0 & 0x7FF, w1, w2))
        a += 6
    return lists


def main():
    d = load()
    cmd = sys.argv[1] if len(sys.argv) > 1 else 'lists'
    if cmd == 'lists':
        for start, recs in zone_lists(d):
            print(f'list @{start}')
            for c, rect, btn in recs:
                flags = []
                if rect & 0x8000: flags.append('anchor7')
                if rect & 0x4000: flags.append('anchor18')
                if btn & 0x800: flags.append('disabled')
                print(f'   cmd {c:#05x} rect {rect & 0x3FFF:4} buttons {btn & 0xFF:#04x} '
                      f'x{btn >> 8:02x} {" ".join(flags):18} {name(c)}')
    elif cmd == 'keys':
        a = KEYS
        while a < NODES:
            c, key = struct.unpack_from('<HH', d, a - DATA_BASE)
            if c == 0x8000 and key == 0:
                print('-- end'); a += 4; continue
            if c & 0x8000:
                print(f'list @{(a - KEYS) // 4}')
            mods = []
            if key & 0x200: mods.append('shift')
            if key & 0x400: mods.append('alt')
            if key & 0x800: mods.append('ctrl')
            print(f'   cmd {c & 0x7FFF:#05x} key {key & 0xFF:#04x} {"+".join(mods):10} {name(c & 0x7FFF)}')
            a += 4
    elif cmd == 'screens':
        for i in range(0, (ROOTS - SCREENS) // 7 + 40):
            a = SCREENS + 7 * i
            if a + 7 - DATA_BASE > len(d):
                break
            c, rect, lst = struct.unpack_from('<HHh', d, a - DATA_BASE)
            print(f'{i:3} cond {c:#06x} rect {rect:#06x} list {lst} b {d[a + 6 - DATA_BASE]:#04x}')


if __name__ == '__main__':
    main()
