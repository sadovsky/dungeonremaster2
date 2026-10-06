#!/usr/bin/env python3
"""Unpack a DOS4GW LE executable into a flat, relocated memory image.

Each object is placed at its preferred base address and every internal
fixup is applied, so the output can be loaded into Ghidra (raw binary,
x86:LE:32:default) or disassembled with capstone without a custom loader.

Usage: le_unpack.py SKULL.EXE out_dir
Writes out_dir/objN.bin per object and out_dir/layout.json describing
bases, sizes, flags, entry point and the list of fixup sites.
"""
import json
import struct
import sys
from pathlib import Path


def u8(d, o): return d[o]
def u16(d, o): return struct.unpack_from('<H', d, o)[0]
def i16(d, o): return struct.unpack_from('<h', d, o)[0]
def u32(d, o): return struct.unpack_from('<I', d, o)[0]


def unpack(exe: bytes):
    le = u32(exe, 0x3C)
    assert exe[le:le + 2] == b'LE', 'not an LE executable'
    H = lambda o: u32(exe, le + o)
    page_size = H(0x28)
    last_page = H(0x2C)
    n_pages = H(0x14)
    data_pages = H(0x80)
    objtab, nobj = le + H(0x40), H(0x44)
    fixpage = le + H(0x68)
    fixrec = le + H(0x6C)

    objs = []
    for i in range(nobj):
        vsize, base, flags, pidx, pcnt, _ = struct.unpack_from('<6I', exe, objtab + 24 * i)
        objs.append(dict(index=i + 1, vsize=vsize, base=base, flags=flags,
                         first_page=pidx, page_count=pcnt))

    # Load page contents. Object page table entries are 4 bytes (LE format):
    # 3-byte big-endian page number + 1 byte flags; pages are contiguous here.
    mem = {}
    for ob in objs:
        buf = bytearray(ob['vsize'])
        for k in range(ob['page_count']):
            page_no = ob['first_page'] + k  # 1-based
            size = last_page if page_no == n_pages else page_size
            src = data_pages + (page_no - 1) * page_size
            chunk = exe[src:src + size]
            buf[k * page_size:k * page_size + len(chunk)] = chunk
        mem[ob['index']] = buf

    fixups = []
    for ob in objs:
        for k in range(ob['page_count']):
            page_no = ob['first_page'] + k
            start = fixrec + u32(exe, fixpage + 4 * (page_no - 1))
            end = fixrec + u32(exe, fixpage + 4 * page_no)
            p = start
            while p < end:
                src_type, flags = exe[p], exe[p + 1]
                p += 2
                if src_type & 0x20:
                    count = exe[p]; p += 1
                    srcoffs = None
                else:
                    srcoffs = [i16(exe, p)]; p += 2
                assert flags & 3 == 0, f'non-internal fixup {flags:#x}'
                if flags & 0x40:
                    tobj = u16(exe, p); p += 2
                else:
                    tobj = exe[p]; p += 1
                kind = src_type & 0x0F
                if kind == 0x02:  # 16-bit selector only
                    toff = 0
                elif flags & 0x10:
                    toff = u32(exe, p); p += 4
                else:
                    toff = u16(exe, p); p += 2
                if srcoffs is None:
                    srcoffs = [i16(exe, p + 2 * j) for j in range(count)]
                    p += 2 * count
                target = objs[tobj - 1]['base'] + toff
                buf = mem[ob['index']]
                for so in srcoffs:
                    off = k * page_size + so
                    if off < 0 or off >= len(buf):
                        continue  # fixup straddling a page edge is listed on both pages
                    site = ob['base'] + off
                    if kind == 0x07:  # 32-bit offset
                        if off + 4 <= len(buf):
                            struct.pack_into('<I', buf, off, target)
                    elif kind == 0x08:  # 32-bit self-relative
                        if off + 4 <= len(buf):
                            struct.pack_into('<i', buf, off, target - (site + 4))
                    elif kind == 0x05:  # 16-bit offset
                        if off + 2 <= len(buf):
                            struct.pack_into('<H', buf, off, target & 0xFFFF)
                    elif kind == 0x02:  # selector: leave a recognisable marker
                        if off + 2 <= len(buf):
                            struct.pack_into('<H', buf, off, tobj)
                    elif kind == 0x06:  # 16:32 far pointer
                        if off + 6 <= len(buf):
                            struct.pack_into('<IH', buf, off, target, tobj)
                    else:
                        raise ValueError(f'unhandled fixup type {src_type:#x}')
                    fixups.append((site, kind, target))

    eip = objs[H(0x18) - 1]['base'] + H(0x1C)
    esp = objs[H(0x20) - 1]['base'] + H(0x24)
    return objs, mem, fixups, eip, esp


def main():
    exe = Path(sys.argv[1]).read_bytes()
    out = Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
    objs, mem, fixups, eip, esp = unpack(exe)
    for ob in objs:
        (out / f"obj{ob['index']}.bin").write_bytes(mem[ob['index']])
        ob['executable'] = bool(ob['flags'] & 4)
    json.dump(dict(objects=objs, entry=eip, stack=esp,
                   fixups=sorted(set(fixups))), open(out / 'layout.json', 'w'))
    print(f'entry {eip:#x}, {len(fixups)} fixups')
    for ob in objs:
        print(f"obj{ob['index']} base {ob['base']:#x} size {ob['vsize']:#x} "
              f"{'code' if ob['executable'] else 'data'}")


if __name__ == '__main__':
    main()
