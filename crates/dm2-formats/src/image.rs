//! Image decoders for GRAPHICS.DAT type-1 entries, plus the master palette.
//!
//! Formats: docs/02-graphics-dat.md, "Image encoding" and "Palettes".

use crate::gdat::{Gdat, Key, kind};

/// Height-word tag (bits 10..15) for LZSS-compressed 8-bit images.
pub const TAG_8BPP: u16 = 31;
/// Height-word tag for uncompressed images with a 10-byte header.
pub const TAG_RAW: u16 = 32;

#[derive(Clone, Debug)]
pub struct Image {
    pub width: u16,
    pub height: u16,
    /// Palette indices, row-major, `width * height`.
    pub pixels: Vec<u8>,
    /// Source depth: 4 (via a 16-entry colour map) or 8.
    pub bpp: u8,
    /// Upper 6 bits of the width and height words (meaning partly unknown).
    pub width_tag: u16,
    pub height_tag: u16,
    /// For 4-bit sources: nibble value -> palette index. Callers that need
    /// the original nibbles (e.g. colour-key or recolouring) can invert it.
    pub colour_map: Option<[u8; 16]>,
}

#[derive(Debug, PartialEq)]
pub enum DecodeError {
    TooShort,
    UnknownFormat(u8),
    Truncated,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DecodeError {}

fn u16le(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}

/// Decode one type-1 entry.
pub fn decode(entry: &[u8]) -> Result<Image, DecodeError> {
    if entry.len() < 4 {
        return Err(DecodeError::TooShort);
    }
    let (w0, w1) = (u16le(entry, 0), u16le(entry, 2));
    let (w, h) = (w0 & 0x3FF, w1 & 0x3FF);
    let (width_tag, height_tag) = (w0 >> 10, w1 >> 10);
    let n = w as usize * h as usize;
    let mk = |pixels: Vec<u8>, bpp: u8, colour_map: Option<[u8; 16]>| Image {
        width: w,
        height: h,
        pixels,
        bpp,
        width_tag,
        height_tag,
        colour_map,
    };
    match height_tag {
        TAG_RAW => {
            if entry.len() < 10 {
                return Err(DecodeError::TooShort);
            }
            let body = &entry[10..];
            if u16le(entry, 4) == 8 {
                let px = body.get(..n).ok_or(DecodeError::Truncated)?.to_vec();
                Ok(mk(px, 8, None))
            } else {
                let cmap = trailing_map(entry)?;
                let stride = (w as usize).div_ceil(2);
                let mut px = Vec::with_capacity(n);
                for y in 0..h as usize {
                    for x in 0..w as usize {
                        let b = *body.get(y * stride + x / 2).ok_or(DecodeError::Truncated)?;
                        let nib = if x & 1 == 0 { b >> 4 } else { b & 0x0F };
                        px.push(cmap[nib as usize]);
                    }
                }
                Ok(mk(px, 4, Some(cmap)))
            }
        }
        TAG_8BPP => {
            if entry.len() < 8 {
                return Err(DecodeError::TooShort);
            }
            let src = &entry[8..];
            let mut px = match entry[6] {
                1 => lzw_rle(src),
                2 => lzss(src, 4),
                3 => lzss(src, 5),
                f => return Err(DecodeError::UnknownFormat(f)),
            };
            if px.len() < n {
                return Err(DecodeError::Truncated);
            }
            px.truncate(n);
            Ok(mk(px, 8, None))
        }
        _ => {
            let cmap = trailing_map(entry)?;
            let nib = decode_4bpp(entry, w as usize, h as usize, None)?;
            Ok(mk(nib.iter().map(|&v| cmap[v as usize]).collect(), 4, Some(cmap)))
        }
    }
}

fn trailing_map(entry: &[u8]) -> Result<[u8; 16], DecodeError> {
    let at = entry.len().checked_sub(16).ok_or(DecodeError::TooShort)?;
    Ok(entry[at..].try_into().unwrap())
}

struct Nibbles<'a> {
    d: &'a [u8],
    p: usize,
}

impl Nibbles<'_> {
    fn get(&mut self) -> Result<u8, DecodeError> {
        let b = *self.d.get(self.p >> 1).ok_or(DecodeError::Truncated)?;
        let v = if self.p & 1 == 0 { b >> 4 } else { b & 0x0F };
        self.p += 1;
        Ok(v)
    }

    fn count(&mut self) -> Result<usize, DecodeError> {
        let n = self.get()? as usize;
        if n != 15 {
            return Ok(n + 2);
        }
        let n = ((self.get()? as usize) << 4) | self.get()? as usize;
        if n != 0xFF {
            return Ok(n + 17);
        }
        let mut v = 0usize;
        for _ in 0..4 {
            v = (v << 4) | self.get()? as usize;
        }
        Ok(v)
    }
}

/// Nibble-RLE 4-bit decoder (game: 0x12E4F; delta variant 0x13258).
/// Returns nibble values. With `base`, decodes a delta image whose table
/// has five colours and whose op 5 keeps the base pixel.
pub fn decode_4bpp(entry: &[u8], w: usize, h: usize, base: Option<&[u8]>) -> Result<Vec<u8>, DecodeError> {
    let mut ns = Nibbles { d: entry, p: 8 };
    let ntab = if base.is_some() { 5 } else { 6 };
    let mut table = [0u8; 6];
    for t in table.iter_mut().take(ntab) {
        *t = ns.get()?;
    }
    let total = w * h;
    let mut px = vec![0u8; total];
    let mut i = 0;
    while i < total {
        let cmd = ns.get()?;
        let op = (cmd & 7) as usize;
        if op == 6 {
            let n = if cmd & 8 != 0 { ns.count()? } else { 1 };
            for _ in 0..n {
                if i < total {
                    px[i] = if i >= w { px[i - w] } else { 0 };
                }
                i += 1;
            }
        } else if op == 5 && base.is_some() {
            let base = base.unwrap();
            let n = if cmd & 8 != 0 { ns.count()? } else { 1 };
            for _ in 0..n {
                if i < total {
                    px[i] = base.get(i).copied().unwrap_or(0);
                }
                i += 1;
            }
        } else {
            // The literal colour (op 7) is read before the run count.
            let colour = if op < ntab { table[op] } else { ns.get()? };
            let n = if cmd & 8 != 0 { ns.count()? } else { 1 };
            let end = (i + n).min(total);
            px[i..end].fill(colour);
            i += n;
        }
    }
    Ok(px)
}

/// LZSS used by 8-bit formats 2 (`len_bits` 4) and 3 (`len_bits` 5).
/// Game: 0x5AD2B and 0x5AD8D (hand-written assembly).
pub fn lzss(src: &[u8], len_bits: u32) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(src.len() * 3);
    let mut i = 0;
    let mut flags: u32 = 0;
    loop {
        flags >>= 1;
        if flags & 0x100 == 0 {
            let Some(&b) = src.get(i) else { break };
            flags = b as u32 | 0xFF00;
            i += 1;
        }
        if flags & 1 != 0 {
            let Some(&b) = src.get(i) else { break };
            out.push(b);
            i += 1;
        } else {
            if i + 2 > src.len() {
                break;
            }
            let (b1, b2) = (src[i] as usize, src[i + 1] as usize);
            i += 2;
            let n = (b1 & ((1 << len_bits) - 1)) + 3;
            let dist = (b2 << (8 - len_bits)) + (b1 >> len_bits);
            if dist == 0 || dist > out.len() {
                break; // corrupt stream; stop rather than panic
            }
            for _ in 0..n {
                out.push(out[out.len() - dist]);
            }
        }
    }
    out
}

/// Format 1: Unix-compress LZW (9..12 bits, code 256 = clear, codes read in
/// groups of eight) followed by 0x90 run-length expansion. Game: 0x5AADA.
pub fn lzw_rle(src: &[u8]) -> Vec<u8> {
    struct Reader<'a> {
        src: &'a [u8],
        pos: usize,
        nbits: u32,
        maxcode: usize,
        clear: bool,
        val: u128,
        bitoff: u32,
        left: u32,
    }
    impl Reader<'_> {
        fn next(&mut self, free: usize) -> Option<usize> {
            if free > self.maxcode || self.clear || self.left == 0 {
                if free > self.maxcode {
                    self.nbits += 1;
                    self.maxcode = if self.nbits == 12 { 0x1000 } else { (1 << self.nbits) - 1 };
                }
                if self.clear {
                    self.nbits = 9;
                    self.maxcode = 0x1FF;
                    self.clear = false;
                }
                let n = (self.nbits as usize).min(self.src.len().saturating_sub(self.pos));
                if n == 0 {
                    return None;
                }
                let mut v = 0u128;
                for (k, &b) in self.src[self.pos..self.pos + n].iter().enumerate() {
                    v |= (b as u128) << (8 * k);
                }
                self.pos += n;
                self.val = v;
                self.bitoff = 0;
                self.left = if n == self.nbits as usize { 8 } else { (n as u32 * 8) / self.nbits };
                if self.left == 0 {
                    return None;
                }
            }
            let c = ((self.val >> self.bitoff) as usize) & ((1 << self.nbits) - 1);
            self.bitoff += self.nbits;
            self.left -= 1;
            Some(c)
        }
    }

    let mut rd = Reader { src, pos: 0, nbits: 9, maxcode: 0x1FF, clear: false, val: 0, bitoff: 0, left: 0 };
    let mut prefix = vec![0u16; 4096];
    let mut suffix: Vec<u8> = (0..4096u32).map(|k| k as u8).collect();
    let mut out = Vec::new();
    let (mut rle, mut last) = (false, 0u8);
    let mut emit = |b: u8, out: &mut Vec<u8>| {
        if rle {
            if b == 0 {
                out.push(0x90);
            } else {
                out.extend(std::iter::repeat_n(last, b as usize - 1));
            }
            rle = false;
        } else if b == 0x90 {
            rle = true;
        } else {
            out.push(b);
            last = b;
        }
    };
    let mut free = 0x101usize;
    let Some(mut old) = rd.next(free) else { return out };
    let mut fin = old as u8;
    emit(fin, &mut out);
    let mut stack = Vec::new();
    while let Some(code) = rd.next(free) {
        if code == 256 {
            free = 0x100;
            rd.clear = true;
            continue;
        }
        let incode = code;
        let mut c = code;
        stack.clear();
        if c >= free {
            stack.push(fin);
            c = old;
        }
        while c >= 256 {
            stack.push(suffix[c]);
            c = prefix[c] as usize;
        }
        fin = suffix[c];
        stack.push(fin);
        for &b in stack.iter().rev() {
            emit(b, &mut out);
        }
        if free < 4096 {
            prefix[free] = old as u16;
            suffix[free] = fin;
            free += 1;
        }
        old = incode;
    }
    out
}

/// 256-colour master palette: entry (1,0,9,254), 256 x (index, R, G, B),
/// 8-bit components.
pub fn master_palette(g: &Gdat) -> Option<[[u8; 3]; 256]> {
    let raw = g.get(Key::new(1, 0, kind::TABLE_1024, 254))?;
    if raw.len() < 1024 {
        return None;
    }
    let mut pal = [[0u8; 3]; 256];
    for (i, c) in pal.iter_mut().enumerate() {
        c.copy_from_slice(&raw[4 * i + 1..4 * i + 4]);
    }
    Some(pal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gdat::default_path;

    #[test]
    fn lzss_roundtrip_small() {
        // flags 0b..0111_1011: lit 'a', lit 'b', ref(len 3, dist 2), lit 'c'...
        let src = [0b0000_1011u8, b'a', b'b', 0x20, 0x00, b'c'];
        assert_eq!(lzss(&src, 4), b"ababac".to_vec());
    }

    #[test]
    fn decode_all_images() {
        let p = default_path();
        if !p.exists() {
            eprintln!("skipping: {} not found", p.display());
            return;
        }
        let g = Gdat::open(p).unwrap();
        let pal = master_palette(&g).expect("palette");
        assert_eq!(pal[255], [255, 255, 255]);
        let mut seen = std::collections::HashSet::new();
        let (mut n4, mut n8) = (0, 0);
        for r in &g.records {
            if r.key.kind != kind::IMAGE {
                continue;
            }
            let e = r.entry().unwrap();
            if !seen.insert(e) {
                continue;
            }
            let data = g.entry(e).unwrap();
            let img = decode(data).unwrap_or_else(|err| panic!("entry {e}: {err}"));
            assert_eq!(img.pixels.len(), img.width as usize * img.height as usize, "entry {e}");
            match img.bpp {
                4 => n4 += 1,
                _ => n8 += 1,
            }
        }
        assert_eq!(seen.len(), 4031);
        assert!(n4 > 800 && n8 > 3000, "4bpp {n4}, 8bpp {n8}");
    }

    #[test]
    fn four_bit_stream_ends_at_colour_map() {
        let p = default_path();
        if !p.exists() {
            return;
        }
        let g = Gdat::open(p).unwrap();
        // The credits-sized dialog background (entry 3) and an icon (603).
        for e in [3u16, 603] {
            let d = g.entry(e).unwrap();
            let (w, h) = ((u16le(d, 0) & 0x3FF) as usize, (u16le(d, 2) & 0x3FF) as usize);
            let px = decode_4bpp(d, w, h, None).unwrap();
            assert_eq!(px.len(), w * h);
        }
    }
}
