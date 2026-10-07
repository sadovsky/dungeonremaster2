//! Think, behaviour selection, the program interpreter, frame events and
//! frame timing (docs/08 "The creature tick" onward).

use dm2_formats::dungeon::{Element, ThingRef, ThingType};

use crate::effects::Effect;
use crate::movement;
use crate::state::GameState;
use crate::viewport::{DX, DY};

use super::anim::{Anim, NO_FRAME};
use super::data::{CreatureData, Row, WANDER_LISTS};
use super::fight;
use super::goals;
use super::kinds;
use super::ops;
use super::merchant;
use super::planner::{self, Searcher};
use super::slot::{Packed, NO_ACTION};
use super::{creature_type, facing, group_at, hp, rec_u16, set_facing, set_rec_u16, status, type_info, Ctx, ACTION_DIE};

/// Interpreter results (docs/08 "Handler results").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Res {
    Done,
    Failed,
    InProgress,
}

/// Actions with known meanings.
pub mod action {
    pub const IDLE: u8 = 0;
    /// The dying action: hits and queued actions leave it alone (0x24DB5, 0x24E62).
    pub const DYING: u8 = 0x13;
    pub const WALK: u8 = 1;
    pub const WALK_NEAR: u8 = 2;
    pub const STEP_LEFT: u8 = 3;
    pub const STEP_RIGHT: u8 = 4;
    pub const TURN_LEFT: u8 = 6;
    pub const TURN_RIGHT: u8 = 7;
    pub const ATTACK: u8 = 8;
    pub const BACK_OFF: u8 = 9;
    pub const WAIT_ROW: u8 = 0x1D;
    pub const LOOK_A: u8 = 0x27;
    pub const LOOK_B: u8 = 0x28;
    pub const MOVE_ATTACK: u8 = 0x26;
    pub const FLAG_CHANGE: u8 = 0x33;
    pub const TRANSFORM: u8 = 0x3B;
    pub const TRANSFORM_END: u8 = 0x3C;
    pub const GIVE_UP: u8 = 0x55;
}

/// Iteration cap for one think (the original stops after 0x20 steps).
const MAX_STEPS: usize = 0x21;

pub fn direction_toward(x: i32, y: i32, tx: i32, ty: i32) -> u8 {
    let (dx, dy) = (tx - x, ty - y);
    if dx.abs() > dy.abs() {
        if dx > 0 { 1 } else { 3 }
    } else if dy > 0 {
        2
    } else {
        0
    }
}

/// Direction from (x, y) toward (tx, ty) as the original's 0x1863D picks it:
/// the axis with the larger distance wins, and an exact diagonal draws one
/// random bit to choose (set: the x axis).
pub fn direction_toward_rand(x: i32, y: i32, tx: i32, ty: i32, rng: &mut crate::rng::Rng) -> u8 {
    let (sx, sy) = (x - tx, y - ty);
    let (mut ax, mut ay) = (sx.abs(), sy.abs());
    if ax == ay {
        if rng.bit() != 0 {
            ax += 1;
        } else {
            ay += 1;
        }
    }
    if ax < ay {
        if sy > 0 { 0 } else { 2 }
    } else if sx > 0 {
        3
    } else {
        1
    }
}

fn manhattan(x: i32, y: i32, tx: i32, ty: i32) -> i32 {
    (x - tx).abs() + (y - ty).abs()
}

pub(super) fn party_here(g: &GameState, map: usize, x: i32, y: i32) -> bool {
    g.party.map == map && g.party.x == x && g.party.y == y && g.champions.iter().any(|c| c.is_alive())
}

pub(super) fn searcher(ctx: &Ctx) -> Searcher {
    Searcher {
        map: ctx.map,
        x: ctx.x,
        y: ctx.y,
        mask: ctx.info.terrain(),
        size: ctx.info.door_size().max(1),
        group: ctx.thing,
        attack_mask: u16::from_le_bytes([ctx.info.raw[0x0E], ctx.info.raw[0x0F]]),
        range: u16::from_le_bytes([ctx.info.raw[0x14], ctx.info.raw[0x15]]) >> 12,
        info0: ctx.info.raw[0],
        cflags: ctx.cflags,
    }
}

pub(super) fn set_action(g: &mut GameState, ctx: &Ctx, a: u8) {
    ctx.slot_mut(g).action = a;
}

/// Queue a turn toward `dir` (0x2C005). Returns false if already facing it.
/// The turn is always one quarter: the target stored in slot +0x1D is the
/// neighbouring facing on the turn's side (action 6 turns left, 7 right).
/// For a full turn-around a random bit picks the side, and the creature
/// turns the rest of the way on a later think.
pub(super) fn queue_turn(g: &mut GameState, ctx: &Ctx, dir: u8) -> bool {
    let f = facing(g, ctx.thing);
    if dir == f {
        return false;
    }
    let right = if dir == (f + 1) & 3 {
        true
    } else if dir == (f + 3) & 3 {
        false
    } else {
        g.rng.bit() != 0
    };
    let s = ctx.slot_mut(g);
    s.turn_to = if right { (f + 1) & 3 } else { (f + 3) & 3 };
    s.action = if right { action::TURN_RIGHT } else { action::TURN_LEFT };
    true
}

/// The movement test (0x2D792), reduced to its outcome: queue a move,
/// turn-step, turn or attack toward `dir`. Returns true if an action was
/// queued.
pub fn move_test(g: &mut GameState, ctx: &Ctx, dir: u8, mode: u8) -> bool {
    move_test_flags(g, ctx, dir, mode, false)
}

/// The movement test with the opcode flag 0x80 that `?` and `@` pass
/// (0x27F54, 0x27FC8): the party's square then only fails the test,
/// without turning toward the party or attacking it.
pub fn move_test_step(g: &mut GameState, ctx: &Ctx, dir: u8, mode: u8) -> bool {
    move_test_flags(g, ctx, dir, mode, true)
}

fn move_test_flags(g: &mut GameState, ctx: &Ctx, dir: u8, mode: u8, step_only: bool) -> bool {
    let (nx, ny) = (ctx.x + DX[dir as usize], ctx.y + DY[dir as usize]);
    let f = facing(g, ctx.thing);
    if party_here(g, ctx.map, nx, ny) {
        if step_only {
            return false;
        }
        if f != dir {
            return queue_turn(g, ctx, dir);
        }
        let s = ctx.slot_mut(g);
        s.target = Packed::new(ctx.map, nx, ny);
        s.dir_arg = dir;
        s.action = action::ATTACK;
        return true;
    }
    if group_at(g, ctx.map, nx, ny).is_some() {
        return false;
    }
    if !super::terrain::can_enter(g, ctx.map, nx, ny, ctx.info.terrain(), ctx.info.door_size().max(1)) {
        return false;
    }
    if destination_danger(g, ctx, nx, ny, mode, dir) {
        return false;
    }
    let a = if mode == 6 {
        action::BACK_OFF
    } else if f == dir {
        action::WALK
    } else if (f + 1) & 3 == dir {
        action::STEP_RIGHT
    } else if (f + 3) & 3 == dir {
        action::STEP_LEFT
    } else {
        return queue_turn(g, ctx, dir);
    };
    let s = ctx.slot_mut(g);
    s.target = Packed::new(ctx.map, nx, ny);
    s.dir_arg = dir;
    s.turn_to = dir;
    s.mode = mode;
    s.action = a;
    true
}

/// Start a new action on event 0x22 (0x25420): take the queued action, or
/// think when none is queued.
pub fn begin_action(g: &mut GameState, d: &CreatureData, ctx: &Ctx) {
    // A new action restarts the turn-step phase counter (slot +0x1F) and
    // clears the armed byte (+0x21) that a failed frame event set, so the
    // new sequence fires its events again instead of chaining through. The
    // draw log shows this: a creature whose move failed chains through the
    // rest of that walk, but its next walk steps frame by frame. The store
    // that clears the byte was not located in the code.
    let s = ctx.slot_mut(g);
    s.stage = 0;
    s.armed = 0;
    // The context setup for a new action keeps the action just finished
    // (0x7F56A, "no action" read as 0) before clearing it; goal kind 0x0A
    // tests that action's flags.
    let prev = ctx.slot(g).action;
    g.creature_prev_action = if prev == NO_ACTION { 0 } else { prev };
    let queued = ctx.slot(g).queued;
    if queued == NO_ACTION {
        set_action(g, ctx, NO_ACTION);
        think(g, d, ctx);
        if ctx.slot(g).action == NO_ACTION {
            set_action(g, ctx, action::IDLE);
        }
    } else {
        let s = ctx.slot_mut(g);
        s.queued = NO_ACTION;
        s.action = queued;
        if queued == action::TURN_LEFT || queued == action::TURN_RIGHT {
            let f = facing(g, ctx.thing);
            ctx.slot_mut(g).turn_to = if queued == action::TURN_LEFT { (f + 3) & 3 } else { (f + 1) & 3 };
        }
    }
}

/// Dungeon-script condition `n` (0x150AE): 0-0x3F test a flag bit,
/// 0x40-0x7F a byte variable, 0x80-0xBF a word variable; non-zero = true.
pub fn script_condition(g: &GameState, n: u16) -> bool {
    let v = &g.legacy;
    match n {
        0..=0x3F => v.flags[(n >> 3) as usize] & (1 << (n & 7)) != 0,
        0x40..=0x7F => v.byte_vars[(n - 0x40) as usize] != 0,
        0x80..=0xBF => v.word_vars[(n - 0x80) as usize] != 0,
        _ => false,
    }
}

/// Status bits refreshed from set selection's raw draw `r` (0x259CC):
/// - bit 15: cleared on the party's map; elsewhere set (clearing bit 14)
///   when `r` misses 0x70 with a program running, or 0x30 without one;
/// - bit 14: toggled while bit 15 is clear, when `r` misses 0x380 (badly
///   hurt) or 0xF80;
/// - bit 5: cleared when `r % (16 - info word 0x16 bits 4-7)` is 0;
/// - bit 13: set for classes with flag byte 1 bit 2, else cleared when `r`
///   misses 0x38;
/// - bits 4, 6 and 12: cleared when `r` misses 0x3000, 3 and 0x8008.
fn refresh_status(g: &GameState, ctx: &Ctx, mut st: u16, r: u16) -> u16 {
    if ctx.map == g.party.map {
        st &= !0x8000;
    } else {
        let mask = if ctx.slot(g).program >= 0 { 0x70 } else { 0x30 };
        if r & mask == 0 {
            st = (st | 0x8000) & !0x4000;
        }
    }
    if st & 0x8000 == 0 {
        let mask = if st & 8 != 0 { 0x380 } else { 0xF80 };
        if r & mask == 0 {
            st ^= 0x4000;
        }
    }
    let k = 16 - ((ctx.info.alertness_word() >> 4) & 0xF);
    if r % k == 0 {
        st &= !0x20;
    }
    if ctx.cflags & 0x400 != 0 {
        st |= 0x2000;
    } else if r & 0x38 == 0 {
        st &= !0x2000;
    }
    if r & 0x3000 == 0 {
        st &= !0x10;
    }
    if r & 3 == 0 {
        st &= !0x40;
    }
    if r & 0x8008 == 0 {
        st &= !0x1000;
    }
    st
}

/// Refresh status bit 3 ("badly hurt") and choose the behaviour set
/// (0x25962 / 0x259CC). Returns the behaviour list address.
fn select_set(g: &mut GameState, d: &CreatureData, ctx: &Ctx) -> u32 {
    let c = ctx.thing;
    // Set selection loads the AI class index, even for the cheap wander
    // lists that skip the context setup.
    g.creature_class_loaded = true;
    // 0x259CC starts with a raw draw r that refreshes several status bits
    // before the set is chosen (read from the disassembly; the decompile
    // drops this block).
    let r = (g.rng.rnd() & 0xFFFF) as u16;
    crate::rng::trace_tag("think", (c.0 & 0x3FF) as u32);
    let mut st = refresh_status(g, ctx, status(g, c), r);
    let n = if ctx.cflags & 2 != 0 { 2 } else { (((c.0 & 3) + 1) * 2 - 1) * 2 };
    if g.rng.random(n) == 0 {
        let base = ctx.info.base_hp().max(1) as u32;
        if (hp(g, c) as u32 * 100) / base < 25 {
            st |= 8;
        } else {
            st &= !8;
        }
    }
    st &= !2;
    set_rec_u16(g, c, 0x0A, st);
    let sets = d.behaviour_sets(ctx.class);
    let (mut exact, mut subset, mut overlap) = (None, None, None);
    for (i, &(mask, _)) in sets.iter().enumerate() {
        if mask == 0 {
            break;
        }
        if mask & 0xC000 == 0xC000 {
            // A dungeon-script condition picks this set outright (0x150AE).
            if script_condition(g, mask & 0x3FFF) {
                exact = Some(i);
                break;
            }
            continue;
        }
        if overlap.is_none() && mask & st != 0 {
            overlap = Some(i);
        }
        if subset.is_none() && mask & st == mask {
            subset = Some(i);
        }
        if mask == st {
            exact = Some(i);
            break;
        }
    }
    let default = sets.len().saturating_sub(1);
    let i = exact.or(subset).or(overlap).unwrap_or(default);
    if ctx.slot(g).set != i as i8 {
        let s = ctx.slot_mut(g);
        s.program = -1;
        s.step = 0;
        s.set = i as i8;
    }
    sets.get(i).map(|&(_, l)| l).unwrap_or(0)
}

/// Pick a program from the behaviour list with the planner (0x26008).
fn pick_behaviour(g: &mut GameState, d: &CreatureData, ctx: &Ctx, list: u32) -> Option<u8> {
    let cur = ctx.slot(g).program;
    let mut goals = Vec::new();
    for e in d.behaviour_list(list) {
        let pass = if cur >= 0 && e.program as i8 == cur {
            true
        } else {
            match e.probability {
                0 => true,
                p if p > 0 => g.rng.random(p as u16) == 0,
                p => g.rng.random(p.unsigned_abs() as u16) != 0,
            }
        };
        if !pass {
            continue;
        }
        let step = if cur >= 0 && e.program as i8 == cur { ctx.slot(g).step } else { 0 };
        let Some(row) = d.row(e.program, step) else { continue };
        for mut gl in goals::build(g, d, ctx, e.program, row.goal_kind(), row.goal_arg, e.goal_data) {
            gl.data = e.goal_data;
            goals.push(gl);
        }
    }
    let found = planner::search(g, &searcher(ctx), &goals)?;
    let gl = goals[found.goal];
    // Starting the chosen program (0x25C59) copies the goal record's words
    // +8 and +10 (spec words +4 and +6) into the default item kinds that
    // `N` and `]` fall back to (0x7F7D8, 0x7F7DA).
    let s = ctx.slot_mut(g);
    s.kind_a = gl.mode;
    s.kind_b = gl.value;
    s.goal_kind = gl.kind;
    s.goal_data = gl.data;
    s.goal_tag = gl.tag;
    // A goal on another layer is approached through the stairs leading there.
    let (tx, ty) = if found.map != ctx.map { found.via.unwrap_or((found.x, found.y)) } else { (found.x, found.y) };
    ctx.slot_mut(g).target = Packed::new(ctx.map, tx, ty);
    Some(gl.program)
}

/// AI context setup (0x24BFC): once per creature event, roll alertness. With
/// n = (15 - (info word 0x16 & 15)) * 2, the creature is alert (0x7F589)
/// when `n / 4 + random(n + 1)` is at most the ticks elapsed since its last
/// completed action (slot +4, low byte of the tick, taken mod 256).
pub fn context_roll(g: &mut GameState, ctx: &Ctx) {
    if g.creature_ctx_rolled {
        return;
    }
    g.creature_ctx_rolled = true;
    g.creature_class_loaded = true;
    let n = (15 - (ctx.info.alertness_word() & 15)) * 2;
    let elapsed = {
        let d = (g.tick as u8).wrapping_sub(ctx.slot(g).act_tick) as i8 as i16;
        if d < 0 { d + 256 } else { d }
    };
    let threshold = (n as i16 >> 2) + g.rng.random(n + 1) as i16;
    g.creature_alert_roll = u16::from(threshold <= elapsed);
    if std::env::var("DM2_PLANDBG").ok().and_then(|t| t.parse::<u32>().ok()) == Some(g.tick) {
        eprintln!("ROLL tick {} thing {:#x} n {} elapsed {} threshold {} alert {}", g.tick, ctx.thing.0 & 0x3FFF, n, elapsed, threshold, g.creature_alert_roll);
    }
}

/// Does the scan stop at (x, y) (0x2B9FC)? Walls, closing or closed doors
/// (a door type that passes missiles lets it through on a random bit),
/// closed trick walls, kind-0xE clouds and solid creature groups block.
pub(super) fn blocks_scan(g: &mut GameState, map: usize, x: i32, y: i32) -> bool {
    let sq = g.dungeon.square(map, x, y);
    let e = sq.0 >> 5;
    if e == 0 {
        return true;
    }
    if e == 4 && matches!(sq.0 & 7, 3 | 4) {
        let passes = crate::doors::door_at(g, map, x, y)
            .map(|d| crate::doors::door_type(g, map, d))
            .is_some_and(|t| g.attrs.get(14, t, 0x10) != 0);
        if !passes || g.rng.bit() == 0 {
            return true;
        }
    }
    if e == 6 && sq.0 & 4 == 0 {
        return true;
    }
    if sq.0 & 0x10 == 0 {
        return false;
    }
    for t in g.dungeon.things_at(map, x, y) {
        match t.kind() {
            ThingType::Cloud => {
                if g.dungeon.record_word(t, 1).unwrap_or(0) & 0x7F == 0x0E {
                    return true;
                }
            }
            ThingType::Creature => {
                if let Some(c) = group_at(g, map, x, y) {
                    let w = super::info_of(g, c).map_or(0, |i| u16::from_le_bytes([i.raw[0], i.raw[1]]));
                    let solid = if w & 1 != 0 { (w >> 6) & 3 < 2 } else { w & 0x20 == 0 };
                    if solid {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Direction a missile thing is flying: bits 10-11 of its timeline event's
/// word at +8 (the event index is the missile's word 3).
fn missile_dir(g: &GameState, m: ThingRef) -> Option<u8> {
    let slot = g.dungeon.record_word(m, 3)?;
    g.timeline.get(slot).map(|e| ((e.w8() >> 10) & 3) as u8)
}

/// Danger scan (0x2D52D): is a harmful missile flying toward (x, y)? For
/// each direction it may first roll `rnd & 7` (always when record word +0xA
/// bit 7 is set; otherwise only when (x, y) is the creature's own square and
/// the direction is behind it, unless class flag 0x400 or info flag 4 rules
/// the roll out) and skips the direction on a non-zero roll. It then looks up
/// to three squares out for a missile heading back toward (x, y) whose
/// impact would do damage, stopping at squares that block (0x2B9FC).
pub(crate) fn danger_scan(g: &mut GameState, ctx: &Ctx, x: i32, y: i32) -> bool {
    let behind = (facing(g, ctx.thing) + 2) & 3;
    for dir in 0..4u8 {
        let roll = if ctx.cflags & 0x400 != 0 {
            false
        } else if rec_u16(g, ctx.thing, 0x0A) & 0x80 != 0 {
            true
        } else {
            ctx.info.raw[0] & 4 == 0 && x == ctx.x && y == ctx.y && dir == behind
        };
        if roll && g.rng.rnd() & 7 != 0 {
            continue;
        }
        let (mut cx, mut cy) = (x, y);
        for _ in 0..3 {
            cx += DX[dir as usize];
            cy += DY[dir as usize];
            let m = &g.dungeon.maps[ctx.map];
            if cx < 0 || cy < 0 || cx >= m.width as i32 || cy >= m.height as i32 {
                break;
            }
            let back = (dir + 2) & 3;
            for t in g.dungeon.things_at(ctx.map, cx, cy) {
                if t.kind() == ThingType::Missile
                    && missile_dir(g, t) == Some(back)
                    && crate::missiles::threat_damage(g, t) != 0
                {
                    return true;
                }
            }
            if (cx, cy) != (ctx.x, ctx.y) && blocks_scan(g, ctx.map, cx, cy) {
                break;
            }
        }
    }
    false
}

/// The movement test's danger checks on a destination for modes 4 and 5
/// (0x2D792): a missile there not already flying in the tested direction
/// that would hurt refuses the move, and in mode 5 so does the danger scan,
/// unless a kind-0xE cloud is on the square. Missiles on the creature's own
/// square are ignored.
fn destination_danger(g: &mut GameState, ctx: &Ctx, x: i32, y: i32, mode: u8, dir: u8) -> bool {
    let m = mode & 0x1F;
    if m != 4 && m != 5 {
        return false;
    }
    let mut calm_cloud = false;
    for t in g.dungeon.things_at(ctx.map, x, y) {
        match t.kind() {
            ThingType::Cloud => {
                if g.dungeon.record_word(t, 1).unwrap_or(0) & 0x7F == 0x0E {
                    calm_cloud = true;
                }
            }
            ThingType::Missile if (x, y) != (ctx.x, ctx.y) => {
                if missile_dir(g, t) != Some(dir) && crate::missiles::threat_damage(g, t) != 0 {
                    return true;
                }
            }
            _ => {}
        }
    }
    m == 5 && !calm_cloud && danger_scan(g, ctx, x, y)
}

/// The wander list's think (0x262F7, list 0x73392, read from the
/// disassembly): one raw draw r = rnd & 7. r 4-7 stands still; r 0 turns to
/// face (tick & 3) through 0x2C005; r 1-3 looks at the square ahead in the
/// current facing, records it as the target, and walks there (action 2)
/// unless it is a wall or solid rock, or a map-edge link (0x1D113) to a map
/// whose creature list lacks this type (0x1F9FF).
fn wander(g: &mut GameState, ctx: &Ctx) {
    let r = g.rng.rnd() & 7;
    if r > 3 {
        set_action(g, ctx, action::IDLE);
        return;
    }
    if r == 0 {
        queue_turn(g, ctx, (g.tick & 3) as u8);
        return;
    }
    let f = facing(g, ctx.thing) as usize;
    let (tx, ty) = (ctx.x + DX[f], ctx.y + DY[f]);
    ctx.slot_mut(g).target = super::slot::Packed::new(ctx.map, tx, ty);
    let blocked = match g.dungeon.square(ctx.map, tx, ty).element() {
        Element::Wall | Element::Rock => true,
        Element::Teleporter => crate::movement::edge_link(g, ctx.map, tx, ty)
            .is_some_and(|link| !g.dungeon.map_lists(link.map).creature_types.contains(&ctx.ty)),
        _ => false,
    };
    set_action(g, ctx, if blocked { action::IDLE } else { action::WALK_NEAR });
}

/// Think's danger block (0x262F7 after the context setup, read from the
/// disassembly). The creature first tests standing on its own square with
/// the movement test (0x2D792 with the current square as destination), which
/// in mode 5 runs the danger scan there. If it can't stay, it is in danger
/// when, in mode 5, the scan finds a missile; otherwise when it is not
/// alert, or (alert) when no path leads out (0x2C404, taken as available
/// here) or `random((info word 0x18 >> 10 & 3) + 1) <= 1`. In danger it
/// flags record word +0xA bit 13, may flee from a scanned missile (class
/// flag 0x10, action 0x55), and otherwise tries four directions to step
/// away, retrying once with mode 0 on a random bit. Returns true when an
/// action was chosen.
fn danger_block(g: &mut GameState, ctx: &Ctx, mode: u8, quarter: &mut u8) -> bool {
    let can_stand = super::terrain::can_enter(g, ctx.map, ctx.x, ctx.y, ctx.info.terrain(), ctx.info.door_size().max(1))
        && !destination_danger(g, ctx, ctx.x, ctx.y, mode, 0xFF);
    if can_stand {
        return false;
    }
    let danger = if mode == 5 && danger_scan(g, ctx, ctx.x, ctx.y) {
        true
    } else if g.creature_alert_roll != 0 {
        // TODO(0x2C404): the path test is taken as finding a way out.
        let n = ((ctx.info.word18() >> 8) & 0xF) >> 2;
        g.rng.random(n + 1) <= 1
    } else {
        true
    };
    if !danger {
        return false;
    }
    let w = rec_u16(g, ctx.thing, 0x0A);
    set_rec_u16(g, ctx.thing, 0x0A, w | 0x2000);
    loop {
        if ctx.cflags & 0x10 != 0 {
            let w = rec_u16(g, ctx.thing, 0x0A);
            let scan = if w & 8 != 0 && g.rng.rand4() != 0 {
                true
            } else if w & 0x40 != 0 && g.rng.bit() != 0 {
                true
            } else {
                g.rng.rand4() == 0
            };
            if scan && danger_scan(g, ctx, ctx.x, ctx.y) {
                let s = ctx.slot_mut(g);
                s.program = -1;
                s.step = 0;
                set_action(g, ctx, 0x55);
                return true;
            }
        }
        // TODO(0x33E61): the planner's escape direction (slot +0x1B) is not
        // modelled; the original falls back to these draws when it finds none.
        let mut dir = if g.rng.bit() != 0 { (facing(g, ctx.thing) + 2) & 3 } else { g.rng.rand4() as u8 };
        let turn: u8 = if g.rng.bit() != 0 { 1 } else { 3 };
        let m = if *quarter != 0 { 0 } else { mode };
        for _ in 0..4 {
            if move_test(g, ctx, dir, m | 0x80) {
                let s = ctx.slot_mut(g);
                s.program = -1;
                s.step = 0;
                return true;
            }
            dir = (dir + turn) & 3;
        }
        *quarter += 1;
        if *quarter == 1 && g.rng.bit() != 0 {
            continue;
        }
        return false;
    }
}

/// Think (0x262F7): choose and start the next action.
pub fn think(g: &mut GameState, d: &CreatureData, ctx: &Ctx) {
    let list = select_set(g, d, ctx);
    let mode: u8 = if ctx.cflags & 0x40 != 0 { 0 } else if ctx.cflags & 0x20 == 0 { 5 } else { 4 };
    if list == WANDER_LISTS[0] {
        // 0x262F7, list 0x73399: stand still, no draws.
        set_action(g, ctx, action::IDLE);
        return;
    }
    if list == WANDER_LISTS[1] {
        wander(g, ctx);
        return;
    }
    // Context setup with its alertness roll (0x24BFC), then a two-bit draw
    // (0x1C6F6) whose zero test 0x262F7 keeps for later; both happen here in
    // the original, before the move tests.
    context_roll(g, ctx);
    let mut quarter = u8::from(g.rng.rand4() == 0);
    if mode != 0 && danger_block(g, ctx, mode, &mut quarter) {
        return;
    }
    // The behaviour picker (0x26008) runs on every think. Its two-bit "keep
    // the current plan" roll only applies while the event's path cache is in
    // use (0x25D49: globals 0x7F7D4/D5/D7, reset by the context setup), which
    // it never is at this point, so think plans without a draw. The same
    // program keeps its step; a different one starts at step 0; no match
    // restarts the default program 0x11.
    match pick_behaviour(g, d, ctx, list) {
        Some(p) => {
            if ctx.slot(g).program != p as i8 {
                let s = ctx.slot_mut(g);
                s.program = p as i8;
                s.step = 0;
            }
        }
        None => {
            let s = ctx.slot_mut(g);
            s.program = 0x11;
            s.step = 0;
        }
    }
    run_program(g, d, ctx);
}

/// Run one program row's opcode (tests only).
#[cfg(test)]
pub(super) fn run_row(g: &mut GameState, d: &CreatureData, ctx: &Ctx, program: u8, step: i8) -> Option<Res> {
    let row = d.row(program, step)?;
    Some(opcode(g, d, ctx, &row))
}

/// Run program steps until one queues an action (0x261B3 / 0x27CD2 /
/// next-step rules at 0x26249).
pub fn run_program(g: &mut GameState, d: &CreatureData, ctx: &Ctx) {
    for _ in 0..MAX_STEPS {
        let (prog, step) = (ctx.slot(g).program, ctx.slot(g).step);
        if prog < 0 {
            return;
        }
        let Some(row) = d.row(prog as u8, step) else {
            ctx.slot_mut(g).program = -1;
            return;
        };
        if row.op < 0 {
            // Inline directive: −10 sets program variable (byte 1) to byte 2.
            if row.op == -10 {
                let i = (row.on_done & 1) as usize;
                ctx.slot_mut(g).vars[i] = row.on_other as u8 as u16;
            }
            ctx.slot_mut(g).step = step + 1;
            continue;
        }
        let res = opcode(g, d, ctx, &row);
        if res == Res::InProgress {
            return;
        }
        let next = if res == Res::Done { row.on_done } else { row.on_other };
        let s = ctx.slot_mut(g);
        match next {
            n if n >= 0 => s.step = n,
            -2 | -3 => {
                s.program = -1;
                s.step = 0;
            }
            -5 => s.step = step + 1,
            -6 => s.step = (step - 1).max(0),
            -7 => {}
            -8 => s.step = step + 2,
            _ => {
                s.program = -1;
                s.step = 0;
            }
        }
        if ctx.slot(g).action != NO_ACTION {
            return;
        }
    }
    set_action(g, ctx, action::IDLE);
}

/// Does creature `c` carry an item of `kind` (0x2FF1E over its
/// possessions)? Negative kinds (−1, −2) mean "none" in programs.
fn possession_matches(g: &GameState, c: ThingRef, kind: i8) -> bool {
    kind >= 0 && kinds::possession_of_kind(g, c, c, kind as u8).is_some()
}

pub(super) fn square_ahead(g: &GameState, ctx: &Ctx, n: i32) -> (i32, i32) {
    let f = facing(g, ctx.thing) as usize;
    (ctx.x + DX[f] * n, ctx.y + DY[f] * n)
}

/// Execute one opcode (dispatch at 0x27CD2).
fn opcode(g: &mut GameState, d: &CreatureData, ctx: &Ctx, row: &Row) -> Res {
    let op = row.op as u8;
    let (a3, a4) = (row.arg3, row.arg4);
    let mode: u8 = if ctx.cflags & 0x40 != 0 { 0 } else if ctx.cflags & 0x20 == 0 { 5 } else { 4 };
    match op {
        b'?' => {
            // Step ahead (0x27F28): in progress if the move starts; when the
            // way is blocked the original reports "done" (−2), not "failed",
            // so the program takes its done jump (usually the next row).
            let f = facing(g, ctx.thing);
            if move_test_step(g, ctx, f, mode) { Res::InProgress } else { Res::Done }
        }
        b'@' => {
            // Step to a side, else turn toward one (0x27F6E). The handler
            // hands back the raw value of the move or turn routine, never
            // the in-progress code 0xFC, so the program always takes the
            // row's "other" jump while the queued move or turn carries on.
            let f = facing(g, ctx.thing);
            let first = if g.rng.bit() != 0 { 1 } else { 3 };
            for t in [first, 4 - first] {
                if move_test_step(g, ctx, (f + t) & 3, mode) {
                    return Res::Failed;
                }
            }
            queue_turn(g, ctx, (f + first) & 3);
            Res::Failed
        }
        b'A' => {
            set_action(g, ctx, ACTION_DIE);
            Res::InProgress
        }
        b'B' | b'S' | b'X' | b'`' => {
            // Interact with the target square; for now: attack the party
            // when it stands there and is adjacent.
            let t = ctx.slot(g).target;
            let adjacent = (t.x() - ctx.x).abs() + (t.y() - ctx.y).abs() == 1;
            if adjacent && party_here(g, ctx.map, t.x(), t.y()) {
                let dir = direction_toward(ctx.x, ctx.y, t.x(), t.y());
                if move_test(g, ctx, dir, mode) { Res::InProgress } else { Res::Failed }
            } else {
                Res::Failed
            }
        }
        b'C' => {
            set_action(g, ctx, action::IDLE);
            Res::InProgress
        }
        b'E' => {
            // Pick up the first item on the square ahead into possessions.
            if ctx.info.item_flags() & 8 == 0 {
                return Res::Failed;
            }
            let (ax, ay) = square_ahead(g, ctx, 1);
            let item = g.dungeon.things_at(ctx.map, ax, ay).into_iter().find(|t| {
                matches!(t.kind(), ThingType::Weapon | ThingType::Clothing | ThingType::Scroll | ThingType::Potion | ThingType::Container | ThingType::Misc)
            });
            match item {
                Some(it) => {
                    g.dungeon.remove_thing(ctx.map, ax, ay, it);
                    let head = rec_u16(g, ctx.thing, 2);
                    g.dungeon.set_record_word(it, 0, head);
                    set_rec_u16(g, ctx.thing, 2, it.0);
                    Res::Done
                }
                None => Res::Failed,
            }
        }
        b'G' => {
            // Drop the first possession on the creature's square.
            let first = ThingRef(rec_u16(g, ctx.thing, 2));
            if !first.is_thing() || ctx.info.item_flags() & 8 == 0 {
                return Res::Failed;
            }
            let next = g.dungeon.record_word(first, 0).unwrap_or(ThingRef::END.0);
            set_rec_u16(g, ctx.thing, 2, next);
            g.dungeon.add_thing(ctx.map, ctx.x, ctx.y, first);
            Res::Done
        }
        b'H' => {
            let (x2, y2) = square_ahead(g, ctx, 2);
            let other = group_at(g, ctx.map, x2, y2).is_some_and(|o| {
                type_info(g, d, creature_type(g, o)).is_some_and(|(i, _)| i.raw[0] & 1 == 0)
            });
            if other || party_here(g, ctx.map, x2, y2) {
                Res::Done
            } else {
                set_action(g, ctx, action::WAIT_ROW);
                Res::InProgress
            }
        }
        b'I' => {
            let mut c = merchant::Counters { countdown: ctx.slot(g).vars[0] as u32, ..Default::default() };
            let (ax, ay) = square_ahead(g, ctx, 1);
            let customer = party_here(g, ctx.map, ax, ay) || group_at(g, ctx.map, ax, ay).is_some();
            // Money detection needs the per-creature kind table (TODO); treat
            // a customer ahead as holding money.
            let o = merchant::wait(&mut c, false, customer, &mut g.rng);
            ctx.slot_mut(g).vars[0] = c.countdown as u16;
            merchant_result(g, ctx, o)
        }
        b'F' => ops::op_f(g, ctx, a3, a4),
        b'J' => ops::op_j(g, ctx),
        b'K' => ops::op_k(g, ctx),
        b'M' => ops::op_m(g, ctx, a3),
        b'N' => ops::op_n(g, ctx, a3, a4),
        b'W' => ops::op_w(g, ctx, a3),
        b'Y' => ops::op_y(g, ctx, a3),
        b'[' => ops::op_open_bracket(g, ctx),
        b'\\' => ops::op_backslash(g, ctx),
        b']' => ops::op_close_bracket(g, ctx, a3),
        b'L' => {
            let (ax, ay) = square_ahead(g, ctx, 1);
            let s = ctx.slot_mut(g);
            s.target = Packed::new(ctx.map, ax, ay);
            s.action = if a3 == 1 { 0x16 } else { 0x15 };
            Res::InProgress
        }
        b'O' | b'^' => {
            if a3 >= 0 {
                set_action(g, ctx, a3 as u8);
                Res::InProgress
            } else {
                Res::Failed
            }
        }
        b'P' => {
            let bit = 1u16 << (a3 as u16 & 15);
            let st = status(g, ctx.thing);
            let want = match a4 & 15 {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            };
            match want {
                None => {
                    if st & bit != 0 { Res::Done } else { Res::Failed }
                }
                Some(on) if (st & bit != 0) == on => Res::Done,
                Some(on) => {
                    set_rec_u16(g, ctx.thing, 0x0A, if on { st | bit } else { st & !bit });
                    if a4 & 0x10 == 0 {
                        set_action(g, ctx, action::FLAG_CHANGE);
                        Res::InProgress
                    } else {
                        Res::Done
                    }
                }
            }
        }
        b'R' => act_on_target(g, ctx),
        b'Q' => {
            // 0x2923E. A target on the creature's own square is done with no
            // draw (the draw logs show none there, though the code reads as if
            // the roll came first). Otherwise the chance roll: the alertness
            // word's top nibble, quartered while status bit 0x2000 is set; a
            // roll below it sets a flag.
            let t = ctx.slot(g).target;
            if t.map() != ctx.map || (t.x(), t.y()) == (ctx.x, ctx.y) {
                return Res::Done;
            }
            let flag = {
                let mut chance = (ctx.info.alertness_word() >> 12) as u16;
                if status(g, ctx.thing) & 0x2000 != 0 {
                    chance >>= 2;
                }
                chance != 0 && (g.rng.rnd() & 15) < chance as u32
            };
            // With no path to follow (0x33E61 returns 0: seen for a target on
            // the next square that holds the party, which cannot be entered;
            // an empty target square is approached along a path instead, as
            // the pit run's draws show) the creature only faces the target: done
            // once it faces it; otherwise, with the flag and a random bit, it
            // idles (0xFC, tentatively modelled as idle and stop); else a
            // quarter turn toward it (0x2C005).
            let occupied = party_here(g, t.map(), t.x(), t.y())
                || group_at(g, t.map(), t.x(), t.y()).is_some_and(|c| c.0 & 0x3FFF != ctx.thing.0 & 0x3FFF);
            if manhattan(ctx.x, ctx.y, t.x(), t.y()) == 1 && occupied {
                let dir = direction_toward_rand(ctx.x, ctx.y, t.x(), t.y(), &mut g.rng);
                if dir == facing(g, ctx.thing) {
                    return Res::Done;
                }
                if flag && g.rng.bit() != 0 {
                    set_action(g, ctx, 0);
                    return Res::InProgress;
                }
                queue_turn(g, ctx, dir);
                return Res::InProgress;
            }
            if flag {
                return Res::Failed;
            }
            let dir = planner::first_step(g, &searcher(ctx), t.x(), t.y(), planner::DEFAULT_LIMIT + 4)
                .unwrap_or_else(|| direction_toward(ctx.x, ctx.y, t.x(), t.y()));
            if move_test(g, ctx, dir, mode) { Res::InProgress } else { Res::Failed }
        }
        b'T' => {
            let list = select_set(g, d, ctx);
            match pick_behaviour(g, d, ctx, list) {
                Some(_) => Res::Done,
                None => Res::Failed,
            }
        }
        b'U' => {
            if g.party.map != ctx.map {
                return Res::Failed;
            }
            let dir = direction_toward(ctx.x, ctx.y, g.party.x, g.party.y);
            let (nx, ny) = (ctx.x + DX[dir as usize], ctx.y + DY[dir as usize]);
            if g.dungeon.square(ctx.map, nx, ny).element() == Element::Wall {
                return Res::Failed;
            }
            if move_test(g, ctx, dir, mode) || queue_turn(g, ctx, dir) { Res::InProgress } else { Res::Failed }
        }
        b'V' => {
            let a = if g.rng.bit() != 0 { action::LOOK_A } else { action::LOOK_B };
            let dir = g.rng.rand4() as u8;
            let s = ctx.slot_mut(g);
            s.turn_to = dir;
            s.action = a;
            Res::InProgress
        }
        b'Z' => {
            if status(g, ctx.thing) & 0x80 != 0 || a3 != 0 {
                set_action(g, ctx, 0x23 + (a3.max(0) as u8).min(2));
                Res::InProgress
            } else {
                Res::Failed
            }
        }
        b'a' => {
            if (g.rng.random(100) as i32) < a3 as i32 { Res::Done } else { Res::Failed }
        }
        b'b' => {
            if possession_matches(g, ctx.thing, a3) || possession_matches(g, ctx.thing, a4) {
                Res::Done
            } else {
                Res::Failed
            }
        }
        _ => Res::Failed,
    }
}

fn merchant_result(g: &mut GameState, ctx: &Ctx, o: merchant::Outcome) -> Res {
    if let Some(a) = o.action {
        set_action(g, ctx, a);
        return Res::InProgress;
    }
    if o.done { Res::Done } else { Res::Failed }
}

// ---------------------------------------------------------------------------
// Frame events (0x2B75E)

/// Run the gameplay event of the current frame (0x2B75E). Returns the
/// handler's result as the original does: 0 on success (the action tick is
/// then recorded when the action's flags ask for it), non-zero on failure.
/// The caller ORs it into the slot's armed byte (+0x21), so a failed event
/// lets the sequence chain on to its next frame.
pub fn frame_event(g: &mut GameState, d: &CreatureData, ctx: &Ctx) -> u8 {
    let a = ctx.slot(g).action;
    let failed = match a {
        // 0x29DE7: 1 when the movement test fails, else 0 (even if the
        // move itself is then blocked).
        action::WALK | action::WALK_NEAR | action::BACK_OFF => !move_frame(g, d, ctx),
        // 0x29F39: a three-phase turn-step driven by slot +0x1F. Phase 0
        // turns and succeeds, phase 1 moves, later phases return the
        // target's y bits left in DX (non-zero unless y is 0).
        action::STEP_LEFT | action::STEP_RIGHT => {
            let stage = ctx.slot(g).stage;
            let r = match stage {
                0 => {
                    let t = ctx.slot(g).turn_to;
                    set_facing(g, ctx.thing, t);
                    false
                }
                1 => !move_frame(g, d, ctx),
                _ => ctx.slot(g).target.y() != 0,
            };
            if let Some(s) = g.creature_slots.get_mut(ctx.si).and_then(|s| s.as_mut()) {
                s.stage = s.stage.wrapping_add(1);
            }
            r
        }
        // 0x2A357 leaves CX at the dispatcher's 0: success.
        action::TURN_LEFT | action::TURN_RIGHT | action::LOOK_A | action::LOOK_B => {
            let t = ctx.slot(g).turn_to;
            set_facing(g, ctx.thing, t);
            false
        }
        // 0x2A3B9: 0 when a blow is attempted, 1 otherwise.
        action::ATTACK | action::MOVE_ATTACK => !attack_frame(g, d, ctx),
        // 0x2B35D preserves CX: success.
        action::TRANSFORM => {
            transform(g, d, ctx);
            false
        }
        // 0x29F6B returns ESI, the caller's slot pointer: always non-zero.
        5 => true,
        // Actions without a handler leave CX at 0: success.
        _ => false,
    };
    if g.creature_slots.get(ctx.si).and_then(|s| s.as_ref()).is_none() {
        return 0;
    }
    if !failed && d.action_flags(a) & 3 != 0 {
        ctx.slot_mut(g).act_tick = g.tick as u8;
    }
    u8::from(failed)
}

fn find_thing(g: &GameState, c: ThingRef) -> Option<(usize, i32, i32)> {
    for (mi, m) in g.dungeon.maps.iter().enumerate() {
        for x in 0..m.width as i32 {
            for y in 0..m.height as i32 {
                if g.dungeon.square(mi, x, y).has_things()
                    && g.dungeon.things_at(mi, x, y).iter().any(|t| t.0 & 0x3FFF == c.0 & 0x3FFF)
                {
                    return Some((mi, x, y));
                }
            }
        }
    }
    None
}

/// Move the group to the slot's target square (0x29DE7).
fn move_frame(g: &mut GameState, d: &CreatureData, ctx: &Ctx) -> bool {
    let t = ctx.slot(g).target;
    if party_here(g, ctx.map, t.x(), t.y()) {
        return attack_frame(g, d, ctx);
    }
    let ok = group_at(g, ctx.map, t.x(), t.y()).is_none()
        && super::terrain::can_enter(g, ctx.map, t.x(), t.y(), ctx.info.terrain(), ctx.info.door_size().max(1));
    if !ok {
        return false;
    }
    let c = ctx.thing;
    movement::move_thing(g, c, Some((ctx.map, ctx.x, ctx.y)), Some((t.map(), t.x(), t.y())));
    let pos = if group_at(g, t.map(), t.x(), t.y()).is_some_and(|o| o.0 & 0x3FFF == c.0 & 0x3FFF) {
        Some((t.map(), t.x(), t.y()))
    } else {
        find_thing(g, c)
    };
    match pos {
        Some((m, x, y)) => ctx.slot_mut(g).pos = Packed::new(m, x, y),
        None => {
            // Gone (fell off the dungeon); free the slot.
            super::deactivate(g, ctx.si);
        }
    }
    true
}

/// Melee attack on the target square (0x2A3B9).
fn attack_frame(g: &mut GameState, d: &CreatureData, ctx: &Ctx) -> bool {
    let t = ctx.slot(g).target;
    let (tx, ty) = (t.x(), t.y());
    if party_here(g, ctx.map, tx, ty) {
        let living: Vec<usize> = (0..g.champions.len()).filter(|&i| g.champions[i].is_alive()).collect();
        if living.is_empty() {
            return false;
        }
        let flags = ctx.info.raw[0];
        let count = if flags & 8 == 0 {
            if ctx.info.jitter() & 0x20 == 0 { 1 } else { 2 }
        } else if flags & 0x10 == 0 {
            living.len()
        } else {
            g.rng.random(living.len() as u16) as usize + 1
        };
        let mut pool = living.clone();
        for _ in 0..count.min(living.len()) {
            let k = if flags & 0x10 != 0 { g.rng.random(pool.len() as u16) as usize } else { 0 };
            let idx = pool.remove(k.min(pool.len() - 1));
            fight::attack_champion(g, d, &ctx.info, idx);
            if pool.is_empty() {
                break;
            }
        }
        g.party_status.last_attacked = g.tick;
        return true;
    }
    if g.dungeon.square(ctx.map, tx, ty).element() == Element::Door {
        let s = ctx.info.attack() as u16;
        let dmg = g.rng.random(s + (s >> 1));
        return crate::doors::bash(g, ctx.map, tx, ty, dmg, 0, false);
    }
    if let Some(o) = group_at(g, ctx.map, tx, ty) {
        if let Some((oi, _)) = type_info(g, d, creature_type(g, o)) {
            if let Some(dmg) = fight::creature_vs_creature(&ctx.info, &oi, &mut g.rng) {
                let (f, ch) = super::hit_flags::CREATURE;
                super::hit(g, o, ctx.map, tx, ty, f, ch, dmg);
            }
            return true;
        }
    }
    false
}

/// Transform into the creature type in slot +0x1E (0x2B35D).
pub(super) fn transform(g: &mut GameState, d: &CreatureData, ctx: &Ctx) -> bool {
    let to = ctx.slot(g).arg;
    // The original ends with sound (3, 0, 0x81) when the change happens and
    // (3, 0, 0x8B) when it doesn't, at the creature's square.
    let sound = |g: &mut GameState, sub: u8| {
        g.effects.push(Effect::Sound { cat: 3, idx: 0, sub, map: ctx.map, x: ctx.x, y: ctx.y });
    };
    let Some((info, _)) = type_info(g, d, to) else {
        sound(g, 0x8B);
        return false;
    };
    let base = info.base_hp();
    let h = base + g.rng.random(base / 8 + 1);
    super::set_rec_u8(g, ctx.thing, 4, to);
    set_rec_u16(g, ctx.thing, 6, h.max(1));
    ctx.slot_mut(g).queued = action::TRANSFORM_END;
    sound(g, 0x81);
    true
}

// ---------------------------------------------------------------------------
// Frame timing (0x3023F)

/// Delay before the next step, after playing the frame's sound and
/// re-rolling jitter and flip.
pub fn frame_delay(g: &mut GameState, ctx: &Ctx, an: &Anim) -> u16 {
    frame_delay_ex(g, ctx, an, true)
}

/// `frame_delay` with the off-map slowdown branch optionally disabled. At
/// activation (0x306A8 → 0x3023F) the original never takes that branch:
/// its draw log shows the extra-tick and jitter draws there but no
/// off-map bit, which appears on every later step.
pub fn frame_delay_ex(g: &mut GameState, ctx: &Ctx, an: &Anim, allow_off_map: bool) -> u16 {
    crate::rng::trace_tag("frame", (ctx.thing.0 & 0x3FF) as u32);
    let s = ctx.slot(g).clone();
    crate::rng::trace_frame(s.action as u32, s.seq_start as u32, s.seq_off as u32);
    let off = if s.seq_off == NO_FRAME { 0 } else { s.seq_off };
    let f = an.frame(s.seq_start, off);
    let mut jit = s.jitter;
    if f.jitter() && !matches!(s.action, 0x23..=0x25) {
        jit &= 0xC0;
        let j = ctx.info.jitter();
        for (shift, bits) in [(0u8, j & 3), (3u8, (j >> 2) & 3)] {
            if bits != 0 {
                let mut v = g.rng.random(bits as u16 + 1) as u8;
                if g.rng.bit() != 0 {
                    v = v.wrapping_neg() & 7;
                }
                jit |= (v & 7) << shift;
            }
        }
    }
    if f.random_flip() {
        if g.rng.bit() != 0 {
            jit |= 0x40;
        } else {
            jit &= !0x40;
        }
    }
    ctx.slot_mut(g).jitter = jit;
    if f.sound() != 0x7F {
        // Frame sounds are played at volume 0x80 (0x3023F).
        g.effects.push(Effect::SoundAt { vol: 0x80, mode: 1, cat: 15, idx: ctx.ty, sub: f.sound(), map: ctx.map, x: ctx.x, y: ctx.y });
    }
    let st = status(g, ctx.thing);
    if st & 0x40 != 0 {
        return f.base_ticks().min(1).max(1);
    }
    let extra = if f.extra_ticks() != 0 { g.rng.random(f.extra_ticks()) } else { 0 };
    let mut delay = extra + f.base_ticks();
    // 0x3023F reads the class flags through the loaded class index, which is
    // -1 unless this event loaded the context (think or a frame event), so a
    // plain frame step sees the entry before the table. In the shipped data
    // that entry has the bit set, so plain steps never take the off-map
    // slowdown: only the frames that loaded the context run four times slower.
    let cflags = if g.creature_class_loaded {
        ctx.cflags
    } else {
        g.creature_data.as_ref().map_or(ctx.cflags, |d| d.class_flags_unloaded())
    };
    let off_map = allow_off_map && ctx.map != g.party.map && cflags >> 16 & 1 == 0;
    if s.action == ACTION_DIE && g.party_status.counter_0b != 0 && ctx.info.flags1() & 0x10 == 0 {
        delay *= 3;
    } else if off_map && st & 0x8000 != 0 && st & 2 == 0 {
        // 0x3023F: away from the party's map, a creature in this state runs
        // four times slower plus a random tick.
        delay = delay * 4 + g.rng.bit();
    } else if g.party_status.asleep {
        delay *= if ctx.map != g.party.map { 4 } else { 2 };
    } else if st & 8 != 0 {
        if st & 0x4000 == 0 || delay > 2 {
            delay = delay * 75 / 100;
        }
        delay = delay.max(1);
    }
    // A zero delay would re-run the creature in the same tick forever.
    delay.max(1)
}

#[cfg(test)]
mod tests {

    /// 0x2FD7B: the n-th set bit of the mask, counting from 1.
    #[test]
    fn nth_bit_counts_set_bits_from_one() {
        assert_eq!(super::nth_bit(0b1011_0010, 1), 0b10);
        assert_eq!(super::nth_bit(0b1011_0010, 2), 0b1_0000);
        assert_eq!(super::nth_bit(0b1011_0010, 4), 0b1000_0000);
        assert_eq!(super::nth_bit(0b1011_0010, 5), 0);
        assert_eq!(super::nth_bit(0, 1), 0);
    }

    use super::*;

    #[test]
    fn randomised_direction_breaks_diagonal_ties_with_one_bit() {
        use crate::rng::Rng;
        // Off the diagonal no draw is made, and the longer axis wins.
        let mut r = Rng::new(1);
        assert_eq!(direction_toward_rand(5, 5, 9, 6, &mut r), 1);
        assert_eq!(direction_toward_rand(5, 9, 6, 5, &mut r), 0);
        assert_eq!(r.state, 1, "no draw off the diagonal");
        // On a diagonal one bit decides: set picks the x axis.
        for seed in 0..16u32 {
            let mut a = Rng::new(seed);
            let mut b = Rng::new(seed);
            let x_axis = b.bit() != 0;
            let d = direction_toward_rand(5, 5, 8, 8, &mut a);
            assert_eq!(d, if x_axis { 1 } else { 2 });
            assert_eq!(a.state, b.state, "exactly one draw on a tie");
        }
    }

    #[test]
    fn direction_toward_prefers_the_longer_axis() {
        assert_eq!(direction_toward(5, 5, 9, 6), 1);
        assert_eq!(direction_toward(5, 5, 1, 5), 3);
        assert_eq!(direction_toward(5, 5, 5, 9), 2);
        assert_eq!(direction_toward(5, 5, 6, 1), 0);
    }

    #[test]
    fn hp_helper_reads_record() {
        // Exercised against real data in creatures::tests.
        let _ = hp;
    }
}


/// Opcode `R` (0x27E28): commit to acting on the slot's target from where the
/// creature stands. The goal's mode byte becomes the slot argument, the
/// attack mask is the creature's ANDed with the goal's value word, and the
/// path test runs in committing mode with move flags 2 for goal type 8, 3 for
/// type 9 and 0 otherwise. Returns the test's code: 0xFC (in progress) once
/// committed, 0xFD (failed) when a filter refuses.
/// Not modelled: the distance analysis 0x26A67, which can clear mask bit 8.
fn act_on_target(g: &mut GameState, ctx: &Ctx) -> Res {
    let s = ctx.slot(g);
    let flags = match s.goal_kind {
        8 => 2,
        9 => 3,
        _ => 0,
    };
    let (mode, value, t) = (s.kind_a, s.kind_b, s.target);
    ctx.slot_mut(g).arg = mode as u8;
    let sr = searcher(ctx);
    let (tx, ty) = (t.x(), t.y());
    // The distance analysis (0x26A67) counts what the creature carries of
    // the behaviour's item kinds; with nothing to throw, the throw attack
    // (mask value 8) is dropped before the goal value narrows the mask.
    let mut mask = sr.attack_mask;
    if mask & 8 != 0 && carried_for_goal(g, ctx) < 1 {
        mask &= !8;
    }
    let Some(ok) = planner::path_filter(g, &sr, mask & value, flags, ctx.map, ctx.x, ctx.y, tx, ty) else {
        return Res::Failed;
    };
    commit_attack(g, ctx, flags, tx, ty, ok, None)
}

/// R's distance analysis (0x26A67 with the chosen goal's data and tag).
fn carried_for_goal(g: &GameState, ctx: &Ctx) -> i32 {
    let s = ctx.slot(g);
    let (data, tag) = (s.goal_data, s.goal_tag);
    match g.creature_data.clone() {
        Some(d) => goals::carried_count(g, &d, ctx.thing, data, tag),
        None => 0,
    }
}

/// The committing half of the path test (0x2C898-0x2CC1A). Face the target
/// (a queued quarter turn counts as committed); then a coin `b` and, at
/// distance 1 or less with melee bits (0-2) in the mask, a melee strike
/// unless ranged bits are also present and a second coin says otherwise:
/// - melee: the mask keeps bits 0-2; with move flags 0 or 1 the cell is that
///   of the champion the creature faces (0x45938), else (dir + 2 + b) & 3;
/// - ranged: the mask keeps bits 3-11; with move flags 0 or 1, three times in
///   four a new coin picks the near or far side, falling back to the other
///   side when no champion stands there (0x458F4); the cell is (dir + b) & 3.
/// The attack is the n-th set bit of the mask for n = random(count) + 1.
fn commit_attack(g: &mut GameState, ctx: &Ctx, flags: u8, tx: i32, ty: i32, ok: planner::PathOk, dir: Option<u8>) -> Res {
    let (x, y) = (ctx.x, ctx.y);
    let dir = match dir {
        Some(d) => d,
        None if ok.d == 0 && ctx.map == g.party.map && (tx, ty) == (g.party.x, g.party.y) => (g.party.dir + 2) & 3,
        None => direction_toward_rand(x, y, tx, ty, &mut g.rng),
    };
    if queue_turn(g, ctx, dir) {
        return Res::InProgress;
    }
    let mut b = g.rng.bit() as u8;
    let mut mask = ok.mask;
    let melee = ok.d <= 1 && mask & 7 != 0 && (mask & 0xFF8 == 0 || g.rng.bit() != 0);
    let cell = if melee {
        mask &= 7;
        let hit = if flags <= 1 { champion_facing(g, x, y) } else { None };
        match hit {
            Some(c) => g.champions[c].raw[0x1D],
            None => (b + 2 + dir) & 3,
        }
    } else {
        mask &= 0xFF8;
        if flags <= 1 && g.rng.rand4() != 0 {
            b = g.rng.bit() as u8;
            let mut c = if b != 0 { (dir + 2) & 3 } else { dir };
            if champion_in_cell(g, c).is_none() {
                c = (c + 3) & 3;
                if champion_in_cell(g, c).is_none() {
                    b = 1 - b;
                }
            }
        }
        (dir + b) & 3
    };
    let n = mask.count_ones() as u16;
    let k = g.rng.random(n) + 1;
    let bit = nth_bit(mask, k);
    let s = ctx.slot_mut(g);
    let missile = match bit {
        1 => {
            s.action = 8;
            None
        }
        2 => {
            s.action = 0x26;
            None
        }
        4 => {
            s.action = 0x0A;
            s.arg = 0x0B;
            None
        }
        8 => {
            s.action = 0x0E + b;
            None
        }
        0x10 => Some(0x80),
        0x20 => Some(0x83),
        0x40 => Some(0x82),
        0x80 => Some(0x87),
        0x100 => Some(0x86),
        0x200 => Some(0x81),
        0x400 => Some(0x89),
        0x800 => Some(0x8A),
        _ => None,
    };
    if let Some(m) = missile {
        s.arg = m;
        s.action = 0x27 + b;
    }
    s.target = Packed::new(ctx.map, tx, ty);
    s.dir_arg = dir;
    s.cell_arg = if s.action == 0x0A { ok.steal_cell } else { cell };
    s.mode = flags;
    Res::InProgress
}

/// The first living champion standing in `cell`, in party order (0x458F4).
fn champion_in_cell(g: &GameState, cell: u8) -> Option<usize> {
    g.champions.iter().position(|c| c.raw[0x1D] == cell && c.is_alive())
}

/// The champion a creature at (x, y) next to the party strikes (0x45938
/// with 0xFF): the party's four cells ordered from the side toward the
/// creature (four bytes of the table at 0x716EC from the direction, 0x1869A),
/// each pair swapped on a coin flip, then the first holding a champion.
fn champion_facing(g: &mut GameState, x: i32, y: i32) -> Option<usize> {
    if g.champions.is_empty() {
        return None;
    }
    let (px, py) = (g.party.x, g.party.y);
    if (x - px).abs() + (y - py).abs() >= 2 {
        return None;
    }
    let dir = direction_toward_rand(px, py, x, y, &mut g.rng);
    let data = g.data.clone()?;
    let mut cells = [0u8; 4];
    for (k, c) in cells.iter_mut().enumerate() {
        *c = data.exe.u8_at(CELL_ORDER_RANDOM + dir as u32 + k as u32)?;
    }
    if g.rng.bit() != 0 {
        cells.swap(0, 1);
    }
    if g.rng.bit() != 0 {
        cells.swap(2, 3);
    }
    cells.iter().find_map(|&c| champion_in_cell(g, c))
}

/// Table of party cells by direction for 0x1869A's random order.
const CELL_ORDER_RANDOM: u32 = 0x716EC;

/// The n-th set bit of `mask`, counting from 1 (0x2FD7B); 0 if there is none.
fn nth_bit(mask: u16, n: u16) -> u16 {
    let mut left = n;
    for k in 0..16 {
        let bit = 1u16 << k;
        if mask & bit != 0 {
            left -= 1;
            if left == 0 {
                return bit;
            }
        }
    }
    0
}
