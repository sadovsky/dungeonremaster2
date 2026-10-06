//! Indexed-colour bitmaps, sprites and the basic blitter.

use dm2_formats::gdat::{Gdat, Key};
use dm2_formats::image;

pub const SCREEN_W: usize = 320;
pub const SCREEN_H: usize = 200;

/// An 8-bit indexed bitmap.
#[derive(Clone)]
pub struct Bitmap {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Bitmap {
    pub fn new(w: usize, h: usize) -> Self {
        Bitmap { w, h, px: vec![0; w * h] }
    }

    pub fn fill(&mut self, c: u8) {
        self.px.fill(c);
    }

    /// Copy another bitmap in at (x, y), clipped.
    pub fn paste(&mut self, src: &Bitmap, x: i32, y: i32) {
        for row in 0..src.h as i32 {
            let dy = y + row;
            if dy < 0 || dy >= self.h as i32 {
                continue;
            }
            for col in 0..src.w as i32 {
                let dx = x + col;
                if dx < 0 || dx >= self.w as i32 {
                    continue;
                }
                self.px[dy as usize * self.w + dx as usize] = src.px[row as usize * src.w + col as usize];
            }
        }
    }
}

/// A decoded GRAPHICS.DAT image ready for blitting.
///
/// `px` holds raw source values: nibbles for 4-bit images (mapped through
/// `cmap` at draw time, after the colour-key test, as the game does) and
/// palette indices for 8-bit images.
pub struct Sprite {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
    pub cmap: Option<[u8; 16]>,
    /// Drawing offset applied when placing through the layout table.
    pub off: (i32, i32),
}

fn s8(v: u16) -> i32 {
    (v & 0xFF) as u8 as i8 as i32
}

fn s6(v: u16) -> i32 {
    if v >= 32 { v as i32 - 64 } else { v as i32 }
}

impl Sprite {
    /// Load image (cat, idx, 1, sub), with its drawing offset (docs/04,
    /// "Image drawing offset"; SKULL.EXE 0x3EED3).
    pub fn load(g: &Gdat, cat: u8, idx: u8, sub: u8) -> Option<Sprite> {
        let raw = g.get(Key::new(cat, idx, 1, sub))?;
        if raw.len() < 4 {
            return None;
        }
        let w0 = u16::from_le_bytes([raw[0], raw[1]]);
        let w1 = u16::from_le_bytes([raw[2], raw[3]]);
        let (w, h) = ((w0 & 0x3FF) as usize, (w1 & 0x3FF) as usize);
        let (wtag, htag) = (w0 >> 10, w1 >> 10);
        let (px, cmap) = if htag == image::TAG_8BPP || htag == image::TAG_RAW {
            (image::decode(raw).ok()?.pixels, None)
        } else {
            let nib = image::decode_4bpp(raw, w, h, None).ok()?;
            let mut m = [0u8; 16];
            m.copy_from_slice(&raw[raw.len() - 16..]);
            (nib, Some(m))
        };
        let mut off = if wtag == 32 {
            let v = g.lookup(Key::new(cat, idx, 12, sub)).unwrap_or(0);
            (s8(v >> 8), s8(v))
        } else if htag == image::TAG_8BPP {
            (s8(raw[4] as u16), s8(raw[5] as u16))
        } else if htag == image::TAG_RAW {
            (0, 0)
        } else {
            (s6(wtag), s6(htag))
        };
        let base = g.lookup(Key::new(cat, idx, 12, 0xFE)).unwrap_or(0);
        off.0 += s8(base >> 8);
        off.1 += s8(base);
        Some(Sprite { w, h, px, cmap, off })
    }

    /// Copy the visible part described by a placement into `dst`.
    /// `flip` bit 0 mirrors horizontally, bit 1 vertically. Source values
    /// equal to `key` are transparent.
    pub fn blit(&self, dst: &mut Bitmap, p: &crate::layout::Placement, flip: u8, key: Option<u8>) {
        for row in 0..p.h {
            let dy = p.y + row;
            if dy < 0 || dy >= dst.h as i32 {
                continue;
            }
            let mut sy = (p.skip_y + row) as usize;
            if flip & 2 != 0 {
                sy = self.h - 1 - sy;
            }
            for col in 0..p.w {
                let dx = p.x + col;
                if dx < 0 || dx >= dst.w as i32 {
                    continue;
                }
                let mut sx = (p.skip_x + col) as usize;
                if flip & 1 != 0 {
                    sx = self.w - 1 - sx;
                }
                let v = self.px[sy * self.w + sx];
                if Some(v) == key {
                    continue;
                }
                let c = match &self.cmap {
                    Some(m) => m[(v & 15) as usize],
                    None => v,
                };
                dst.px[dy as usize * dst.w + dx as usize] = c;
            }
        }
    }
}
