//! Which sound keys a map registers (docs/11-audio.md, "Sound registration").
//!
//! The original's play function (0x15CA9) only plays a key the resource
//! manager registered while loading the party's map (0x161D9, looked up by
//! 0x15C10). The registered set follows the map's resource request list
//! built by 0x3AB31 and filtered to sound entries by 0x3D826:
//!
//! - global categories: 1, 7, 0x10, 0x15, 0x18, all of their sounds;
//!   category 0x0D indexes 0, 0x2F, 0x7E and 0x9F; category 0x1A indexes
//!   0x80 and 0x81; category 3 index 0;
//! - per map: category 3 index map + 1 (wall actuator sounds), the map's
//!   graphics set (8) and environment set (0x17), the shared 0xFE index of
//!   categories 8, 9, 0x0A and 0x16;
//! - the map's wall, floor and door ornaments (9, 0x0A, 0x0B), doors (0x0E);
//! - champions: the party's portraits and any portrait actuator (0x7E) on
//!   the map (category 0x16);
//! - creatures (category 0x0F): every key of a type that is in the map's
//!   creature list, has type attribute 6 set, or is made by a creature
//!   generator (wall actuator 0x2E) on the map; other types only register
//!   subs 0xFA-0xFD, so their sounds never play.
//!
//! Simplified: the door and door-ornament categories are taken whole
//! (the original narrows them to the map's door types), and the option
//! flags that widen the lists (0x803FA, 0x803FB) are assumed clear.

use std::collections::HashSet;

use dm2_formats::dungeon::{Dungeon, Element, ThingType};
use dm2_formats::gdat::{Gdat, Key};

/// Sound entries type (GRAPHICS.DAT type 2).
const SOUND: u8 = 2;

/// Actuator kinds read while scanning the map.
const ACT_GENERATOR: u16 = 0x2E;
const ACT_PORTRAIT: u16 = 0x7E;

#[derive(Clone, Debug, Default)]
pub struct Registry {
    pub map: usize,
    keys: HashSet<(u8, u8, u8)>,
}

impl Registry {
    pub fn contains(&self, cat: u8, idx: u8, sub: u8) -> bool {
        self.keys.contains(&(cat, idx, sub))
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The keys registered while the party is on `map`. `portraits` are the
    /// recruited champions' portrait numbers.
    pub fn for_map(gdat: &Gdat, dg: &Dungeon, map: usize, portraits: &[u8]) -> Registry {
        let Some(md) = dg.maps.get(map) else { return Registry { map, ..Default::default() } };
        let lists = dg.map_lists(map);
        let set = md.tileset;
        // Creature types with the full key set (byte 1 of the 0x3AB31 flags).
        let mut full_types: HashSet<u8> = lists.creature_types.iter().copied().collect();
        let mut champions: HashSet<u8> = portraits.iter().copied().collect();
        champions.insert(0xFE);
        for x in 0..md.width as i32 {
            for y in 0..md.height as i32 {
                let wall = dg.square(map, x, y).element() == Element::Wall;
                for t in dg.things_at(map, x, y) {
                    if t.kind() != ThingType::Actuator {
                        continue;
                    }
                    let w1 = dg.record_word(t, 1).unwrap_or(0);
                    match w1 & 0x7F {
                        ACT_GENERATOR if wall => {
                            full_types.insert((w1 >> 7) as u8);
                        }
                        ACT_PORTRAIT => {
                            champions.insert((w1 >> 7) as u8);
                        }
                        _ => {}
                    }
                }
            }
        }
        let mut keys = HashSet::new();
        for r in &gdat.records {
            let k = r.key;
            if k.kind != SOUND {
                continue;
            }
            let wanted = match k.cat {
                0x01 | 0x07 | 0x10 | 0x15 | 0x18 => true,
                0x03 => k.idx == 0 || k.idx as usize == map + 1,
                0x08 => k.idx == 0xFE || k.idx == set,
                0x17 => k.idx == set,
                0x09 => k.idx == 0xFE || lists.wall_ornaments.contains(&k.idx),
                0x0A => k.idx == 0xFE || lists.floor_ornaments.contains(&k.idx),
                0x0B | 0x0E => true,
                0x0D => matches!(k.idx, 0x00 | 0x2F | 0x7E | 0x9F),
                0x0F => {
                    let attr6 = gdat.lookup(Key::new(0x0F, k.idx, 11, 6)).unwrap_or(0) != 0;
                    full_types.contains(&k.idx) || attr6 || (0xFA..=0xFD).contains(&k.sub)
                }
                0x16 => champions.contains(&k.idx),
                0x1A => matches!(k.idx, 0x80 | 0x81),
                _ => false,
            };
            if wanted {
                keys.insert((k.cat, k.idx, k.sub));
            }
        }
        Registry { map, keys }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> Option<(Gdat, Dungeon)> {
        let dir = crate::assets::default_data_dir();
        let g = Gdat::open(dir.join("GRAPHICS.DAT")).ok()?;
        let d = Dungeon::parse(&std::fs::read(dir.join("DUNGEON.DAT")).ok()?).ok()?;
        Some((g, d))
    }

    #[test]
    fn listed_creature_types_register_their_sounds() {
        let Some((g, dg)) = data() else { return };
        // Map 1's creature list includes types 0x1C and 0x29.
        let r = Registry::for_map(&g, &dg, 1, &[]);
        let listed = dg.map_lists(1).creature_types;
        for rec in g.records.iter().filter(|r| r.key.kind == SOUND && r.key.cat == 0x0F) {
            if listed.contains(&rec.key.idx) {
                assert!(r.contains(0x0F, rec.key.idx, rec.key.sub), "{:?}", rec.key);
            }
        }
    }

    #[test]
    fn unlisted_creature_sounds_are_not_registered() {
        let Some((g, dg)) = data() else { return };
        let r = Registry::for_map(&g, &dg, 0, &[]);
        let listed = dg.map_lists(0).creature_types;
        // A creature sound outside subs 0xFA-0xFD of a type the start map
        // doesn't list (and without attribute 6) must not play.
        let unlisted = g.records.iter().find(|rec| {
            let k = rec.key;
            k.kind == SOUND
                && k.cat == 0x0F
                && !listed.contains(&k.idx)
                && !(0xFA..=0xFD).contains(&k.sub)
                && g.lookup(Key::new(0x0F, k.idx, 11, 6)).unwrap_or(0) == 0
        });
        if let Some(rec) = unlisted {
            assert!(!r.contains(0x0F, rec.key.idx, rec.key.sub), "{:?}", rec.key);
        }
        // Global interface sounds are always registered.
        if g.records.iter().any(|rec| rec.key == Key::new(0x18, 0, SOUND, 0x89)) {
            assert!(r.contains(0x18, 0, 0x89));
        }
    }

    #[test]
    fn wall_actuator_sounds_follow_the_map() {
        let Some((g, dg)) = data() else { return };
        let on3 = Registry::for_map(&g, &dg, 3, &[]);
        let on5 = Registry::for_map(&g, &dg, 5, &[]);
        for rec in g.records.iter().filter(|r| r.key.kind == SOUND && r.key.cat == 3) {
            let k = rec.key;
            if k.idx == 4 {
                assert!(on3.contains(3, 4, k.sub));
                assert!(!on5.contains(3, 4, k.sub));
            }
        }
    }
}
