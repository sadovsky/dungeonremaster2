//! Actuators and sensors (docs/05-timeline.md, "Actuators").
//!
//! Actuator record (8 bytes): word 1 bits 0-6 type, 7-15 data; word 2 bit 0
//! busy, bit 2 enabled/state, bits 3-4 action (0 set, 1 clear, 2 toggle,
//! 3 follow), bit 5 inverted, bit 6 sound, bits 7-10 delay; word 3 bits 4-5
//! target cell, 6-10 target x, 11-15 target y.

use dm2_formats::dungeon::{Element, ThingRef, ThingType};

use crate::creatures;
use crate::effects::Effect;
use crate::hooks;
use crate::movement::{self, Mover};
use crate::state::GameState;
use crate::timeline::Event;

pub const SET: u8 = 0;
pub const CLEAR: u8 = 1;
pub const TOGGLE: u8 = 2;
pub const FOLLOW: u8 = 3;

/// New value of a flag under an action (0x57D27).
pub fn resolve_action(action: u8, current: bool) -> bool {
    match action {
        SET => true,
        TOGGLE => !current,
        _ => false,
    }
}

/// The opposite action (set <-> clear; toggle stays toggle).
fn inverse(action: u8) -> u8 {
    match action {
        SET => CLEAR,
        CLEAR => SET,
        a => a,
    }
}

/// A decoded actuator.
#[derive(Clone, Copy, Debug)]
pub struct Actuator {
    pub thing: ThingRef,
    pub w1: u16,
    pub w2: u16,
    pub w3: u16,
}

impl Actuator {
    pub fn load(g: &GameState, t: ThingRef) -> Actuator {
        let w = |n| g.dungeon.record_word(t, n).unwrap_or(0);
        Actuator { thing: t, w1: w(1), w2: w(2), w3: w(3) }
    }
    pub fn kind(&self) -> u16 {
        self.w1 & 0x7F
    }
    pub fn data(&self) -> u16 {
        self.w1 >> 7
    }
    pub fn busy(&self) -> bool {
        self.w2 & 1 != 0
    }
    pub fn enabled(&self) -> bool {
        self.w2 & 4 != 0
    }
    pub fn action(&self) -> u8 {
        (self.w2 >> 3 & 3) as u8
    }
    pub fn inverted(&self) -> bool {
        self.w2 & 0x20 != 0
    }
    pub fn sound(&self) -> bool {
        self.w2 & 0x40 != 0
    }
    pub fn delay(&self) -> u32 {
        (self.w2 >> 7 & 0xF) as u32
    }
    pub fn target(&self) -> (i32, i32, u8) {
        ((self.w3 >> 6 & 0x1F) as i32, (self.w3 >> 11) as i32, (self.w3 >> 4 & 3) as u8)
    }
    /// "Matches": a set, or a clear when inverted.
    fn triggered_by(&self, action: u8) -> bool {
        (!self.inverted() && action == SET) || (self.inverted() && action == CLEAR)
    }
}

fn set_w(g: &mut GameState, t: ThingRef, n: usize, v: u16) {
    g.dungeon.set_record_word(t, n, v);
}

/// Schedule a square action (0x4BBE4). Priority: clear 3, toggle 2, set 1,
/// so on one tick clears land before toggles before sets.
pub fn square_action(g: &mut GameState, map: usize, x: i32, y: i32, cell: u8, action: u8, tick: u32) {
    let mut ev = Event::new(4, map as u8, tick);
    ev.prio = match action {
        SET => 1,
        CLEAR => 3,
        _ => 2,
    };
    ev.x = x as u8;
    ev.y = y as u8;
    ev.b8 = cell;
    ev.b9 = action;
    g.schedule(ev);
}

/// Fire an actuator at its target (0x4BC4C).
pub fn fire(g: &mut GameState, map: usize, a: &Actuator, action: u8, extra_delay: u32) {
    let (x, y, cell) = a.target();
    let tick = g.tick.wrapping_add(a.delay()).wrapping_add(extra_delay);
    square_action(g, map, x, y, cell, action, tick);
}

/// Global item number (0x1EF0C): per-type base plus the index within the
/// category; 0x1FF for things that aren't items.
pub fn item_number(g: &GameState, t: ThingRef) -> u16 {
    if !t.is_thing() {
        return 0x1FF;
    }
    let w = |n| g.dungeon.record_word(t, n).unwrap_or(0);
    match t.kind() {
        ThingType::Weapon => w(1) & 0x7F,
        ThingType::Clothing => 0x80 + (w(1) & 0x7F),
        ThingType::Misc => 0x100 + (w(1) & 0x7F),
        ThingType::Potion => 0x180 + (w(1) >> 8 & 0x7F),
        ThingType::Creature => 0x1B0 + g.dungeon.record(t).map_or(0, |r| r[4] as u16),
        ThingType::Container => 0x1E0 + ((w(2) >> 13 & 7) | (w(2) >> 1 & 3) << 3),
        ThingType::Scroll => 0x1FC,
        _ => 0x1FF,
    }
}

fn is_item(t: ThingRef) -> bool {
    (5..=10).contains(&(t.kind() as u8))
}

// ---------------------------------------------------------------------------
// Floor sensors (0x4CDCC)

/// Run the floor sensors for a mover leaving or entering a square. For
/// things this also unlinks (leaving) or links (entering) the thing, around
/// the content scan exactly as the original does. `party_present` tells
/// whether the party stands on the square besides the mover (for the party
/// itself: whether this is a re-placement on the same square).
pub fn floor_sensors(g: &mut GameState, map: usize, x: i32, y: i32, mover: Mover, party_present: bool, entering: bool) {
    let ty = mover.thing_type();
    let mover_thing = match mover {
        Mover::Party => ThingRef::NONE,
        Mover::Thing(t) => t,
    };
    let kind = item_number(g, mover_thing);
    if !entering && ty.is_some() {
        g.dungeon.remove_thing(map, x, y, mover_thing);
    }
    let wall_cell = (g.dungeon.square(map, x, y).element() == Element::Wall).then(|| mover_thing.cell());

    // Scan what else is here.
    let (mut items, mut creatures_here, mut matching, mut other) = (false, false, false, false);
    for t in g.dungeon.things_at(map, x, y) {
        match wall_cell {
            None => match t.kind() {
                ThingType::Creature if !creatures::is_airborne(g, t) => {
                    creatures_here = true;
                    // TODO(0x2FED6): a creature carrying item `kind` counts as matching.
                }
                ThingType::Text if ty.is_none() && entering && !party_present => {
                    let w1 = g.dungeon.record_word(t, 1).unwrap_or(0);
                    if w1 & 6 == 0 && w1 & 1 != 0 {
                        g.effects.push(Effect::ShowText { map, thing: t });
                    }
                }
                k if (5..14).contains(&(k as u8)) => {
                    items = true;
                    let n = item_number(g, t);
                    matching |= n == kind;
                    other |= n != kind;
                }
                _ => {}
            },
            Some(c) => {
                if t.cell() == c && (t.kind() as u8) > 4 {
                    items = true;
                    let n = item_number(g, t);
                    matching |= n == kind;
                    other |= n != kind;
                }
            }
        }
    }
    if entering && ty.is_some() {
        g.dungeon.add_thing(map, x, y, mover_thing);
    }

    for t in g.dungeon.things_at(map, x, y) {
        match t.kind() {
            ThingType::Actuator => {
                let a = Actuator::load(g, t);
                if a.kind() == 0 {
                    continue;
                }
                let state = match wall_cell {
                    None => floor_trigger(g, &a, ty, kind, entering, party_present, items, creatures_here, matching),
                    Some(c) if t.cell() == c => match a.kind() {
                        // TODO(0x1A): ornament alcoves comparing item kinds via attribute (9, ornament, 0x0E).
                        0x29 if !items => Some(entering),
                        0x2A if !matching && a.data() == kind => Some(entering),
                        0x2B if !other && a.data() != kind => Some(entering),
                        _ => None,
                    },
                    Some(_) => None,
                };
                if let Some(state) = state {
                    sensor_fire(g, map, x, y, &a, state);
                }
            }
            ThingType::Text if wall_cell.is_none() => {
                let w1 = g.dungeon.record_word(t, 1).unwrap_or(0);
                if w1 & 6 != 2 {
                    continue;
                }
                match w1 >> 11 {
                    9 if ty.is_none() && !party_present && hooks::champion_count(g) != 0 => {
                        // Random pulse: set next tick, clear 5 ticks later.
                        let chance = w1 >> 3 & 0xFF;
                        if g.rng.random(100) < chance && (w1 & 1 != 0) != entering {
                            let now = g.tick;
                            square_action(g, map, x, y, 0, SET, now + 1);
                            square_action(g, map, x, y, 0, CLEAR, now + 5);
                        }
                    }
                    // TODO(kind 10): load-based chance of being pushed back (event 0x5D).
                    _ => {}
                }
            }
            ThingType::Door | ThingType::Teleporter | ThingType::Text => {}
            _ => break,
        }
    }
}

/// Floor actuator trigger conditions (0x4CDCC types 1-8). Returns the
/// sensed state when the actuator should evaluate, else None.
#[allow(clippy::too_many_arguments)]
fn floor_trigger(
    g: &GameState,
    a: &Actuator,
    ty: Option<ThingType>,
    kind: u16,
    entering: bool,
    party_present: bool,
    items: bool,
    creatures_here: bool,
    matching: bool,
) -> Option<bool> {
    let party = ty.is_none();
    let fire_if = |c: bool| c.then_some(entering);
    match a.kind() {
        // Anything: only when nothing else that counts remains.
        1 => fire_if(!party_present && !items && !creatures_here),
        // Party or creature.
        2 if ty.is_none_or(|t| (t as u8) < 5) => fire_if(!party_present && !creatures_here),
        // Party, optionally only when facing direction data - 1.
        3 if party && hooks::champion_count(g) != 0 => {
            if a.data() == 0 {
                fire_if(!party_present)
            } else {
                Some(entering && a.data() == g.party.dir as u16 + 1)
            }
        }
        // A specific item kind.
        4 if a.data() == kind => fire_if(!matching),
        // Creatures only.
        7 if ty.is_some_and(|t| (t as u8) < 5) => fire_if(!creatures_here),
        // Party carrying item `data`.
        8 if party => Some(hooks::party_carries(g, a.data())),
        _ => None,
    }
}

/// Common tail of the sensors: inversion, follow mode, sound, fire.
fn sensor_fire(g: &mut GameState, map: usize, x: i32, y: i32, a: &Actuator, state: bool) {
    let state = state ^ a.inverted();
    let mut action = a.action();
    if action == FOLLOW {
        action = if state { SET } else { CLEAR };
    } else if !state {
        return;
    }
    if a.sound() {
        g.effects.push(Effect::Sound { cat: 9, idx: 0, sub: 0x88, map, x, y });
    }
    fire(g, map, a, action, 0);
}

// ---------------------------------------------------------------------------
// Wall sensors: clicking a wall or pushing an item into it (0x4C134)

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WallClick {
    /// At least one actuator fired.
    pub fired: bool,
    /// The item in hand should be consumed (type 3 with word 2 bit 2).
    pub consume_item: bool,
}

/// Click the wall at (map, x, y), side `cell`, holding `item` (None = empty
/// hand). Implements the button, hand-state, item-lock and toggle sensors
/// (types 1, 2, 3, 0x17); other wall sensor types are TODO.
pub fn click_wall(g: &mut GameState, map: usize, x: i32, y: i32, cell: u8, item: Option<ThingRef>) -> WallClick {
    let mut out = WallClick::default();
    let things = g.dungeon.things_at(map, x, y);
    for t in things {
        if t.cell() != cell || t.kind() != ThingType::Actuator {
            continue;
        }
        let a = Actuator::load(g, t);
        let empty = item.is_none();
        // `quiet` mirrors the original's flag: true means "don't fire".
        let (quiet, follow_value) = match a.kind() {
            1 => {
                if a.action() == FOLLOW {
                    continue;
                }
                (false, false)
            }
            2 => {
                let q = empty != a.inverted();
                (q, q)
            }
            3 => {
                let matched = item.is_some_and(|i| item_number(g, i) == a.data());
                if matched && a.enabled() {
                    out.consume_item = true;
                }
                let q = matched == a.inverted();
                (q, q)
            }
            0x17 if empty => {
                let w2 = a.w2 ^ 4;
                set_w(g, t, 2, w2);
                let q = (w2 & 0x20 != 0) == (w2 & 4 != 0);
                (q, q)
            }
            _ => continue,
        };
        let mut action = a.action();
        let mut quiet = quiet;
        if action == FOLLOW {
            action = follow_value as u8;
            quiet = false;
        }
        if !quiet {
            if a.sound() {
                g.effects.push(Effect::Sound { cat: 9, idx: 0, sub: 0x88, map, x, y });
            }
            fire(g, map, &a, action, 0);
            out.fired = true;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Square-action handlers for floors and walls

/// Floor-type square action (0x57476): text visibility and floor actuators.
pub fn floor_handler(g: &mut GameState, ev: Event) {
    let (map, x, y) = (ev.map as usize, ev.x as i32, ev.y as i32);
    for t in g.dungeon.things_at(map, x, y) {
        match t.kind() {
            ThingType::Text => floor_text(g, ev, t),
            ThingType::Actuator => floor_actuator(g, ev, t),
            ThingType::Door | ThingType::Teleporter => {}
            _ => break,
        }
    }
}

fn floor_text(g: &mut GameState, ev: Event, t: ThingRef) {
    let (map, x, y) = (ev.map as usize, ev.x as i32, ev.y as i32);
    let w1 = g.dungeon.record_word(t, 1).unwrap_or(0);
    let mode = w1 & 6;
    if mode != 0 && mode != 2 {
        return;
    }
    match w1 >> 11 {
        5 => {
            let was = w1 & 1 != 0;
            let now = resolve_action(ev.b9, was);
            set_w(g, t, 1, w1 & !1 | now as u16);
            let party_here = g.party.map == map && g.party.x == x && g.party.y == y;
            if mode == 0 && !was && now && party_here {
                g.effects.push(Effect::ShowText { map, thing: t });
            }
        }
        0x13 | 0x16 if ev.b9 == SET => {
            // TODO: the original adds a random phase to the delay; one generator
            // call is made here to keep the sequence aligned.
            let r = g.rng.rnd();
            let mut e = Event::new(0x5E, map as u8, g.tick.wrapping_add(1 + (r & 3)));
            e.x = ev.x;
            e.y = ev.y;
            e.b9 = (w1 >> 3) as u8;
            g.schedule(e);
        }
        0x17 if ev.b9 == SET => {
            g.effects.push(Effect::Sound { cat: 3, idx: map as u8 + 1, sub: (w1 >> 3) as u8, map, x, y });
        }
        _ => {}
    }
}

fn floor_actuator(g: &mut GameState, ev: Event, t: ThingRef) {
    let map = ev.map as usize;
    let a = Actuator::load(g, t);
    match a.kind() {
        0x0B | 0x28 => creatures::floor_trap(g, map, ev.x as i32, ev.y as i32, t, ev.b9),
        0x20 => timer(g, ev, &a, false),
        0x45 => timer(g, ev, &a, true),
        0x27 => {
            let v = resolve_action(ev.b9, a.w2 & 1 != 0);
            set_w(g, t, 2, a.w2 & !1 | v as u16);
        }
        0x2E => {
            // Move or rotate the party (through 0x4BED2).
            let (tx, ty) = if a.enabled() {
                let (tx, ty, _) = a.target();
                (tx, ty)
            } else {
                (ev.x as i32, ev.y as i32)
            };
            let mut dir = (a.w2 >> 3 & 3) as u8;
            if !a.inverted() {
                dir = (dir + g.party.dir) & 3;
            }
            movement::teleport_party(g, tx, ty, map, dir);
        }
        0x3A => creatures::floor_signal(g, map, ev.x as i32, ev.y as i32, ev.b9 == SET),
        0x3B | 0x40 | 0x47 | 0x48 | 0x49 => item_relay(g, ev, &a),
        0x3D => relay(g, ev, &a, a.data() as u32),
        0x2C => animated_ornament(g, ev, &a, false),
        0x32 => one_shot_ornament(g, ev, &a, false),
        // TODO: 0x42-0x44.
        _ => {}
    }
}

/// Wall-type square action (0x58304): things on the event's cell.
pub fn wall_handler(g: &mut GameState, ev: Event) {
    let map = ev.map as usize;
    for t in g.dungeon.things_at(map, ev.x as i32, ev.y as i32) {
        if (t.kind() as u8) > 3 {
            break;
        }
        if t.cell() != ev.b8 {
            continue;
        }
        match t.kind() {
            ThingType::Text => wall_text(g, ev, t),
            ThingType::Actuator => wall_actuator(g, ev, t),
            _ => {}
        }
    }
}

fn wall_text(g: &mut GameState, ev: Event, t: ThingRef) {
    let (map, x, y) = (ev.map as usize, ev.x as i32, ev.y as i32);
    let w1 = g.dungeon.record_word(t, 1).unwrap_or(0);
    if w1 & 6 != 0 && w1 & 6 != 2 {
        return;
    }
    match w1 >> 11 {
        k @ (5 | 7) => {
            let v = resolve_action(ev.b9, w1 & 1 != 0);
            set_w(g, t, 1, w1 & !1 | v as u16);
            if k == 7 {
                // Mirror onto the matching text on the linked layer.
                let delta = if g.attrs.get(9, (w1 >> 3) as u8, 0x11) == 0 { 1 } else { -1 };
                if let Some((m2, x2, y2)) = crate::world::layer_map(&g.dungeon, g.party.map, delta, x, y) {
                    for u in g.dungeon.things_at(m2, x2, y2) {
                        if u.kind() == ThingType::Text && u.cell() == ev.b8 {
                            let uw = g.dungeon.record_word(u, 1).unwrap_or(0);
                            if uw & 6 == 2 && uw >> 11 == 7 {
                                set_w(g, u, 1, uw & !1 | v as u16);
                            }
                        }
                    }
                }
            }
        }
        0x17 if ev.b9 == SET => {
            g.effects.push(Effect::Sound { cat: 3, idx: map as u8 + 1, sub: (w1 >> 3) as u8, map, x, y });
        }
        _ => {}
    }
}

pub(crate) fn wall_actuator(g: &mut GameState, ev: Event, t: ThingRef) {
    let (map, x, y) = (ev.map as usize, ev.x as i32, ev.y as i32);
    let a = Actuator::load(g, t);
    let data = a.data();
    match a.kind() {
        // Shooters: missiles belong to the combat code.
        0x07 | 0x08 | 0x09 | 0x0A | 0x0E | 0x0F => {
            let (tx, ty, cell) = a.target();
            let _ = (tx, ty);
            g.effects.push(Effect::Shoot { map, x, y, cell, dir: ev.b8, actuator: t });
        }
        0x12 => g.effects.push(Effect::EndGame),
        0x16 => {
            // Cross-map relay: same action, tick and priority on map `data`.
            let m2 = (data & 0x3F) as usize;
            if m2 >= g.dungeon.maps.len() {
                return;
            }
            let (tx, ty, _) = a.target();
            let cell = if g.dungeon.square(m2, tx, ty).element() == Element::Wall { (data >> 6 & 3) as u8 } else { 0 };
            let mut e = Event::new(4, m2 as u8, g.tick);
            e.prio = ev.prio;
            e.x = tx as u8;
            e.y = ty as u8;
            e.b8 = cell;
            e.b9 = ev.b9;
            g.schedule(e);
        }
        0x1D => counter(g, ev, &a),
        0x1E | 0x33..=0x37 => {
            let on = resolve_action(ev.b9, a.enabled());
            let w2 = a.w2 & !4 | (on as u16) << 2;
            set_w(g, t, 2, w2);
            if w2 & 1 == 0 && on {
                clock_start(g, map, t);
            }
        }
        0x20 => timer(g, ev, &a, false),
        0x45 => timer(g, ev, &a, true),
        0x26 => {
            let on = resolve_action(ev.b9, a.enabled());
            set_w(g, t, 2, a.w2 & !4 | (on as u16) << 2);
        }
        0x2D => gate(g, ev, &a),
        0x2E => {
            if ev.b9 == SET {
                let (tx, ty, _) = a.target();
                let dir = if a.enabled() { g.rng.rand4() as u8 } else { a.action() };
                let _ = creatures::spawn(g, data, map, tx, ty, dir);
                // TODO: word 2 bit 5 stores a value in the new group's word +8.
                if a.sound() {
                    g.effects.push(Effect::Sound { cat: 0, idx: 0, sub: 0x89, map, x: tx, y: ty });
                }
            }
        }
        0x31 => {
            if !a.busy() {
                let mut e = Event::new(0x5B, map as u8, g.tick.wrapping_add(data as u32));
                [e.x, e.y] = t.0.to_le_bytes();
                g.schedule(e);
                set_w(g, t, 2, a.w2 | 1);
                if a.triggered_by(ev.b9) {
                    let action = if a.enabled() { a.action() } else { ev.b9 };
                    fire(g, map, &a, action, 0);
                }
            }
        }
        0x3B | 0x40 | 0x47 | 0x48 | 0x49 => item_relay(g, ev, &a),
        0x3C => {
            if a.triggered_by(ev.b9) {
                generate_item(g, map, &a);
            }
        }
        0x3D => relay(g, ev, &a, data as u32),
        0x46 => {
            // Set/clear/toggle bit 13 of the target square's door or teleporter.
            let (tx, ty, _) = a.target();
            if let Some(u) = g
                .dungeon
                .things_at(map, tx, ty)
                .into_iter()
                .find(|u| matches!(u.kind(), ThingType::Door | ThingType::Teleporter))
            {
                let w = g.dungeon.record_word(u, 1).unwrap_or(0);
                let v = resolve_action(ev.b9, w & 0x2000 != 0);
                set_w(g, u, 1, w & !0x2000 | (v as u16) << 13);
            }
        }
        0x2C => animated_ornament(g, ev, &a, true),
        0x32 => one_shot_ornament(g, ev, &a, true),
        // TODO: 0x41 (randomise from an ornament attribute), 0x42-0x44.
        _ => {}
    }
}

/// The ornament an actuator shows: word 2 bits 12-15 index the map's wall
/// (or floor) ornament list, 0 meaning none (0x1FC2C / 0x1FC82).
fn ornament_of(g: &GameState, map: usize, a: &Actuator, wall: bool) -> Option<(u8, u8)> {
    let slot = (a.w2 >> 12) as usize;
    if slot == 0 {
        return None;
    }
    let lists = g.dungeon.map_lists(map);
    let list = if wall { &lists.wall_ornaments } else { &lists.floor_ornaments };
    list.get(slot - 1).map(|&o| (if wall { 9 } else { 10 }, o))
}

/// An ornament's animation cycle length (0x56CF4): its number attribute
/// 0x0D, else the length of its frame-digit text (type 5, sub 0x0D), else 1.
fn ornament_cycle(g: &GameState, map: usize, a: &Actuator, wall: bool) -> u32 {
    let Some((cat, orn)) = ornament_of(g, map, a, wall) else { return 1 };
    let n = g.attrs.get(cat, orn, 0x0D) & 0x7FFF;
    if n != 0 {
        return n as u32;
    }
    let Some(data) = g.data.as_ref() else { return 1 };
    crate::font::text(&data.gdat, cat, orn, 0x0D, &Default::default()).map_or(1, |t| (t.len() as u32).max(1))
}

/// Actuator 0x2C (0x56F11): an animated ornament switched on and off.
/// Word 2 bit 2 holds the switch, bit 0 "animating", and word 1 bits 7-14
/// the animation phase. Switching on starts the animation aligned to the
/// tick; switching off lets the current cycle finish (event 0x59 clears
/// bit 0 at the end of the cycle unless it was switched on again). An
/// inverted actuator whose action is "follow" also passes the event on.
fn animated_ornament(g: &mut GameState, ev: Event, a: &Actuator, wall: bool) {
    let map = ev.map as usize;
    let old = a.w2 & 4 != 0;
    let new = resolve_action(ev.b9, old);
    let mut w2 = a.w2 & !4 | u16::from(new) << 2;
    let mut w1 = a.w1;
    if new != old {
        let n = ornament_cycle(g, map, a, wall).max(1);
        if !new {
            let phase = ((w1 >> 7 & 0xFF) as u32 + g.tick) % n;
            if phase == 0 {
                w2 &= !1;
            } else {
                let mut e = Event::new(0x59, map as u8, g.tick.wrapping_add(n - phase));
                e.set_w8(a.thing.0);
                g.schedule(e);
            }
        } else if w2 & 1 == 0 {
            w2 |= 1;
            let start = ((n - g.tick % n) % n) as u16;
            w1 = (w1 & 0x807F) | (start & 0xFF) << 7;
            // The original also schedules a repeating ornament sound
            // (event 0x5A, attribute 0x88) when the sound bit is set; the
            // first one plays now.
            if a.sound() {
                if let Some((cat, idx)) = ornament_of(g, map, a, wall) {
                    g.effects.push(Effect::Sound { cat, idx, sub: 0x88, map, x: ev.x as i32, y: ev.y as i32 });
                }
            }
        }
    }
    set_w(g, a.thing, 1, w1);
    set_w(g, a.thing, 2, w2);
    if a.inverted() && a.action() == FOLLOW {
        fire(g, map, a, ev.b9, 0);
    }
}

/// Actuator 0x32 (0x570B1): play the ornament's animation once. If it is
/// not already playing (word 2 bit 0), mark it busy, reset the frame counter
/// (word 1 bits 7-15) and start event 0x55 next tick; with the sound bit,
/// play the ornament's sound 0x88. When word 2 bit 2 is set it also relays
/// the event like 0x3D (0x571F3).
fn one_shot_ornament(g: &mut GameState, ev: Event, a: &Actuator, wall: bool) {
    let map = ev.map as usize;
    if a.w2 & 1 == 0 {
        set_w(g, a.thing, 2, a.w2 | 1);
        set_w(g, a.thing, 1, a.w1 & 0x7F);
        let mut e = Event::new(EVENT_ORNAMENT_STEP, map as u8, g.tick.wrapping_add(1));
        e.x = ev.x;
        e.y = ev.y;
        e.set_w8(a.thing.0);
        e.w10 = u16::from(wall);
        g.schedule(e);
        if a.sound() {
            if let Some((cat, idx)) = ornament_of(g, map, a, wall) {
                g.effects.push(Effect::Sound { cat, idx, sub: 0x88, map, x: ev.x as i32, y: ev.y as i32 });
            }
        }
    }
    if a.w2 & 4 != 0 {
        let a = Actuator::load(g, a.thing);
        relay(g, ev, &a, 0);
    }
}

/// Event type that steps a one-shot ornament animation.
pub const EVENT_ORNAMENT_STEP: u8 = 0x55;

/// Event 0x55 (0x59293): advance a one-shot ornament's frame counter; at the
/// end of a cycle clear its busy bit, otherwise come back next tick.
pub fn ornament_step(g: &mut GameState, ev: Event) {
    let t = ThingRef(ev.w8());
    if !t.is_thing() {
        return;
    }
    let a = Actuator::load(g, t);
    let n = ornament_cycle(g, ev.map as usize, &a, ev.w10 != 0).max(1);
    let count = ((a.w1 >> 7) + 1) & 0x1FF;
    set_w(g, t, 1, a.w1 & 0x7F | count << 7);
    if count as u32 % n == 0 {
        set_w(g, t, 2, a.w2 & !1);
    } else {
        g.schedule(Event { tick: g.tick.wrapping_add(1), ..ev });
    }
}

/// Timers 0x20 and 0x45 (0x572A8).
fn timer(g: &mut GameState, ev: Event, a: &Actuator, long: bool) {
    let go = !a.enabled() || a.triggered_by(ev.b9);
    if !go {
        return;
    }
    let delay = if long { (a.data() as u32) << a.delay() } else { a.delay() + a.data() as u32 };
    let action = if a.enabled() { a.action() } else { ev.b9 };
    let (tx, ty, cell) = a.target();
    let tick = g.tick.wrapping_add(delay);
    square_action(g, ev.map as usize, tx, ty, cell, action, tick);
}

/// Relay 0x3D (0x571F3) with extra delay `extra`.
fn relay(g: &mut GameState, ev: Event, a: &Actuator, extra: u32) {
    let map = ev.map as usize;
    if a.action() == FOLLOW {
        if !a.inverted() {
            fire(g, map, a, ev.b9, extra);
        } else {
            // A pulse: forward now, then undo after the extra delay.
            fire(g, map, a, ev.b9, 0);
            if extra != 0 {
                fire(g, map, a, inverse(ev.b9), extra);
            }
        }
    } else if a.triggered_by(ev.b9) {
        fire(g, map, a, a.action(), extra);
    }
}

/// Up/down counter 0x1D: a clear counts up, a set counts down.
fn counter(g: &mut GameState, ev: Event, a: &Actuator) {
    let is_zero = |d: u16| d == 0 || d & 0x100 != 0;
    let mut d = a.data();
    let was = is_zero(d);
    if ev.b9 == CLEAR {
        d = (d + 1) & 0x1FF;
    } else if ev.b9 == SET && (!a.enabled() || d != 0) {
        d = d.wrapping_sub(1) & 0x1FF;
    }
    set_w(g, a.thing, 1, a.w1 & 0x7F | d << 7);
    let now = is_zero(d);
    if now == was {
        return;
    }
    let map = ev.map as usize;
    if a.action() == FOLLOW {
        fire(g, map, a, (now == a.inverted()) as u8, 0);
    } else if now {
        fire(g, map, a, a.action(), 0);
    }
}

/// Gate 0x2D: a countdown (data 1-400) or a percentage chance (401-499).
fn gate(g: &mut GameState, ev: Event, a: &Actuator) {
    let map = ev.map as usize;
    let d = a.data();
    if (1..=400).contains(&d) {
        set_w(g, a.thing, 1, a.w1 & 0x7F | (d - 1) << 7);
        fire(g, map, a, ev.b9, 0);
    } else if (401..500).contains(&d) {
        let r = g.rng.random(100);
        let fail = d - 400 <= r;
        if a.action() == FOLLOW {
            fire(g, map, a, fail as u8, 0);
        } else if !fail {
            fire(g, map, a, ev.b9, 0);
        }
    }
}

/// Item relays 0x3B, 0x40, 0x47, 0x48, 0x49 (0x57E6C): move matching items
/// between the event's square and the actuator's target.
fn item_relay(g: &mut GameState, ev: Event, a: &Actuator) {
    if !a.triggered_by(ev.b9) {
        return;
    }
    let map = ev.map as usize;
    let reverse = matches!(a.kind(), 0x47 | 0x49);
    let first_only = matches!(a.kind(), 0x48 | 0x49);
    if a.kind() == 0x40 {
        return; // TODO: match against the kind list loaded from GRAPHICS.DAT.
    }
    let (tx, ty, tcell) = a.target();
    let ((sx, sy, scell), (dx, dy, dcell)) = if reverse {
        ((tx, ty, tcell), (ev.x as i32, ev.y as i32, ev.b8))
    } else {
        ((ev.x as i32, ev.y as i32, ev.b8), (tx, ty, tcell))
    };
    let any_cell = g.dungeon.square(map, sx, sy).element() != Element::Wall;
    let candidates: Vec<ThingRef> = g
        .dungeon
        .things_at(map, sx, sy)
        .into_iter()
        .filter(|&t| is_item(t) && (any_cell || t.cell() == scell))
        .collect();
    // TODO: also search the possessions of creatures on the square.
    for t in candidates {
        let n = item_number(g, t);
        if n != a.data() && a.data() != 0x1FF {
            continue;
        }
        let moved = ThingRef(t.0 & 0x3FFF | (dcell as u16) << 14);
        g.dungeon.remove_thing(map, sx, sy, t);
        let dest_wall = g.dungeon.square(map, dx, dy).element() == Element::Wall;
        if dest_wall {
            g.dungeon.add_thing(map, dx, dy, moved);
        } else {
            movement::move_thing(g, moved, None, Some((map, dx, dy)));
        }
        if first_only {
            break;
        }
    }
}

/// Create a free-standing item from its item number (0x1F180 ranges);
/// the caller places it.
pub fn create_item(g: &mut GameState, n: u16) -> Option<ThingRef> {
    let (ty, idx) = match n {
        0..=127 => (ThingType::Weapon, n),
        128..=255 => (ThingType::Clothing, n - 128),
        256..=383 => (ThingType::Misc, n - 256),
        384..=431 => (ThingType::Potion, n - 384),
        480..=507 => (ThingType::Container, n - 480),
        // TODO: creatures (432-479) and scrolls (508).
        _ => return None,
    };
    if ty == ThingType::Container {
        // Empty content list in word 1; the type index is split over word
        // 2 bits 13-15 (low three bits) and bits 1-2 (next two).
        let t = alloc_thing(g, ty)?;
        g.dungeon.set_record_word(t, 1, ThingRef::END.0);
        g.dungeon.set_record_word(t, 2, (idx & 7) << 13 | ((idx >> 3) & 3) << 1);
        return Some(t);
    }
    let t = alloc_thing(g, ty)?;
    // Bit 7 (weapons, clothing, misc) and bit 15 (potions) are set on every
    // item in the original file. TODO(0x1F07C): initial charges.
    let w1 = match ty {
        ThingType::Potion => 0x8000 | idx << 8,
        _ => 0x80 | idx,
    };
    g.dungeon.set_record_word(t, 1, w1);
    Some(t)
}

/// Item generator 0x3C: create item number `data` at the target (0x1F180,
/// 0x1F1FC for the number; placement as 0x57D4C).
fn generate_item(g: &mut GameState, map: usize, a: &Actuator) {
    let Some(t) = create_item(g, a.data()) else { return };
    let (tx, ty_, cell) = a.target();
    let placed = ThingRef(t.0 & 0x3FFF | (cell as u16) << 14);
    if g.dungeon.square(map, tx, ty_).element() == Element::Wall {
        g.dungeon.add_thing(map, tx, ty_, placed);
    } else {
        movement::move_thing(g, placed, None, Some((map, tx, ty_)));
    }
}

/// Take a free record of `ty` (next word 0xFFFF) or append one.
pub fn alloc_thing(g: &mut GameState, ty: ThingType) -> Option<ThingRef> {
    let size = ty.record_size();
    if size == 0 {
        return None;
    }
    let recs = &mut g.dungeon.things[ty as usize];
    let count = recs.len() / size;
    let idx = (0..count)
        .find(|&i| u16::from_le_bytes([recs[i * size], recs[i * size + 1]]) == 0xFFFF)
        .unwrap_or_else(|| {
            recs.resize(recs.len() + size, 0);
            count
        });
    if idx >= 1024 {
        return None;
    }
    let r = &mut recs[idx * size..idx * size + size];
    r.fill(0);
    r[0..2].copy_from_slice(&ThingRef::END.0.to_le_bytes());
    Some(ThingRef((ty as u16) << 10 | idx as u16))
}

// ---------------------------------------------------------------------------
// Clocks (0x592FA / event 0x56) and re-arm events

/// Start a clock actuator (types 0x1E, 0x33-0x37): first tick at
/// `tick + tick % period`, period = data × {1, 8, 16, 32, 64, 128}.
pub fn clock_start(g: &mut GameState, map: usize, t: ThingRef) {
    let a = Actuator::load(g, t);
    let mult: u32 = match a.kind() {
        0x1E => 1,
        0x33 => 8,
        0x34 => 16,
        0x35 => 32,
        0x36 => 64,
        0x37 => 128,
        _ => return,
    };
    let data = a.data() as u32;
    if data == 0 {
        return;
    }
    let period = data * mult;
    let mut e = Event::new(0x56, map as u8, g.tick % period + g.tick);
    [e.x, e.y] = t.0.to_le_bytes();
    e.b8 = mult as u8;
    g.schedule(e);
    set_w(g, t, 2, a.w2 | 1);
}

/// Event 0x56: a clock period elapsed (0x593CF).
pub fn clock_tick(g: &mut GameState, mut ev: Event) {
    let t = ThingRef(u16::from_le_bytes([ev.x, ev.y]));
    let a = Actuator::load(g, t);
    let map = ev.map as usize;
    let cont = if a.action() == FOLLOW {
        ev.b9 ^= 1;
        let phase = ev.b9 & 1 != 0;
        fire(g, map, &a, if phase { SET } else { CLEAR }, 0);
        phase || a.enabled()
    } else if a.enabled() {
        fire(g, map, &a, a.action(), 0);
        true
    } else {
        false
    };
    if cont {
        ev.tick = ev.tick.wrapping_add(ev.b8 as u32 * a.data() as u32);
        g.schedule(ev);
    } else {
        let a = Actuator::load(g, t);
        set_w(g, t, 2, a.w2 & !1);
    }
}

/// Events 0x57 and 0x5B: re-arm an actuator (clear word 2 bit 0).
pub fn rearm(g: &mut GameState, ev: Event) {
    let t = ThingRef(u16::from_le_bytes([ev.x, ev.y]));
    if t.is_thing() {
        let w2 = g.dungeon.record_word(t, 2).unwrap_or(0);
        set_w(g, t, 2, w2 & !1);
    }
}

/// Event 0x5C: set bit 0 of word 1 of the thing in bytes 6-7.
pub fn set_visible(g: &mut GameState, ev: Event) {
    let t = ThingRef(u16::from_le_bytes([ev.x, ev.y]));
    if t.is_thing() {
        let w1 = g.dungeon.record_word(t, 1).unwrap_or(0);
        set_w(g, t, 1, w1 | 1);
    }
}

/// Event 0x59: if the thing in bytes 8-9 has word 2 bit 2 clear, clear its
/// busy bit.
pub fn release(g: &mut GameState, ev: Event) {
    let t = ThingRef(ev.w8());
    if t.is_thing() {
        let w2 = g.dungeon.record_word(t, 2).unwrap_or(0);
        if w2 & 4 == 0 {
            set_w(g, t, 2, w2 & !1);
        }
    }
}
