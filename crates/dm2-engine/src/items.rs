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
    // TODO(docs/09): money containers count coins at 1/5 weight; not yet modelled.
    pub fn weight(&self, t: ThingRef) -> u16 {
        if !t.is_thing() {
            return 0;
        }
        let mut w = self.attr(t, ATTR_WEIGHT);
        let per = self.attr(t, ATTR_WEIGHT_PER_CHARGE);
        if per != 0 {
            w = w.wrapping_add(self.charges(t).wrapping_mul(per));
        }
        if t.kind() as usize == 9 && self.dungeon.record(t).is_some_and(|r| r[4] & 6 == 0) {
            let mut c = ThingRef(self.dungeon.record_word(t, 1).unwrap_or(0xFFFE));
            let mut guard = 0;
            while c.is_thing() && guard < 256 {
                w = w.wrapping_add(self.weight(c));
                c = ThingRef(self.dungeon.record_word(c, 0).unwrap_or(0xFFFE));
                guard += 1;
            }
        }
        w
    }
}
