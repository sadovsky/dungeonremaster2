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
pub mod goals;
pub mod kinds;
pub mod merchant;
pub mod ops;
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
        crate::rng::trace_context(None, Some((s.thing.0 & 0x3FFF) as u32));
        crate::rng::trace_frame(s.action as u32, s.seq_start as u32, s.seq_off as u32);
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

/// Home square of an active creature (slot +0x0C).
pub fn home_of(g: &GameState, c: ThingRef) -> Option<slot::Packed> {
    slot_of(g, c).and_then(|si| g.creature_slots[si].as_ref()).map(|s| s.home)
}

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

/// Flags and chance a caller passes to the hit handler (0x24E62).
///
/// Low byte: status bit (in record word +0x0A) set when the chance roll
/// passes, or cleared with 0x8000. 0x4000: turn toward the party.
/// 0x2000: allow interrupting the creature's current action.
pub mod hit_flags {
    /// Champion melee (0x18A57): turn, interruptible, status bit 2; chance 90.
    pub const MELEE: (u16, i16) = (0x6002, 90);
    /// Missiles, explosions and clouds (0x1726B, 0x16746): bit 13; chance 100.
    pub const MISSILE: (u16, i16) = (0x200D, 100);
    /// A closing door (0x564C6): bit 6; chance 100.
    pub const DOOR: (u16, i16) = (0x2006, 100);
    /// A thing landing on the group (move routine, 0x4B108): interrupt only.
    pub const FALL: (u16, i16) = (0x2000, 0);
}

/// Damage a creature group with the missile/effect flags; see `hit`.
pub fn damage(g: &mut GameState, c: ThingRef, map: usize, x: i32, y: i32, amount: u16) {
    let (f, ch) = hit_flags::MISSILE;
    hit(g, c, map, x, y, f, ch, amount);
}

/// The hit handler (0x24E62), with the original's draws in order: an
/// optional turn-request bit, the fear roll on the owed damage, the turn
/// toward the party, and the status-bit chance roll. Owed damage is
/// applied on the creature's next step. Called with amount 0 too (a missed
/// blow still makes its draws).
pub fn hit(g: &mut GameState, c: ThingRef, map: usize, x: i32, y: i32, flags: u16, chance: i16, amount: u16) {
    if amount > 0 {
        g.effects.push(Effect::CreatureDamaged { thing: c, amount });
    }
    let Some(d) = g.creature_data.clone() else { return };
    let mut flags = flags;
    let mut turn = flags & 0x4000 != 0;
    if turn {
        flags &= !0x4000;
        if g.rng.bit() != 0 {
            turn = false;
        }
    }
    let interruptible = flags & 0x2000 != 0;
    flags &= !0x2000;
    let Some((info, class)) = type_info(g, &d, creature_type(g, c)) else { return };
    let cflags = d.class_flags(class);
    // Info word 0 bit 0: the group is dormant until woken.
    let dormant = info.raw[0] & 1 != 0;
    let si = match slot_of(g, c) {
        Some(si) => si,
        None if dormant => return,
        None => match activate(g, &d, c, map, x, y) {
            Some(si) => si,
            None => return,
        },
    };
    // The handler reads the slot directly; it doesn't load the creature's
    // context, so the current-creature global stays as it was.
    let Some(Some(slot)) = g.creature_slots.get_mut(si) else { return };
    let pending = slot.pending_damage.saturating_add(amount);
    slot.pending_damage = pending;
    let mut rolled = false;
    if !dormant && chance > 0 {
        let mut st = status(g, c);
        if st & 4 == 0 {
            let p = pending as u32;
            let afraid = p > 30
                || (p > 4 && g.rng.rand4() == 0)
                || p * 100 / (info.base_hp().max(1) as u32) > 15;
            if afraid {
                st |= 4;
                turn = true;
            }
            set_rec_u16(g, c, 0x0A, st);
        }
        if turn && cflags & 0x80 == 0 && g.rng.bit() != 0 {
            if let Some(action) = turn_toward_party(g, c, x, y, st) {
                queue_action(g, si, action);
            }
        }
        let r = g.rng.random(100);
        if chance as i32 > r as i32 {
            rolled = true;
            let bit = 1u16 << (flags & 0x0F);
            let st = status(g, c);
            set_rec_u16(g, c, 0x0A, if flags & 0x8000 != 0 { st & !bit } else { st | bit });
        }
    }
    let mut interrupt = false;
    if !dormant && interruptible && chance == 0 {
        interrupt = true;
    }
    let action = g.creature_slots.get(si).and_then(|s| s.as_ref()).map_or(0, |s| s.action);
    if !interrupt && rolled && (interruptible || (flags & 0x8000 == 0 && flags & 0x40 != 0)) {
        let af = d.action_flags(action);
        interrupt = af & 0x10 == 0 || (cflags & 0x410 != 0 && af & 2 != 0);
    }
    if action == ai::action::DYING {
        return;
    }
    if interrupt || pending >= hp(g, c) {
        // 0x3059D: the event moves to the next tick, as a continue (0x21)
        // while record word +8 is unset, else a step (0x22).
        let kind = if rec_u16(g, c, 8) == 0xFFFF { EV_CONTINUE } else { EV_STEP };
        reschedule(g, si, kind, 1);
    }
}

/// The turn a hit asks for (0x24FEF-0x250B8): face the party, or away from
/// it while afraid. Returns the turn action (6 or 7) or None.
fn turn_toward_party(g: &mut GameState, c: ThingRef, x: i32, y: i32, st: u16) -> Option<u8> {
    let (px, py) = (g.party.x, g.party.y);
    let mut dir = ai::direction_toward_rand(x, y, px, py, &mut g.rng);
    let facing = ((rec_u16(g, c, 0x0E) >> 8) & 3) as u8;
    if st & 8 != 0 && g.rng.rand4() != 0 {
        dir = (dir + 2) & 3;
    } else if facing != dir && g.rng.rand4() == 0 {
        dir = (dir + 2) & 3;
    }
    if facing == (dir + 2) & 3 {
        Some(6 + u8::from(g.rng.bit() != 0))
    } else if facing == dir {
        if g.rng.rand4() != 0 {
            None
        } else {
            Some(6 + u8::from(g.rng.bit() == 0))
        }
    } else {
        Some(6 + u8::from(facing == (dir + 3) & 3))
    }
}

/// Queue an action for a group's next step (0x24DB5 with no interrupt):
/// refused while its current or queued action is dying.
fn queue_action(g: &mut GameState, si: usize, action: u8) {
    if let Some(Some(s)) = g.creature_slots.get_mut(si) {
        if s.action != ai::action::DYING && s.queued != ai::action::DYING {
            s.queued = action;
        }
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
        // A dormant group (info bit 0 set) without a slot wakes here (0x2538C).
        if let Some(d) = g.creature_data.clone() {
            let dormant = type_info(g, &d, creature_type(g, c)).is_some_and(|(i, _)| i.inanimate());
            if dormant && rec_u8(g, c, 5) == 0xFF {
                activate(g, &d, c, map, x, y);
            }
        }
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

/// Size of the active-slot pool, as the original's game start computes it
/// (0x342A3 → 0x342F9): creature groups placed in the dungeon whose type has
/// info bit 0 clear, plus 100, capped at the number of creature records.
/// With the shipped dungeon that is min(80 + 100, 374) = 180.
pub fn pool_size(g: &GameState, d: &CreatureData) -> usize {
    let mut unflagged = 0usize;
    for (m, md) in g.dungeon.maps.iter().enumerate() {
        for x in 0..md.width as i32 {
            for y in 0..md.height as i32 {
                for t in g.dungeon.things_at(m, x, y) {
                    if t.kind() != ThingType::Creature {
                        continue;
                    }
                    let c = ThingRef(t.0 & 0x3FFF);
                    let ty = creature_type(g, c);
                    if type_info(g, d, ty).is_some_and(|(i, _)| !i.inanimate()) {
                        unflagged += 1;
                    }
                }
            }
        }
    }
    let total = g.dungeon.thing_count(ThingType::Creature);
    (unflagged + 100).min(total).max(POOL_SIZE.min(total))
}

/// Give a creature group an active slot and start it (0x306A8).
pub fn activate(g: &mut GameState, d: &CreatureData, c: ThingRef, map: usize, x: i32, y: i32) -> Option<usize> {
    if let Some(si) = slot_of(g, c) {
        return Some(si);
    }
    if g.creature_slots.is_empty() {
        let n = pool_size(g, d);
        g.creature_slots.resize(n, None);
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
        // 0x306A8 then runs the frame scheduler (0x3023F) at once: it starts
        // the current action's sequence (0x14E42 → 0x14F1B, drawing a random
        // number per branch frame) and rolls jitter, flip and extra ticks.
        // These draws happen at activation in the original.
        if let (Some(an), Some(ctx)) = (d.anim(ty), Ctx::load(g, d, si)) {
            let action = ctx.slot(g).action;
            let start = an.seq_start(action);
            let mut off = NO_FRAME;
            an.advance(start, &mut off, &mut g.rng);
            let sl = ctx.slot_mut(g);
            sl.seq_start = start;
            sl.seq_off = off;
            // The rolls happen, but the first step is due on the next tick
            // whatever delay they produce: in the original every creature
            // activated at play start thinks on tick 1.
            let _ = ai::frame_delay_ex(g, &ctx, &an, false);
            reschedule(g, si, EV_STEP, 1);
            return Some(si);
        }
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

/// Per-map creature pass (0x34106), run for the party's map on arrival and
/// for every map at play start (see `pass_all_maps`). Squares are visited
/// column by column; on each, the first creature group without a slot is
/// either activated (info bit 0 clear) or left dormant with a frame-cycle
/// state (bit 0 set). Dormant groups wake only through the actuator signal
/// (`floor_signal`, 0x2538C) or when hit.
pub fn activate_map(g: &mut GameState, map: usize) {
    let Some(d) = g.creature_data.clone() else { return };
    let m = &g.dungeon.maps[map];
    let (w, h) = (m.width as i32, m.height as i32);
    for x in 0..w {
        for y in 0..h {
            let Some(c) = group_at(g, map, x, y) else { continue };
            if rec_u8(g, c, 5) != 0xFF {
                continue;
            }
            let ty = creature_type(g, c);
            let Some((info, _)) = type_info(g, &d, ty) else { continue };
            if info.inanimate() {
                set_dormant_state(g, &d, c, ty);
            } else {
                activate(g, &d, c, map, x, y);
            }
        }
    }
}

/// The per-map pass over every map in order (0x34236): at play start, after
/// loading and after saving. This is what keeps creatures on other maps
/// active (and drawing random numbers) from the first tick.
pub fn pass_all_maps(g: &mut GameState) {
    for map in 0..g.dungeon.maps.len() {
        activate_map(g, map);
    }
}

/// Dormant creature state (0x301F3 → 0x14E42, merged at 0x341A6): record
/// word +8 gets the first frame of action 0x11's sequence and word +10 its
/// frame count, flagged 0x9000 (or `(w12 & 0x3F) << 6 | 0x8000` when record
/// word +12 is set). Bits 0x6000 of the old word survive, and an old
/// `0x8001` pattern (mask 0x803F) is kept. No random numbers are drawn.
fn set_dormant_state(g: &mut GameState, d: &CreatureData, c: ThingRef, ty: u8) {
    let Some(anim) = d.anim(ty) else { return };
    let start = anim.seq_start(0x11);
    let mut count: u16 = 0;
    loop {
        let f = anim.frame(start, count);
        count += 1;
        if f.cont() == 0 || count >= 0x3F {
            break;
        }
    }
    let w12 = rec_u16(g, c, 0x0C);
    let new = if w12 == 0 { count | 0x9000 } else { count | (w12 & 0x3F) << 6 | 0x8000 };
    let old = rec_u16(g, c, 0x0A);
    let mut v = new | (old & 0x6000);
    if old & 0x803F == 0x8001 {
        v = (v & 0x7FC0) | 0x8001;
    }
    set_rec_u16(g, c, 8, start);
    set_rec_u16(g, c, 0x0A, v);
}

/// (Re)schedule a slot's timeline event `delay` ticks from now.
pub fn reschedule(g: &mut GameState, si: usize, kind: u8, delay: u32) {
    let Some(s) = g.creature_slots.get(si).and_then(|s| s.as_ref()) else { return };
    if let Some(old) = s.event {
        g.timeline.delete(old);
    }
    let pos = s.pos;
    let mut ev = Event::new(kind, pos.map() as u8, g.tick.wrapping_add(delay));
    // The priority byte is the creature's type (record byte +4, 0x3059D), so
    // same-tick creature events run higher types first.
    ev.prio = creature_type(g, s.thing);
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
    match g.creature_map_seen {
        // Play start, or just after a load or save: every map (0x34236).
        None => {
            g.creature_map_seen = Some(g.party.map);
            pass_all_maps(g);
        }
        // The party arrived on another map (0x24629 → 0x34106).
        Some(m) if m != g.party.map => {
            g.creature_map_seen = Some(g.party.map);
            activate_map(g, g.party.map);
        }
        _ => {}
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
    // A new event loads a fresh AI context (0x24A88), re-arming 0x24BFC.
    g.creature_ctx_rolled = false;
    g.creature_class_loaded = false;
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

/// The pain cry of a hurt but living creature (0x31348). Only creatures
/// whose type flag 0x01 is clear and whose AI class has flag 0x8000 cry
/// out: one time in eight at once, otherwise when the blow exceeds 3% of
/// the type's info byte 2 or 5% of the remaining hit points, with a coin
/// flip when the status word has bit 3 and else a one-in-four roll. The
/// random draws happen whether or not a sound plays, as in the original.
fn hurt_cry(g: &mut GameState, ctx: &Ctx, owed: u16, hp_before: u16) {
    if ctx.info.raw[0] & 1 != 0 || ctx.cflags & 0x8000 == 0 {
        return;
    }
    let c = ctx.thing;
    let cry = if g.rng.rnd() & 7 == 0 {
        true
    } else if (ctx.info.raw[2] as u32 * 3) / 100 < owed as u32 || (hp_before as u32 * 5) / 100 < owed as u32 {
        let flipped = status(g, c) & 8 != 0 && g.rng.bit() != 0;
        flipped || g.rng.rand4() == 0
    } else {
        false
    };
    if cry {
        let sub = 9 + g.rng.bit() as u8;
        g.effects.push(Effect::Sound { cat: 15, idx: ctx.ty, sub, map: ctx.map, x: ctx.x, y: ctx.y });
    }
}

/// 0x31348: subtract owed damage; on death start the death action.
/// Returns true if the creature was removed outright (inanimate).
fn apply_damage(g: &mut GameState, ctx: &Ctx, owed: u16) -> bool {
    let c = ctx.thing;
    let h = hp(g, c);
    if ctx.info.raw[1] == 0xFF {
        return false;
    }
    // A hurt creature (type flag 0x01 clear, class without flag 0x04)
    // sends a clear action to its home square next tick, whether or not it
    // survives (0x31348 via 0x4BBE4).
    if ctx.info.raw[0] & 1 == 0 && ctx.cflags & 4 == 0 {
        if let Some(home) = home_of(g, c) {
            let t = g.tick.wrapping_add(1);
            crate::actuators::square_action(g, home.map(), home.x(), home.y(), 0, crate::actuators::CLEAR, t);
        }
    }
    if owed < h {
        hurt_cry(g, ctx, owed, h);
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
        // 0x25420 runs the event of the current frame whether or not the
        // sequence continues past it.
        if fire && f.event() {
            // 0x25420 sets up the context (and rolls alertness) before a
            // frame event.
            ai::context_roll(g, ctx);
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
    /// Alternate drawing descriptor: while the slot's action (+0x1A) is
    /// 0x13, words +0xE / +0x10 (start + offset) pick a second descriptor
    /// that supplies placement and scale (0x14CF2 caller in the creature
    /// drawer).
    pub alt_frame: Option<u16>,
}

/// Current animation state of a creature group, or None when inactive.
pub fn view(g: &GameState, c: ThingRef) -> Option<CreatureView> {
    let s = g.creature_slots.get(slot_of(g, c)?)?.as_ref()?;
    let off = if s.seq_off == NO_FRAME { 0 } else { s.seq_off };
    let alt_frame = (s.action == 0x13).then(|| s.vars[0].wrapping_add(s.vars[1]));
    Some(CreatureView { action: s.action, frame: s.seq_start + off, jitter: s.jitter, facing: facing(g, c), alt_frame })
}

/// Load the creature tables from the user's files into a game state.
pub fn set_data(g: &mut GameState, d: Rc<CreatureData>) {
    g.creature_data = Some(d);
}

#[cfg(test)]
mod tests;

/// A missile passing nearby wakes a creature (docs/05, "Missile flight"):
/// make sure it holds an active slot so its AI starts thinking.
pub fn alert(g: &mut GameState, c: ThingRef, map: usize, x: i32, y: i32) {
    if slot_of(g, c).is_some() {
        return;
    }
    if let Some(d) = g.creature_data.clone() {
        let _ = activate(g, &d, c, map, x, y);
    }
}

/// Spell-reflecting creature type (docs/05: type flag 0x02).
/// Tentative: the flag is read from info byte 0, like the other type flags.
pub fn reflects_spells(g: &GameState, c: ThingRef) -> bool {
    info_of(g, c).is_some_and(|i| i.raw[0] & 0x02 != 0)
}

/// Explosion resistance nibble: info word +0x18 bits 4-7 (15 = immune),
/// docs/07 "Explosions".
pub fn resistance(g: &GameState, c: ThingRef) -> u8 {
    info_of(g, c).map_or(0, |i| ((i.word18() >> 4) & 15) as u8)
}

/// Defence values a missile or melee hit needs, from the type info record
/// (docs/08). None until creature data is loaded; callers then apply raw
/// damage.
pub fn defence(g: &GameState, c: ThingRef) -> Option<crate::combat::CreatureDefence> {
    info_of(g, c).map(|i| crate::combat::CreatureDefence::from_info(&i.raw))
}
