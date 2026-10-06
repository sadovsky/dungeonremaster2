//! Interface font and text (docs/04-rendering.md §7, docs/13-text.md).
//!
//! Glyphs and strings are read from the user's GRAPHICS.DAT at runtime.

use dm2_formats::gdat::{Gdat, Key};

use crate::gfx::Bitmap;
use crate::layout::Layout;

pub const GLYPH_W: i32 = 6;
pub const GLYPH_H: i32 = 6;

/// The 5×6 interface font, entry (1,0,7,0): 6 rows × 128 characters, one
/// byte per glyph row with pixels in bits 4..0 (bit 4 leftmost).
pub struct Font {
    rows: Vec<u8>,
}

impl Font {
    pub fn load(g: &Gdat) -> Option<Font> {
        let rows = g.get(Key::new(1, 0, 7, 0))?;
        (rows.len() >= 768).then(|| Font { rows: rows[..768].to_vec() })
    }

    /// Size of a (possibly multi-line) string in pixels. The sixth column of
    /// each cell is spacing, so the last one is trimmed.
    pub fn measure(text: &[u8]) -> (i32, i32) {
        let lines = text.split(|&c| c == b'\n');
        let (mut w, mut n) = (0, 0);
        for l in lines {
            w = w.max(l.len() as i32 * GLYPH_W - 1);
            n += 1;
        }
        (w.max(0), n * GLYPH_H)
    }

    /// Draw text with its top-left at (x, y). `bg` of None leaves the
    /// background transparent (the game's 0x4000 colour flag).
    pub fn draw(&self, dst: &mut Bitmap, x: i32, y: i32, text: &[u8], fg: u8, bg: Option<u8>) {
        let (mut cx, mut cy) = (x, y);
        for &ch in text {
            if ch == b'\n' {
                cx = x;
                cy += GLYPH_H;
                continue;
            }
            let c = (ch & 0x7F) as usize;
            for row in 0..GLYPH_H {
                let bits = self.rows[row as usize * 128 + c];
                for col in 0..GLYPH_W {
                    let on = col < 5 && bits & (0x10 >> col) != 0;
                    let colour = if on { Some(fg) } else { bg };
                    if let Some(v) = colour {
                        let (px, py) = (cx + col, cy + row);
                        if px >= 0 && py >= 0 && (px as usize) < dst.w && (py as usize) < dst.h {
                            dst.px[py as usize * dst.w + px as usize] = v;
                        }
                    }
                }
            }
            cx += GLYPH_W;
        }
    }

    /// Draw text placed at layout id `rid` (0x1C021: measure, then place).
    pub fn draw_at(&self, dst: &mut Bitmap, layout: &Layout, rid: u16, text: &[u8], fg: u8, bg: Option<u8>) {
        let (w, h) = Self::measure(text);
        if w == 0 {
            return;
        }
        if let Some(p) = layout.resolve(rid, w, h, (w, h)) {
            self.draw(dst, p.x - p.skip_x, p.y - p.skip_y, text, fg, bg);
        }
    }
}

/// Undo the text obfuscation: byte i decodes as (!b - i) & 0xFF.
pub fn deobfuscate(raw: &[u8]) -> Vec<u8> {
    raw.iter().enumerate().map(|(i, &b)| (!b).wrapping_sub(i as u8)).collect()
}

/// Context the escape expander needs (docs/13 escape table). In the
/// original these are globals the caller fills before fetching the text;
/// here the caller fills the matching fields.
#[derive(Default)]
pub struct TextContext<'a> {
    /// Code 7: the current champion's name (champion index at 0x7F988).
    pub champion_name: Option<&'a [u8]>,
    /// Code 0: a number from the current message (0x7F214).
    pub number: Option<i32>,
    /// Codes 10-14: numbers from the words at 0x7F996, 0x7F98A, 0x7F994,
    /// 0x7F992 and 0x7F986. The load line, for example, puts the load's
    /// kilograms in 12, its tenths in 13 and the maximum in 14.
    pub slots: [Option<i32>; 5],
    /// Code 25: a number from the word at 0x760C3.
    pub number25: Option<i32>,
    /// Code 17: sub-index of an interface word (7, 0, 5, n), e.g. a class
    /// name (byte 0x7F990).
    pub interface_word: Option<u8>,
}

/// Fetch text (cat, idx, 5, sub) and expand escapes (0x3A921 + 0x3A6AB).
pub fn text(g: &Gdat, cat: u8, idx: u8, sub: u8, ctx: &TextContext) -> Option<Vec<u8>> {
    text_depth(g, cat, idx, sub, ctx, 0)
}

fn text_depth(g: &Gdat, cat: u8, idx: u8, sub: u8, ctx: &TextContext, depth: u32) -> Option<Vec<u8>> {
    let raw = g.get(Key::new(cat, idx, 5, sub))?;
    let flags = g.lookup(Key::new(0, 0, 11, 0)).unwrap_or(0);
    let plain = if flags & 0x08 != 0 { deobfuscate(raw) } else { raw.to_vec() };
    let end = plain.iter().position(|&b| b == 0).unwrap_or(plain.len());
    let mut out = Vec::new();
    expand(g, &plain[..end], ctx, &mut out, depth);
    Some(out)
}

fn expand(g: &Gdat, s: &[u8], ctx: &TextContext, out: &mut Vec<u8>, depth: u32) {
    let mut i = 0;
    while i < s.len() {
        let code = if s[i] == 1 && i + 1 < s.len() {
            i += 2;
            Some(s[i - 1].wrapping_sub(0x20))
        } else if s[i..].starts_with(b".Z") && s.len() >= i + 5 && s[i + 2..i + 5].iter().all(u8::is_ascii_digit) {
            let n = (s[i + 2] - b'0') * 100 + (s[i + 3] - b'0') * 10 + (s[i + 4] - b'0');
            i += 5;
            Some(n)
        } else {
            out.push(s[i]);
            i += 1;
            None
        };
        let Some(code) = code else { continue };
        let nested = |cat: u8, idx: u8, sub: u8, out: &mut Vec<u8>| {
            if depth < 4 {
                if let Some(t) = text_depth(g, cat, idx, sub, ctx, depth + 1) {
                    out.extend(t);
                }
            }
        };
        match code {
            0 => out.extend(ctx.number.unwrap_or(0).to_string().bytes()),
            2 => nested(1, 0xFE, 0, out),
            7 => out.extend_from_slice(ctx.champion_name.unwrap_or(b"")),
            10..=14 => out.extend(ctx.slots[(code - 10) as usize].unwrap_or(0).to_string().bytes()),
            17 => {
                if let Some(n) = ctx.interface_word {
                    nested(7, 0, n, out);
                }
            }
            25 => out.extend(ctx.number25.unwrap_or(0).to_string().bytes()),
            27 => nested(1, 0xFE, 6, out),
            // Codes 1, 3, 4, 8, 9, 15, 20, 22-24, 26 and 28 produce drive,
            // disk and directory names or save-slot labels for the DOS file
            // dialogs; the remake has no such dialogs, so they expand to
            // nothing.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_and_deobfuscate() {
        assert_eq!(Font::measure(b"AB"), (11, 6));
        assert_eq!(Font::measure(b"A\nBCD"), (17, 12));
        let enc: Vec<u8> = b"HI".iter().enumerate().map(|(i, &c)| !(c.wrapping_add(i as u8))).collect();
        assert_eq!(deobfuscate(&enc), b"HI");
    }

    #[test]
    fn load_line_expands_its_number_codes() {
        let Ok(g) = Gdat::open(dm2_formats::gdat::default_path()) else { return };
        let ctx = TextContext { slots: [None, None, Some(67), Some(3), Some(912)], ..Default::default() };
        let Some(t) = text(&g, 7, 0, 0x2A, &ctx) else { return };
        let s = String::from_utf8_lossy(&t);
        // Only properties are checked, so no game text is stored here.
        assert!(s.contains("67") && s.contains("912"), "numbers substituted");
        assert!(!t.contains(&1), "no escape bytes left");
        assert!(!s.contains(".Z"), "no .Z escapes left");
    }
}
