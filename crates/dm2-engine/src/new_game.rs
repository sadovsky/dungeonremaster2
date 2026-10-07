//! New-game initialisation done while the dungeon loads (docs/05, "New
//! game"): the per-creature pass of 0x3624F.

use dm2_formats::dungeon::{ThingRef, ThingType};

use crate::state::GameState;

/// 36-byte creature info records in the user's SKULL.EXE (docs/08).
const INFO: u32 = 0x71968;
const INFO_SIZE: u32 = 36;
const INFO_COUNT: u16 = 63;

fn info_byte(g: &GameState, ty: u8, off: u32) -> Option<u8> {
    let index = g.attrs.get(15, ty, 5);
    if index >= INFO_COUNT {
        return None;
    }
    g.data.as_ref()?.exe.u8_at(INFO + INFO_SIZE * index as u32 + off)
}

/// The creature case of 0x3624F, run over every square of every map in
/// order (maps, then columns, then rows, then each square's list).
///
/// Every creature group's hit points (record word +6) are set to its type's
/// base value (info word +4). Then:
/// - info byte 0 bit 0 clear: word +10 is cleared and word +0xC records the
///   group's home (x in bits 0-4, y in bits 5-9, map in bits 10-15);
/// - bit 0 set: words +8 and +10 are cleared and, unless record byte +0xE
///   bit 7 is set, every link of the group's possession chain gets a random
///   cell (one 2-bit draw per link, starting with word +2 itself).
///
/// The draws come before the starting champion is recruited, so they decide
/// its food and water and every later random result.
pub fn init_creatures(g: &mut GameState) {
    let maps = g.dungeon.maps.len();
    for m in 0..maps {
        let (w, h) = (g.dungeon.maps[m].width as i32, g.dungeon.maps[m].height as i32);
        for x in 0..w {
            for y in 0..h {
                if !g.dungeon.square(m, x, y).has_things() {
                    continue;
                }
                for t in g.dungeon.things_at(m, x, y) {
                    if t.kind() == ThingType::Creature {
                        init_creature(g, t, m, x, y);
                    }
                }
            }
        }
    }
}

fn word(g: &GameState, t: ThingRef, n: usize) -> u16 {
    g.dungeon.record_word(t, n).unwrap_or(0)
}

fn init_creature(g: &mut GameState, c: ThingRef, map: usize, x: i32, y: i32) {
    let ty = g.dungeon.record(c).map_or(0, |r| r[4]);
    let (Some(lo), Some(hi), Some(flags)) = (info_byte(g, ty, 4), info_byte(g, ty, 5), info_byte(g, ty, 0)) else {
        return;
    };
    g.dungeon.set_record_word(c, 3, u16::from_le_bytes([lo, hi]));
    g.dungeon.set_record_word(c, 5, 0);
    if flags & 1 == 0 {
        let home = (x as u16 & 0x1F) | (y as u16 & 0x1F) << 5 | (map as u16 & 0x3F) << 10;
        g.dungeon.set_record_word(c, 6, home);
        return;
    }
    g.dungeon.set_record_word(c, 4, 0);
    let flag_e = g.dungeon.record(c).map_or(0, |r| r[0x0E]);
    let first = word(g, c, 1);
    if flag_e & 0x80 != 0 || first == ThingRef::END.0 {
        return;
    }
    let r = g.rng.rand4();
    g.dungeon.set_record_word(c, 1, (first & 0x3FFF) | r << 14);
    let mut cur = ThingRef(first);
    while cur.is_thing() {
        let next = word(g, cur, 0);
        if next != ThingRef::END.0 {
            let r = g.rng.rand4();
            g.dungeon.set_record_word(cur, 0, (next & 0x3FFF) | r << 14);
        }
        cur = ThingRef(next);
    }
}
