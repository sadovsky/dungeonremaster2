//! Doors (docs/05-timeline.md, "Doors").
//!
//! The door's position lives in its square (bits 0-2: 0 open .. 4 closed,
//! 5 destroyed); its record's word 1 holds the runtime animation bits:
//! bit 9 direction (set = opening), bit 10 animating, bit 12 cleared when a
//! closing move starts.

use dm2_formats::dungeon::{ThingRef, ThingType};

use crate::actuators::resolve_action;
use crate::creatures;
use crate::effects::Effect;
use crate::hooks;
use crate::state::GameState;
use crate::timeline::Event;

const OPENING: u16 = 1 << 9;
const ANIMATING: u16 = 1 << 10;
const BIT12: u16 = 1 << 12;

pub fn door_at(g: &GameState, map: usize, x: i32, y: i32) -> Option<ThingRef> {
    g.dungeon.things_at(map, x, y).into_iter().find(|t| t.kind() == ThingType::Door)
}

/// Door type for attribute lookups (0x1FE1C): word 1 bit 0 picks the map's
/// door type 0 or 1; 0xFF when that type is not enabled.
pub fn door_type(g: &GameState, map: usize, door: ThingRef) -> u8 {
    let m = &g.dungeon.maps[map];
    let t = if g.dungeon.record_word(door, 1).unwrap_or(0) & 1 == 0 { m.door_type0 } else { m.door_type1 };
    t.unwrap_or(0xFF)
}

fn state(g: &GameState, map: usize, x: i32, y: i32) -> u8 {
    g.dungeon.square(map, x, y).0 & 7
}

fn set_state(g: &mut GameState, map: usize, x: i32, y: i32, s: u8) {
    let sq = g.dungeon.square(map, x, y).0;
    g.dungeon.set_square(map, x, y, sq & !7 | s);
}

fn word1(g: &GameState, d: ThingRef) -> u16 {
    g.dungeon.record_word(d, 1).unwrap_or(0)
}

/// Square action on a door square (0x568F7): open, close or reverse.
pub fn action(g: &mut GameState, mut ev: Event) {
    let (map, x, y) = (ev.map as usize, ev.x as i32, ev.y as i32);
    let s = state(g, map, x, y);
    if s == 5 {
        return;
    }
    let Some(d) = door_at(g, map, x, y) else { return };
    let mut w = word1(g, d);
    let act = ev.b9;
    if w & ANIMATING != 0 {
        let opening = resolve_action(act, w & OPENING != 0);
        w = (w & !OPENING) | if opening { OPENING } else { 0 };
        if !opening {
            w &= !BIT12;
        }
        g.dungeon.set_record_word(d, 1, w);
        return;
    }
    let start = match s {
        0 if act == 1 || act == 2 => {
            w &= !OPENING;
            true
        }
        4 if act == 0 || act == 2 => {
            w |= OPENING;
            true
        }
        0 | 4 => false,
        _ => {
            w = (w & !OPENING) | if act == 0 { OPENING } else { 0 };
            true
        }
    };
    if start {
        w |= ANIMATING;
        if w & OPENING == 0 {
            w &= !BIT12;
        }
    } else {
        w &= !ANIMATING;
    }
    g.dungeon.set_record_word(d, 1, w);
    if start {
        // The same record becomes a type-1 event at the same tick.
        ev.kind = 1;
        g.schedule(ev);
    }
}

/// One door animation step (event 0x01, 0x564C6).
pub fn step(g: &mut GameState, mut ev: Event) {
    let (map, x, y) = (ev.map as usize, ev.x as i32, ev.y as i32);
    let mut s = state(g, map, x, y);
    if s == 5 {
        return;
    }
    let Some(d) = door_at(g, map, x, y) else { return };
    let mut w = word1(g, d);
    if w & ANIMATING == 0 {
        return;
    }
    ev.tick = ev.tick.wrapping_add(1);
    let closing = w & OPENING == 0;
    let mut blocked = false;
    let dtype = door_type(g, map, d);
    if closing {
        if s == 4 {
            g.dungeon.set_record_word(d, 1, w & !ANIMATING);
            return;
        }
        let size_check = w & 0x20 != 0;
        let damage = g.attrs.get(14, dtype, 0x0F);
        let party_here = g.party.map == map && g.party.x == x && g.party.y == y;
        if party_here && s != 0 && hooks::champion_count(g) != 0 {
            // Closing onto the party: snap fully open and hurt the champions.
            s = 0;
            set_state(g, map, x, y, 0);
            if hooks::damage_party(g, damage) != 0 {
                g.effects.push(Effect::Sound { cat: 0x16, idx: 0xFF, sub: 0x8A, map, x, y });
            }
            blocked = true;
        }
        if let Some(c) = creatures::group_at(g, map, x, y) {
            if !creatures::is_non_material(g, c) {
                let size = if size_check { creatures::door_size(g, c) } else { 1 };
                if size <= s as u16 {
                    let mut dmg = damage;
                    if creatures::halves_door_damage(g, c) {
                        dmg = (dmg >> 1).max(1);
                    }
                    creatures::damage(g, c, map, x, y, dmg);
                    s = s.saturating_sub(1);
                    set_state(g, map, x, y, s);
                    g.effects.push(Effect::Sound { cat: 0x15, idx: 0xFE, sub: 0x85, map, x, y });
                    blocked = true;
                }
            }
        }
        if blocked {
            ev.tick = ev.tick.wrapping_add(1);
        }
    } else if s == 0 {
        g.dungeon.set_record_word(d, 1, w & !ANIMATING);
        return;
    }
    let mut cont = blocked;
    if !blocked {
        let n = if closing { s + 1 } else { s - 1 };
        set_state(g, map, x, y, n);
        cont = if closing { n != 4 } else { n != 0 };
        let sub = if !cont && n == 4 { 0x8F } else { 0x8E };
        g.effects.push(Effect::Sound { cat: 14, idx: dtype, sub, map, x, y });
    }
    w = word1(g, d);
    if cont {
        g.dungeon.set_record_word(d, 1, w | ANIMATING);
        g.schedule(ev);
    } else {
        g.dungeon.set_record_word(d, 1, w & !ANIMATING);
    }
}

/// Damage a door (0x18D9E). `magic` selects which flag allows it: word 1
/// bit 8 for physical bashing, bit 7 for spells. A closed door whose type
/// strength (attribute 14/type/11/0x0E) is at most `damage` is destroyed,
/// at once or after `delay` ticks through event 0x02. Returns true if it
/// gives way.
pub fn bash(g: &mut GameState, map: usize, x: i32, y: i32, damage: u16, delay: u32, magic: bool) -> bool {
    let Some(d) = door_at(g, map, x, y) else { return false };
    let w = word1(g, d);
    let allowed = if magic { w & 0x80 != 0 } else { w & 0x100 != 0 };
    if !allowed {
        return false;
    }
    let strength = g.attrs.get(14, door_type(g, map, d), 0x0E);
    if strength > damage || state(g, map, x, y) != 4 {
        return false;
    }
    if delay == 0 {
        set_state(g, map, x, y, 5);
    } else {
        let mut ev = Event::new(2, map as u8, g.tick.wrapping_add(delay));
        ev.x = x as u8;
        ev.y = y as u8;
        g.schedule(ev);
    }
    true
}

/// Event 0x02: the door is destroyed (0x56AF9).
pub fn destroy(g: &mut GameState, ev: Event) {
    set_state(g, ev.map as usize, ev.x as i32, ev.y as i32, 5);
}
