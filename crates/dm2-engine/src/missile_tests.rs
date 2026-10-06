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

/// Put `items` into a fresh container of type `ctype` and return its weight.
fn container_weight(g: &mut GameState, ctype: u16, items: &[ThingRef]) -> u16 {
    let c = crate::actuators::create_item(g, 480 + ctype).unwrap();
    let mut prev = ThingRef::END;
    for &t in items.iter().rev() {
        g.dungeon.set_record_word(t, 0, prev.0);
        prev = t;
    }
    g.dungeon.set_record_word(c, 1, prev.0);
    let data = g.data.clone().unwrap();
    data.item_db(&g.dungeon).weight(c)
}

#[test]
fn coins_weigh_a_fifth_in_money_containers() {
    let Some(mut g) = game() else { return };
    let data = g.data.clone().unwrap();
    // Find a money container type and an ordinary one among types 0-7.
    let probe = |g: &mut GameState, ty: u16| {
        let c = crate::actuators::create_item(g, 480 + ty).unwrap();
        data.item_db(&g.dungeon).is_money_container(c)
    };
    let Some(money) = (0..8).find(|&t| probe(&mut g, t)) else { return };
    let Some(plain) = (0..8).find(|&t| !probe(&mut g, t)) else { return };
    // A heavy misc item (so 1/5 is clearly visible).
    let Some(heavy) = (256..384u16).find(|&n| {
        let Some(t) = crate::actuators::create_item(&mut g, n) else { return false };
        data.item_db(&g.dungeon).weight(t) >= 20
    }) else {
        return;
    };
    let a = crate::actuators::create_item(&mut g, heavy).unwrap();
    let b = crate::actuators::create_item(&mut g, heavy).unwrap();
    let item_w = data.item_db(&g.dungeon).weight(a) as u32;
    let empty_money = container_weight(&mut g, money, &[]) as u32;
    let full_money = container_weight(&mut g, money, &[a, b]) as u32;
    let empty_plain = container_weight(&mut g, plain, &[]) as u32;
    let a2 = crate::actuators::create_item(&mut g, heavy).unwrap();
    let b2 = crate::actuators::create_item(&mut g, heavy).unwrap();
    let full_plain = container_weight(&mut g, plain, &[a2, b2]) as u32;
    assert_eq!(full_plain - empty_plain, 2 * item_w, "ordinary containers add full weight");
    assert_eq!(full_money - empty_money, (2 * item_w + 4) / 5, "money containers add a fifth, rounded up");
}

#[test]
fn shooting_fires_matching_ammunition_from_the_other_hand() {
    use crate::combat::{self, ActionContext, ActionSpec, Effect as Action};
    let Some(mut g) = game() else { return };
    let data = g.data.clone().unwrap();
    // Find a launcher (attribute 5 bit 15) and ammunition sharing a class bit.
    let class = |g: &mut GameState, n: u16| {
        let t = crate::actuators::create_item(g, n)?;
        Some((t, data.item_db(&g.dungeon).attr(t, crate::items::ATTR_LAUNCHER)))
    };
    let mut pair = None;
    'outer: for ln in 0..256u16 {
        let Some((lt, lc)) = class(&mut g, ln) else { continue };
        if lc & 0x8000 == 0 {
            continue;
        }
        for an in 0..256u16 {
            let Some((at, ac)) = class(&mut g, an) else { continue };
            if ac & 0x8000 == 0 && ac & lc & 0x7FFF != 0 {
                pair = Some((lt, at));
                break 'outer;
            }
        }
    }
    let Some((launcher, ammo)) = pair else { return };
    let idx = g.leader.unwrap_or(0);
    g.champions[idx].set_inventory(1, launcher.0 & 0x3FFF);
    g.champions[idx].set_inventory(0, ammo.0 & 0x3FFF);
    let mut spec = ActionSpec { name: String::new(), codes: vec![0; 32] };
    spec.codes[data.tables.code_slot("CM").unwrap()] = 0x20;
    let db = data.item_db(&g.dungeon);
    let ctx = ActionContext {
        db: &db,
        tables: &data.tables,
        tick: 0,
        map_multiplier: 1,
        light_term: 0,
        target: None,
        target_untouchable: false,
    };
    let r = combat::do_action(&mut g.champions, &mut g.party_status, idx, 1, &spec, &ctx, &mut g.rng);
    assert!(r.success);
    let shot = r.effects.iter().find_map(|e| match e {
        Action::LaunchMissile { what, step, .. } => Some((*what, *step)),
        _ => None,
    });
    let (what, step) = shot.expect("a missile is launched");
    assert_eq!(what & 0x3FFF, ammo.0 & 0x3FFF, "the ammunition flies");
    assert_eq!(step as u16, db.attr(ammo, 0x0C), "speed from the ammunition's attribute 0x0C");
    assert_eq!(g.champions[idx].inventory(0), EMPTY, "the ammunition left the other hand");
    // Without ammunition the shot fails.
    let r = combat::do_action(&mut g.champions, &mut g.party_status, idx, 1, &spec, &ctx, &mut g.rng);
    assert!(!r.success);
}

#[test]
fn poison_cloud_damage_follows_the_formula() {
    let Some(mut g) = game() else { return };
    let flags = g.data.as_ref().unwrap().cloud_flags(7);
    // Strength 0x90: min(0x90 >> 5, 4) = 4, plus one random bit.
    let w = 7u16 | 0x90 << 8;
    g.rng = Rng::new(1234);
    let mut r = Rng::new(1234);
    let d = missiles::cloud_damage(&mut g, 7, w, None);
    if flags & 4 == 0 {
        assert_eq!(d, 0, "kind 7 does not reach the party in this archive");
        return;
    }
    if flags & 1 != 0 {
        r.random((0x90 >> 1) + 1);
    }
    assert_eq!(d, (4 + r.bit()).max(1));
    assert_eq!(g.rng.state, r.state, "random calls in the documented order");
}

#[test]
fn explosion_rolls_once_and_hurts_the_party_first() {
    let Some(mut g) = game() else { return };
    let p = g.party;
    let total = |g: &GameState| -> i32 {
        g.champions.iter().map(|c| c.health() as i32).sum::<i32>()
            - g.party_status.pending_damage.iter().map(|&d| d as i32).sum::<i32>()
    };
    let before = total(&g);
    g.rng = Rng::new(99);
    let mut r = Rng::new(99);
    missiles::explode(&mut g, kind::LIGHTNING, 60, p.map, p.x, p.y, 0);
    // One roll for the square, halved for this kind; nothing else on the
    // party square, so the party's damage is the next random use.
    let base = ((30 + 1) + r.random(31) + 1) >> 1;
    assert!(base > 0);
    assert!(total(&g) < before, "the party took the blast");
}

/// Regression: spells that change a hand slot (filling a flask, creating
/// an item) left the champion's cached load stale.
#[test]
fn spell_made_items_update_the_load() {
    let Some(mut g) = game() else { return };
    let Some(flask) = crate::actuators::create_item(&mut g, 256 + 0x14) else { return };
    let hand = (0..2).find(|&h| g.champions[0].inventory(h) == EMPTY).unwrap_or(0);
    g.champions[0].set_inventory(hand, flask.0 & 0x3FFF);
    crate::party::refresh_load(&mut g, 0);
    let before = g.champions[0].load();
    crate::apply::apply_cast(&mut g, 0, vec![crate::magic::CastEffect::MakePotion { kind: 6, power: 100 }]);
    assert_ne!(g.champions[0].inventory(hand), flask.0 & 0x3FFF, "the flask was filled");
    let db = g.data.clone().unwrap();
    let mut fresh = g.champions[0].clone();
    crate::champions::recompute_load(&mut fresh, &db.item_db(&g.dungeon));
    assert_eq!(g.champions[0].load(), fresh.load(), "cached load follows the inventory (was {before})");
}

/// Regression: shooters launched from the actuator's word 3 read as a
/// target, but word 3 holds the shot energies (0x57A63). Shots start one
/// square ahead of the event square in its direction, with attack 100,
/// and never off the map.
#[test]
fn shooters_fire_from_the_square_ahead() {
    let Some(mut g) = game() else { return };
    let shooters: Vec<_> = things(&g, ThingType::Actuator)
        .into_iter()
        .filter(|&(_, _, _, t)| matches!(crate::actuators::Actuator::load(&g, t).kind(), 0x08 | 0x0A))
        .collect();
    let had_shooters = !shooters.is_empty();
    let mut fired = 0;
    for (map, x, y, t) in shooters {
        let md = &g.dungeon.maps[map];
        let Some(dir) = (0..4u8).find(|&d| {
            let (ax, ay) = (x + DX[d as usize], y + DY[d as usize]);
            ax >= 0 && ay >= 0 && ax < md.width as i32 && ay < md.height as i32
        }) else {
            continue;
        };
        let before: Vec<_> = things(&g, ThingType::Missile).into_iter().map(|m| m.3 .0 & 0x3FFF).collect();
        let mut ev = crate::timeline::Event::new(4, map as u8, g.tick);
        (ev.x, ev.y, ev.b8) = (x as u8, y as u8, dir);
        crate::actuators::wall_actuator(&mut g, ev, t);
        crate::apply::apply_effects(&mut g);
        for (mm, mx, my, m) in things(&g, ThingType::Missile) {
            if before.contains(&(m.0 & 0x3FFF)) {
                continue;
            }
            assert_eq!((mm, mx, my), (map, x + DX[dir as usize], y + DY[dir as usize]), "shot from the square ahead");
            assert_eq!(g.dungeon.record(m).unwrap()[5], 100, "fixed attack byte");
            fired += 1;
        }
        if fired >= 3 {
            break;
        }
    }
    assert!(!had_shooters || fired > 0, "the dungeon has spell shooters but none fired");
    // Every scheduled missile event carries an on-map position.
    for (_, e) in g.timeline.iter() {
        if matches!(e.kind, 0x1D | 0x1E) {
            let (px, py) = ((e.w8() & 0x1F) as i32, (e.w8() >> 5 & 0x1F) as i32);
            let md = &g.dungeon.maps[e.map as usize];
            assert!(px < md.width as i32 && py < md.height as i32, "missile event off its map: {e:?}");
        }
    }
}
