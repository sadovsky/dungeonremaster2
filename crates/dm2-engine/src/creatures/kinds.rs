//! Item-kind classification for creatures (docs/08 "Item kinds";
//! SKULL.EXE 0x2F636, set builder 0x1538D, possession search 0x2FF1E).
//!
//! A kind is a byte: bits 0-5 select a set, bit 7 inverts the answer. Set
//! 0x3F matches anything. The other sets are defined per creature type by
//! text entries (15, type, 5, kind + 0x10) listing item numbers, read from
//! the user's GRAPHICS.DAT at runtime.

use dm2_formats::dungeon::{ThingRef, ThingType};
use dm2_formats::gdat::{Gdat, Key};

use crate::state::GameState;

use super::rec_u16;

/// 512 item numbers per set.
pub type KindSet = [u8; 64];

/// Parse a set definition (0x1538D). Numbers and `a-b` ranges are added to
/// a base chosen by the letter before them: W weapons (0), A clothing
/// (0x80), J misc (0x100), P potions (0x180), C containers (0x1E0, or 0 when
/// classifying creatures), S scrolls (0x1FC).
pub fn parse_set(text: &[u8], creatures: bool) -> KindSet {
    let mut set = [0u8; 64];
    let mut num: i32 = 0;
    let mut start: i32 = -1;
    let mut base: i32 = -1;
    let mut have_num = false;
    let mut bytes = text.to_vec();
    bytes.push(0);
    for &b in &bytes {
        if b.is_ascii_digit() {
            have_num = true;
            num = num * 10 + (b - b'0') as i32;
            continue;
        }
        if b == b'-' {
            start = num;
            num = 0;
            continue;
        }
        if have_num {
            if start < 0 {
                start = num;
            }
            for n in start..=num {
                let bit = n + base;
                if (0..512).contains(&bit) {
                    set[(bit >> 3) as usize] |= 1 << (bit & 7);
                }
            }
            num = 0;
            start = -1;
            base = -1;
            have_num = false;
        }
        base = match b {
            b'A' => 0x80,
            b'C' if creatures => 0,
            b'C' => 0x1E0,
            b'J' => 0x100,
            b'P' => 0x180,
            b'S' => 0x1FC,
            b'W' => 0,
            _ => base,
        };
    }
    set
}

/// Raw (deobfuscated) text of a kind definition, without escape expansion.
fn set_text(g: &Gdat, ctype: u8, set: u8) -> Option<Vec<u8>> {
    let raw = g.get(Key::new(15, ctype, 5, set.wrapping_add(0x10)))?;
    let flags = g.lookup(Key::new(0, 0, 11, 0)).unwrap_or(0);
    let plain = if flags & 0x08 != 0 { crate::font::deobfuscate(raw) } else { raw.to_vec() };
    let end = plain.iter().position(|&b| b == 0).unwrap_or(plain.len());
    Some(plain[..end].to_vec())
}

/// Look up (and cache) the set `set` of creature type `ctype`. None when the
/// creature type defines no such set (the original then answers "no").
pub fn set_for(g: &GameState, ctype: u8, set: u8, creatures: bool) -> Option<KindSet> {
    let d = g.creature_data.as_ref()?;
    let key = (ctype, set, creatures);
    if let Some(s) = d.kind_sets.borrow().get(&key) {
        return *s;
    }
    let s = set_text(&d.gdat, ctype, set).filter(|t| !t.is_empty()).map(|t| parse_set(&t, creatures));
    d.kind_sets.borrow_mut().insert(key, s);
    s
}

/// A container that holds money (key (20, idx, 5, 0x40) exists; 0x1F2AB).
pub fn is_money_container(g: &GameState, t: ThingRef) -> bool {
    if t.kind() != ThingType::Container {
        return false;
    }
    let Some(d) = g.creature_data.as_ref() else { return false };
    let w2 = g.dungeon.record_word(t, 2).unwrap_or(0);
    if w2 & 6 != 0 {
        return false;
    }
    let idx = ((w2 >> 13) | ((w2 >> 1) & 3) << 3) as u8;
    d.gdat.record(Key::new(20, idx, 5, 0x40)).is_some()
}

/// Does thing `t` belong to kind `kind` as seen by creature `who` (0x2F636)?
pub fn matches(g: &GameState, who: ThingRef, t: ThingRef, kind: u8) -> bool {
    if !t.is_thing() {
        return false;
    }
    let invert = kind & 0x80 != 0;
    let mut k = kind & 0x3F;
    match k {
        0x3F => return !invert,
        // "Anything but a money container."
        0x3E => return is_money_container(g, t) == invert,
        // Money containers count as kind 0x29; anything else is tested as 7.
        0x29 => {
            if is_money_container(g, t) {
                return !invert;
            }
            k = 7;
        }
        _ => {}
    }
    if (0x10..0x13).contains(&k) || k == 0x28 {
        if k == 0x28 {
            if is_money_container(g, t) {
                return !invert;
            }
            k = 0x10;
        }
        // Per-creature variant group: record word +8 selects one of
        // several consecutive sets.
        let grp = rec_u16(g, who, 8) as i16;
        k = (k as i16).wrapping_add(grp.wrapping_mul(3)) as u8;
    }
    let ctype = g.dungeon.record(who).map_or(0, |r| r[4]);
    let ty = t.kind();
    let creatures = ty == ThingType::Creature;
    let Some(set) = set_for(g, ctype, k, creatures) else { return false };
    let n: u16 = match ty {
        ThingType::Creature => g.dungeon.record(t).map_or(0, |r| r[4] as u16),
        ThingType::Weapon | ThingType::Clothing | ThingType::Scroll | ThingType::Potion | ThingType::Container | ThingType::Misc => {
            crate::actuators::item_number(g, t)
        }
        _ => return false,
    };
    let hit = n < 512 && set[(n >> 3) as usize] & (1 << (n & 7)) != 0;
    hit != invert
}

/// First thing in the chain starting at `first` that is an item or
/// creature of kind `kind`, optionally only in `cell` (0xFF = any)
/// (0x2FF1E).
pub fn find_in_chain(g: &GameState, who: ThingRef, first: ThingRef, kind: u8, cell: u8) -> Option<ThingRef> {
    let mut t = first;
    for _ in 0..1024 {
        if !t.is_thing() {
            return None;
        }
        let ty = t.kind() as u16;
        if (5..14).contains(&ty) && (cell == 0xFF || cell == t.cell()) && matches(g, who, t, kind) {
            return Some(t);
        }
        t = ThingRef(g.dungeon.record_word(t, 0).unwrap_or(ThingRef::END.0));
    }
    None
}

/// The first possession of creature `c` of kind `kind`.
pub fn possession_of_kind(g: &GameState, who: ThingRef, c: ThingRef, kind: u8) -> Option<ThingRef> {
    find_in_chain(g, who, ThingRef(rec_u16(g, c, 2)), kind, 0xFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(s: &KindSet, n: usize) -> bool {
        s[n >> 3] & (1 << (n & 7)) != 0
    }

    #[test]
    fn parses_letters_numbers_and_ranges() {
        let s = parse_set(b"W3 A1-2 P5", false);
        assert!(has(&s, 3));
        assert!(has(&s, 0x81) && has(&s, 0x82) && !has(&s, 0x83));
        assert!(has(&s, 0x185));
        assert!(!has(&s, 0));
    }

    #[test]
    fn container_letter_depends_on_target() {
        assert!(has(&parse_set(b"C4", false), 0x1E4));
        assert!(has(&parse_set(b"C4", true), 4));
    }

    #[test]
    fn base_resets_after_each_number() {
        // A number with no letter before it lands one below its value.
        let s = parse_set(b"J2 7", false);
        assert!(has(&s, 0x102) && has(&s, 6));
    }
}
