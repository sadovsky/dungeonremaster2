//! Map-set backdrops: the category-23 scripts drawn after the floor and
//! before the cells (0x54699, docs/04 "Backdrops").
//!
//! Each map graphics set may carry text entries (23, set, 5, n) for n in
//! 0..100. A non-empty entry is a script of two-letter keys followed by
//! numbers; it draws image (23, set, 1, n) through the lit drawer.

use dm2_formats::gdat::{Gdat, Key};

/// Number after the last occurrence of `key` in a script (0x3F791): an
/// optional '=' and '-' may follow the key; a missing key reads as 0.
pub fn key_value(script: &[u8], key: &[u8; 2]) -> i32 {
    let mut value = 0;
    let mut i = 0;
    while i + 1 < script.len() {
        if script[i] == key[0] && script[i + 1] == key[1] {
            let mut j = i + 2;
            if script.get(j) == Some(&b'=') {
                j += 1;
            }
            let neg = script.get(j) == Some(&b'-');
            if neg {
                j += 1;
            }
            let mut v: i32 = 0;
            while let Some(d) = script.get(j).filter(|c| c.is_ascii_digit()) {
                v = v.wrapping_mul(10).wrapping_add((d - b'0') as i32);
                j += 1;
            }
            value = if neg { -v } else { v };
            i = j;
        } else {
            i += 1;
        }
    }
    value
}

/// One backdrop to draw: the image index, the layout id, the mirroring
/// kind, an x offset in screen pixels and the scale (64ths).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backdrop {
    pub index: u8,
    pub rid: u16,
    pub flip_kind: u8,
    pub xoff: i32,
    pub scale: i32,
}

/// Rotate the world offset from the party to an anchor into view space
/// (0x542F6). Returns (lateral, forward, distance); distance is 0 when the
/// anchor is not ahead.
pub fn view_space(dir: u8, gx: i32, gy: i32, ax: i32, ay: i32) -> (i32, i32, i32) {
    let (fwd, lat) = match dir & 3 {
        0 => (gy - ay, ax - gx),
        1 => (ax - gx, ay - gy),
        2 => (ay - gy, gx - ax),
        _ => (gx - ax, gy - ay),
    };
    if fwd < 1 {
        return (lat, fwd, 0);
    }
    let sq = (fwd * fwd + lat * lat) as u32;
    let dist = if sq <= 2 {
        1
    } else {
        // Newton's integer square root, as the original iterates it.
        let mut r = sq >> 1;
        loop {
            let n = (sq / r + r) >> 1;
            if n >= r {
                break r as i32;
            }
            r = n;
        }
    };
    (lat, fwd, dist)
}

/// The scripts of a map set that draw something from this viewpoint
/// (0x5439A for each non-empty entry 0..100). `gx`/`gy` is the party's
/// global position (map origin plus square).
pub fn plan(g: &Gdat, set: u8, gx: i32, gy: i32, dir: u8) -> Vec<Backdrop> {
    (0..100u8).filter_map(|n| plan_one(g, set, n, gx, gy, dir)).collect()
}

/// One backdrop script (category 23, sub `n`), if it draws something from
/// this viewpoint. Also used for the weather's cloud and storm scripts
/// (subs 0x67-0x6C, 0x5A808).
pub fn plan_one(g: &Gdat, set: u8, n: u8, gx: i32, gy: i32, dir: u8) -> Option<Backdrop> {
    g.record(Key::new(23, set, 1, n))?;
    let raw = g.get(Key::new(23, set, 5, n))?;
    let text = crate::font::deobfuscate(raw);
    let script: Vec<u8> = text.into_iter().take_while(|&b| b != 0).collect();
    if script.is_empty() {
        return None;
    }
    let rid = key_value(&script, b"cd") as u16;
    let flip_kind = key_value(&script, b"fw") as u8;
    let b = match key_value(&script, b"mv") {
        0 => Backdrop { index: n, rid, flip_kind, xoff: 0, scale: 0x40 },
        1 => {
            let (ax, ay) = (key_value(&script, b"xl"), key_value(&script, b"yl"));
            let (lat, _fwd, dist) = view_space(dir, gx, gy, ax, ay);
            if dist == 0 {
                return None;
            }
            let fd = key_value(&script, b"fd");
            let v = (0x40 - (dist - fd)).max(1);
            let scale = ((v * 0x80 >> 6) + 1) >> 1;
            Backdrop { index: n, rid, flip_kind, xoff: lat * 0xD2 / dist, scale }
        }
        _ => return None,
    };
    (b.rid != 0).then_some(b)
}

/// Mirroring for a backdrop (0x54874): kinds 8 and 0x40 follow the map
/// set's floor-flip mode, 2 and 0x20 its ceiling-flip mode, using the
/// position parity `par`. Time-driven modes use the tick.
pub fn mirrored(flip_kind: u8, set_flags: u16, par: u8, tick: u32) -> bool {
    let floor = |_: ()| -> bool {
        if set_flags & 8 == 0 {
            false
        } else if set_flags & 0x10 != 0 {
            tick & 7 > 3
        } else {
            par != 0
        }
    };
    let ceiling = |_: ()| -> bool {
        if set_flags & 2 != 0 {
            if set_flags & 4 == 0 {
                par == 0
            } else {
                tick & 7 < 4
            }
        } else {
            false
        }
    };
    match flip_kind {
        8 | 0x40 => floor(()),
        2 | 0x20 => ceiling(()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_parse_like_the_original() {
        let s = b"cd6000xl38yl40mv1fd8";
        assert_eq!(key_value(s, b"cd"), 6000);
        assert_eq!(key_value(s, b"xl"), 38);
        assert_eq!(key_value(s, b"mv"), 1);
        assert_eq!(key_value(s, b"fw"), 0);
        assert_eq!(key_value(b"ab=-12", b"ab"), -12);
        assert_eq!(key_value(b"ab1ab7", b"ab"), 7);
    }

    #[test]
    fn view_space_rotates_by_facing() {
        // Anchor 3 north, 1 east of the party.
        assert_eq!(view_space(0, 10, 10, 11, 7), (1, 3, 3));
        // Facing south it is behind.
        assert_eq!(view_space(2, 10, 10, 11, 7).2, 0);
        // Facing east: 1 ahead, 3 to the left.
        assert_eq!(view_space(1, 10, 10, 11, 7), (-3, 1, 3));
    }
}
