//! Wall writing (SKULL.EXE 0x4F3DF, docs/04 "Floor and wall ornaments").
//!
//! The text is laid out at 1:1 on a transparent bitmap the size of the
//! map set's writing panel, using the 8×8 glyph strip (8, set, 3), and the
//! result is then scaled and placed exactly like the panel. Both the
//! glyphs and the text come from the user's data files at runtime.

use dm2_formats::dungeon::{Dungeon, ThingRef};

use crate::assets::Assets;
use crate::font;
use crate::gfx::Sprite;

/// Glyph cell size (0x7173A / 0x7173C); lines are two pixels apart.
const GLYPH_W: usize = 8;
const GLYPH_H: usize = 8;
const LINE_H: usize = GLYPH_H + 2;

/// Glyph index in the strip (0x4F3C0): A-Z, '.', anything else blank.
fn glyph(c: u8) -> usize {
    match c {
        b'A'..=b'Z' => (c - b'A') as usize,
        b'.' => 27,
        _ => 26,
    }
}

/// The text a wall-writing thing shows (0x1DF6C): packed dungeon text in
/// mode 0, a GRAPHICS.DAT message (3, 0, 5, n) in mode 1 when bits 11-15
/// are 14 (the form this dungeon uses).
pub(super) fn text_of(a: &Assets, dg: &Dungeon, t: ThingRef) -> Option<Vec<u8>> {
    let w1 = dg.record_word(t, 1)?;
    match (w1 & 7) >> 1 {
        0 => {
            // Escape codes (29/30) select strings from SKULL.EXE tables the
            // viewport has no access to; they are dropped.
            let s = dg.decode_text((w1 >> 3) as usize);
            let mut out = Vec::with_capacity(s.len());
            let mut skip = false;
            for b in s.bytes() {
                match b {
                    b'{' => skip = true,
                    b'}' => skip = false,
                    _ if !skip => out.push(b),
                    _ => {}
                }
            }
            Some(out)
        }
        // Mode 1 shows text only when bits 11-15 are 14; the message is the
        // low byte of bits 3-15.
        1 if w1 >> 11 == 14 => font::text(&a.gdat, 3, 0, (w1 >> 3) as u8, &font::TextContext::default()),
        _ => None,
    }
}

/// Lay `text` out on a panel of `pw`×`ph` pixels. The result keeps the
/// strip's raw nibbles and colour map; `key` marks the empty pixels.
pub(super) fn compose(a: &mut Assets, set: u8, text: &[u8], pw: usize, ph: usize, key: u8) -> Option<Sprite> {
    let strip = a.sprite(8, set, 3)?;
    let key = key & 15;
    let mut px = vec![key; pw * ph];
    let lines: Vec<&[u8]> = text.split(|&c| c == b'\n').collect();
    let mut y = ph as i32 / 2 - (lines.len() * LINE_H) as i32 / 2;
    for line in lines {
        let x0 = pw as i32 / 2 - (GLYPH_W * line.len()) as i32 / 2;
        if x0 >= 0 {
            for (i, &c) in line.iter().enumerate() {
                let (sx, dx) = (glyph(c) * GLYPH_W, x0 as usize + i * GLYPH_W);
                for gy in 0..GLYPH_H {
                    let dy = y + gy as i32;
                    if dy < 0 || dy as usize >= ph || gy >= strip.h {
                        continue;
                    }
                    for gx in 0..GLYPH_W {
                        if sx + gx >= strip.w || dx + gx >= pw {
                            continue;
                        }
                        let v = strip.px[gy * strip.w + sx + gx];
                        if v & 15 != key {
                            px[dy as usize * pw + dx + gx] = v;
                        }
                    }
                }
            }
        }
        y += LINE_H as i32;
    }
    Some(Sprite { w: pw, h: ph, px, cmap: strip.cmap, off: (0, 0) })
}

#[cfg(test)]
mod tests {
    use super::glyph;

    #[test]
    fn glyph_indices() {
        assert_eq!(glyph(b'A'), 0);
        assert_eq!(glyph(b'Z'), 25);
        assert_eq!(glyph(b'.'), 27);
        assert_eq!(glyph(b' '), 26);
        assert_eq!(glyph(b'7'), 26);
    }
}
