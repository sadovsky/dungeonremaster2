//! Possession, trade and scripted-action opcodes: `F`, `J`, `K`, `M`, `N`,
//! `W`, `Y`, `[`, `\` and `]` (docs/08 "Opcodes", "Merchants").

use dm2_formats::dungeon::{ThingRef, ThingType};
use dm2_formats::gdat::Key;

use crate::state::GameState;
use crate::viewport::{DX, DY};

use super::ai::{move_test, set_action, Res};
use super::kinds;
use super::merchant::{self, Counters};
use super::slot::Packed;
use super::{facing, group_at, rec_u16, set_rec_u16, Ctx};

/// Item kind sets the merchant opcodes use.
const KIND_COINS: u8 = 0x10;
const KIND_GEMS: u8 = 7;

fn ahead(g: &GameState, ctx: &Ctx) -> (i32, i32) {
    let f = facing(g, ctx.thing) as usize;
    (ctx.x + DX[f], ctx.y + DY[f])
}

fn group_ahead(g: &GameState, ctx: &Ctx) -> Option<ThingRef> {
    let (x, y) = ahead(g, ctx);
    group_at(g, ctx.map, x, y)
}

/// Possession cell seen from the creature: `rel` relative to its facing,
/// turned round (+2) because the possessions belong to the creature it faces.
fn facing_cell(g: &GameState, ctx: &Ctx, rel: u8) -> u8 {
    rel.wrapping_add(facing(g, ctx.thing)).wrapping_add(2) & 3
}

/// Unlink a thing from a creature's possession chain (0x1D542 on +2).
fn unlink_possession(g: &mut GameState, c: ThingRef, t: ThingRef) -> bool {
    let same = |a: ThingRef, b: ThingRef| a.0 & 0x3FFF == b.0 & 0x3FFF;
    let head = ThingRef(rec_u16(g, c, 2));
    let after = ThingRef(g.dungeon.record_word(t, 0).unwrap_or(ThingRef::END.0));
    if same(head, t) {
        set_rec_u16(g, c, 2, after.0);
        return true;
    }
    let mut cur = head;
    for _ in 0..1024 {
        if !cur.is_thing() {
            return false;
        }
        let next = ThingRef(g.dungeon.record_word(cur, 0).unwrap_or(ThingRef::END.0));
        if same(next, t) {
            g.dungeon.set_record_word(cur, 0, after.0);
            return true;
        }
        cur = next;
    }
    false
}

/// Put one possession down on the creature's own square, in the cell it
/// faces (0x2EA68 mode 0x81, reduced to the item transfer).
fn drop_possession(g: &mut GameState, ctx: &Ctx, t: ThingRef) {
    if unlink_possession(g, ctx.thing, t) {
        let cell = facing(g, ctx.thing) as u16;
        g.dungeon.add_thing(ctx.map, ctx.x, ctx.y, ThingRef((t.0 & 0x3FFF) | cell << 14));
    }
}

/// Destroy every possession of `kind` (0x2FF89). Money containers are
/// searched too; matching contents are destroyed with them.
fn destroy_kind(g: &mut GameState, ctx: &Ctx, kind: u8) {
    let mut t = ThingRef(rec_u16(g, ctx.thing, 2));
    for _ in 0..1024 {
        if !t.is_thing() {
            return;
        }
        let next = ThingRef(g.dungeon.record_word(t, 0).unwrap_or(ThingRef::END.0));
        if (5..14).contains(&(t.kind() as u16)) && kinds::matches(g, ctx.thing, t, kind) {
            unlink_possession(g, ctx.thing, t);
            g.dungeon.free_thing(t);
        }
        t = next;
    }
}

/// Coin denominations (0x154E0): misc items whose flags attribute has bit
/// 0x4000, valued by attribute 2, ascending. Built from the user's
/// GRAPHICS.DAT at runtime.
pub fn denominations(g: &GameState) -> Vec<u32> {
    let Some(d) = g.creature_data.as_ref() else { return Vec::new() };
    let mut v: Vec<u32> = (0u8..0x80)
        .filter(|&i| d.gdat.lookup(Key::new(21, i, 11, 0)).unwrap_or(0) & 0x4000 != 0)
        .map(|i| d.gdat.lookup(Key::new(21, i, 11, 2)).unwrap_or(0) as u32)
        .collect();
    v.sort_unstable();
    v
}

/// Value of the items of `kind` in cell `cell` of a possession chain
/// (0x286C8 → 0x1C8E5). The original prices each item through a
/// per-creature valuation (0x15737); this uses the item's value attribute.
/// None = nothing of that kind there (the original's 0xFFFF).
fn pile_value(g: &GameState, who: ThingRef, owner: ThingRef, kind: u8, cell: u8) -> Option<u32> {
    let db = g.data.as_ref().map(|d| d.item_db(&g.dungeon));
    let mut total: Option<u32> = None;
    let mut t = ThingRef(rec_u16(g, owner, 2));
    for _ in 0..1024 {
        if !t.is_thing() {
            break;
        }
        if t.cell() == cell && kinds::matches(g, who, t, kind) {
            let v = db.as_ref().map_or(0, |db| db.attr(t, crate::items::ATTR_VALUE) as u32);
            total = Some(total.unwrap_or(0) + v);
        }
        t = ThingRef(g.dungeon.record_word(t, 0).unwrap_or(ThingRef::END.0));
    }
    total
}

/// Anything in `cell` that is neither coins nor gems.
fn stray_goods(g: &GameState, who: ThingRef, owner: ThingRef, cell: u8) -> bool {
    let mut t = ThingRef(rec_u16(g, owner, 2));
    for _ in 0..1024 {
        if !t.is_thing() {
            return false;
        }
        if t.cell() == cell && !kinds::matches(g, who, t, KIND_COINS) && !kinds::matches(g, who, t, KIND_GEMS) {
            return true;
        }
        t = ThingRef(g.dungeon.record_word(t, 0).unwrap_or(ThingRef::END.0));
    }
    false
}

/// The merchant counters live in slot words +0x0C, +0x0E and +0x10.
fn counters(g: &GameState, ctx: &Ctx) -> Counters {
    let s = ctx.slot(g);
    Counters { owed: s.home.0 as u32, countdown: s.vars[0] as u32, last_offer: s.vars[1] as u32 }
}

fn store(g: &mut GameState, ctx: &Ctx, c: Counters) {
    let s = ctx.slot_mut(g);
    s.home = Packed(c.owed as u16);
    s.vars[0] = c.countdown as u16;
    s.vars[1] = c.last_offer as u16;
}

fn outcome(g: &mut GameState, ctx: &Ctx, o: merchant::Outcome) -> Res {
    if let Some(a) = o.action {
        set_action(g, ctx, a);
    }
    if o.done {
        Res::Done
    } else if o.failed {
        Res::Failed
    } else {
        Res::InProgress
    }
}

/// `F` (0x28344): does the group ahead carry kind `a3`, in the cell `a4`
/// relative to our facing (−1 = any cell)?
pub fn op_f(g: &mut GameState, ctx: &Ctx, a3: i8, a4: i8) -> Res {
    if a3 == -1 {
        return Res::Failed;
    }
    let cell = if a4 == -1 { 0xFF } else { facing_cell(g, ctx, a4 as u8) };
    let Some(o) = group_ahead(g, ctx) else { return Res::Failed };
    let first = ThingRef(rec_u16(g, o, 2));
    if kinds::find_in_chain(g, ctx.thing, first, a3 as u8, cell).is_some() { Res::Done } else { Res::Failed }
}

/// `M` (0x281F3): target the square ahead; if the group there holds kind
/// `a3` (default 0x3F), queue action 0x18 to take it.
pub fn op_m(g: &mut GameState, ctx: &Ctx, a3: i8) -> Res {
    let kind = if a3 < 0 { 0x3F } else { a3 as u8 };
    let (x, y) = ahead(g, ctx);
    let opposite = (facing(g, ctx.thing) + 2) & 3;
    {
        let s = ctx.slot_mut(g);
        s.target = Packed::new(ctx.map, x, y);
        s.cell_arg = opposite;
        s.arg = kind;
    }
    let Some(o) = group_at(g, ctx.map, x, y) else { return Res::Failed };
    if kinds::possession_of_kind(g, ctx.thing, o, kind).is_some() {
        set_action(g, ctx, 0x18);
        Res::InProgress
    } else {
        Res::Done
    }
}

/// `N` (0x2905A): destroy possessions of kind `a4` (−1 = behaviour default,
/// −2 = skip), then put one of kind `a3` (−1 = default) down here.
pub fn op_n(g: &mut GameState, ctx: &Ctx, a3: i8, a4: i8) -> Res {
    if a4 != -2 {
        let k = if a4 == -1 { ctx.slot(g).kind_b } else { a4 as u8 as u16 };
        if k != 0xFFFF && ThingRef(rec_u16(g, ctx.thing, 2)).is_thing() {
            destroy_kind(g, ctx, k as u8);
        }
    }
    if !ThingRef(rec_u16(g, ctx.thing, 2)).is_thing() {
        return Res::Failed;
    }
    let k = if a3 == -1 { ctx.slot(g).kind_a } else { a3 as u8 as u16 };
    if k == 0xFFFF {
        return Res::Done;
    }
    match kinds::possession_of_kind(g, ctx.thing, ctx.thing, k as u8) {
        Some(t) => {
            drop_possession(g, ctx, t);
            Res::Done
        }
        None => Res::Failed,
    }
}

/// `W` (0x294CA): movement test toward facing + `a3` (a non-zero turn uses
/// the back-off mode).
pub fn op_w(g: &mut GameState, ctx: &Ctx, a3: i8) -> Res {
    let dir = (facing(g, ctx.thing).wrapping_add(a3 as u8)) & 3;
    let mode = if a3 == 0 { 0 } else { 6 };
    if move_test(g, ctx, dir, mode) { Res::InProgress } else { Res::Failed }
}

/// `]` (0x29BE7): queue action 0x3D + `a3` with the behaviour's default
/// kinds as its mode and argument.
pub fn op_close_bracket(g: &mut GameState, ctx: &Ctx, a3: i8) -> Res {
    let s = ctx.slot_mut(g);
    s.mode = s.kind_a as u8;
    s.arg = s.kind_b as u8;
    s.action = 0x3Du8.wrapping_add(a3 as u8);
    Res::Done
}

/// `\` (0x29B16): a text thing on our square whose word 1 marks "transform
/// into creature type n" (bits 1-2 = 1, bits 11-15 = 1, type in bits 3-10)
/// makes the creature transform (action 0x3B). Otherwise action 0x33.
pub fn op_backslash(g: &mut GameState, ctx: &Ctx) -> Res {
    for t in g.dungeon.things_at(ctx.map, ctx.x, ctx.y) {
        let ty = t.kind() as u16;
        if ty > 3 {
            break;
        }
        if t.kind() == ThingType::Text {
            let w1 = g.dungeon.record_word(t, 1).unwrap_or(0);
            if w1 & 6 == 2 && (w1 >> 3) >> 8 == 1 {
                let s = ctx.slot_mut(g);
                s.arg = (w1 >> 3) as u8;
                s.action = super::ai::action::TRANSFORM;
                return Res::Done;
            }
        }
    }
    // The original also registers a creature event of kind 0x13 here
    // (0x24DB5); not modelled.
    set_action(g, ctx, super::ai::action::FLAG_CHANGE);
    Res::Failed
}

/// `[` (0x299BA): consume the first possession if it is still usable
/// (weapons and clothing: byte 2 bit 7 clear; potions: byte 3 bit 7 clear;
/// misc: byte 2 bit 7 clear). Otherwise, 1 in 8: put things down here and
/// remember this square in variable 0; else 1 in 16: forget it.
pub fn op_open_bracket(g: &mut GameState, ctx: &Ctx) -> Res {
    let first = ThingRef(rec_u16(g, ctx.thing, 2));
    if !first.is_thing() {
        return Res::Failed;
    }
    let rec = g.dungeon.record(first).map(|r| r.to_vec()).unwrap_or_default();
    let usable = match first.kind() {
        ThingType::Weapon | ThingType::Clothing | ThingType::Misc => rec.get(2).is_some_and(|b| b & 0x80 == 0),
        ThingType::Potion => rec.get(3).is_some_and(|b| b & 0x80 == 0),
        _ => false,
    };
    if usable {
        unlink_possession(g, ctx.thing, first);
        g.dungeon.free_thing(first);
        return Res::Done;
    }
    if g.rng.rnd() & 7 == 0 {
        drop_possession(g, ctx, first);
        ctx.slot_mut(g).vars[0] = Packed::new(ctx.map, ctx.x, ctx.y).0;
    } else if g.rng.rnd() & 15 == 0 {
        ctx.slot_mut(g).vars[0] = 0xFFFF;
    }
    Res::Failed
}

/// `Y` (0x296D5): deal with the group ahead (action 0x1D). With `a3` = 0
/// it records the value of the coins that group shows on our side in
/// variable 1. The payout modes (`a3` ≠ 0: converting money containers and
/// handing out change, 0x29598 / 0x15958) are not modelled yet.
pub fn op_y(g: &mut GameState, ctx: &Ctx, a3: i8) -> Res {
    let Some(o) = group_ahead(g, ctx) else { return Res::Failed };
    set_action(g, ctx, merchant::action::WAIT);
    if a3 != 0 {
        return Res::Done;
    }
    let cell = facing_cell(g, ctx, 0);
    let coins = pile_value(g, ctx.thing, o, KIND_COINS, cell);
    let other = kinds::possession_of_kind(g, ctx.thing, o, 0x3E);
    match (coins, other) {
        (None | Some(0), None) => Res::Failed,
        (c, _) => {
            ctx.slot_mut(g).vars[1] = c.unwrap_or(0) as u16;
            Res::Done
        }
    }
}

/// `J` (0x28711): haggle over the goods the group ahead offers. Money sits
/// in the cell facing us, goods in the cell behind it.
pub fn op_j(g: &mut GameState, ctx: &Ctx) -> Res {
    let Some(o) = group_ahead(g, ctx) else { return Res::Failed };
    let money_cell = facing_cell(g, ctx, 0);
    let goods_cell = (money_cell + 2) & 3;
    let offer = pile_value(g, ctx.thing, o, KIND_COINS, money_cell).unwrap_or(0)
        + pile_value(g, ctx.thing, o, KIND_GEMS, money_cell).unwrap_or(0);
    let goods = pile_value(g, ctx.thing, o, 0x3F, goods_cell).unwrap_or(0);
    let stray = stray_goods(g, ctx.thing, o, money_cell);
    let denoms = denominations(g);
    let mut c = counters(g, ctx);
    let out = merchant::haggle(&mut c, stray, goods, offer, &denoms, &mut g.rng);
    store(g, ctx, c);
    outcome(g, ctx, out)
}

/// `K` (0x28E99): settle a sale with the group ahead.
pub fn op_k(g: &mut GameState, ctx: &Ctx) -> Res {
    let Some(o) = group_ahead(g, ctx) else { return Res::Failed };
    let money_cell = facing_cell(g, ctx, 0);
    let goods_cell = (money_cell + 2) & 3;
    let paid = pile_value(g, ctx.thing, o, KIND_COINS, money_cell).unwrap_or(0)
        + pile_value(g, ctx.thing, o, KIND_GEMS, money_cell).unwrap_or(0);
    let goods = pile_value(g, ctx.thing, o, 0x3F, goods_cell).unwrap_or(0);
    let gems_goods = pile_value(g, ctx.thing, o, KIND_GEMS, goods_cell).unwrap_or(0);
    let stray = stray_goods(g, ctx.thing, o, money_cell);
    let denoms = denominations(g);
    let mut c = counters(g, ctx);
    let out = merchant::settle(&mut c, stray, goods, gems_goods, paid, &denoms);
    store(g, ctx, c);
    outcome(g, ctx, out)
}
