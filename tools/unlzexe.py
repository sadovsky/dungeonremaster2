#!/usr/bin/env python3
"""Unpack an LZEXE 0.91 compressed DOS executable's load image.

Usage: unlzexe.py IN.EXE OUT.BIN

Writes the decompressed load module (code and data as they would sit in
memory, starting at the load segment). Relocations are not applied; the
output is for disassembly only (see docs/05, "Tick length").
"""
import struct
import sys
from pathlib import Path


class Bits:
    """LZEXE bit reader: 16-bit little-endian words, bits taken LSB first,
    with the next word loaded as soon as the current one is used up."""
    def __init__(self, data, pos):
        self.d, self.p = data, pos
        self._load()

    def _load(self):
        self.word = struct.unpack_from('<H', self.d, self.p)[0]
        self.p += 2
        self.left = 16

    def bit(self):
        b = self.word & 1
        self.word >>= 1
        self.left -= 1
        if self.left == 0:
            self._load()
        return b

    def byte(self):
        b = self.d[self.p]
        self.p += 1
        return b


def unpack(exe):
    hdr_paras, cs, ip = struct.unpack_from('<H', exe, 8)[0], *struct.unpack_from('<HH', exe, 0x16)[::-1]
    base = hdr_paras * 16
    loader = base + cs * 16
    # Loader data block at CS:0: IP, CS, SP, SS, compressed size (paragraphs) ...
    real_ip, real_cs, real_sp, real_ss, cmp_paras = struct.unpack_from('<5H', exe, loader)
    # The compressed stream sits just below the loader; it starts at the load module.
    src = Bits(exe, base)
    out = bytearray()
    while True:
        if src.bit():
            out.append(src.byte())
            continue
        if not src.bit():
            length = ((src.bit() << 1) | src.bit()) + 2
            span = src.byte() - 0x100
        else:
            lo, hi = src.byte(), src.byte()
            span = lo | ((hi & 0xF8) << 5) | 0xE000
            span -= 0x10000
            length = hi & 7
            if length:
                length += 2
            else:
                length = src.byte()
                if length == 0:
                    break  # end of stream
                if length == 1:
                    continue  # segment boundary marker
                length += 1
        for _ in range(length):
            out.append(out[span])
    return out, (real_cs, real_ip, real_ss, real_sp)


def main():
    exe = Path(sys.argv[1]).read_bytes()
    out, (cs, ip, ss, sp) = unpack(exe)
    Path(sys.argv[2]).write_bytes(out)
    print(f'{len(out)} bytes, entry {cs:04x}:{ip:04x}, stack {ss:04x}:{sp:04x}')


if __name__ == '__main__':
    main()
