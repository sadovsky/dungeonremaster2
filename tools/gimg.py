#!/usr/bin/env python3
"""Image decoders for GRAPHICS.DAT (type 1 entries) and the master palette.

See docs/02-graphics-dat.md, sections "Image encoding" and "Palettes".
Used by `gdat.py export`.
"""
import struct
import zlib

TAG_8BPP = 31   # height-word tag: LZSS-compressed 8-bit image
TAG_RAW = 32    # height-word tag: uncompressed image with a 10-byte header


class Image:
    """Decoded image: `pixels` are palette indices, row-major, w*h."""
    def __init__(self, w, h, pixels, bpp, flags):
        self.w, self.h, self.pixels, self.bpp, self.flags = w, h, pixels, bpp, flags


def lzss(src, len_bits):
    """Formats 2 (len_bits=4) and 3 (len_bits=5): flag byte LSB first,
    1 = literal byte, 0 = 2-byte back reference into the output."""
    out = bytearray()
    i = 0
    flags = 0
    while True:
        flags >>= 1
        if not flags & 0x100:
            if i >= len(src):
                break
            flags = src[i] | 0xFF00
            i += 1
        if flags & 1:
            if i >= len(src):
                break
            out.append(src[i])
            i += 1
        else:
            if i + 2 > len(src):
                break
            b1, b2 = src[i], src[i + 1]
            i += 2
            n = (b1 & ((1 << len_bits) - 1)) + 3
            dist = (b2 << (8 - len_bits)) + (b1 >> len_bits)
            for _ in range(n):
                out.append(out[len(out) - dist])
    return bytes(out)


def lzw_rle(src):
    """Format 1: Unix-compress style LZW (9..12 bit codes, 256 = clear,
    codes read in groups of eight) followed by 0x90 run-length expansion.
    No image in the PC GRAPHICS.DAT uses it, but the game supports it."""
    pos = 0
    nbits, maxcode, free = 9, 0x1FF, 0x101
    clear_flag = False

    def codes():
        nonlocal pos, nbits, maxcode, clear_flag
        val = bitoff = cnt = 0
        while True:
            if free > maxcode or clear_flag or cnt == 0:
                if free > maxcode:
                    nbits += 1
                    maxcode = 0x1000 if nbits == 12 else (1 << nbits) - 1
                if clear_flag:
                    nbits, maxcode, clear_flag = 9, 0x1FF, False
                n = min(nbits, len(src) - pos)
                if n <= 0:
                    return
                val = int.from_bytes(src[pos:pos + n], 'little')
                pos += n
                cnt = 8 if n == nbits else (n * 8) // nbits
                bitoff = 0
            yield (val >> bitoff) & ((1 << nbits) - 1)
            bitoff += nbits
            cnt -= 1

    prefix = [0] * 4096
    suffix = list(range(256)) + [0] * (4096 - 256)
    out = bytearray()
    rle, last = False, 0

    def emit(b):
        nonlocal rle, last
        if rle:
            if b == 0:
                out.append(0x90)
            else:
                out.extend([last] * (b - 1))
            rle = False
        elif b == 0x90:
            rle = True
        else:
            out.append(b)
            last = b

    it = codes()
    old = next(it, None)
    if old is None:
        return bytes(out)
    fin = old
    emit(old)
    for code in it:
        if code == 256:
            free, clear_flag = 0x100, True
            continue
        incode = code
        stack = []
        if code >= free:
            stack.append(fin)
            code = old
        while code >= 256:
            stack.append(suffix[code])
            code = prefix[code]
        fin = suffix[code]
        stack.append(fin)
        for b in reversed(stack):
            emit(b)
        if free < 4096:
            prefix[free], suffix[free] = old, fin
            free += 1
        old = incode
    return bytes(out)


class _Nibbles:
    def __init__(self, data, pos):
        self.d, self.p = data, pos

    def get(self):
        b = self.d[self.p >> 1]
        v = b & 0x0F if self.p & 1 else b >> 4   # high nibble first
        self.p += 1
        return v

    def count(self):
        n = self.get()
        if n != 15:
            return n + 2
        n = (self.get() << 4) | self.get()
        if n != 0xFF:
            return n + 17
        return (self.get() << 12) | (self.get() << 8) | (self.get() << 4) | self.get()


def decode_4bpp(data, w, h, base=None):
    """Nibble-RLE 4-bit image; returns nibble values (0..15), w*h.
    With `base` (a decoded image of the same size) it is a delta image:
    the table has 5 colours and op 5 copies the base pixel."""
    ns = _Nibbles(data, 8)  # stream starts right after the 4-byte header
    ntab = 5 if base is not None else 6
    table = [ns.get() for _ in range(ntab)]
    total = w * h
    px = bytearray(total)
    i = 0
    while i < total:
        cmd = ns.get()
        op = cmd & 7
        if op == 6:                       # copy from the row above
            n = ns.count() if cmd & 8 else 1
            for _ in range(n):
                if i < total:
                    px[i] = px[i - w] if i >= w else 0
                i += 1
        elif op == 5 and base is not None:  # keep base pixel
            n = ns.count() if cmd & 8 else 1
            for _ in range(n):
                if i < total:
                    px[i] = base[i]
                i += 1
        else:
            colour = table[op] if op < ntab else ns.get()  # 7 = literal nibble
            n = ns.count() if cmd & 8 else 1               # count after colour
            for _ in range(n):
                if i < total:
                    px[i] = colour
                i += 1
    return bytes(px), (ns.p + 1) // 2


def decode(entry):
    """Decode a type-1 entry to an Image of palette indices."""
    w0, w1 = struct.unpack_from('<HH', entry, 0)
    w, h = w0 & 0x3FF, w1 & 0x3FF
    flags = (w0 >> 10, w1 >> 10)
    tag = w1 >> 10
    if tag == TAG_RAW:
        bpp = struct.unpack_from('<H', entry, 4)[0]
        body = entry[10:]
        if bpp == 8:
            return Image(w, h, bytes(body[:w * h]), 8, flags)
        stride = (w + 1) // 2
        cmap = entry[-16:]
        px = bytearray()
        for y in range(h):
            row = body[y * stride:(y + 1) * stride]
            for x in range(w):
                nib = row[x >> 1] >> 4 if not x & 1 else row[x >> 1] & 0x0F
                px.append(cmap[nib])
        return Image(w, h, bytes(px), 4, flags)
    if tag == TAG_8BPP:
        fmt = entry[6]
        src = entry[8:]
        if fmt == 1:
            px = lzw_rle(src)
        elif fmt in (2, 3):
            px = lzss(src, 4 if fmt == 2 else 5)
        else:
            raise ValueError(f'unknown 8-bit format {fmt}')
        return Image(w, h, px[:w * h], 8, flags)
    nib, _ = decode_4bpp(entry, w, h)
    cmap = entry[-16:]
    return Image(w, h, bytes(cmap[n] for n in nib), 4, flags)


def master_palette(gdat):
    """Entry (1,0,9,254): 256 x (index, R, G, B), 8-bit components."""
    raw = gdat.entry(gdat.lookup(1, 0, 9, 254))
    return [tuple(raw[4 * i + 1:4 * i + 4]) for i in range(256)]


def write_png(path, w, h, pixels, palette):
    """Minimal 8-bit indexed PNG writer (no external dependencies)."""
    def chunk(t, d):
        return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xFFFFFFFF)
    raw = b''.join(b'\0' + bytes(pixels[y * w:(y + 1) * w]) for y in range(h))
    plte = b''.join(bytes(c) for c in palette)
    with open(path, 'wb') as f:
        f.write(b'\x89PNG\r\n\x1a\n')
        f.write(chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 3, 0, 0, 0)))
        f.write(chunk(b'PLTE', plte))
        f.write(chunk(b'IDAT', zlib.compress(raw, 9)))
        f.write(chunk(b'IEND', b''))
