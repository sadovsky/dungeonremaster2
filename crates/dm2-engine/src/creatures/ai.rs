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
    }
}

pub(super) fn set_action(g: &mut GameState, ctx: &Ctx, a: u8) {
    ctx.slot_mut(g).action = a;
}

/// Queue a turn toward `dir` (0x2C005). Returns false if already facing it.
pub(super) fn queue_turn(g: &mut GameState, ctx: &Ctx, dir: u8) -> bool {
    let f = facing(g, ctx.thing);
    if dir == f {
        return false;
    }
    let a = if dir == (f + 1) & 3 {
        action::TURN_RIGHT
    } else if dir == (f + 3) & 3 {
        action::TURN_LEFT
    } else if g.rng.bit() != 0 {
        action::TURN_RIGHT
    } else {
        action::TURN_LEFT
    };
    let s = ctx.slot_mut(g);
    s.turn_to = dir;
    s.action = a;
    true
}

/// The movement test (0x2D792), reduced to its outcome: queue a move,
/// turn-step, turn or attack toward `dir`. Returns true if an action was
/// queued.
pub fn move_test(g: &mut GameState, ctx: &Ctx, dir: u8, mode: u8) -> bool {
    let (nx, ny) = (ctx.x + DX[dir as usize], ctx.y + DY[dir as usize]);
    let f = facing(g, ctx.thing);
    if party_here(g, ctx.map, nx, ny) {
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

/// Refresh status bit 3 ("badly hurt") and choose the behaviour set
/// (0x25962 / 0x259CC). Returns the behaviour list address.
fn select_set(g: &mut GameState, d: &CreatureData, ctx: &Ctx) -> u32 {
    let c = ctx.thing;
    let mut st = status(g, c);
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
    // A goal on another layer is approached through the stairs leading there.
    let (tx, ty) = if found.map != ctx.map { found.via.unwrap_or((found.x, found.y)) } else { (found.x, found.y) };
    ctx.slot_mut(g).target = Packed::new(ctx.map, tx, ty);
    Some(gl.program)
}

/// Think (0x262F7): choose and start the next action.
pub fn think(g: &mut GameState, d: &CreatureData, ctx: &Ctx) {
    let list = select_set(g, d, ctx);
    let mode: u8 = if ctx.cflags & 0x40 != 0 { 0 } else if ctx.cflags & 0x20 == 0 { 5 } else { 4 };
    if WANDER_LISTS.contains(&list) {
        // Cheap random wander.
        let r = g.rng.rnd() & 0x7F;
        if r < 4 {
            let dir = if r == 0 { (g.tick & 3) as u8 } else { g.rng.rand4() as u8 };
            if !move_test(g, ctx, dir, mode) {
                queue_turn(g, ctx, dir);
            }
        }
        return;
    }
    // A creature that can't stay where it is tries to step away.
    if mode != 0 && !super::terrain::can_enter(g, ctx.map, ctx.x, ctx.y, ctx.info.terrain(), ctx.info.door_size().max(1)) {
        let mut dir = if g.rng.bit() != 0 { (facing(g, ctx.thing) + 2) & 3 } else { g.rng.rand4() as u8 };
        let turn: u8 = if g.rng.bit() != 0 { 1 } else { 3 };
        for _ in 0..4 {
            if move_test(g, ctx, dir, mode | 0x80) {
                let s = ctx.slot_mut(g);
                s.program = -1;
                s.step = 0;
                return;
            }
            dir = (dir + turn) & 3;
        }
    }
    // Re-plan when idle, or now and then unless the class is single-minded.
    let replan = ctx.slot(g).program < 0 || (ctx.cflags & 1 == 0 && g.rng.random(4) == 0);
    if replan {
        match pick_behaviour(g, d, ctx, list) {
            Some(p) => {
                if ctx.slot(g).program != p as i8 {
                    let s = ctx.slot_mut(g);
                    s.program = p as i8;
                    s.step = 0;
                }
            }
            None if ctx.slot(g).program < 0 => {
                // Nothing matched: the default program (0x11).
                let s = ctx.slot_mut(g);
                s.program = 0x11;
                s.step = 0;
            }
            None => {}
        }
    }
    run_program(g, d, ctx);
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
            let f = facing(g, ctx.thing);
            if move_test(g, ctx, f, mode) { Res::InProgress } else { Res::Failed }
        }
        b'@' => {
            let f = facing(g, ctx.thing);
            let first = if g.rng.bit() != 0 { 1 } else { 3 };
            for t in [first, 4 - first] {
                if move_test(g, ctx, (f + t) & 3, mode) {
                    return Res::InProgress;
                }
            }
            if queue_turn(g, ctx, (f + first) & 3) { Res::InProgress } else { Res::Failed }
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
        b'Q' | b'R' => {
            let t = ctx.slot(g).target;
            if t.map() != ctx.map || (t.x(), t.y()) == (ctx.x, ctx.y) {
                return Res::Done;
            }
            if op == b'Q' {
                let mut chance = (ctx.info.alertness_word() >> 12) as u16;
                if status(g, ctx.thing) & 4 != 0 {
                    chance /= 4;
                }
                if chance != 0 && g.rng.random(16) < chance {
                    return Res::Failed;
                }
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

/// Run the gameplay event of the current frame. Returns the "armed" bits.
pub fn frame_event(g: &mut GameState, d: &CreatureData, ctx: &Ctx) -> u8 {
    let a = ctx.slot(g).action;
    let handled = match a {
        action::WALK | action::WALK_NEAR | action::BACK_OFF => move_frame(g, d, ctx),
        action::STEP_LEFT | action::STEP_RIGHT => {
            let t = ctx.slot(g).turn_to;
            set_facing(g, ctx.thing, t);
            move_frame(g, d, ctx)
        }
        action::TURN_LEFT | action::TURN_RIGHT | action::LOOK_A | action::LOOK_B => {
            let t = ctx.slot(g).turn_to;
            set_facing(g, ctx.thing, t);
            true
        }
        action::ATTACK | action::MOVE_ATTACK => attack_frame(g, d, ctx),
        action::TRANSFORM => transform(g, d, ctx),
        _ => true,
    };
    if handled && d.action_flags(a) & 3 != 0 {
        ctx.slot_mut(g).act_tick = g.tick as u8;
    }
    0
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
                super::damage(g, o, ctx.map, tx, ty, dmg);
            }
            return true;
        }
    }
    false
}

/// Transform into the creature type in slot +0x1E (0x2B35D).
fn transform(g: &mut GameState, d: &CreatureData, ctx: &Ctx) -> bool {
    let to = ctx.slot(g).arg;
    let Some((info, _)) = type_info(g, d, to) else { return false };
    let base = info.base_hp();
    let h = base + g.rng.random(base / 8 + 1);
    super::set_rec_u8(g, ctx.thing, 4, to);
    set_rec_u16(g, ctx.thing, 6, h.max(1));
    ctx.slot_mut(g).queued = action::TRANSFORM_END;
    true
}

// ---------------------------------------------------------------------------
// Frame timing (0x3023F)

/// Delay before the next step, after playing the frame's sound and
/// re-rolling jitter and flip.
pub fn frame_delay(g: &mut GameState, ctx: &Ctx, an: &Anim) -> u16 {
    let s = ctx.slot(g).clone();
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
        g.effects.push(Effect::Sound { cat: 15, idx: ctx.ty, sub: f.sound(), map: ctx.map, x: ctx.x, y: ctx.y });
    }
    let st = status(g, ctx.thing);
    if st & 0x40 != 0 {
        return f.base_ticks().min(1).max(1);
    }
    let extra = if f.extra_ticks() != 0 { g.rng.random(f.extra_ticks()) } else { 0 };
    let mut delay = extra + f.base_ticks();
    let off_map = ctx.map != g.party.map && ctx.cflags >> 16 & 1 == 0;
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
    use super::*;

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
