//! Item identity and attributes (docs/09-items.md).
//!
//! Only what champions and combat need so far: which GRAPHICS.DAT
//! category and index describe a thing, attribute lookup, charges and
//! weight totals.

use dm2_formats::dungeon::{Dungeon, ThingRef, ThingType};
use dm2_formats::gdat::{Gdat, Key};

pub const ATTR_FLAGS: u8 = 0;
pub const ATTR_WEIGHT: u8 = 1;
pub const ATTR_VALUE: u8 = 2;
pub const ATTR_FOOD: u8 = 3;
pub const ATTR_SLOTS: u8 = 4;
pub const ATTR_LAUNCHER: u8 = 5;
/// Icon animation: frame count, mode and gating bits (0x37F76).
pub const ATTR_ICON_ANIM: u8 = 6;
pub const ATTR_MELEE_BONUS: u8 = 8;
pub const ATTR_DAMAGE: u8 = 9;
pub const ATTR_ARMOUR: u8 = 0x0B;
pub const ATTR_SLAYER: u8 = 0x0D;
pub const ATTR_WEIGHT_PER_CHARGE: u8 = 0x34;
pub const ATTR_VALUE_PER_CHARGE: u8 = 0x35;

/// Read-only view of everything needed to describe items.
pub struct ItemDb<'a> {
    pub gdat: &'a Gdat,
    pub dungeon: &'a Dungeon,
    /// Thing type -> GRAPHICS.DAT category (0xFF none), from SKULL.EXE.
    pub categories: &'a [u8; 16],
}

impl ItemDb<'_> {
    /// Resolve a missile to the thing it carries (word 1).
    fn resolve(&self, t: ThingRef) -> ThingRef {
        let mut t = t;
        for _ in 0..4 {
            if t.0 >= 0xFF80 || t.kind() != ThingType::from_index(14) {
                break;
            }
            match self.dungeon.record_word(t, 1) {
                Some(w) => t = ThingRef(w),
                None => break,
            }
        }
        t
    }

    /// (category, index) keying this thing's images and attributes (0x1EFA8).
    pub fn key(&self, t: ThingRef) -> Option<(u8, u8)> {
        if t.0 == 0xFFFF || t.0 == 0xFFFE {
            return None;
        }
        let t = self.resolve(t);
        let ty = t.kind() as usize;
        let cat = self.categories[ty];
        if cat == 0xFF {
            return None;
        }
        let w1 = self.dungeon.record_word(t, 1).unwrap_or(0);
        let idx = match ty {
            5 | 6 | 10 | 15 => w1 & 0x7F,
            7 => 0,
            8 => (w1 >> 8) & 0x7F,
            9 => {
                let w2 = self.dungeon.record_word(t, 2).unwrap_or(0);
                (w2 >> 13) | ((w2 >> 1) & 3) << 3
            }
            4 => self.dungeon.record(t).map_or(0, |r| r[4] as u16),
            _ => 0,
        };
        Some((cat, idx as u8))
    }

    /// Attribute `n` of a thing; missing keys read as 0 (0x1F12F).
    pub fn attr(&self, t: ThingRef, n: u8) -> u16 {
        self.key(t)
            .and_then(|(c, i)| self.gdat.lookup(Key::new(c, i, 11, n)))
            .unwrap_or(0)
    }

    /// Maximum charges for the thing's type (0x1F5D0): 15 for weapons and
    /// clothing, 3 for misc items, otherwise 0.
    pub fn max_charges(&self, t: ThingRef) -> u16 {
        match t.kind() as usize {
            5 | 6 => 15,
            10 => 3,
            _ => 0,
        }
    }

    /// Icon sub-index for a thing (0x37F76); see `icon_sub`.
    pub fn icon_sub(&self, t: ThingRef, equipped: bool, tick: u32, facing: u8, visual: u32) -> u8 {
        icon_sub(self.attr(t, ATTR_ICON_ANIM), t.0, self.charges(t), self.max_charges(t), equipped, tick, facing, visual)
    }

    /// Charges or stack count (0x1F606).
    pub fn charges(&self, t: ThingRef) -> u16 {
        let w1 = self.dungeon.record_word(t, 1).unwrap_or(0);
        match t.kind() as usize {
            5 => (w1 >> 10) & 15,
            6 => (w1 >> 9) & 15,
            10 => w1 >> 14,
            _ => 0,
        }
    }

    /// Weight in tenths of a kilogram, including charges and container
    /// contents (0x1F6E4 with n = 1).
    pub fn weight(&self, t: ThingRef) -> u16 {
        self.total(t, ATTR_WEIGHT, 0)
    }

    /// Value, including charges, potion power and container contents
    /// (0x1F6E4 with n = 2).
    pub fn value(&self, t: ThingRef) -> u16 {
        self.total(t, ATTR_VALUE, 0)
    }

    /// A money container (0x1F2AB): a closed container whose type has a
    /// contents rule, text (20, index, 5, 0x40).
    pub fn is_money_container(&self, t: ThingRef) -> bool {
        if t.kind() as usize != 9 || !self.dungeon.record(t).is_some_and(|r| r[4] & 6 == 0) {
            return false;
        }
        self.key(t).is_some_and(|(_, i)| self.gdat.record(Key::new(20, i, 5, 0x40)).is_some())
    }

    /// Attribute `n` plus its adjustments (0x1F6E4): per-charge extras for
    /// weight and value, potion power for value, and container contents.
    /// In a money container misc items count as stacks, attribute × (stack
    /// count + 1), and for weight the stacked total is divided by 5,
    /// rounding up.
    fn total(&self, t: ThingRef, n: u8, depth: u32) -> u16 {
        if !t.is_thing() {
            return 0;
        }
        let mut v = self.attr(t, n) as u32;
        let per = match n {
            ATTR_WEIGHT => self.attr(t, ATTR_WEIGHT_PER_CHARGE),
            ATTR_VALUE => self.attr(t, ATTR_VALUE_PER_CHARGE),
            _ => 0,
        };
        if per != 0 {
            v += self.charges(t) as u32 * per as u32;
        }
        if n == ATTR_VALUE && t.kind() as usize == 8 && v > 1 {
            let power = (self.dungeon.record_word(t, 1).unwrap_or(0) & 0xFF) as u32;
            let half = v >> 1;
            v = half + power * half / 255;
        }
        if t.kind() as usize == 9 && depth < 8 && self.dungeon.record(t).is_some_and(|r| r[4] & 6 == 0) {
            let money = self.is_money_container(t);
            let mut stacked = 0u32;
            let mut c = ThingRef(self.dungeon.record_word(t, 1).unwrap_or(0xFFFE));
            let mut guard = 0;
            while c.is_thing() && guard < 256 {
                if money && c.kind() as usize == 10 {
                    let w1 = self.dungeon.record_word(c, 1).unwrap_or(0);
                    stacked += self.attr(c, n) as u32 * (((w1 & 0x3FFF) >> 8) as u32 + 1);
                } else {
                    v += self.total(c, n, depth + 1) as u32;
                }
                c = ThingRef(self.dungeon.record_word(c, 0).unwrap_or(0xFFFE));
                guard += 1;
            }
            if money {
                v += if n == ATTR_WEIGHT { (stacked + 4) / 5 } else { stacked };
            }
        }
        v.min(u16::MAX as u32) as u16
    }
}

/// Base icon sub-index of every item (frame 0).
pub const ICON_BASE: u8 = 0x18;

/// Icon sub-index from attribute 6 (0x37F76).
///
/// `anim` bits 0-4 are a frame count n, bits 5-7 a group size for mode 4,
/// bits 8-12 the mode. Bit 15 animates only while the item is properly
/// `equipped`; bit 14 only while it is the item an action is running with
/// (not tracked: treated as inactive). An active gate moves the first frame
/// one up and uses one frame fewer. Modes: 0 tick mod n; 5 the same offset
/// by the item number; 1 a random frame (`visual`, never the game RNG);
/// 2 the party's facing; 3 the charge fraction; 4 charge groups of m frames
/// that also cycle with the tick; 6 like 4 offset by the item number.
#[allow(clippy::too_many_arguments)]
pub fn icon_sub(anim: u16, thing: u16, charges: u16, max_charges: u16, equipped: bool, tick: u32, facing: u8, visual: u32) -> u8 {
    let mut base = ICON_BASE as i32;
    let mut n = (anim & 0x1F) as i32;
    if n == 0 {
        return ICON_BASE;
    }
    let mut active = false;
    if anim & 0x8000 != 0 {
        if !equipped {
            return ICON_BASE;
        }
        active = true;
    }
    if anim & 0x4000 != 0 {
        return ICON_BASE;
    }
    if active {
        base += 1;
        n -= 1;
    }
    if n == 0 {
        return base as u8;
    }
    let t = tick as i32;
    let item = (thing & 0x3FF) as i32;
    let frame = match (anim >> 8) & 0x1F {
        0 => t.rem_euclid(n),
        5 => (t + item).rem_euclid(n),
        1 => (visual % n as u32) as i32,
        2 => facing as i32,
        3 => {
            if charges == 0 {
                0
            } else {
                (charges as i32 * n) / (max_charges as i32 + 1) + 1
            }
        }
        mode @ (4 | 6) => {
            let m = ((anim & 0xE0) >> 5).max(1) as i32;
            if charges == 0 {
                0
            } else {
                let tt = if mode == 6 { t + item } else { t };
                tt.rem_euclid(m) + ((charges as i32 * (n / m)) / (max_charges as i32 + 1)) * m + 1
            }
        }
        _ => 0,
    };
    (base + frame) as u8
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    #[test]
    fn icon_frames() {
        // No animation.
        assert_eq!(icon_sub(0, 0, 0, 0, false, 7, 0, 0), ICON_BASE);
        // Mode 0, 4 frames: cycles with the tick.
        assert_eq!(icon_sub(4, 0, 0, 0, false, 6, 0, 0), ICON_BASE + 2);
        // Mode 5 offsets by the item number.
        assert_eq!(icon_sub(0x0504, 3, 0, 0, false, 6, 0, 0), ICON_BASE + 1);
        // Mode 2 follows the facing.
        assert_eq!(icon_sub(0x0204, 0, 0, 0, false, 0, 3, 0), ICON_BASE + 3);
        // Bit 15: base frame unless equipped; equipped shifts up by one.
        assert_eq!(icon_sub(0x8004, 0, 0, 0, false, 1, 0, 0), ICON_BASE);
        assert_eq!(icon_sub(0x8004, 0, 0, 0, true, 1, 0, 0), ICON_BASE + 1 + 1);
        // Mode 3: charge fraction, 0 charges shows the base.
        assert_eq!(icon_sub(0x0304, 0, 0, 15, false, 0, 0, 0), ICON_BASE);
        assert_eq!(icon_sub(0x0304, 0, 15, 15, false, 0, 0, 0), ICON_BASE + (15 * 4) / 16 + 1);
        // Mode 4: two groups of 2 frames.
        let a = 0x0404 | (2 << 5);
        assert_eq!(icon_sub(a, 0, 15, 15, false, 1, 0, 0), ICON_BASE + 1 + ((15 * 2) / 16) as u8 * 2 + 1);
    }
}

/// A set of item numbers (0-511), one bit each, as built by 0x1538D.
#[derive(Clone, PartialEq, Eq)]
pub struct KindSet(pub [u8; 64]);

impl KindSet {
    pub fn contains(&self, n: u16) -> bool {
        n < 512 && self.0[(n >> 3) as usize] & 1 << (n & 7) != 0
    }

    /// Parse a kind list (0x1538D). A letter selects the item-number base
    /// for the numbers after it: W weapons 0, A clothing 0x80, J misc
    /// 0x100, P potions 0x180, C containers 0x1E0 (0 when `no_containers`),
    /// S the scroll 0x1FC; other letters keep the base unset. Digits form a
    /// number and '-' makes it the start of a range. When the next non-digit
    /// arrives, the pending number or range is marked at base + number.
    pub fn parse(text: &[u8], no_containers: bool) -> KindSet {
        let mut bits = [0u8; 64];
        let (mut num, mut from, mut base, mut pending) = (0i32, -1i32, -1i32, false);
        for &c in text.iter().chain(std::iter::once(&0u8)) {
            if c.is_ascii_digit() {
                pending = true;
                num = num * 10 + (c - b'0') as i32;
                continue;
            }
            if c == b'-' {
                from = num;
                num = 0;
                continue;
            }
            if pending {
                let start = if from < 0 { num } else { from };
                for k in start..=num {
                    let n = k + base;
                    if (0..512).contains(&n) {
                        bits[(n >> 3) as usize] |= 1 << (n & 7);
                    }
                }
                num = 0;
                from = -1;
                base = -1;
                pending = false;
            }
            match c {
                b'A' => base = 0x80,
                b'C' => base = if no_containers { 0 } else { 0x1E0 },
                b'J' => base = 0x100,
                b'P' => base = 0x180,
                b'S' => base = 0x1FC,
                b'W' => base = 0,
                _ => {}
            }
            if c == 0 {
                break;
            }
        }
        KindSet(bits)
    }

    /// Kind list (15, `idx`, 5, `sub` + 0x10) from GRAPHICS.DAT text; empty
    /// when the text is missing.
    pub fn load(g: &Gdat, idx: u8, sub: u8, no_containers: bool) -> KindSet {
        let text = crate::font::text(g, 15, idx, sub.wrapping_add(0x10), &Default::default()).unwrap_or_default();
        KindSet::parse(&text, no_containers)
    }
}

#[cfg(test)]
mod kind_tests {
    use super::KindSet;

    #[test]
    fn kind_list_grammar() {
        let k = KindSet::parse(b"W2-4J7P0S", false);
        assert!(k.contains(2) && k.contains(3) && k.contains(4) && !k.contains(5));
        assert!(k.contains(0x100 + 7));
        assert!(k.contains(0x180));
        assert!(!k.contains(0x1FC), "a letter with no number marks nothing");
        let c = KindSet::parse(b"C3", false);
        assert!(c.contains(0x1E3));
        let c = KindSet::parse(b"C3", true);
        assert!(c.contains(3), "containers fall back to base 0");
    }

    #[test]
    fn real_kind_lists_load() {
        let Ok(g) = dm2_formats::gdat::Gdat::open(dm2_formats::gdat::default_path()) else { return };
        // Some creature type carries at least one non-empty kind list.
        let any = (0..=255u8).any(|i| (0..16u8).any(|s| KindSet::load(&g, i, 0x10 + s, false).0.iter().any(|&b| b != 0)));
        assert!(any);
    }
}
