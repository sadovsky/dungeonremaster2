//! Creatures and their AI (docs/08-creatures-ai.md).
//!
//! Tables come from the user's SKULL.EXE and GRAPHICS.DAT at runtime
//! (`data`). Active creatures get a slot (`slot`); each runs off two timeline
//! events: 0x22 starts a new action (running *think* when none is queued),
//! 0x21 continues the current animation sequence. Frame events (attacks,
//! moves, turns, death) run from `ai::frame_event`.

pub mod ai;
pub mod anim;
pub mod data;
pub mod fight;
pub mod merchant;
pub mod planner;
pub mod slot;
pub mod terrain;

use std::rc::Rc;

use dm2_formats::dungeon::{ThingRef, ThingType};

use crate::effects::Effect;
use crate::state::GameState;
use crate::timeline::Event;

use anim::{Step, NO_FRAME};
use data::{CreatureData, Info};
use slot::{Slot, POOL_SIZE};

/// Timeline event types.
pub const EV_CONTINUE: u8 = 0x21;
pub const EV_STEP: u8 = 0x22;
/// Marks `Event::w10` as holding a slot index.
const SLOT_TAG: u16 = 0x8000;

/// Action played when a creature dies (0x31348 queues it).
pub const ACTION_DIE: u8 = 0x13;

// ---------------------------------------------------------------------------
// Record access (DUNGEON.DAT type 4, 16 bytes)

pub fn rec_u8(g: &GameState, c: ThingRef, o: usize) -> u8 {
    g.dungeon.record(c).and_then(|r| r.get(o).copied()).unwrap_or(0)
}

pub fn rec_u16(g: &GameState, c: ThingRef, o: usize) -> u16 {
    g.dungeon.record(c).and_then(|r| r.get(o..o + 2)).map(|b| u16::from_le_bytes([b[0], b[1]])).unwrap_or(0)
}

pub fn set_rec_u8(g: &mut GameState, c: ThingRef, o: usize, v: u8) {
    if let Some(b) = g.dungeon.record_mut(c).and_then(|r| r.get_mut(o)) {
        *b = v;
    }
}

pub fn set_rec_u16(g: &mut GameState, c: ThingRef, o: usize, v: u16) {
    if let Some(b) = g.dungeon.record_mut(c).and_then(|r| r.get_mut(o..o + 2)) {
        b.copy_from_slice(&v.to_le_bytes());
    }
}

pub fn creature_type(g: &GameState, c: ThingRef) -> u8 {
    rec_u8(g, c, 4)
}

pub fn hp(g: &GameState, c: ThingRef) -> u16 {
    rec_u16(g, c, 6)
}

pub fn status(g: &GameState, c: ThingRef) -> u16 {
    rec_u16(g, c, 0x0A)
}

pub fn facing(g: &GameState, c: ThingRef) -> u8 {
    (rec_u16(g, c, 0x0E) >> 8 & 3) as u8
}

pub fn set_facing(g: &mut GameState, c: ThingRef, dir: u8) {
    let w = rec_u16(g, c, 0x0E);
    set_rec_u16(g, c, 0x0E, w & !0x0300 | ((dir as u16 & 3) << 8));
}

/// Info record and AI class for a creature type (attributes 5 and 1).
pub fn type_info(g: &GameState, d: &CreatureData, ty: u8) -> Option<(Info, u16)> {
    let info = d.info(g.attrs.get(15, ty, 5))?;
    Some((info, g.attrs.get(15, ty, 1)))
}

// ---------------------------------------------------------------------------
// Per-step context (the globals 0x7F548-0x7F57E load)

#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub si: usize,
    pub thing: ThingRef,
    pub ty: u8,
    pub info: Info,
    pub class: u16,
    pub cflags: u32,
    pub map: usize,
    pub x: i32,
    pub y: i32,
}

impl Ctx {
    pub fn load(g: &GameState, d: &CreatureData, si: usize) -> Option<Ctx> {
        let s = g.creature_slots.get(si)?.as_ref()?;
        let ty = creature_type(g, s.thing);
        let (info, class) = type_info(g, d, ty)?;
        Some(Ctx {
            si,
            thing: s.thing,
            ty,
            info,
            class,
            cflags: d.class_flags(class),
            map: s.pos.map(),
            x: s.pos.x(),
            y: s.pos.y(),
        })
    }

    pub fn slot<'a>(&self, g: &'a GameState) -> &'a Slot {
        g.creature_slots[self.si].as_ref().expect("active slot")
    }

    pub fn slot_mut<'a>(&self, g: &'a mut GameState) -> &'a mut Slot {
        g.creature_slots[self.si].as_mut().expect("active slot")
    }
}

// ---------------------------------------------------------------------------
// Hooks used by the dungeon mechanics

/// The creature group on a square, if any (0x2FBA9).
pub fn group_at(g: &GameState, map: usize, x: i32, y: i32) -> Option<ThingRef> {
    g.dungeon.things_at(map, x, y).into_iter().find(|t| t.kind() == ThingType::Creature)
}

fn info_of(g: &GameState, c: ThingRef) -> Option<Info> {
    let d = g.creature_data.clone()?;
    type_info(g, &d, creature_type(g, c)).map(|(i, _)| i)
}

/// Non-material creatures (info flag 0x20) ignore doors and don't block
/// trick walls.
pub fn is_non_material(g: &GameState, c: ThingRef) -> bool {
    info_of(g, c).is_some_and(|i| i.raw[0] & 0x20 != 0)
}

/// Size class used by closing doors (info bits 6-7; at least 1).
pub fn door_size(g: &GameState, c: ThingRef) -> u16 {
    info_of(g, c).map(|i| i.door_size().max(1)).unwrap_or(1)
}

/// Creatures whose info flag +0x19 bit 0x10 halves door damage.
pub fn halves_door_damage(g: &GameState, c: ThingRef) -> bool {
    info_of(g, c).is_some_and(|i| i.flags19() & 0x10 != 0)
}

/// Damage a creature group (0x24E62): owed damage is applied on its next step.
pub fn damage(g: &mut GameState, c: ThingRef, map: usize, x: i32, y: i32, amount: u16) {
    g.effects.push(Effect::CreatureDamaged { thing: c, amount });
    if amount == 0 {
        return;
    }
    let Some(d) = g.creature_data.clone() else { return };
    let si = match slot_of(g, c) {
        Some(si) => si,
        None => match activate(g, &d, c, map, x, y) {
            Some(si) => si,
            None => return,
        },
    };
    let Some(ctx) = Ctx::load(g, &d, si) else { return };
    let mut st = status(g, c);
    let hpv = hp(g, c);
    let mut pending = ctx.slot(g).pending_damage;
    let r = fight::take_hit(&ctx.info, ctx.cflags, &mut st, &mut pending, hpv, amount, &mut g.rng);
    set_rec_u16(g, c, 0x0A, st);
    ctx.slot_mut(g).pending_damage = pending;
    if r.turned_to_party && g.party.map == ctx.map {
        let dir = ai::direction_toward(ctx.x, ctx.y, g.party.x, g.party.y);
        ctx.slot_mut(g).turn_to = dir;
    }
    if r.interrupt {
        // Interrupt the current action: step again at once.
        reschedule(g, si, EV_STEP, 0);
    }
}

/// Teleporter scope class: 2 if the creature has attribute 0x1E, else 1.
pub fn teleport_class(g: &GameState, c: ThingRef) -> u8 {
    if g.attrs.get(15, creature_type(g, c), 0x1E) != 0 {
        2
    } else {
        1
    }
}

/// Things that don't fall into pits (0x49FCB): flying creatures (terrain
/// mask allows open pits), missiles, clouds.
pub fn is_airborne(g: &GameState, t: ThingRef) -> bool {
    match t.kind() {
        ThingType::Missile | ThingType::Cloud => true,
        ThingType::Creature => info_of(g, t).is_some_and(|i| i.terrain() & terrain::PIT_OPEN & !terrain::PIT_CLOSED != 0),
        _ => false,
    }
}

/// Create a creature of `kind` facing `dir` (0x30BA6). Returns the new group.
pub fn spawn(g: &mut GameState, kind: u16, map: usize, x: i32, y: i32, dir: u8) -> Option<ThingRef> {
    if group_at(g, map, x, y).is_some() {
        return None;
    }
    let c = crate::actuators::alloc_thing(g, ThingType::Creature)?;
    let ty = kind as u8;
    let base = g.creature_data.clone().and_then(|d| type_info(g, &d, ty)).map(|(i, _)| i.base_hp()).unwrap_or(1);
    let hpv = base + g.rng.random(base / 8 + 1);
    set_rec_u16(g, c, 2, ThingRef::END.0);
    set_rec_u8(g, c, 4, ty);
    set_rec_u8(g, c, 5, 0xFF);
    set_rec_u16(g, c, 6, hpv.max(1));
    set_rec_u16(g, c, 8, 0);
    set_rec_u16(g, c, 0x0A, 0);
    set_rec_u16(g, c, 0x0C, 0);
    set_rec_u16(g, c, 0x0E, (dir as u16 & 3) << 8);
    g.dungeon.add_thing(map, x, y, c);
    if map == g.party.map {
        if let Some(d) = g.creature_data.clone() {
            activate(g, &d, c, map, x, y);
        }
    }
    Some(c)
}

/// Floor actuator types 0x0B/0x28 (0x56BA5): creature-affecting trap.
/// Wakes and damages the group on the event square. Tentative.
pub fn floor_trap(g: &mut GameState, map: usize, ev_x: i32, ev_y: i32, _actuator: ThingRef, _action: u8) {
    if let Some(c) = group_at(g, map, ev_x, ev_y) {
        if slot_of(g, c).is_none() {
            if let Some(d) = g.creature_data.clone() {
                activate(g, &d, c, map, ev_x, ev_y);
            }
        }
    }
}

/// Floor actuator type 0x3A (0x2538C): a signal to the group on the square.
/// Sets or clears status bit 0x10 (tentative).
pub fn floor_signal(g: &mut GameState, map: usize, x: i32, y: i32, set: bool) {
    if let Some(c) = group_at(g, map, x, y) {
        let s = status(g, c);
        set_rec_u16(g, c, 0x0A, if set { s | 0x10 } else { s & !0x10 });
    }
}

/// Event 0x5E (0x30BA6 path from text kinds 0x13/0x16): spawn the creature
/// type in the event parameter.
pub fn text_spawn_event(g: &mut GameState, map: usize, x: i32, y: i32, param: u8) {
    let dir = g.rng.rand4() as u8;
    let _ = spawn(g, param as u16, map, x, y, dir);
}

// ---------------------------------------------------------------------------
// Slots and activation

pub fn slot_of(g: &GameState, c: ThingRef) -> Option<usize> {
    let s = rec_u8(g, c, 5);
    (s != 0xFF).then_some(s as usize).filter(|&s| {
        g.creature_slots.get(s).and_then(|x| x.as_ref()).is_some_and(|x| x.thing.0 & 0x3FF == c.0 & 0x3FF)
    })
}

/// Give a creature group an active slot and start it (0x306A8).
pub fn activate(g: &mut GameState, d: &CreatureData, c: ThingRef, map: usize, x: i32, y: i32) -> Option<usize> {
    if let Some(si) = slot_of(g, c) {
        return Some(si);
    }
    if g.creature_slots.len() < POOL_SIZE {
        g.creature_slots.resize(POOL_SIZE, None);
    }
    let si = g.creature_slots.iter().position(|s| s.is_none())?;
    let ty = creature_type(g, c);
    let (info, _) = type_info(g, d, ty)?;
    let mut s = Slot::new(ThingRef(c.0 & 0x3FFF), map, x, y, g.tick);
    // A first action of 0x11 instead of 0 when record +8 is 0xFFFF.
    s.action = if rec_u16(g, c, 8) == 0xFFFF { 0x11 } else { 0 };
    g.creature_slots[si] = Some(s);
    set_rec_u8(g, c, 5, si as u8);
    if !info.inanimate() {
        // Status: set bit 15 (recently activated), clear bit 14.
        let st = status(g, c);
        set_rec_u16(g, c, 0x0A, (st | 0x8000) & !0x4000);
    }
    reschedule(g, si, EV_STEP, 1);
    Some(si)
}

/// Free a slot (0x3085A); the group stays on the map, inactive.
pub fn deactivate(g: &mut GameState, si: usize) {
    let Some(s) = g.creature_slots.get_mut(si).and_then(|s| s.take()) else { return };
    if let Some(ev) = s.event {
        g.timeline.delete(ev);
    }
    set_rec_u8(g, s.thing, 5, 0xFF);
}

/// Activate every creature group on `map` (the party arrived there).
pub fn activate_map(g: &mut GameState, map: usize) {
    let Some(d) = g.creature_data.clone() else { return };
    let m = &g.dungeon.maps[map];
    let (w, h) = (m.width as i32, m.height as i32);
    for x in 0..w {
        for y in 0..h {
            if let Some(c) = group_at(g, map, x, y) {
                activate(g, &d, c, map, x, y);
            }
        }
    }
}

/// (Re)schedule a slot's timeline event `delay` ticks from now.
pub fn reschedule(g: &mut GameState, si: usize, kind: u8, delay: u32) {
    let Some(s) = g.creature_slots.get(si).and_then(|s| s.as_ref()) else { return };
    if let Some(old) = s.event {
        g.timeline.delete(old);
    }
    let pos = s.pos;
    let mut ev = Event::new(kind, pos.map() as u8, g.tick.wrapping_add(delay));
    ev.x = pos.x() as u8;
    ev.y = pos.y() as u8;
    ev.w10 = SLOT_TAG | si as u16;
    let id = g.schedule(ev);
    if let Some(s) = g.creature_slots.get_mut(si).and_then(|s| s.as_mut()) {
        s.event = id;
    }
}

/// Per-tick hook from `GameState::advance`: activate creatures on the
/// party's map when it changes.
pub fn update(g: &mut GameState) {
    if g.creature_data.is_none() || g.champions.is_empty() && g.creature_map_seen.is_some() {
        return;
    }
    if g.creature_map_seen != Some(g.party.map) {
        g.creature_map_seen = Some(g.party.map);
        activate_map(g, g.party.map);
    }
}

// ---------------------------------------------------------------------------
// The creature step (events 0x21 / 0x22)

/// Timeline entry point for events 0x21 and 0x22.
pub fn event(g: &mut GameState, ev: Event) {
    let Some(d) = g.creature_data.clone() else { return };
    let si = if ev.w10 & SLOT_TAG != 0 {
        (ev.w10 & !SLOT_TAG) as usize
    } else {
        match group_at(g, ev.map as usize, ev.x as i32, ev.y as i32).and_then(|c| slot_of(g, c)) {
            Some(si) => si,
            None => return,
        }
    };
    let Some(s) = g.creature_slots.get_mut(si).and_then(|s| s.as_mut()) else { return };
    s.event = None;
    step(g, &d, si, ev.kind == EV_CONTINUE);
}

/// 0x257CC: regeneration, owed damage, then the animation driver.
fn step(g: &mut GameState, d: &CreatureData, si: usize, continuing: bool) {
    let Some(ctx) = Ctx::load(g, d, si) else {
        deactivate(g, si);
        return;
    };
    let c = ctx.thing;
    let mut owed = {
        let s = ctx.slot_mut(g);
        std::mem::take(&mut s.pending_damage)
    };
    if hp(g, c) == 0 {
        set_rec_u16(g, c, 6, 1);
        owed = owed.max(1);
    }
    // Regeneration (info +3): every |n| × 4 ticks; negative n hurts instead.
    let n = ctx.info.regen() as i16;
    if n != 0 {
        let p = n.unsigned_abs();
        let now = (g.tick >> 2) as u8;
        let since = now.wrapping_sub(ctx.slot(g).regen_stamp) as u16;
        let q = since / p;
        if q > 0 {
            if n < 0 {
                owed = owed.saturating_add(q);
            } else if hp(g, c) < ctx.info.base_hp() {
                set_rec_u16(g, c, 6, (hp(g, c) + q).min(ctx.info.base_hp()));
            }
            ctx.slot_mut(g).regen_stamp = now.wrapping_sub((since % p) as u8);
        }
    }
    if owed > 0 {
        if !ctx.info.inanimate() {
            let st = status(g, c);
            set_rec_u16(g, c, 0x0A, st & 0x7FFF);
        }
        if apply_damage(g, &ctx, owed) {
            return;
        }
    }
    drive(g, d, &ctx, continuing);
}

/// 0x31348: subtract owed damage; on death start the death action.
/// Returns true if the creature was removed outright (inanimate).
fn apply_damage(g: &mut GameState, ctx: &Ctx, owed: u16) -> bool {
    let c = ctx.thing;
    let h = hp(g, c);
    if owed < h {
        set_rec_u16(g, c, 6, h - owed);
        return false;
    }
    set_rec_u16(g, c, 6, 1);
    g.effects.push(Effect::CreatureDied { thing: c, map: ctx.map, x: ctx.x, y: ctx.y });
    if ctx.info.inanimate() {
        remove(g, ctx);
        return true;
    }
    let s = ctx.slot_mut(g);
    s.queued = ACTION_DIE;
    s.program = -1;
    false
}

/// Remove a dead creature: drop its possessions and free its slot.
pub fn remove(g: &mut GameState, ctx: &Ctx) {
    let c = ctx.thing;
    let mut p = ThingRef(rec_u16(g, c, 2));
    let mut guard = 0;
    while p.is_thing() && guard < 64 {
        guard += 1;
        let next = ThingRef(g.dungeon.record_word(p, 0).unwrap_or(ThingRef::END.0));
        g.dungeon.add_thing(ctx.map, ctx.x, ctx.y, p);
        p = next;
    }
    set_rec_u16(g, c, 2, ThingRef::END.0);
    deactivate(g, ctx.si);
    if g.dungeon.remove_thing(ctx.map, ctx.x, ctx.y, c) {
        // Free the record (next word 0xFFFF marks a free record).
        g.dungeon.set_record_word(c, 0, 0xFFFF);
    }
}

/// The animation driver (0x25420).
fn drive(g: &mut GameState, d: &CreatureData, ctx: &Ctx, continuing: bool) {
    let Some(an) = d.anim(ctx.ty) else {
        // No animation data: think anyway so the creature still acts.
        if !continuing && !ctx.info.inanimate() {
            ai::begin_action(g, d, ctx);
        }
        reschedule(g, ctx.si, EV_STEP, 4);
        return;
    };
    // Frozen by the party-wide counter unless exempt (info +1 bit 0x10).
    if g.party_status.counter_0b != 0 && ctx.info.flags1() & 0x10 == 0 && ctx.slot(g).action != ACTION_DIE {
        reschedule(g, ctx.si, if continuing { EV_CONTINUE } else { EV_STEP }, 4);
        return;
    }
    let mut first = false;
    let mut more;
    if !continuing {
        first = true;
        if !ctx.info.inanimate() {
            ai::begin_action(g, d, ctx);
        }
        let action = ctx.slot(g).action;
        if (0x32..0x35).contains(&action) {
            reschedule(g, ctx.si, EV_STEP, (action - 0x32) as u32);
            return;
        }
        let start = an.seq_start(action);
        let s = ctx.slot_mut(g);
        s.seq_start = start;
        s.seq_off = NO_FRAME;
        let mut off = NO_FRAME;
        more = an.advance(start, &mut off, &mut g.rng);
        ctx.slot_mut(g).seq_off = off;
    } else {
        let (start, mut off) = (ctx.slot(g).seq_start, ctx.slot(g).seq_off);
        more = an.advance(start, &mut off, &mut g.rng);
        ctx.slot_mut(g).seq_off = off;
    }
    // Frame events, chained in zero time while armed and flagged.
    let mut ended = !more;
    for _ in 0..16 {
        let s = ctx.slot(g);
        let f = an.frame(s.seq_start, s.seq_off);
        let fire = first || s.armed == 0 || !f.chain();
        if fire && f.event() && more {
            let armed = ai::frame_event(g, d, ctx);
            if g.creature_slots.get(ctx.si).and_then(|s| s.as_ref()).is_none() {
                return; // removed by the event
            }
            ctx.slot_mut(g).armed |= armed;
        }
        first = false;
        let s = ctx.slot(g);
        if !(s.armed != 0 && f.chain() && more) {
            break;
        }
        let (start, mut off) = (s.seq_start, s.seq_off);
        match an.next(start, &mut off) {
            Step::Stopped => break,
            Step::Playing => more = true,
            Step::Ended => {
                more = false;
                ended = true;
            }
        }
        ctx.slot_mut(g).seq_off = off;
    }
    if ended && ctx.slot(g).action == ACTION_DIE {
        remove(g, ctx);
        return;
    }
    let delay = ai::frame_delay(g, ctx, &an);
    reschedule(g, ctx.si, if more { EV_CONTINUE } else { EV_STEP }, delay as u32);
}

// ---------------------------------------------------------------------------
// Read-only view for rendering

/// What the viewport needs to draw a creature: its current action and
/// animation frame (index into the type's (15, type, 7, 252/253) tables),
/// plus the drawing jitter byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreatureView {
    pub action: u8,
    pub frame: u16,
    pub jitter: u8,
    pub facing: u8,
}

/// Current animation state of a creature group, or None when inactive.
pub fn view(g: &GameState, c: ThingRef) -> Option<CreatureView> {
    let s = g.creature_slots.get(slot_of(g, c)?)?.as_ref()?;
    let off = if s.seq_off == NO_FRAME { 0 } else { s.seq_off };
    Some(CreatureView { action: s.action, frame: s.seq_start + off, jitter: s.jitter, facing: facing(g, c) })
}

/// Load the creature tables from the user's files into a game state.
pub fn set_data(g: &mut GameState, d: Rc<CreatureData>) {
    g.creature_data = Some(d);
}

#[cfg(test)]
mod tests;
