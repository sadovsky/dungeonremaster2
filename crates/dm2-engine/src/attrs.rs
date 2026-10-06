//! Numeric attributes from GRAPHICS.DAT (type 11 keys), as the game reads
//! them with 0x3C5D3: a missing key reads as 0.

use std::collections::HashMap;

use dm2_formats::gdat::Gdat;

#[derive(Clone, Debug, Default)]
pub struct Attributes {
    map: HashMap<(u8, u8, u8), u16>,
}

/// Language code kept when building the table (F high nibble; 0x10 = English).
const LANGUAGE: u8 = 0x10;

impl Attributes {
    pub fn from_gdat(g: &Gdat) -> Attributes {
        let mut map = HashMap::new();
        for r in &g.records {
            let lang = r.f & 0xF0;
            if r.key.kind == 11 && (lang == 0 || lang == LANGUAGE) {
                map.insert((r.key.cat, r.key.idx, r.key.sub), r.value);
            }
        }
        Attributes { map }
    }

    /// Attribute (cat, idx, 11, sub); 0 when absent.
    pub fn get(&self, cat: u8, idx: u8, sub: u8) -> u16 {
        self.map.get(&(cat, idx, sub)).copied().unwrap_or(0)
    }

    pub fn set(&mut self, cat: u8, idx: u8, sub: u8, v: u16) {
        self.map.insert((cat, idx, sub), v);
    }
}
