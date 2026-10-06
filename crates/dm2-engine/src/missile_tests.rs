//! Scenario tests for missiles, effects, death and the starting party on
//! the user's real data. They skip when the original files are absent and
//! assert only behaviour, never game values.

use std::rc::Rc;

use dm2_formats::dungeon::{Dungeon, Element, ThingRef, ThingType};

use crate::assets::default_data_dir;
use crate::champions::{Champion, EMPTY};
use crate::data::GameData;
use crate::missiles::{self, kind};
use crate::party;
use crate::rng::Rng;
use crate::state::GameState;
use crate::viewport::{DX, DY};

fn data() -> Option<Rc<GameData>> {
    GameData::load_default().map(Rc::new)
}

fn game() -> Option<GameState> {
    let bytes = std::fs::read(default_data_dir().join("DUNGEON.DAT")).ok()?;
    let dg = Dungeon::parse(&bytes).ok()?;
    Some(GameState::new_game_with(&dg, data()?))
}

fn run(g: &mut GameState, ticks: u32) {
    for _ in 0..ticks {
        g.advance();
    }
}

fn things(g: &GameState, kind: ThingType) -> Vec<(usize, i32, i32, ThingRef)> {
    let mut out = Vec::new();
    for (m, md) in g.dungeon.maps.iter().enumerate() {
        for x in 0..md.width as i32 {
            for y in 0..md.height as i32 {
                for t in g.dungeon.things_at(m, x, y) {
                    if t.kind() == kind {
                        out.push((m, x, y, t));
                    }
                }
            }
        }
    }
    out
}

#[test]
fn new_game_recruits_the_starting_champion() {
    let Some(g) = game() else { return };
    assert_eq!(g.champions.len(), 1, "the original recruits one champion at the start");
    assert_eq!(g.leader, Some(0));
    let c = &g.champions[0];
    assert!(c.is_alive());
    assert_eq!(c.facing(), g.party.dir);
    assert_eq!(c.cell(), g.party.dir);
    assert!(!c.name().is_empty());
}

#[test]
fn fireball_flies_into_a_wall_and_bursts() {
    let Some(mut g) = game() else { return };
    let p = g.party;
    // A direction from the party square whose neighbour is a wall.
    let Some(dir) = (0..4u8).find(|&d| {
        g.dungeon.square(p.map, p.x + DX[d as usize], p.y + DY[d as usize]).element() == Element::Wall
    }) else {
        return;
    };
    let m = missiles::launch(&mut g, kind::FIREBALL, p.map, p.x, p.y, dir, dir, 120, 60, 4, false).unwrap();
    assert!(g.dungeon.things_at(p.map, p.x, p.y).iter().any(|t| t.0 & 0x3FFF == m.0 & 0x3FFF));
    run(&mut g, 4);
    assert!(things(&g, ThingType::Missile).is_empty(), "missile should be gone after hitting the wall");
    // The burst cloud lives one tick, then is removed.
    run(&mut g, 3);
    assert!(things(&g, ThingType::Cloud).is_empty());
}

#[test]
fn missile_runs_out_of_energy_and_drops_its_item() {
    let Some(mut g) = game() else { return };
    let p = g.party;
    let Some(item) = crate::actuators::create_item(&mut g, 0) else { return };
    let before = g.dungeon.things_at(p.map, p.x, p.y).len();
    // Energy below the step: stops on its first real step.
    missiles::launch(&mut g, item.0, p.map, p.x, p.y, p.dir, p.dir, 2, 2, 5, true);
    run(&mut g, 3);
    assert!(things(&g, ThingType::Missile).is_empty());
    let here = g.dungeon.things_at(p.map, p.x, p.y);
    assert!(here.iter().any(|t| t.0 & 0x3FFF == item.0 & 0x3FFF), "item lands where the missile stopped");
    assert!(here.len() > before);
}

#[test]
fn impact_damage_rng_order_is_fixed() {
    let Some(mut g) = game() else { return };
    g.rng = Rng::new(1234);
    let (d, atype, _) = missiles::impact_damage(&mut g, kind::FIREBALL, 100, 64);
    // Replay the documented order by hand: two 4-bit rolls, random(n), rand4.
    let mut r = Rng::new(1234);
    let base = (r.rnd() & 15) + (r.rnd() & 15) + 10;
    let n = ((((base + 100) >> 4) + 1) >> 1) as u16 + 1;
    let mut e = base as i32 + r.random(n) as i32 + r.rand4() as i32;
    e = e.max(2 * (e - (32 - 64 / 8))).min(200);
    assert_eq!(d as i32, e);
    assert_eq!(atype, crate::combat::attack::FIRE);
    assert_eq!(g.rng.state, r.state);
}

#[test]
fn dying_champion_drops_everything_and_leaves_bones() {
    let Some(mut g) = game() else { return };
    let p = g.party;
    let carried: Vec<u16> = (0..30).map(|s| g.champions[0].inventory(s)).filter(|&t| t != EMPTY).collect();
    g.champions[0].set_health(1);
    g.party_status.pending_damage[0] = 50;
    run(&mut g, 1);
    assert!(!g.champions[0].is_alive());
    assert!(g.game_over, "last champion dead ends the game");
    let here = g.dungeon.things_at(p.map, p.x, p.y);
    for t in carried {
        assert!(here.iter().any(|h| h.0 & 0x3FFF == t & 0x3FFF), "possession {t:#x} dropped");
    }
    let bones: Vec<ThingRef> = here.into_iter().filter(|&t| party::bones_owner(&g, t) == Some(0)).collect();
    assert_eq!(bones.len(), 1);
}

#[test]
fn leadership_passes_to_a_living_champion() {
    let Some(mut g) = game() else { return };
    let mut second = Champion::default();
    second.set_health(40);
    second.set_max_health(40);
    for s in 0..30 {
        second.set_inventory(s, EMPTY);
    }
    g.champions.push(second);
    g.champions[0].set_health(1);
    g.party_status.pending_damage[0] = 50;
    run(&mut g, 1);
    assert_eq!(g.leader, Some(1));
    assert!(!g.game_over);
}

#[test]
fn bones_on_an_altar_bring_the_champion_back() {
    let Some(mut g) = game() else { return };
    // The shipped archive gives no wall ornament the altar attribute, so
    // mark the ornament of the first actuator that has one as an altar.
    let mut altar = None;
    'outer: for (m, md) in g.dungeon.maps.iter().enumerate() {
        let lists = g.dungeon.map_lists(m);
        for x in 0..md.width as i32 {
            for y in 0..md.height as i32 {
                for t in g.dungeon.things_at(m, x, y) {
                    let slot = g.dungeon.record_word(t, 2).unwrap_or(0) >> 12;
                    if t.kind() != ThingType::Actuator || slot == 0 {
                        continue;
                    }
                    if let Some(&orn) = lists.wall_ornaments.get(slot as usize - 1) {
                        g.attrs.set(9, orn, 0x0C, 1);
                        altar = Some((m, x, y));
                        break 'outer;
                    }
                }
            }
        }
    }
    let (m, x, y) = altar.expect("some actuator carries a wall ornament");
    assert!(party::is_altar(&g, m, x, y));
    let max_before = g.champions[0].max_health();
    g.champions[0].set_health(1);
    g.party_status.pending_damage[0] = 50;
    run(&mut g, 1);
    let p = g.party;
    let bones = g.dungeon.things_at(p.map, p.x, p.y).into_iter().find(|&t| party::bones_owner(&g, t) == Some(0)).unwrap();
    g.dungeon.remove_thing(p.map, p.x, p.y, bones);
    g.dungeon.add_thing(m, x, y, bones);
    party::item_dropped(&mut g, m, x, y, bones);
    run(&mut g, 10);
    let c = &g.champions[0];
    assert!(c.is_alive(), "revived after the three stages");
    let expect = (max_before - max_before / 64 - 1).max(25);
    assert_eq!(c.max_health(), expect);
    // Revived at half health; regeneration may already have added a little.
    assert!(c.health() >= expect / 2 && c.health() < expect);
    assert!(!g.dungeon.things_at(m, x, y).iter().any(|&t| party::bones_owner(&g, t) == Some(0)));
}

#[test]
fn party_effect_expires() {
    let Some(mut g) = game() else { return };
    party::party_effect(&mut g, 1, 1, 20, 5);
    assert_eq!(g.champions[0].shield_value(), 20);
    run(&mut g, 4);
    assert_eq!(g.champions[0].shield_value(), 20);
    run(&mut g, 3);
    assert_eq!(g.champions[0].shield_value(), 0);
}

#[test]
fn light_spell_wears_off() {
    let Some(mut g) = game() else { return };
    crate::apply::light(&mut g, 0x26, 100);
    assert!(g.light > 0);
    // Light kind 0x26 lasts at least 2000 ticks.
    run(&mut g, 1900);
    assert!(g.light > 0);
    run(&mut g, 2000);
    assert_eq!(g.light, 0);
}

#[test]
fn revive_formula() {
    let Some(mut g) = game() else { return };
    g.champions[0].set_max_health(200);
    g.champions[0].set_health(0);
    party::revive(&mut g, 0);
    // max − max/64 − 1, at least 25; health is half of it.
    assert_eq!(g.champions[0].max_health(), 200 - 3 - 1);
    assert_eq!(g.champions[0].health(), 98);
    g.champions[0].set_max_health(20);
    g.champions[0].set_health(0);
    party::revive(&mut g, 0);
    assert_eq!(g.champions[0].max_health(), 25);
}

/// Throw a potion of `pkind` at an adjacent wall; returns the game after
/// the impact and the potion's thing reference.
fn throw_potion_at_wall(pkind: u16) -> Option<(GameState, ThingRef)> {
    let mut g = game()?;
    let p = g.party;
    let dir = (0..4u8).find(|&d| {
        g.dungeon.square(p.map, p.x + DX[d as usize], p.y + DY[d as usize]).element() == Element::Wall
    })?;
    // Item numbers 384-431 are potions.
    let potion = crate::actuators::create_item(&mut g, 384)?;
    g.dungeon.set_record_word(potion, 1, pkind << 8 | 100);
    missiles::launch(&mut g, potion.0, p.map, p.x, p.y, dir, dir, 120, 60, 4, true)?;
    run(&mut g, 3);
    Some((g, potion))
}

fn on_any_square(g: &GameState, t: ThingRef) -> bool {
    [ThingType::Potion].iter().any(|&k| things(g, k).iter().any(|(_, _, _, r)| r.0 & 0x3FFF == t.0 & 0x3FFF))
}

#[test]
fn thrown_poison_potion_bursts_into_a_cloud() {
    let Some((g, potion)) = throw_potion_at_wall(3) else { return };
    assert!(things(&g, ThingType::Missile).is_empty());
    assert!(!on_any_square(&g, potion), "a kind-3 potion is consumed by the burst");
    assert!(!things(&g, ThingType::Cloud).is_empty(), "the burst leaves a poison cloud");
}

#[test]
fn thrown_ordinary_potion_drops_intact() {
    let Some((g, potion)) = throw_potion_at_wall(0) else { return };
    assert!(things(&g, ThingType::Missile).is_empty());
    assert!(on_any_square(&g, potion), "other potions survive the impact");
}
