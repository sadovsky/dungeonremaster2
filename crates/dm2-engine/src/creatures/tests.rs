//! Creature tests on the user's own data (skipped when it is absent). They
//! check structure and determinism; no game values are written here.

use std::rc::Rc;

use dm2_formats::dungeon::{Dungeon, ThingType};
use dm2_formats::gdat::Gdat;

use crate::assets::default_data_dir;
use crate::attrs::Attributes;
use crate::exe_tables::default_exe_path;
use crate::state::GameState;

use super::data::CreatureData;
use super::*;

fn load() -> Option<(GameState, Rc<CreatureData>)> {
    let dir = default_data_dir();
    let gdat = Rc::new(Gdat::open(dir.join("GRAPHICS.DAT")).ok()?);
    let exe = std::fs::read(default_exe_path()).ok()?;
    let dg = Dungeon::parse(&std::fs::read(dir.join("DUNGEON.DAT")).ok()?).ok()?;
    let d = Rc::new(CreatureData::load(gdat.clone(), &exe).ok()?);
    let mut g = GameState::new_game(&dg);
    g.set_attributes(Attributes::from_gdat(&gdat));
    let mut c = crate::champions::Champion::default();
    c.set_health(200);
    c.set_max_health(200);
    g.champions.push(c);
    set_data(&mut g, d.clone());
    Some((g, d))
}

/// Every creature group on the dungeon's maps: (map, x, y, group).
fn groups(g: &GameState) -> Vec<(usize, i32, i32, ThingRef)> {
    let mut out = Vec::new();
    for (mi, m) in g.dungeon.maps.iter().enumerate() {
        for x in 0..m.width as i32 {
            for y in 0..m.height as i32 {
                if let Some(c) = group_at(g, mi, x, y) {
                    out.push((mi, x, y, c));
                }
            }
        }
    }
    out
}

#[test]
fn every_placed_creature_has_tables() {
    let Some((g, d)) = load() else { return };
    let all = groups(&g);
    assert!(!all.is_empty());
    for &(_, _, _, c) in &all {
        let ty = creature_type(&g, c);
        let (info, class) = type_info(&g, &d, ty).unwrap_or_else(|| panic!("type {ty} has no info record"));
        assert!(info.base_hp() > 0, "type {ty}");
        let sets = d.behaviour_sets(class);
        assert!(sets.last().is_some_and(|s| s.0 == 0), "class {class} sets end with a zero mask");
        for &(_, list) in &sets {
            for e in d.behaviour_list(list) {
                let row = d.row(e.program, 0).expect("program row");
                assert!(row.op < 0 || (0x3F..=0x62).contains(&(row.op as u8)), "opcode {:#x}", row.op);
            }
        }
        if let Some(an) = d.anim(ty) {
            assert!(an.frame_count() > 0);
        }
    }
}

#[test]
fn activation_follows_the_party() {
    let Some((mut g, _)) = load() else { return };
    let Some(&(map, _, _, _)) = groups(&g).first() else { return };
    g.party.map = map;
    g.advance();
    let active = g.creature_slots.iter().flatten().count();
    let on_map = groups(&g).iter().filter(|t| t.0 == map).count();
    assert_eq!(active, on_map.min(slot::POOL_SIZE));
    for s in g.creature_slots.iter().flatten() {
        assert_eq!(s.pos.map(), map);
        assert!(s.event.is_some(), "each active creature has a pending step");
    }
}

fn run_map(map: usize, ticks: u32) -> Option<(u32, Vec<(u16, u16)>, Vec<(usize, i32, i32)>)> {
    let (mut g, _) = load()?;
    g.party.map = map;
    for _ in 0..ticks {
        g.advance();
        g.effects.clear();
    }
    let hp: Vec<(u16, u16)> = g.champions.iter().map(|c| (c.health() as u16, c.max_health() as u16)).collect();
    let pos = g.creature_slots.iter().flatten().map(|s| (s.pos.map(), s.pos.x(), s.pos.y())).collect();
    Some((g.rng.state, hp, pos))
}

#[test]
fn creature_simulation_is_deterministic() {
    let Some((g, _)) = load() else { return };
    let Some(&(map, _, _, _)) = groups(&g).first() else { return };
    let a = run_map(map, 300);
    let b = run_map(map, 300);
    assert_eq!(a, b);
}

#[test]
fn creatures_animate_and_act() {
    let Some((mut g, d)) = load() else { return };
    // Pick the map with the most animated creatures.
    let all = groups(&g);
    let Some(map) = (0..g.dungeon.maps.len()).max_by_key(|&m| {
        all.iter().filter(|t| t.0 == m && d.anim(creature_type(&g, t.3)).is_some()).count()
    }) else {
        return;
    };
    g.party.map = map;
    let mut frames_seen = std::collections::HashSet::new();
    for _ in 0..400 {
        g.advance();
        for s in g.creature_slots.iter().flatten() {
            if let Some(v) = view(&g, s.thing) {
                frames_seen.insert((s.thing.0, v.frame));
            }
        }
        g.effects.clear();
    }
    assert!(frames_seen.len() > groups(&g).iter().filter(|t| t.0 == map).count(), "frames advance over time");
}

#[test]
fn damage_owed_kills_and_removes() {
    let Some((mut g, _)) = load() else { return };
    let Some(&(map, x, y, c)) = groups(&g).iter().find(|t| {
        // a creature with a normal (animate) info record
        let d = g.creature_data.clone().unwrap();
        type_info(&g, &d, creature_type(&g, t.3)).is_some_and(|(i, _)| !i.inanimate())
    }) else {
        return;
    };
    g.party.map = map;
    g.advance();
    let h = hp(&g, c);
    damage(&mut g, c, map, x, y, h + 10);
    let mut gone = false;
    for _ in 0..200 {
        g.advance();
        if !g.dungeon.things_at(map, x, y).iter().any(|t| t.kind() == ThingType::Creature && t.0 & 0x3FF == c.0 & 0x3FF)
            && slot_of(&g, c).is_none()
        {
            gone = true;
            break;
        }
    }
    assert!(gone, "a creature dealt more than its HP is removed after its death action");
    assert!(g.effects.iter().any(|e| matches!(e, Effect::CreatureDied { .. })) || gone);
}

#[test]
fn kind_sets_parse_for_every_placed_type() {
    let Some((g, _)) = load() else { return };
    let mut defined = 0;
    for (_, _, _, c) in groups(&g) {
        let ty = creature_type(&g, c);
        for set in 0..0x30u8 {
            if kinds::set_for(&g, ty, set, false).is_some() {
                defined += 1;
            }
        }
    }
    // Structural only: at least some creature types define item-kind sets.
    assert!(defined > 0);
}

#[test]
fn goal_data_builds_goals_for_every_class() {
    let Some((g, d)) = load() else { return };
    let Some(&(si_map, x, y, c)) = groups(&g).first() else { return };
    let mut g = g;
    let Some(si) = activate(&mut g, &d, c, si_map, x, y) else { return };
    let Some(ctx) = Ctx::load(&g, &d, si) else { return };
    let mut built = 0;
    for class in 0..64u16 {
        for (_, list) in d.behaviour_sets(class) {
            for e in d.behaviour_list(list) {
                let Some(row) = d.row(e.program, 0) else { continue };
                let goals = goals::build(&g, &d, &ctx, e.program, row.goal_kind(), row.goal_arg, e.goal_data);
                for gl in &goals {
                    assert!(gl.kind < 0x1C, "goal type out of range");
                    assert!(gl.limit <= 0x7F);
                }
                built += goals.len();
            }
        }
    }
    assert!(built > 0);
}

#[test]
fn darkness_step_is_in_range() {
    let Some((g, d)) = load() else { return };
    assert!(fight::darkness_level(&g, &d) <= 5);
}

#[test]
fn script_conditions_read_flags_bytes_and_words() {
    let Some((mut g, _)) = load() else { return };
    assert!(!ai::script_condition(&g, 3));
    g.legacy.flags[0] |= 1 << 3;
    assert!(ai::script_condition(&g, 3));
    g.legacy.byte_vars[2] = 9;
    assert!(ai::script_condition(&g, 0x42));
    g.legacy.word_vars[5] = 1;
    assert!(ai::script_condition(&g, 0x85));
    assert!(!ai::script_condition(&g, 0xC0));
}

#[test]
fn close_bracket_queues_scripted_action_with_defaults() {
    let Some((mut g, d)) = load() else { return };
    let Some(&(m, x, y, c)) = groups(&g).first() else { return };
    let Some(si) = activate(&mut g, &d, c, m, x, y) else { return };
    let Some(ctx) = Ctx::load(&g, &d, si) else { return };
    {
        let s = ctx.slot_mut(&mut g);
        s.kind_a = 0x12;
        s.kind_b = 0x34;
    }
    assert_eq!(ops::op_close_bracket(&mut g, &ctx, 2), ai::Res::Done);
    let s = ctx.slot(&g);
    assert_eq!((s.action, s.mode, s.arg), (0x3F, 0x12, 0x34));
}

#[test]
fn open_bracket_rolls_are_deterministic() {
    let Some((g0, d)) = load() else { return };
    let Some(&(m, x, y, c)) = groups(&g0).first() else { return };
    let run = |seed: u32| {
        let mut g = g0.clone();
        g.rng = crate::rng::Rng::new(seed);
        let si = activate(&mut g, &d, c, m, x, y)?;
        let ctx = Ctx::load(&g, &d, si)?;
        let r = ops::op_open_bracket(&mut g, &ctx);
        Some((r, g.rng.state, ctx.slot(&g).vars[0]))
    };
    assert_eq!(run(77), run(77));
}

/// Groups whose type can cry out when hurt (type flag 0x01 clear, AI class
/// flag 0x8000), activated: (map, x, y, slot index).
fn criers(g: &mut GameState, d: &Rc<CreatureData>) -> Vec<(usize, i32, i32, usize)> {
    let mut out = Vec::new();
    for (m, x, y, c) in groups(g) {
        let Some((info, class)) = type_info(g, d, creature_type(g, c)) else { continue };
        if info.raw[0] & 1 != 0 || info.raw[1] == 0xFF || d.class_flags(class) & 0x8000 == 0 {
            continue;
        }
        if let Some(si) = activate(g, d, c, m, x, y) {
            out.push((m, x, y, si));
        }
    }
    out
}

#[test]
fn hurt_creatures_cry_out_sometimes() {
    let Some((mut g, d)) = load() else { return };
    let all = criers(&mut g, &d);
    let Some(&(_, _, _, si)) = all.first() else { return };
    let ctx = Ctx::load(&g, &d, si).unwrap();
    set_rec_u16(&mut g, ctx.thing, 6, 1000);
    let mut cries = 0;
    for seed in 0..64u32 {
        let mut h = g.clone();
        h.rng.state = seed.wrapping_mul(0x9E37_79B9);
        h.effects.clear();
        let before = h.rng.state;
        assert!(!apply_damage(&mut h, &ctx, 1));
        assert_ne!(h.rng.state, before, "the cry roll draws a random number");
        cries += h.effects.iter().filter(|e| matches!(e, Effect::Sound { cat: 15, sub: 9 | 10, .. })).count();
    }
    // One time in eight at least, so 64 seeds almost surely produce some.
    assert!(cries > 0, "no pain cry in 64 rolls");
}

#[test]
fn a_landed_blow_plays_the_hit_sound() {
    let Some((mut g, _)) = load() else { return };
    let (dx, dy) = (crate::viewport::DX, crate::viewport::DY);
    for (m, x, y, _) in groups(&g) {
        for dir in 0..4u8 {
            let (px, py) = (x - dx[dir as usize], y - dy[dir as usize]);
            if crate::world::blocks(&g.dungeon, m, px, py) || group_at(&g, m, px, py).is_some() {
                continue;
            }
            g.party = crate::world::PartyPos { map: m, x: px, y: py, dir };
            g.effects.clear();
            crate::apply::apply_action(&mut g, 0, &[crate::combat::Effect::DamageCreature { amount: 3, attack_type: 0 }]);
            assert!(
                g.effects
                    .iter()
                    .any(|e| matches!(e, Effect::Sound { cat: 15, sub: 0x8D, x: sx, y: sy, .. } if *sx == x && *sy == y)),
                "no hit sound at the creature's square"
            );
            return;
        }
    }
}

#[test]
fn transforming_plays_its_sound() {
    let Some((mut g, d)) = load() else { return };
    let Some((m, x, y, c)) = groups(&g).into_iter().next() else { return };
    let si = activate(&mut g, &d, c, m, x, y).unwrap();
    let ctx = Ctx::load(&g, &d, si).unwrap();
    ctx.slot_mut(&mut g).arg = creature_type(&g, c);
    g.effects.clear();
    assert!(ai::transform(&mut g, &d, &ctx));
    assert!(g.effects.iter().any(|e| matches!(e, Effect::Sound { cat: 3, idx: 0, sub: 0x81, .. })));
}
