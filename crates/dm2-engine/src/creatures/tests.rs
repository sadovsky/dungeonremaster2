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
