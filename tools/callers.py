#!/usr/bin/env python3
"""List the functions that call ADDR, with the calling line.

  callers.py ADDR

Works on the gitignored decompilation listing re/skull_decomp.c.
"""
import re
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / 're/skull_decomp.c'


def main():
    target = f'FUN_{int(sys.argv[1], 16):08x}('
    current = None
    for line in SRC.read_text().splitlines():
        m = re.match(r'//==== \S+ @ ([0-9a-f]{8})', line)
        if m:
            current = int(m.group(1), 16)
            continue
        if target in line and not line.lstrip().startswith(('void', 'int', 'uint', 'short', 'ushort', 'byte', 'char', 'undefined')):
            print(f'{current:#x}: {line.strip()[:140]}')


if __name__ == '__main__':
    main()
