//! Moving the party and things between squares (docs/05-timeline.md,
//! "Moving things", "Party movement", "Map transitions").
//!
//! Map changes for the party are deferred: `GameState::pending_map` is
//! applied at the start of the next tick by `arrive`, which places the party
//! and runs the floor sensors there, as the original main loop does.

use dm2_formats::dungeon::{Element, ThingRef, ThingType};

use crate::actuators;
use crate::creatures;
use crate::doors;
use crate::effects::Effect;
use crate::hooks;
use crate::state::GameState;
use crate::viewport::{DX, DY};
use crate::world::{self, Move, PartyPos};

/// What is being moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mover {
    Party,
    Thing(ThingRef),
}

impl Mover {
    pub fn thing_type(self) -> Option<ThingType> {
        match self {
            Mover::Party => None,
            Mover::Thing(t) => Some(t.kind()),
        }
    }
}

/// Final destination after following pits, teleporters and stairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dest {
    pub map: usize,
    pub x: i32,
    pub y: i32,
    /// The moved thing with its (possibly rotated) cell; unused for the party.
    pub thing: ThingRef,
    pub falls: u16,
}

fn teleporter_at(g: &GameState, map: usize, x: i32, y: i32) -> Option<ThingRef> {
    g.dungeon.things_at(map, x, y).into_iter().find(|t| t.kind() == ThingType::Teleporter)
}

/// Text-thing marker on a square: word 1 bits 1-2 equal to 1, kind in bits
/// 11-15, id in bits 3-10. Returns (kind, id) for each such text thing.
fn markers(g: &GameState, map: usize, x: i32, y: i32) -> Vec<(u16, u16)> {
    g.dungeon
        .things_at(map, x, y)
        .into_iter()
        .filter(|t| t.kind() == ThingType::Text)
        .filter_map(|t| g.dungeon.record_word(t, 1))
        .filter(|w| w & 6 == 2)
        .map(|w| (w >> 11, w >> 3 & 0xFF))
        .collect()
}

/// Random pit destination (0x4A34A with 0x4D88A). When the map's graphics
/// set has attribute 0x6A, a pit holding a kind-0x0C marker with id n sends
/// the faller to a random kind-0x0B marker with the same id anywhere in the
/// dungeon: count them all (maps in order, squares column-major, things in
/// list order), roll random(count), and take the (roll + 1)-th.
fn random_pit_destination(g: &mut GameState, map: usize, x: i32, y: i32) -> Option<(usize, i32, i32)> {
    let tileset = g.dungeon.maps[map].tileset;
    if g.attrs.get(8, tileset, 0x6A) == 0 {
        return None;
    }
    let id = markers(g, map, x, y).into_iter().find(|&(k, _)| k == 0x0C).map(|(_, id)| id)?;
    let mut targets = Vec::new();
    for (m, md) in g.dungeon.maps.iter().enumerate() {
        for tx in 0..md.width as i32 {
            for ty in 0..md.height as i32 {
                if !g.dungeon.square(m, tx, ty).has_things() {
                    continue;
                }
                for (k, n) in markers(g, m, tx, ty) {
                    if k == 0x0B && n == id {
                        targets.push((m, tx, ty));
                    }
                }
            }
        }
    }
    let r = g.rng.random(targets.len() as u16) as usize;
    targets.get(r).copied()
}

/// Follow pits, active teleporters and (for items) stairs from a square
/// (0x4A34A). Applies the side effects the original applies while walking
/// the chain: party facing changes, fall damage, teleporter sounds.
pub fn resolve(g: &mut GameState, mover: Mover, map: usize, x: i32, y: i32) -> Dest {
    let mut thing = match mover {
        Mover::Party => ThingRef::NONE,
        Mover::Thing(t) => t,
    };
    let ty = mover.thing_type();
    // Teleporter scope class: 2 for the party, 1 or 2 for creatures, 3 otherwise.
    let mode = match ty {
        None => 2,
        Some(ThingType::Creature) => creatures::teleport_class(g, thing),
        Some(_) => 3,
    };
    let airborne = matches!(mover, Mover::Thing(t) if creatures::is_airborne(g, t));
    let (mut map, mut x, mut y) = (map, x, y);
    let mut falls = 0u16;
    for _ in 0..50 {
        let sq = g.dungeon.square(map, x, y);
        match sq.element() {
            Element::Teleporter => {
                if sq.0 & 8 == 0 {
                    break;
                }
                let Some(t) = teleporter_at(g, map, x, y) else { break };
                let w1 = g.dungeon.record_word(t, 1).unwrap_or(0);
                let w2 = g.dungeon.record_word(t, 2).unwrap_or(0);
                let scope = (w1 >> 13 & 3) as u8;
                if (scope == 1 && ty != Some(ThingType::Creature)) || (mode != 3 && scope & mode == 0) {
                    break;
                }
                let (nx, ny, nm) = ((w1 & 0x1F) as i32, (w1 >> 5 & 0x1F) as i32, (w2 >> 8) as usize);
                let same = (nx, ny, nm) == (x, y, map);
                if nm >= g.dungeon.maps.len() {
                    break;
                }
                let rot = (w1 >> 10 & 3) as u8;
                let absolute = w1 & 0x1000 != 0;
                match ty {
                    None => {
                        if w1 & 0x8000 != 0 {
                            g.effects.push(Effect::Sound { cat: 0x18, idx: 0, sub: 0x89, map, x, y });
                        }
                        let base = if absolute { 0 } else { g.party.dir };
                        g.set_party_facing((base + rot) & 3);
                    }
                    Some(ThingType::Creature) | Some(ThingType::Missile) => {
                        // TODO(0x49EF8 / 0x49F7C): rotate creature groups and missiles.
                    }
                    Some(_) => {
                        if !absolute {
                            let cell = (thing.cell() + rot) & 3;
                            thing = ThingRef(thing.0 & 0x3FFF | (cell as u16) << 14);
                        }
                    }
                }
                map = nm;
                x = nx;
                y = ny;
                if same {
                    break;
                }
            }
            Element::Pit => {
                if sq.0 & 8 == 0 || sq.0 & 1 != 0 || airborne {
                    break;
                }
                // Graphics sets with attribute 0x6A send non-creature falls to a
                // random marker square instead of the layer below (0x4A34A).
                if ty != Some(ThingType::Creature) {
                    if let Some((m, nx, ny)) = random_pit_destination(g, map, x, y) {
                        map = m;
                        x = nx;
                        y = ny;
                        if ty.is_none() {
                            if falls > 0 && hooks::champion_count(g) != 0 {
                                hooks::fall_damage(g, falls);
                            }
                            g.effects.push(Effect::PartyFell { falls });
                        }
                        continue;
                    }
                }
                falls += 1;
                let Some((m, nx, ny)) = world::layer_map(&g.dungeon, map, 1, x, y) else { break };
                map = m;
                x = nx;
                y = ny;
                match ty {
                    None => {
                        if hooks::champion_count(g) != 0 {
                            hooks::fall_damage(g, falls);
                        }
                        g.effects.push(Effect::PartyFell { falls });
                    }
                    Some(ThingType::Creature) => creatures::damage(g, thing, map, x, y, 20),
                    _ => {}
                }
            }
            Element::Stairs if !matches!(ty, None | Some(ThingType::Creature) | Some(ThingType::Missile)) => {
                // Items slide down (or off) stairs.
                if sq.0 & 4 == 0 {
                    let Some((m, nx, ny)) = world::layer_map(&g.dungeon, map, 1, x, y) else { break };
                    map = m;
                    x = nx;
                    y = ny;
                }
                let d = world::stairs_exit_dir(&g.dungeon, map, x, y);
                x += DX[d as usize];
                y += DY[d as usize];
                let back = (d + 2) & 3;
                let cell = (back + (((thing.cell().wrapping_sub(back)).wrapping_add(1) & 2) >> 1)) & 3;
                thing = ThingRef(thing.0 & 0x3FFF | (cell as u16) << 14);
            }
            _ => break,
        }
    }
    Dest { map, x, y, thing, falls }
}

fn party_on(g: &GameState, map: usize, x: i32, y: i32) -> bool {
    g.party.map == map && g.party.x == x && g.party.y == y
}

/// Move a thing (0x4B108 for non-party things). `from` None means the thing
/// is not on a square yet; `to` None removes it. Returns the thing as placed
/// (its cell may have been rotated by teleporters).
pub fn move_thing(
    g: &mut GameState,
    t: ThingRef,
    from: Option<(usize, i32, i32)>,
    to: Option<(usize, i32, i32)>,
) -> Option<ThingRef> {
    let direct = matches!(t.kind(), ThingType::Missile | ThingType::Cloud);
    if let Some((m, x, y)) = from {
        if direct {
            g.dungeon.remove_thing(m, x, y, t);
        } else {
            let here = party_on(g, m, x, y);
            actuators::floor_sensors(g, m, x, y, Mover::Thing(t), here, false);
        }
    }
    let (m, x, y) = to?;
    let d = resolve(g, Mover::Thing(t), m, x, y);
    if direct {
        g.dungeon.add_thing(d.map, d.x, d.y, d.thing);
    } else {
        let here = party_on(g, d.map, d.x, d.y);
        actuators::floor_sensors(g, d.map, d.x, d.y, Mover::Thing(d.thing), here, true);
    }
    Some(d.thing)
}

/// Move the party onto (map, x, y) of the party's current map, or remove it
/// when `to` is None (0x4B108 with thing 0xFFFF).
pub fn move_party(g: &mut GameState, to: Option<(i32, i32)>) {
    let p = g.party;
    let stationary = to == Some((p.x, p.y));
    actuators::floor_sensors(g, p.map, p.x, p.y, Mover::Party, stationary, false);
    let Some((x, y)) = to else { return };
    place_party(g, p.map, x, y, stationary);
}

/// Resolve and place the party arriving at (map, x, y): either on this map
/// now, or on another map at the start of the next tick.
fn place_party(g: &mut GameState, map: usize, x: i32, y: i32, stationary: bool) {
    let d = resolve(g, Mover::Party, map, x, y);
    if d.map != g.party.map {
        g.pending_map = Some(PartyPos { map: d.map, x: d.x, y: d.y, dir: g.party.dir });
        return;
    }
    g.party.x = d.x;
    g.party.y = d.y;
    let stationary = stationary && (d.x, d.y) == (x, y);
    // TODO(docs/05 step 4): push a creature group off the square, or bump back.
    actuators::floor_sensors(g, d.map, d.x, d.y, Mover::Party, stationary, true);
}

/// Apply a pending party map change (main loop step 1, 0x24629).
pub fn arrive(g: &mut GameState, p: PartyPos) {
    let old = g.party.map;
    if p.map != old {
        crate::map_entry::run(g, old, false);
    }
    g.party = PartyPos { map: p.map, x: p.x, y: p.y, dir: p.dir };
    if p.map != old {
        crate::map_entry::run(g, p.map, true);
    }
    place_party(g, p.map, p.x, p.y, false);
}

/// Re-move everything on a square in place, so that a newly opened pit or
/// activated teleporter takes it (0x58C6F).
pub fn drop_square(g: &mut GameState, map: usize, x: i32, y: i32) {
    if party_on(g, map, x, y) && g.pending_map.is_none() {
        move_party(g, Some((x, y)));
    }
    if let Some(c) = creatures::group_at(g, map, x, y) {
        move_thing(g, c, Some((map, x, y)), Some((map, x, y)));
    }
    let items: Vec<ThingRef> = g
        .dungeon
        .things_at(map, x, y)
        .into_iter()
        .filter(|t| (t.kind() as u8) > ThingType::Creature as u8)
        .collect();
    for t in items {
        move_thing(g, t, Some((map, x, y)), Some((map, x, y)));
        // TODO(0x58C6F): update the timeline records of moved missiles/clouds.
    }
}

/// A map-edge link (0x1D113): a teleporter square that also holds an actuator
/// of type 0x27, whose partner square on the target map is a link too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeLink {
    pub dir: u8,
    pub partner_dir: u8,
    pub x: i32,
    pub y: i32,
    pub map: usize,
}

fn link_dir(g: &GameState, map: usize, x: i32, y: i32) -> Option<(u8, ThingRef)> {
    if g.dungeon.square(map, x, y).element() != Element::Teleporter {
        return None;
    }
    let things = g.dungeon.things_at(map, x, y);
    let tele = *things.iter().find(|t| t.kind() == ThingType::Teleporter)?;
    let has_link = things
        .iter()
        .filter(|t| t.kind() == ThingType::Actuator)
        .any(|&t| g.dungeon.record_word(t, 1).unwrap_or(0) & 0x7F == 0x27);
    if !has_link {
        return None;
    }
    let w1 = g.dungeon.record_word(tele, 1).unwrap_or(0);
    Some((((w1 >> 10) as u8 + 2) & 3, tele))
}

pub fn edge_link(g: &GameState, map: usize, x: i32, y: i32) -> Option<EdgeLink> {
    let (dir, tele) = link_dir(g, map, x, y)?;
    let w1 = g.dungeon.record_word(tele, 1).unwrap_or(0);
    let w2 = g.dungeon.record_word(tele, 2).unwrap_or(0);
    let (tx, ty, tm) = ((w1 & 0x1F) as i32, (w1 >> 5 & 0x1F) as i32, (w2 >> 8) as usize);
    if tm >= g.dungeon.maps.len() {
        return None;
    }
    let (partner_dir, _) = link_dir(g, tm, tx, ty)?;
    Some(EdgeLink { dir, partner_dir, x: tx, y: ty, map: tm })
}

/// Move the party to (x, y) on `map` and face `dir` (0x4BED2). Coordinates
/// outside the target map are ignored, as in the original.
pub fn teleport_party(g: &mut GameState, x: i32, y: i32, map: usize, dir: u8) {
    let Some(m) = g.dungeon.maps.get(map) else { return };
    if x < 0 || y < 0 || x >= m.width as i32 || y >= m.height as i32 {
        return;
    }
    if map != g.party.map {
        move_party(g, None);
        g.pending_map = Some(PartyPos { map, x, y, dir: dir & 3 });
        return;
    }
    move_party(g, Some((x, y)));
    g.set_party_facing(dir & 3);
}

/// Take the stairs at the party's square (0x232DD).
pub fn take_stairs(g: &mut GameState) {
    move_party(g, None);
    let mut p = g.party;
    if p.take_stairs(&g.dungeon) {
        g.pending_map = Some(p);
    }
}

/// Outcome of the move classifier (0x23D13).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveClass {
    /// 1: standing on stairs and stepping back.
    StairsBack,
    /// 2: the destination is stairs.
    IntoStairs,
    /// 3: blocked.
    Blocked,
    /// 4/5: a creature group is there.
    Creature,
    /// 6: free.
    Free,
}

pub fn classify(g: &GameState, mv: Move, dest: (i32, i32)) -> MoveClass {
    let p = g.party;
    if mv == Move::Back && g.dungeon.square(p.map, p.x, p.y).element() == Element::Stairs {
        return MoveClass::StairsBack;
    }
    if g.dungeon.square(p.map, dest.0, dest.1).element() == Element::Stairs {
        return MoveClass::IntoStairs;
    }
    if world::blocks(&g.dungeon, p.map, dest.0, dest.1) {
        return MoveClass::Blocked;
    }
    if creatures::group_at(g, p.map, dest.0, dest.1).is_some() {
        return MoveClass::Creature;
    }
    MoveClass::Free
}

/// The party move command (0x235BF). Returns true if the party moved (the
/// caller then applies the move cooldown).
pub fn party_command(g: &mut GameState, mv: Move) -> bool {
    let p = g.party;
    let d = ((p.dir + mv.offset()) & 3) as usize;
    let dest = (p.x + DX[d], p.y + DY[d]);
    // TODO(0x47707): each living champion pays a stamina cost for the step.
    match classify(g, mv, dest) {
        MoveClass::StairsBack => {
            take_stairs(g);
            true
        }
        MoveClass::IntoStairs => {
            move_party(g, None);
            g.party.x = dest.0;
            g.party.y = dest.1;
            take_stairs(g);
            true
        }
        MoveClass::Blocked => {
            let sq = g.dungeon.square(p.map, dest.0, dest.1);
            if hooks::champion_count(g) != 0 && sq.element() == Element::Door && sq.0 & 7 == 4 {
                let power = hooks::bash_power(g);
                doors::bash(g, p.map, dest.0, dest.1, power, 0, false);
            }
            false
        }
        // TODO(0x24171 / 0x23E5B / 0x24328): swap or push the group, else a
        // 5-point bump.
        MoveClass::Creature => false,
        MoveClass::Free => {
            let on_stairs = g.dungeon.square(p.map, p.x, p.y).element() == Element::Stairs;
            if !on_stairs {
                if let Some(l) = edge_link(g, p.map, dest.0, dest.1) {
                    if (l.partner_dir + 2) & 3 != p.dir {
                        teleport_party(g, l.x, l.y, l.map, p.dir);
                        return true;
                    }
                }
            }
            move_party(g, Some(dest));
            true
        }
    }
}
