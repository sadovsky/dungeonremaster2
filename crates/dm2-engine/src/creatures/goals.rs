//! Goal building for the planner (docs/08 "Goal building"; 0x25EF0,
//! dispatch 0x26873 through the table at 0x75248, general builder 0x2759E,
//! goal record writer 0x26896, condition test 0x26D16).
//!
//! A behaviour entry points at *goal data*: a list of 14-byte target specs.
//! The row's goal byte picks a builder; each builder selects the specs
//! carrying its tag (builder 1 uses the row's argument as the tag) whose
//! condition holds, and turns them into planner goals.

use dm2_formats::dungeon::ThingRef;

use crate::state::GameState;

use super::data::CreatureData;
use super::planner::Goal;
use super::slot::Packed;
use super::{creature_type, hp, rec_u16, status, Ctx};

/// One 14-byte target spec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    /// +0: planner goal type.
    pub ty: u8,
    /// +1: condition code (bits 0-5), 0x40 "already running", 0x80 invert.
    pub cond: u8,
    /// +2: condition parameter.
    pub cparam: u16,
    /// +4: goal mode / first argument.
    pub mode: u16,
    /// +6: goal value / second argument (ANDed with the slot's goal mask).
    pub value: u16,
    /// +8: distance limit.
    pub limit: i8,
    /// +9: stored with the goal (+1 of the record).
    pub extra: u8,
    /// +0x0C: tag the builders select on.
    pub tag: u8,
}

impl Spec {
    fn from(b: &[u8]) -> Spec {
        let w = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        Spec { ty: b[0], cond: b[1], cparam: w(2), mode: w(4), value: w(6), limit: b[8] as i8, extra: b[9], tag: b[12] }
    }
}

/// The fixed spec builder 0 uses (0x731D2): "stay here".
const FIXED_SPEC: u32 = 0x731D2;

/// Parse a goal data list (byte +0x0D non-zero = another spec follows).
pub fn parse_specs(d: &CreatureData, addr: u32) -> Vec<Spec> {
    let mut out = Vec::new();
    let mut a = addr;
    for _ in 0..32 {
        let Some(b) = d.bytes_at(a, 14) else { break };
        out.push(Spec::from(b));
        if b[13] == 0 {
            break;
        }
        a += 14;
    }
    out
}

/// Tag each builder selects (0 = builder 0's fixed goal, None = special or
/// unused). Builder 1 uses the row's argument instead.
pub fn builder_tag(builder: u8) -> Option<u8> {
    Some(match builder {
        2 | 6 => 2,
        3 | 7 => 4,
        4 => 1,
        5 => 3,
        8 => 5,
        9 => 6,
        0x0A => 7,
        0x0B => 0x12,
        0x0C => 0x0F,
        0x0D => 0x10,
        0x0F => 0x15,
        0x10 => 0x16,
        _ => return None,
    })
}

/// The distance analysis's carried count (0x26A67, byte +5 of its entry):
/// for each spec of goal data `data` with tag `tag` (at most six), unless
/// its goal type is 8 or its word +4 is 0xFFFF, count the creature's
/// possessions of kind word +4 (0x2697B: items of types 5-13, looking
/// inside containers, not inside creatures).
pub fn carried_count(g: &GameState, d: &CreatureData, who: ThingRef, data: u32, tag: u8) -> i32 {
    if data == 0 {
        return 0;
    }
    let first = ThingRef(rec_u16(g, who, 2));
    parse_specs(d, data)
        .iter()
        .filter(|sp| sp.tag == tag)
        .take(6)
        .filter(|sp| sp.ty != 8 && sp.mode != 0xFFFF)
        .map(|sp| count_of_kind(g, who, first, sp.mode as u8))
        .sum()
}

fn count_of_kind(g: &GameState, who: ThingRef, first: ThingRef, kind: u8) -> i32 {
    let mut n = 0;
    let mut t = first;
    for _ in 0..1024 {
        if !t.is_thing() {
            break;
        }
        let ty = t.kind() as u16;
        if t.kind() == dm2_formats::dungeon::ThingType::Container {
            n += count_of_kind(g, who, ThingRef(g.dungeon.record_word(t, 1).unwrap_or(ThingRef::END.0)), kind);
        }
        if (5..14).contains(&ty) && super::kinds::matches(g, who, t, kind) {
            n += 1;
        }
        t = ThingRef(g.dungeon.record_word(t, 0).unwrap_or(ThingRef::END.0));
    }
    n
}

/// The creature's post: the square packed in its thing record word +0x0C.
pub fn post(g: &GameState, c: ThingRef) -> Packed {
    Packed(rec_u16(g, c, 0x0C))
}

fn on_party_map(g: &GameState, ctx: &Ctx) -> bool {
    ctx.map == g.party.map
}

/// Condition test (0x26D16). `program` is the behaviour's program, for the
/// 0x40 "already running" shortcut.
pub fn condition(g: &GameState, ctx: &Ctx, code: u8, param: u16, program: u8) -> bool {
    if code & 0x40 != 0 && ctx.slot(g).program == program as i8 {
        return true;
    }
    let p = param as i32;
    let here = (ctx.x, ctx.y);
    let st = status(g, ctx.thing);
    let pos_ok = |pk: Packed| pk.map() == ctx.map && (pk.x(), pk.y()) == here;
    let r = match code & 0x3F {
        0 => true,
        // In front of the party (the party faces the creature's direction).
        1 | 0x16 => {
            on_party_map(g, ctx)
                && super::ai::direction_toward(g.party.x, g.party.y, ctx.x, ctx.y) == g.party.dir
                && (code & 0x3F == 1 || (g.party.x - ctx.x).abs() + (g.party.y - ctx.y).abs() <= p.max(1))
        }
        2 => on_party_map(g, ctx) && here == (g.party.x, g.party.y),
        3 => super::kinds::possession_of_kind(g, ctx.thing, ctx.thing, param as u8).is_some(),
        5 => pos_ok(post(g, ctx.thing)),
        0x0D => pos_ok(post(g, ctx.thing)) && super::kinds::possession_of_kind(g, ctx.thing, ctx.thing, param as u8).is_some(),
        6 => st & (1 << (param & 15)) != 0,
        7 => on_party_map(g, ctx),
        // A champion holds an item of kind 0x0B in either hand.
        8 => g.champions.iter().filter(|c| c.is_alive()).any(|c| {
            [c.inventory(1), c.inventory(0)].into_iter().any(|t| super::kinds::matches(g, ctx.thing, ThingRef(t), 0x0B))
        }),
        // Health at or below `param` percent of the type's base.
        0x0E => (hp(g, ctx.thing) as u32 * 100) / ctx.info.base_hp().max(1) as u32 <= param as u32,
        // Fewer creatures of type `param` on this map than min(4, n + 1),
        // n counting types 0x31 and 0x34.
        0x0F => {
            let (mut same, mut special) = (0, 0);
            let m = &g.dungeon.maps[ctx.map];
            for x in 0..m.width as i32 {
                for y in 0..m.height as i32 {
                    if let Some(o) = super::group_at(g, ctx.map, x, y) {
                        let ty = creature_type(g, o);
                        if ty as u16 == param {
                            same += 1;
                        } else if ty == 0x34 || ty == 0x31 {
                            special += 1;
                        }
                    }
                }
            }
            same < (special + 1).min(4)
        }
        0x10 => post(g, ctx.thing).map() == ctx.map,
        // The original compares with the map at 0x7F260 (taken here as the
        // party's map).
        0x12 => on_party_map(g, ctx),
        0x13 => st & (1 << (param & 15)) == 0 && !on_party_map(g, ctx),
        0x14 => st & (1 << (param & 15)) != 0 && on_party_map(g, ctx),
        0x15 => {
            let v = ctx.slot(g).vars.get(param as usize).copied().unwrap_or(0xFFFF);
            pos_ok(Packed(v))
        }
        // 4, 9, 0x0A-0x0C, 0x11: depend on state the engine doesn't track yet.
        _ => false,
    };
    r != (code & 0x80 != 0)
}

/// Turn a spec into a planner goal (0x26896): off the party's map, a class
/// without flag 0x40 searches only a quarter as far.
fn goal_from(g: &GameState, ctx: &Ctx, s: &Spec, program: u8, zero_limit: bool) -> Option<Goal> {
    let mut limit = if zero_limit { 0 } else { s.limit as i32 };
    if ctx.map != g.party.map && limit > 0 && ctx.cflags & 0x40 == 0 {
        limit >>= 2;
    }
    let limit = limit.clamp(-1, 0x7F);
    if limit < 0 {
        return None;
    }
    Some(Goal { kind: s.ty, arg: s.extra as i8, program, limit: limit as u8, data: 0, mode: s.mode, value: s.value, tag: s.tag })
}

/// Build the goals a behaviour entry contributes (0x25EF0 → 0x26873).
pub fn build(g: &GameState, d: &CreatureData, ctx: &Ctx, program: u8, builder: u8, arg: i8, data: u32) -> Vec<Goal> {
    if builder == 0 {
        let s = d.bytes_at(FIXED_SPEC, 14).map(Spec::from);
        return s.and_then(|s| goal_from(g, ctx, &s, program, false)).into_iter().collect();
    }
    if builder == 6 || builder == 7 {
        return attack_goals(g, d, ctx, program, builder, arg, data);
    }
    let (tag, zero_limit) = match builder {
        1 => (arg.unsigned_abs(), arg < 0),
        b => match builder_tag(b) {
            Some(t) => (t, false),
            None => return Vec::new(),
        },
    };
    if data == 0 {
        return Vec::new();
    }
    parse_specs(d, data)
        .iter()
        .filter(|s| s.tag == tag && condition(g, ctx, s.cond, s.cparam, program))
        .filter_map(|s| goal_from(g, ctx, s, program, zero_limit))
        .collect()
}

/// Builders 6 and 7 (0x277FB): attack goals. Nothing unless the creature
/// is alert this think (0x7F589) and its type has attack bits (info word
/// +0x0E). The distance analysis with tag 1 (builder 6) or 3 (builder 7)
/// drops the throw attack (value bit 8) from the goals' values when the
/// creature carries nothing of those specs' kinds; then the specs with tag 2
/// or 4 become goals, with a zero distance limit when the row's argument is
/// non-zero.
fn attack_goals(g: &GameState, d: &CreatureData, ctx: &Ctx, program: u8, builder: u8, arg: i8, data: u32) -> Vec<Goal> {
    if data == 0 || g.creature_alert_roll == 0 {
        return Vec::new();
    }
    let (analysis_tag, tag) = if builder == 6 { (1, 2) } else { (3, 4) };
    let mask = u16::from_le_bytes([ctx.info.raw[0x0E], ctx.info.raw[0x0F]]);
    let mut narrow = 0xFFFFu16;
    if mask & 8 != 0 && carried_count(g, d, ctx.thing, data, analysis_tag) <= 0 {
        narrow &= !8;
    }
    if mask == 0 {
        return Vec::new();
    }
    parse_specs(d, data)
        .iter()
        .filter(|s| s.tag == tag && condition(g, ctx, s.cond, s.cparam, program))
        .filter_map(|s| goal_from(g, ctx, s, program, arg != 0))
        .map(|mut gl| {
            gl.value &= narrow;
            gl
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_fields() {
        let s = Spec::from(&[7, 0x83, 2, 0, 1, 0, 0x0F, 0, 5, 9, 0, 0, 0x12, 1]);
        assert_eq!((s.ty, s.cond, s.cparam, s.mode, s.value, s.limit, s.extra, s.tag), (7, 0x83, 2, 1, 0x0F, 5, 9, 0x12));
    }

    #[test]
    fn builder_tags() {
        assert_eq!(builder_tag(2), Some(2));
        assert_eq!(builder_tag(3), Some(4));
        assert_eq!(builder_tag(0x10), Some(0x16));
        assert_eq!(builder_tag(0x0E), None);
        assert_eq!(builder_tag(1), None);
    }
}
