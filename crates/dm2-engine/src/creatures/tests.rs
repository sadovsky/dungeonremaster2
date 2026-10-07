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
fn new_game_draw_order_matches_the_original() {
    // Measured in DOSBox: the original recruits the starting champion only
    // after the all-maps activation pass, and its first champion starts
    // with food 1697 and water 1686. Equal values here mean the random
    // stream matches the original draw for draw up to the recruit.
    let Some((g0, d)) = load() else { return };
    let Some(gd) = crate::data::GameData::load_default() else { return };
    let g = GameState::new_game_full(&g0.dungeon, Rc::new(gd), Some(d));
    let c = &g.champions[0];
    assert_eq!((c.food(), c.water()), (1697, 1686));
}

/// Random draws and thinks counted over `ticks` ticks of an idle new game.
fn idle_counts(ticks: u32) -> Option<(u32, u32)> {
    let (g0, d) = load()?;
    let gd = crate::data::GameData::load_default()?;
    let mut g = GameState::new_game_full(&g0.dungeon, Rc::new(gd), Some(d));
    crate::rng::trace_start();
    for _ in 0..ticks {
        g.advance();
    }
    let m = crate::rng::trace_take();
    let draws = m.iter().filter(|(k, _)| k.0 != "think" && k.0 != "frame").map(|(_, &v)| v).sum();
    let thinks = m.iter().filter(|(k, _)| k.0 == "think").map(|(_, &v)| v).sum();
    Some((draws, thinks))
}

#[test]
fn first_ticks_draw_like_the_original() {
    // Measured with the hooked DOSBox (docs/05): ticks 0 and 1 after the
    // recruit draw 3 + 417 = 420 times, mostly every awake creature's first
    // think and frame.
    let Some((draws, _)) = idle_counts(2) else { return };
    assert_eq!(draws, 420, "ticks 0-1 drew {draws}, original 420");
}

#[test]
fn creature_events_carry_the_creature_type_as_priority() {
    // 0x3059D: the event's priority byte is the creature's type (record
    // byte +4), which orders same-tick creature events.
    let Some((g0, d)) = load() else { return };
    let Some(gd) = crate::data::GameData::load_default() else { return };
    let g = GameState::new_game_full(&g0.dungeon, Rc::new(gd), Some(d));
    let mut seen = 0;
    for s in g.creature_slots.iter().flatten() {
        let Some(ev) = s.event.and_then(|e| g.timeline.get(e)) else { continue };
        assert_eq!(ev.prio, creature_type(&g, s.thing), "creature {:#x}", s.thing.0);
        seen += 1;
    }
    assert!(seen > 0);
}

#[test]
fn idle_draws_match_the_original_log_through_tick_51() {
    // The original's draw log (re/state/rnglog.txt, recorded with the hooked
    // DOSBox build; local only) against the remake's ordered draws: equal
    // per-tick counts, including setup under tick 0, through tick 51.
    let log = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/state/rnglog.txt");
    let Ok(text) = std::fs::read_to_string(log) else { return };
    let mut orig = [0u32; 52];
    for line in text.lines() {
        if let Some(t) = line.split_whitespace().next().and_then(|t| t.parse::<usize>().ok()) {
            if t < 52 {
                orig[t] += 1;
            }
        }
    }
    let Some((g0, d)) = load() else { return };
    let Some(gd) = crate::data::GameData::load_default() else { return };
    crate::rng::trace_seq_start();
    let mut g = GameState::new_game_full(&g0.dungeon, Rc::new(gd), Some(d));
    while g.tick < 52 {
        g.advance();
    }
    let mut ours = [0u32; 52];
    for (t, ..) in crate::rng::trace_seq_take() {
        if (t as usize) < 52 {
            ours[t as usize] += 1;
        }
    }
    assert_eq!(ours, orig);
}

#[test]
fn walk_upkeep_matches_the_original() {
    // Measured in DOSBox: a new game walking a 22-step loop of map 0 (moves
    // logged at these ticks by the hooked build, turns about 5 ticks after
    // the move before them) saves at tick 217 with food 1683 and water 1679.
    // Upkeep's rest bonus counts from the party's formation (0x7F19C), not
    // from the last move, which is what the 4 food and 2 water at tick 128
    // depend on.
    use crate::state::Command;
    use crate::world::Move;
    let Some((g0, d)) = load() else { return };
    let Some(gd) = crate::data::GameData::load_default() else { return };
    let mut g = GameState::new_game_full(&g0.dungeon, Rc::new(gd), Some(d));
    let moves = [34, 41, 47, 53, 60, 66, 73, 84, 90, 97, 103, 114, 121, 127, 133, 139, 146, 152, 164, 170, 176, 183];
    let turns = [78, 108, 157];
    while g.tick < 217 {
        if moves.contains(&g.tick) {
            g.push_command(Command::Move(Move::Forward));
        }
        if turns.contains(&g.tick) {
            g.push_command(Command::TurnRight);
        }
        g.advance();
    }
    let c = &g.champions[0];
    assert_eq!((g.party.map, g.party.x, g.party.y, g.party.dir), (0, 1, 8, 3));
    assert_eq!((c.stamina(), c.food(), c.water()), (770, 1683, 1679));
}

#[test]
fn idle_creatures_think_as_often_as_the_original() {
    // Original, ticks 2-157 of an idle new game: 663 thinks and 36.2 draws
    // per tick. The off-map slowdown applies only on frames that loaded the
    // creature's class; the wander list draws once per think.
    let Some((d0, t0)) = idle_counts(2) else { return };
    let Some((d1, t1)) = idle_counts(158) else { return };
    let (draws, thinks) = (d1 - d0, t1 - t0);
    let per_tick = draws as f64 / 156.0;
    assert!((600..=730).contains(&thinks), "{thinks} thinks, original 663");
    assert!((32.5..=40.0).contains(&per_tick), "{per_tick:.1} draws per tick, original 36.2");
}

#[test]
fn play_start_activates_every_map_by_info_bit() {
    // 0x34236 at play start: on every map, groups whose type has info bit 0
    // clear get a slot; the others stay dormant with a frame-cycle state.
    let Some((mut g, d)) = load() else { return };
    g.advance();
    let mut active = 0;
    for (_, _, _, c) in groups(&g) {
        let ty = creature_type(&g, c);
        let dormant = type_info(&g, &d, ty).is_some_and(|(i, _)| i.inanimate());
        if dormant {
            assert_eq!(rec_u8(&g, c, 5), 0xFF, "dormant groups get no slot");
            assert_ne!(rec_u16(&g, c, 0x0A) & 0x8000, 0, "dormant groups get the frame-cycle state");
        } else {
            assert!(slot_of(&g, c).is_some(), "every awake group is active, on any map");
            active += 1;
        }
    }
    assert_eq!(g.creature_slots.iter().flatten().count(), active);
    assert_eq!(g.creature_slots.len(), pool_size(&g, &d));
    for s in g.creature_slots.iter().flatten() {
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

#[test]
fn planner_prefers_earlier_goals_over_nearer_ones() {
    // The planner ranks goals by list order (0x3188A): a later goal met at
    // the start square is only kept as the best so far, and the search goes
    // on for the goals before it.
    let Some((mut g, _)) = load() else { return };
    let (map, px, py) = (g.party.map, g.party.x, g.party.y);
    assert_eq!(map, 0, "the new game starts on map 0");
    let s = super::planner::Searcher {
        map,
        x: px,
        y: py - 3,
        mask: 0xFFFF,
        size: 1,
        group: ThingRef(0x3FFE),
        attack_mask: 0,
        range: 0,
        info0: 0,
        cflags: 0,
    };
    let goal = |kind: u8, arg: i8, program: u8, limit: u8| super::planner::Goal { kind, arg, program, limit, data: 0, mode: 0, value: 0, tag: 0 };
    // Goal 0: the party's square, three steps up the start corridor; goal 1:
    // stay put. The party's square wins although the fallback is nearer.
    let found = super::planner::search(&mut g, &s, &[goal(2, 0, 7, 10), goal(0, -1, 9, 0)]).unwrap();
    assert_eq!((found.goal, found.distance), (0, 3));
    // Out of the first goal's reach, the fallback is what remains.
    let found = super::planner::search(&mut g, &s, &[goal(2, 0, 7, 2), goal(0, -1, 9, 0)]).unwrap();
    assert_eq!((found.goal, found.distance), (1, 0));
}

#[test]
fn path_test_reads_the_search_square_on_the_partys_map() {
    // The original runs the path test (0x2C404) with the party's map
    // selected, whatever map the search square is on (checked in DOSBox on
    // a pit fall from map 4 to map 7). Map 4 (5,12) is a wall but map 7
    // (5,12) is open, so a reach-2 creature searching map 4 at (5,13) is in
    // line with the party at map 7 (5,11) only when read on the party's map.
    let Some((mut g, _)) = load() else { return };
    assert_eq!(g.dungeon.square(4, 5, 12).0 >> 5, 0, "map 4 (5,12) is a wall");
    assert_ne!(g.dungeon.square(7, 5, 12).0 >> 5, 0, "map 7 (5,12) is open");
    g.party = crate::world::PartyPos { map: 7, x: 5, y: 11, dir: 0 };
    let s = super::planner::Searcher {
        map: 4,
        x: 6,
        y: 13,
        mask: 0xFFFF,
        size: 1,
        group: ThingRef(0x3FFE),
        attack_mask: 0x40,
        range: 2,
        info0: 0,
        cflags: 0,
    };
    let goal = super::planner::Goal { kind: 7, arg: 0, program: 1, limit: 4, data: 0, mode: 0, value: 0xFFF, tag: 0 };
    assert!(super::planner::satisfies_on(&mut g, &s, &goal, 4, 5, 13, 1, 3, (5, 13)));
}

#[test]
fn party_facing_mask_is_relative_to_the_partys_facing() {
    // Goal kind 2, mode 1 (0x3188A): the party's square counts only when the
    // direction from the party toward the square the search came from, less
    // the party's facing, has its bit in the value mask. 0x0E is "beside or
    // behind the party". Seen at tick 165 of the map 22 combat probe: the
    // creature straight in front of the party does not take this goal.
    let Some((mut g, _)) = load() else { return };
    let s = super::planner::Searcher {
        map: 22,
        x: 3,
        y: 3,
        mask: 0xFFFF,
        size: 1,
        group: ThingRef(0x3FFE),
        attack_mask: 0,
        range: 1,
        info0: 0,
        cflags: 0,
    };
    let goal = super::planner::Goal { kind: 2, arg: 3, program: 42, limit: 4, data: 0, mode: 1, value: 0x0E, tag: 24 };
    // Party at (3,2) facing south: the creature's square (3,3) is in front.
    g.party = crate::world::PartyPos { map: 22, x: 3, y: 2, dir: 2 };
    assert!(!super::planner::satisfies_on(&mut g, &s, &goal, 22, 3, 2, 1, 1, (3, 3)));
    // Facing north, the same square is behind the party: bit 2 is set.
    g.party.dir = 0;
    assert!(super::planner::satisfies_on(&mut g, &s, &goal, 22, 3, 2, 1, 1, (3, 3)));
}

#[test]
fn facing_path_goal_waits_after_an_attack() {
    // Goal kind 0x0A takes the party on the square ahead only when the
    // action the creature just finished (0x7F56A) has bits 0-1 clear in the
    // action flag table. After a melee attack (action 8) it does not: seen at
    // tick 202 of the round 8 combat log, where the original's creature idles
    // instead of attacking again.
    let Some((mut g, d)) = load() else { return };
    assert_ne!(d.action_flags(8) & 3, 0, "melee attack is a flagged action");
    assert_eq!(d.action_flags(0) & 3, 0, "idle is not");
    let s = super::planner::Searcher {
        map: 22,
        x: 3,
        y: 3,
        mask: 0xFFFF,
        size: 1,
        group: ThingRef(0x3FFE),
        attack_mask: 0,
        range: 1,
        info0: 0,
        cflags: 0,
    };
    let goal = super::planner::Goal { kind: 0x0A, arg: 0, program: 5, limit: 0, data: 0, mode: 0xFFFF, value: 0xFFF, tag: 2 };
    // The creature faces north (its default facing) toward the party.
    g.party = crate::world::PartyPos { map: 22, x: 3, y: 2, dir: 2 };
    let ahead = super::facing(&g, s.group) == 0;
    g.creature_prev_action = 8;
    assert!(!super::planner::satisfies_on(&mut g, &s, &goal, 22, 3, 3, 0, 1, (3, 3)));
    g.creature_prev_action = 0;
    assert_eq!(super::planner::satisfies_on(&mut g, &s, &goal, 22, 3, 3, 0, 1, (3, 3)), ahead);
}

/// 0x56BA5's visiting order: the rectangle centred on the event square, rows
/// from the highest y down, squares from the highest x down, clipped to the
/// map. Data-free.
#[test]
fn area_squares_visit_rows_from_the_bottom_right() {
    // Event (2,2), target (3,1): half-sizes 1 and 1.
    let sq = area_squares(2, 2, 3, 1, 10, 10);
    assert_eq!(sq, vec![(3, 3), (2, 3), (1, 3), (3, 2), (2, 2), (1, 2), (3, 1), (2, 1), (1, 1)]);
    // Clipped at the map's top-left corner.
    let sq = area_squares(0, 0, 1, 1, 10, 10);
    assert_eq!(sq, vec![(1, 1), (0, 1), (1, 0), (0, 0)]);
    // Target on the event square: just that square.
    assert_eq!(area_squares(4, 5, 4, 5, 10, 10), vec![(4, 5)]);
}

/// The hit handler's guard for a group without a slot (0x24EFD): a dormant
/// type is woken, an awake type is left alone.
#[test]
fn a_hit_wakes_dormant_groups_only() {
    let Some((mut g, d)) = load() else { return };
    let all = groups(&g);
    let dormant = |g: &GameState, c| type_info(g, &d, creature_type(g, c)).is_some_and(|(i, _)| i.inanimate());
    let awake = all.iter().find(|t| slot_of(&g, t.3).is_none() && !dormant(&g, t.3)).copied();
    let sleeping = all.iter().find(|t| slot_of(&g, t.3).is_none() && dormant(&g, t.3)).copied();
    if let Some((m, x, y, c)) = awake {
        hit(&mut g, c, m, x, y, 0x0002, 60, 0);
        assert!(slot_of(&g, c).is_none(), "an awake group without a slot is left alone");
    }
    if let Some((m, x, y, c)) = sleeping {
        hit(&mut g, c, m, x, y, 0x0002, 60, 0);
        assert!(slot_of(&g, c).is_some(), "a dormant group is woken by a hit");
    }
}

/// `?` and `@` pass flag 0x80 to the movement test (0x27F54, 0x27FC8): with
/// the party on the square ahead the test just fails, leaving the slot's
/// actions alone, while the plain movement test commits to the party.
#[test]
fn step_tests_fail_on_the_party_square() {
    let Some((mut g, d)) = load() else { return };
    let Some((m, x, y, c)) = groups(&g).into_iter().find(|t| {
        type_info(&g, &d, creature_type(&g, t.3)).is_some_and(|(i, _)| !i.inanimate())
    }) else {
        return;
    };
    let Some(si) = activate(&mut g, &d, c, m, x, y) else { return };
    let f = facing(&g, c);
    let (ax, ay) = (x + crate::viewport::DX[f as usize], y + crate::viewport::DY[f as usize]);
    g.party = crate::world::PartyPos { map: m, x: ax, y: ay, dir: 0 };
    let Some(ctx) = Ctx::load(&g, &d, si) else { return };
    let (act, queued) = (ctx.slot(&g).action, ctx.slot(&g).queued);
    assert!(!ai::move_test_step(&mut g, &ctx, f, 5), "the step test fails on the party's square");
    assert_eq!((ctx.slot(&g).action, ctx.slot(&g).queued), (act, queued), "and leaves the slot alone");
    assert!(ai::move_test(&mut g, &ctx, f, 5), "the plain test commits to the party");
}

/// Builders 2 and 3 (0x27663) gate on alertness like 6 and 7 (0x277FB):
/// with the alertness flag clear none of them builds a goal; with it set,
/// some of the behaviour entries that use them do.
#[test]
fn attack_builders_need_alertness() {
    let Some((mut g, d)) = load() else { return };
    let Some((m, x, y, c)) = groups(&g).into_iter().find(|t| {
        type_info(&g, &d, creature_type(&g, t.3)).is_some_and(|(i, _)| i.raw[0x0E] | i.raw[0x0F] != 0)
    }) else {
        return;
    };
    let Some(si) = activate(&mut g, &d, c, m, x, y) else { return };
    let Some(ctx) = Ctx::load(&g, &d, si) else { return };
    let mut built = 0;
    for class in 0..64u16 {
        for (_, list) in d.behaviour_sets(class) {
            for e in d.behaviour_list(list) {
                let Some(row) = d.row(e.program, 0) else { continue };
                if !matches!(row.goal_kind(), 2 | 3 | 6 | 7) {
                    continue;
                }
                g.creature_alert_roll = 0;
                let none = goals::build(&g, &d, &ctx, e.program, row.goal_kind(), row.goal_arg, e.goal_data);
                assert!(none.is_empty(), "builder {} built a goal while not alert", row.goal_kind());
                g.creature_alert_roll = 1;
                built += goals::build(&g, &d, &ctx, e.program, row.goal_kind(), row.goal_arg, e.goal_data).len();
            }
        }
    }
    assert!(built > 0, "alert attack builders build goals");
}

/// Opcode Q (0x2923E) with its target on the next square, holding the party:
/// done once the creature faces it (so a following R attacks at once), not
/// done while the party stands to its side.
#[test]
fn q_is_done_facing_an_occupied_target() {
    let Some((mut g, d)) = load() else { return };
    if d.row(1, 0).map(|r| r.op as u8) != Some(b'Q') {
        return;
    }
    let Some((m, x, y, c)) = groups(&g).into_iter().find(|t| {
        type_info(&g, &d, creature_type(&g, t.3)).is_some_and(|(i, _)| !i.inanimate())
    }) else {
        return;
    };
    let Some(si) = activate(&mut g, &d, c, m, x, y) else { return };
    let f = facing(&g, c);
    for (dir, done) in [(f, true), ((f + 1) & 3, false)] {
        let (tx, ty) = (x + crate::viewport::DX[dir as usize], y + crate::viewport::DY[dir as usize]);
        g.party = crate::world::PartyPos { map: m, x: tx, y: ty, dir: 0 };
        let Some(ctx) = Ctx::load(&g, &d, si) else { return };
        ctx.slot_mut(&mut g).target = crate::creatures::slot::Packed::new(m, tx, ty);
        let r = ai::run_row(&mut g, &d, &ctx, 1, 0);
        assert_eq!(r == Some(ai::Res::Done), done, "party in direction {dir}, facing {f}");
    }
}

/// The movement test's door branch (0x2E4F1): a creature whose terrain mask
/// keeps only 0x4000 of a closed door's class (0x4200) never walks into the
/// door. Its walk step hands the door to the door routine in mode 0 and
/// fails, so the creature stays put; the door's flags and the type's door
/// mask decide whether it then bashes, casts or does nothing. Seen at tick
/// 217 of the round 8 combat log, at the closed door (7,2) on map 36.
#[test]
fn walking_into_a_closed_door_never_enters_it() {
    let Some((mut g, d)) = load() else { return };
    let (map, door) = (36usize, (7, 2));
    assert_eq!(g.dungeon.square(map, door.0, door.1).element(), dm2_formats::dungeon::Element::Door);
    // Close the door (state 4) whatever its starting state.
    let sq = g.dungeon.square(map, door.0, door.1).0;
    g.dungeon.set_square(map, door.0, door.1, (sq & !7) | 4);
    // A group of a type that takes the door branch and may open doors.
    let Some(c) = groups(&g).into_iter().map(|(_, _, _, c)| c).find(|&c| {
        let ty = creature_type(&g, c);
        type_info(&g, &d, ty).is_some_and(|(i, _)| i.terrain() & 0x4200 == 0x4000 && i.door_actions() & 0x6F != 0)
    }) else {
        return;
    };
    let (m0, x0, y0, _) = groups(&g).into_iter().find(|&(_, _, _, t)| t == c).unwrap();
    crate::movement::move_thing(&mut g, c, Some((m0, x0, y0)), Some((map, 6, 2)));
    let Some(si) = activate(&mut g, &d, c, map, 6, 2) else { return };
    set_facing(&mut g, c, 1);
    let ctx = Ctx::load(&g, &d, si).unwrap();
    {
        let s = ctx.slot_mut(&mut g);
        s.target = slot::Packed::new(map, door.0, door.1);
        s.action = ai::action::WALK;
    }
    let failed = ai::frame_event(&mut g, &d, &ctx);
    assert_ne!(failed, 0, "the walk step fails at the door");
    assert_eq!(group_at(&g, map, 6, 2), Some(c), "the creature stays put");
    assert_eq!(group_at(&g, map, door.0, door.1), None, "nothing entered the door");
}

/// Actuator type 0x3A (0x2538C): a set signal on a dormant group wakes it
/// and runs the cycle step 0x252C3, which rebuilds the status word as
/// `count | (status & 0xFC0) | 0x8000` (or `| 0xA000` mid-cycle), dropping
/// bit 0x1000. The remake used to toggle status bit 0x10 instead.
#[test]
fn floor_signal_runs_the_dormant_cycle_step() {
    let Some((mut g, d)) = load() else { return };
    let dormant = groups(&g).into_iter().find(|&(_, _, _, c)| {
        type_info(&g, &d, creature_type(&g, c)).is_some_and(|(i, _)| i.inanimate()) && rec_u8(&g, c, 5) == 0xFF
    });
    let Some((map, x, y, c)) = dormant else { return };
    set_rec_u16(&mut g, c, 0x0A, 0x9001);
    floor_signal(&mut g, map, x, y, true);
    let st = rec_u16(&g, c, 0x0A);
    assert_eq!(st & 0x1000, 0, "the set signal's cycle step drops bit 0x1000: {st:#06x}");
    assert_ne!(st & 0x8000, 0, "the cycle step keeps bit 0x8000: {st:#06x}");
    assert_eq!(st & 0x10, 0, "no status bit 0x10 toggle: {st:#06x}");
}
