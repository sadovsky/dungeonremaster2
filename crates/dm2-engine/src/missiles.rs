//! Missiles, explosions and clouds (docs/05 "Missile flight", docs/07
//! "Missiles" and "Explosions").
//!
//! A missile is a thing of type 14 (8 bytes): word 1 is what flies (an item
//! reference or an explosion kind 0xFF80-0xFFBF), byte 4 the kinetic energy,
//! byte 5 the attack, word 3 its timeline event. Each missile owns one event
//! that moves it one step per tick. Explosions and clouds are things of type
//! 15 (4 bytes): word 1 holds the kind in bits 0-6 and the strength in the
//! high byte; event 0x19 runs their lifetime.

use dm2_formats::dungeon::{Element, ThingRef, ThingType};

use crate::apply;
use crate::combat::attack;
use crate::creatures;
use crate::doors;
use crate::movement;
use crate::state::GameState;
use crate::timeline::Event;
use crate::viewport::{DX, DY};

pub const EVENT_CLOUD: u8 = 0x19;
pub const EVENT_MISSILE_LAUNCH: u8 = 0x1D;
pub const EVENT_MISSILE: u8 = 0x1E;

/// Explosion kinds (missile word 1 values).
pub mod kind {
    pub const FIREBALL: u16 = 0xFF80;
    pub const POISON_BLOB: u16 = 0xFF81;
    pub const LIGHTNING: u16 = 0xFF82;
    pub const HARM_NON_MATERIAL: u16 = 0xFF83;
    pub const OPEN_DOOR: u16 = 0xFF84;
    pub const POISON_BOLT: u16 = 0xFF86;
    pub const POISON_CLOUD: u16 = 0xFF87;
    pub const BREAK_DOOR: u16 = 0xFF8D;
    pub const SPELL_BURST: u16 = 0xFF8E;
    pub const FIZZLE: u16 = 0xFFA8;
    pub const BLAST_A: u16 = 0xFFB0;
    pub const BLAST_B: u16 = 0xFFB1;
    pub const REBIRTH: u16 = 0xFFE4;
}

/// Cloud kind 0x0E reverses missiles that would enter its square.
const CLOUD_BOUNCE: u8 = 0x0E;
/// Head and torso (wound bits 2 and 3).
const PARTS_HEAD_TORSO: u16 = 0x0C;
const PARTS_ALL: u16 = 0x3F;

fn is_explosion(what: u16) -> bool {
    what >= 0xFF80
}

/// Pack the event's position word: x bits 0-4, y 5-9, direction 10-11,
/// step energy 12-15.
fn pack(x: i32, y: i32, dir: u8, step: u8) -> u16 {
    (x as u16 & 0x1F) | (y as u16 & 0x1F) << 5 | (dir as u16 & 3) << 10 | (step as u16 & 15) << 12
}

fn unpack(w: u16) -> (i32, i32, u8, u8) {
    ((w & 0x1F) as i32, (w >> 5 & 0x1F) as i32, (w >> 10 & 3) as u8, (w >> 12) as u8)
}

struct Missile {
    what: u16,
    energy: u8,
    attack: u8,
}

fn read(g: &GameState, m: ThingRef) -> Option<Missile> {
    let r = g.dungeon.record(m)?;
    if u16::from_le_bytes([r[0], r[1]]) == dm2_formats::dungeon::Dungeon::FREE {
        return None;
    }
    Some(Missile { what: u16::from_le_bytes([r[2], r[3]]), energy: r[4], attack: r[5] })
}

/// Create a missile (0x16457) flying `dir` from cell `cell` of (x, y).
/// `by_dungeon` marks shooter launches, whose first step skips nothing.
/// Without a free missile record a thrown item just drops on the square.
#[allow(clippy::too_many_arguments)]
pub fn launch(
    g: &mut GameState,
    what: u16,
    map: usize,
    x: i32,
    y: i32,
    cell: u8,
    dir: u8,
    energy: u8,
    attack: u8,
    step: u8,
    by_dungeon: bool,
) -> Option<ThingRef> {
    let Some(m) = g.dungeon.alloc_thing(ThingType::Missile) else {
        if !is_explosion(what) {
            g.dungeon.add_thing(map, x, y, ThingRef((what & 0x3FFF) | (cell as u16 & 3) << 14));
        }
        return None;
    };
    g.dungeon.set_record_word(m, 1, what);
    if let Some(r) = g.dungeon.record_mut(m) {
        r[4] = energy;
        r[5] = attack;
    }
    let placed = ThingRef(m.0 | (cell as u16 & 3) << 14);
    g.dungeon.add_thing(map, x, y, placed);
    let kind = if by_dungeon { EVENT_MISSILE } else { EVENT_MISSILE_LAUNCH };
    let mut ev = Event::new(kind, map as u8, g.tick.wrapping_add(1));
    [ev.x, ev.y] = placed.0.to_le_bytes();
    ev.set_w8(pack(x, y, dir, step));
    if let Some(slot) = g.schedule(ev) {
        g.dungeon.set_record_word(m, 3, slot);
    }
    Some(placed)
}

/// Damage a missile does on impact (0x16D72): (damage, attack type, side
/// value). The side value is poison for champions and the slayer bonus
/// for creatures. Random calls follow the original order exactly.
pub fn impact_damage(g: &mut GameState, what: u16, energy: u8, attack_byte: u8) -> (i16, u16, i16) {
    let e = energy as u32;
    let mut side: i32 = 0;
    let mut atype = attack::BLUNT;
    let mut base: u32;
    if is_explosion(what) {
        match what {
            kind::POISON_BLOB => {
                let r = g.rng.rnd() & 15;
                side = (r + 10) as i32;
                base = r + (g.rng.rnd() & 31);
            }
            kind::FIREBALL | kind::LIGHTNING => {
                atype = attack::FIRE;
                base = (g.rng.rnd() & 15) + (g.rng.rnd() & 15) + 10;
                if what == kind::LIGHTNING {
                    atype = attack::LIGHTNING;
                    base = (base << 4) + e;
                }
            }
            kind::POISON_BOLT => return ((e >> 3) as i16 + 1, attack::MAGIC, e as i16),
            _ => return (0, attack::MAGIC, 0),
        }
    } else {
        let Some(data) = g.data.clone() else { return (0, atype, 0) };
        let t = ThingRef(what);
        let (dmg_attr, slayer, weight) = {
            let db = data.item_db(&g.dungeon);
            (db.attr(t, crate::items::ATTR_DAMAGE) as u32, db.attr(t, crate::items::ATTR_SLAYER), db.weight(t))
        };
        base = 0;
        if dmg_attr != 0 {
            let k = (attack_byte as u32 >> 4) + 3;
            base = ((dmg_attr + (e >> 1)) & 0xFFFF) * (k * k) >> 7;
            atype = attack::SHARP;
            side = slayer as i32;
            if side != 0 && (g.rng.rnd() & 0x7F) > e {
                side -= g.rng.random((side / 2 + 1) as u16) as i32;
            }
        }
        base += g.rng.rand4() as u32;
        base += weight as u32;
        if (g.rng.rnd() & 0x1FF) < attack_byte as u32 {
            base *= 2;
        }
    }
    let base = base & 0xFFFF;
    let r = g.rng.random(((((base + e) >> 4) + 1) >> 1) as u16 + 1) as i32;
    let mut d = (base as i32 + r + g.rng.rand4() as i32) as i16 as i32;
    d = d.max(2 * (d - (32 - (attack_byte as i32 >> 3))));
    d = d.min(2 * e as i32);
    (d as i16, atype, side as i16)
}

/// Remove a missile from its square and free it. Returns what it carried.
fn retire(g: &mut GameState, m: ThingRef, map: usize, x: i32, y: i32) -> Option<Missile> {
    let ms = read(g, m)?;
    g.dungeon.remove_thing(map, x, y, m);
    g.dungeon.free_thing(m);
    Some(ms)
}

/// A missile ends its flight on (x, y): items drop in its cell, explosion
/// kinds burst there (0x16C0C).
fn land(g: &mut GameState, m: ThingRef, map: usize, x: i32, y: i32) {
    let cell = m.cell();
    let Some(ms) = retire(g, m, map, x, y) else { return };
    if is_explosion(ms.what) {
        explode(g, ms.what, ms.attack, map, x, y, cell);
    } else {
        drop_item(g, ThingRef(ms.what), map, x, y, cell);
    }
}

fn drop_item(g: &mut GameState, item: ThingRef, map: usize, x: i32, y: i32, cell: u8) {
    let placed = ThingRef((item.0 & 0x3FFF) | (cell as u16 & 3) << 14);
    movement::move_thing(g, placed, None, Some((map, x, y)));
    // Thrown potions burst instead (docs/07): TODO(potion kinds 3, 0x13).
}

/// The party is hit by missile `m` (0x1726B mode −3). Returns true if a
/// champion stood in the struck cell.
fn hit_party(g: &mut GameState, m: ThingRef, dir: u8) -> bool {
    let cell = m.cell();
    let Some(idx) = g.champions.iter().position(|c| c.is_alive() && c.cell() & 3 == cell) else { return false };
    let Some(ms) = read(g, m) else { return false };
    let (d, atype, side) = impact_damage(g, ms.what, ms.energy, ms.attack);
    // The champion can parry when facing into the missile.
    let parry = if g.champions[idx].facing() == (dir + 2) & 3 { attack::PARRYABLE } else { 0 };
    apply::damage_champion(g, idx, d, PARTS_HEAD_TORSO, atype | parry);
    if side > 0 && (atype == attack::MAGIC || ms.what == kind::POISON_BLOB) && g.rng.rnd() & 7 != 0 {
        let dose = if ms.what == kind::POISON_BOLT { side * 2 } else { side };
        crate::champions::poison(g, idx, dose);
    }
    true
}

/// A creature group is hit (0x1726B mode −1).
fn hit_creature(g: &mut GameState, m: ThingRef, c: ThingRef, map: usize, x: i32, y: i32) {
    let Some(ms) = read(g, m) else { return };
    let (d, atype, side) = impact_damage(g, ms.what, ms.energy, ms.attack);
    let amount = match creatures::defence(g, c) {
        Some(def) => {
            if def.flags & 0x20 != 0 && ms.what != kind::HARM_NON_MATERIAL {
                return;
            }
            let mut a = (d as i32 * 64) / (def.armour.max(1) as i32) + side as i32;
            if def.flags19 & 0x10 != 0 && atype != attack::FIRE {
                a >>= 2;
            }
            a
        }
        None => d as i32,
    };
    if amount > 0 {
        creatures::damage(g, c, map, x, y, amount as u16);
    }
}

/// Finish a missile that hit something: explosions burst, items drop.
fn impact(g: &mut GameState, m: ThingRef, map: usize, x: i32, y: i32) {
    land(g, m, map, x, y);
}

fn blocks_missile(g: &GameState, map: usize, x: i32, y: i32, from_stairs: bool) -> bool {
    let sq = g.dungeon.square(map, x, y);
    match sq.element() {
        Element::Wall | Element::Rock => true,
        Element::TrickWall => sq.0 & 0b101 == 0,
        Element::Stairs => from_stairs,
        _ => false,
    }
}

fn bounce_cloud_at(g: &GameState, map: usize, x: i32, y: i32) -> bool {
    g.dungeon.things_at(map, x, y).into_iter().any(|t| {
        t.kind() == ThingType::Cloud && g.dungeon.record_word(t, 1).is_some_and(|w| (w & 0x7F) as u8 == CLOUD_BOUNCE)
    })
}

/// Missile step, events 0x1D/0x1E (0x17A7B).
pub fn flight_event(g: &mut GameState, ev: Event) {
    let map = ev.map as usize;
    let mut m = ThingRef(u16::from_le_bytes([ev.x, ev.y]));
    let (x, y, mut dir, step) = unpack(ev.w8());
    let Some(ms) = read(g, m) else { return };
    // 1-2. Hit test on the current square (skipped on the launch tick).
    if ev.kind != EVENT_MISSILE_LAUNCH {
        if let Some(c) = creatures::group_at(g, map, x, y) {
            if is_explosion(ms.what) && creatures::reflects_spells(g, c) {
                if let Some(data) = g.data.clone() {
                    let nd = data.reflect_dir(dir, m.cell(), creatures::facing(g, c) & 1);
                    if nd < 4 {
                        dir = nd;
                    }
                }
            } else {
                hit_creature(g, m, c, map, x, y);
                impact(g, m, map, x, y);
                return;
            }
        } else if g.party.map == map && g.party.x == x && g.party.y == y && hit_party(g, m, dir) {
            impact(g, m, map, x, y);
            return;
        }
    }
    // 3. Energy.
    if ms.energy <= step {
        land(g, m, map, x, y);
        return;
    }
    if let Some(r) = g.dungeon.record_mut(m) {
        r[4] = ms.energy - step;
        r[5] = ms.attack.saturating_sub(step);
    }
    // 4. Advance.
    let cell = m.cell();
    let leaving = cell == dir || cell == (dir + 1) & 3;
    let new_cell = if (dir & 1) == (cell & 1) { (cell + 3) & 3 } else { (cell + 1) & 3 };
    let (mut nx, mut ny) = (x, y);
    if leaving {
        let (tx, ty) = (x + DX[dir as usize], y + DY[dir as usize]);
        let on_stairs = g.dungeon.square(map, x, y).element() == Element::Stairs;
        if blocks_missile(g, map, tx, ty, on_stairs) {
            impact(g, m, map, x, y);
            return;
        }
        if bounce_cloud_at(g, map, tx, ty) {
            dir = (dir + 2) & 3;
        } else {
            nx = tx;
            ny = ty;
        }
    }
    // 5. Move.
    let (mut map2, mut x2, mut y2) = (map, x, y);
    if (nx, ny) == (x, y) {
        if leaving {
            // bounced: stay put, keep the cell
        } else {
            if g.dungeon.square(map, x, y).element() == Element::Door && door_closed(g, map, x, y) {
                door_hit(g, ms.what, map, x, y);
                impact(g, m, map, x, y);
                return;
            }
            g.dungeon.remove_thing(map, x, y, m);
            m = ThingRef((m.0 & 0x3FFF) | (new_cell as u16) << 14);
            g.dungeon.add_thing(map, x, y, m);
        }
    } else {
        let moved = ThingRef((m.0 & 0x3FFF) | (new_cell as u16) << 14);
        g.dungeon.remove_thing(map, x, y, m);
        let d = movement::resolve(g, movement::Mover::Thing(moved), map, nx, ny);
        g.dungeon.add_thing(d.map, d.x, d.y, d.thing);
        m = d.thing;
        (map2, x2, y2) = (d.map, d.x, d.y);
        // Wake creatures on the destination and, through open space, beyond.
        if let Some(c) = creatures::group_at(g, map2, x2, y2) {
            creatures::alert(g, c, map2, x2, y2);
        }
        let (bx, by) = (x2 + DX[dir as usize], y2 + DY[dir as usize]);
        let sq = g.dungeon.square(map2, x2, y2);
        let open = !matches!(sq.element(), Element::Wall | Element::Rock)
            && !(sq.element() == Element::Door && door_closed(g, map2, x2, y2))
            && !(sq.element() == Element::TrickWall && sq.0 & 0b101 == 0);
        if open {
            if let Some(c) = creatures::group_at(g, map2, bx, by) {
                creatures::alert(g, c, map2, bx, by);
            }
        }
    }
    // 6. Reschedule one tick ahead.
    let mut next = Event { kind: EVENT_MISSILE, map: map2 as u8, tick: g.tick.wrapping_add(1), ..ev };
    [next.x, next.y] = m.0.to_le_bytes();
    next.set_w8(pack(x2, y2, dir, step));
    if let Some(slot) = g.schedule(next) {
        g.dungeon.set_record_word(m, 3, slot);
    }
}

fn door_closed(g: &GameState, map: usize, x: i32, y: i32) -> bool {
    matches!(g.dungeon.square(map, x, y).0 & 7, 1..=4)
}

/// A missile strikes a closed door (0x1726B mode 4): door spells open or
/// break it; other explosions and heavy items may bash it.
fn door_hit(g: &mut GameState, what: u16, map: usize, x: i32, y: i32) {
    match what {
        kind::OPEN_DOOR => {
            let mut ev = Event::new(4, map as u8, g.tick.wrapping_add(1));
            ev.x = x as u8;
            ev.y = y as u8;
            ev.b9 = 0; // set = open
            g.schedule(ev);
        }
        kind::BREAK_DOOR | kind::FIREBALL => {
            doors::bash(g, map, x, y, 0xFF, 1, true);
        }
        _ => {}
    }
}

/// Create an explosion or cloud on (x, y) (0x16746) and apply its
/// immediate area damage. Returns the cloud thing.
pub fn explode(g: &mut GameState, what: u16, strength: u8, map: usize, x: i32, y: i32, cell: u8) -> Option<ThingRef> {
    let c = g.dungeon.alloc_thing(ThingType::Cloud)?;
    let kind = ((what.wrapping_add(0x80)) & 0x7F) as u8;
    g.dungeon.set_record_word(c, 1, kind as u16 | (strength as u16) << 8);
    let placed = ThingRef(c.0 | (cell as u16 & 3) << 14);
    g.dungeon.add_thing(map, x, y, placed);
    let delay = match what {
        kind::REBIRTH => 5,
        kind::SPELL_BURST => (strength >> 1) as u32,
        _ => 1,
    };
    let mut ev = Event::new(EVENT_CLOUD, map as u8, g.tick.wrapping_add(delay.max(1)));
    ev.x = x as u8;
    ev.y = y as u8;
    ev.set_w8(placed.0);
    g.schedule(ev);
    if matches!(what, kind::FIREBALL | kind::LIGHTNING | kind::BLAST_A | kind::BLAST_B | kind::SPELL_BURST) {
        let e = strength as u16;
        let groups: Vec<ThingRef> =
            g.dungeon.things_at(map, x, y).into_iter().filter(|t| t.kind() == ThingType::Creature).collect();
        for grp in groups {
            let r = creatures::resistance(g, grp) as u16;
            if r == 15 {
                continue;
            }
            let base = (e / 2 + 1) + g.rng.random(e / 2 + 1) + 1;
            let mut d = base as i32 - g.rng.random(2 * r + 1) as i32;
            if creatures::is_non_material(g, grp) {
                d >>= 2;
            }
            if d > 0 {
                creatures::damage(g, grp, map, x, y, d as u16);
            }
        }
        if g.party.map == map && g.party.x == x && g.party.y == y {
            // TODO(docs/07): the exact party amount; the creature base roll is used.
            let d = (e / 2 + 1) + g.rng.random(e / 2 + 1) + 1;
            apply::damage_party(g, d as i16, PARTS_ALL, attack::FIRE);
        }
    }
    if matches!(what, kind::OPEN_DOOR | kind::BREAK_DOOR) {
        door_hit(g, what, map, x, y);
    }
    Some(placed)
}

/// Cloud lifetime step, event 0x19 (0x18395).
pub fn cloud_event(g: &mut GameState, ev: Event) {
    let map = ev.map as usize;
    let (x, y) = (ev.x as i32, ev.y as i32);
    let c = ThingRef(ev.w8());
    let Some(w) = g.dungeon.record_word(c, 1) else { return };
    if w == dm2_formats::dungeon::Dungeon::FREE {
        return;
    }
    let kind = (w & 0x7F) as u8;
    let strength = (w >> 8) as u8;
    if !matches!(kind, 0 | 2 | CLOUD_BOUNCE) {
        let flags = g.data.as_ref().map_or(0, |d| d.cloud_flags(kind));
        let dmg = |g: &mut GameState| -> u16 {
            if flags & 1 != 0 {
                g.rng.random(strength as u16 / 2 + 1) + 1
            } else {
                strength as u16
            }
        };
        // TODO(0x181F0): kinds 3 and 7 use further per-kind formulas.
        if flags & 4 != 0 && g.party.map == map && g.party.x == x && g.party.y == y {
            let d = dmg(g);
            apply::damage_party(g, d as i16, PARTS_ALL, attack::MAGIC);
        }
        if flags & 8 != 0 {
            if let Some(grp) = creatures::group_at(g, map, x, y) {
                let d = dmg(g);
                creatures::damage(g, grp, map, x, y, d);
            }
        }
        // Lingering clouds decay and stay.
        let next = match kind {
            7 if strength >= 6 => Some(strength - 3),
            0x28 if strength > 0x37 => Some(strength - 0x28),
            _ => None,
        };
        if let Some(s) = next {
            g.dungeon.set_record_word(c, 1, (w & 0xFF) | (s as u16) << 8);
            g.schedule(Event { tick: g.tick.wrapping_add(1), ..ev });
            return;
        }
    }
    g.dungeon.remove_thing(map, x, y, c);
    g.dungeon.free_thing(c);
}
