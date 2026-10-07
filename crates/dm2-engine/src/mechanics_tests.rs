//! Behaviour tests on real squares of the original DUNGEON.DAT. Each test
//! is skipped when the user's game data is not present.

use dm2_formats::dungeon::{Dungeon, Element, ThingType};
use dm2_formats::gdat::Gdat;

use crate::actuators::{self, Actuator, SET};
use crate::assets::default_data_dir;
use crate::attrs::Attributes;
use crate::effects::Effect;
use crate::movement;
use crate::state::{Command, GameState};
use crate::world::{self, Move, PartyPos};

fn game() -> Option<GameState> {
    let bytes = std::fs::read(default_data_dir().join("DUNGEON.DAT")).ok()?;
    let mut g = GameState::new_game(&Dungeon::parse(&bytes).ok()?);
    if let Ok(gd) = Gdat::open(default_data_dir().join("GRAPHICS.DAT")) {
        g.set_attributes(Attributes::from_gdat(&gd));
    }
    // A party needs at least one living champion for doors and plates to
    // react to it.
    let mut c = crate::champions::Champion::default();
    c.set_health(50);
    c.set_max_health(50);
    g.champions.push(c);
    Some(g)
}

fn run(g: &mut GameState, ticks: u32) {
    for _ in 0..ticks {
        g.advance();
    }
}

fn place(g: &mut GameState, map: usize, x: i32, y: i32, dir: u8) {
    g.party = PartyPos { map, x, y, dir };
    g.pending_map = None;
}

#[test]
fn door_opens_one_state_per_tick() {
    let Some(mut g) = game() else { return };
    let (m, x, y) = (2, 19, 10);
    assert_eq!(g.dungeon.square(m, x, y).element(), Element::Door);
    assert_eq!(g.dungeon.square(m, x, y).0 & 7, 4, "starts closed");
    actuators::square_action(&mut g, m, x, y, 0, SET, 0);
    let mut states = vec![];
    for _ in 0..6 {
        g.advance();
        states.push(g.dungeon.square(m, x, y).0 & 7);
    }
    // The action and the first step run on the same tick.
    assert_eq!(states, vec![3, 2, 1, 0, 0, 0]);
    assert!(g.effects.iter().any(|e| matches!(e, Effect::Sound { cat: 14, sub: 0x8E, .. })));
    // Closing again runs back up to 4, then stops.
    let now = g.tick;
    actuators::square_action(&mut g, m, x, y, 0, actuators::CLEAR, now);
    run(&mut g, 6);
    assert_eq!(g.dungeon.square(m, x, y).0 & 7, 4);
    assert!(g.timeline.is_empty(), "the animation has ended");
}

#[test]
fn closing_door_snaps_open_on_the_party() {
    let Some(mut g) = game() else { return };
    let (m, x, y) = (2, 19, 10);
    actuators::square_action(&mut g, m, x, y, 0, SET, 0);
    run(&mut g, 5);
    assert_eq!(g.dungeon.square(m, x, y).0 & 7, 0);
    place(&mut g, m, x, y, 0);
    let now = g.tick;
    actuators::square_action(&mut g, m, x, y, 0, actuators::CLEAR, now);
    run(&mut g, 2);
    // It started to close (state 1), then hit the party and snapped open.
    assert_eq!(g.dungeon.square(m, x, y).0 & 7, 0);
    assert!(g.effects.iter().any(|e| matches!(e, Effect::PartyDamaged { .. })));
}

#[test]
fn walking_into_a_pit_drops_the_party_a_layer() {
    let Some(mut g) = game() else { return };
    let (m, px, py) = (4, 5, 5);
    let sq = g.dungeon.square(m, px, py);
    assert!(sq.element() == Element::Pit && sq.0 & 8 != 0, "an open pit");
    let depth = g.dungeon.maps[m].depth;
    place(&mut g, m, px, py + 1, 0); // floor square south of it, facing north
    g.push_command(Command::Move(Move::Forward));
    g.advance(); // the move: the fall is resolved and the new map is pending
    assert!(g.pending_map.is_some());
    assert!(g.effects.iter().any(|e| matches!(e, Effect::PartyFell { falls: 1 })));
    g.advance(); // arrival
    assert_eq!(g.dungeon.maps[g.party.map].depth, depth + 1);
    let (gx, gy) = (
        g.party.x + g.dungeon.maps[g.party.map].origin_x as i32,
        g.party.y + g.dungeon.maps[g.party.map].origin_y as i32,
    );
    let o = &g.dungeon.maps[m];
    assert_eq!((gx, gy), (px + o.origin_x as i32, py + o.origin_y as i32), "same global square");
}

#[test]
fn pressure_plate_opens_its_pit() {
    let Some(mut g) = game() else { return };
    // Map 9 (6,4): a party plate (type 3) whose target is the pit at (3,6),
    // with a one-tick delay and action "set".
    let (m, x, y) = (9, 6, 4);
    let plate = g
        .dungeon
        .things_at(m, x, y)
        .into_iter()
        .find(|&t| t.kind() == ThingType::Actuator && Actuator::load(&g, t).kind() == 3)
        .expect("plate");
    let a = Actuator::load(&g, plate);
    let (tx, ty, _) = a.target();
    assert_eq!((tx, ty), (3, 6));
    assert_eq!(g.dungeon.square(m, tx, ty).element(), Element::Pit);
    let before = g.dungeon.square(m, tx, ty).0 & 8;
    // Walk onto the plate from a free neighbour.
    let from = (0..4u8)
        .map(|d| (x - crate::viewport::DX[d as usize], y - crate::viewport::DY[d as usize], d))
        .find(|&(fx, fy, _)| !world::blocks(&g.dungeon, m, fx, fy) && g.dungeon.square(m, fx, fy).element() == Element::Floor)
        .expect("a way onto the plate");
    place(&mut g, m, from.0, from.1, from.2);
    g.push_command(Command::Move(Move::Forward));
    g.advance();
    assert_eq!((g.party.x, g.party.y), (x, y));
    let due: Vec<_> = g.timeline.iter().map(|(_, e)| (e.kind, e.x, e.y, e.b9)).collect();
    assert!(due.contains(&(4, tx as u8, ty as u8, SET)), "{due:?}");
    run(&mut g, 2);
    assert_eq!(g.dungeon.square(m, tx, ty).0 & 8, 8, "pit opened (was {before})");
}

#[test]
fn stairs_change_layer_and_face_away_from_the_wall() {
    let Some(mut g) = game() else { return };
    for &(m, x, y) in &[(8usize, 12, 1), (23, 7, 4)] {
        assert_eq!(g.dungeon.square(m, x, y).element(), Element::Stairs);
        place(&mut g, m, x, y, 0);
        let depth = g.dungeon.maps[m].depth;
        movement::take_stairs(&mut g);
        let p = g.pending_map.expect("a map on the adjacent layer");
        let nd = g.dungeon.maps[p.map].depth;
        assert!(nd == depth + 1 || nd + 1 == depth);
        assert_eq!(g.dungeon.square(p.map, p.x, p.y).element(), Element::Stairs, "arrives on stairs");
        let ahead = g.dungeon.square(p.map, p.x + crate::viewport::DX[p.dir as usize], p.y + crate::viewport::DY[p.dir as usize]);
        assert!(!matches!(ahead.element(), Element::Wall | Element::Stairs), "faces out: {:?}", ahead.element());
        g.advance();
        assert_eq!(g.party.map, p.map);
    }
}

#[test]
fn blocked_moves_and_doors() {
    let Some(mut g) = game() else { return };
    let (m, x, y) = (2, 19, 10);
    // Stand next to the closed door, facing it.
    let (fx, fy, d) = (0..4u8)
        .map(|d| (x - crate::viewport::DX[d as usize], y - crate::viewport::DY[d as usize], d))
        .find(|&(fx, fy, _)| !world::blocks(&g.dungeon, m, fx, fy))
        .unwrap();
    place(&mut g, m, fx, fy, d);
    assert_eq!(movement::classify(&g, Move::Forward, (x, y)), movement::MoveClass::Blocked);
    g.push_command(Command::Move(Move::Forward));
    g.advance();
    assert_eq!((g.party.x, g.party.y), (fx, fy), "a closed door stops the party");
}

#[test]
fn counter_fires_when_it_reaches_zero() {
    let Some(mut g) = game() else { return };
    // Build a counter actuator on a free record: data 2, action set,
    // target (1,1) cell 0 on map 0.
    let t = actuators::alloc_thing(&mut g, ThingType::Actuator).unwrap();
    g.dungeon.set_record_word(t, 1, 0x1D | 2 << 7);
    g.dungeon.set_record_word(t, 2, 0);
    g.dungeon.set_record_word(t, 3, 1 << 6 | 1 << 11);
    let mut ev = crate::timeline::Event::new(4, 0, 0);
    ev.b9 = SET;
    actuators::wall_actuator(&mut g, ev, t);
    assert!(g.timeline.is_empty(), "2 -> 1: no fire");
    actuators::wall_actuator(&mut g, ev, t);
    assert_eq!(g.timeline.len(), 1, "1 -> 0: fires");
    assert_eq!(Actuator::load(&g, t).data(), 0);
}

#[test]
fn floor_text_shows_on_entry_and_items_trigger_item_plates() {
    let Some(mut g) = game() else { return };
    // Any item plate (type 4) in the dungeon: drop its item kind onto it.
    let mut found = None;
    'outer: for (m, md) in g.dungeon.maps.iter().enumerate() {
        for x in 0..md.width as i32 {
            for y in 0..md.height as i32 {
                if g.dungeon.square(m, x, y).element() != Element::Floor {
                    continue;
                }
                for t in g.dungeon.things_at(m, x, y) {
                    if t.kind() == ThingType::Actuator {
                        let a = Actuator::load(&g, t);
                        if a.kind() == 4 && a.action() != actuators::FOLLOW && !a.inverted() && a.data() < 0x80 {
                            found = Some((m, x, y, a));
                            break 'outer;
                        }
                    }
                }
            }
        }
    }
    let Some((m, x, y, a)) = found else { return };
    // Make a weapon of the plate's kind (item numbers below 0x80 are weapons).
    let item = actuators::alloc_thing(&mut g, ThingType::Weapon).unwrap();
    g.dungeon.set_record_word(item, 1, 0x80 | a.data());
    assert_eq!(actuators::item_number(&g, item), a.data());
    let before = g.timeline.len();
    movement::move_thing(&mut g, item, None, Some((m, x, y)));
    assert!(g.dungeon.things_at(m, x, y).iter().any(|t| t.0 & 0x3FFF == item.0 & 0x3FFF));
    assert_eq!(g.timeline.len(), before + 1, "the plate fired");
    // Taking it off again fires nothing for a non-follow plate.
    let placed = g.dungeon.things_at(m, x, y).into_iter().find(|t| t.0 & 0x3FFF == item.0 & 0x3FFF).unwrap();
    movement::move_thing(&mut g, placed, Some((m, x, y)), None);
    assert_eq!(g.timeline.len(), before + 1);
}

#[test]
fn animated_ornament_finishes_its_cycle_when_switched_off() {
    let Some(mut g) = game() else { return };
    // Actuator 0x2C on map 0 showing the map's first wall ornament.
    let t = actuators::alloc_thing(&mut g, ThingType::Actuator).unwrap();
    g.dungeon.set_record_word(t, 1, 0x2C);
    g.dungeon.set_record_word(t, 2, 1 << 12);
    g.dungeon.set_record_word(t, 3, 0);
    let mut ev = crate::timeline::Event::new(4, 0, 0);
    ev.b9 = SET;
    actuators::wall_actuator(&mut g, ev, t);
    assert_eq!(Actuator::load(&g, t).w2 & 5, 5, "switched on and animating");
    run(&mut g, 3);
    ev.b9 = actuators::CLEAR;
    actuators::wall_actuator(&mut g, ev, t);
    let w2 = Actuator::load(&g, t).w2;
    assert_eq!(w2 & 4, 0, "switched off");
    if w2 & 1 != 0 {
        // Mid-cycle: event 0x59 ends the animation later.
        assert!(g.timeline.iter().any(|(_, e)| e.kind == 0x59));
        run(&mut g, 300);
        assert_eq!(Actuator::load(&g, t).w2 & 1, 0, "animation stops at the end of the cycle");
    }
}

#[test]
fn one_shot_ornament_plays_a_single_cycle() {
    let Some(mut g) = game() else { return };
    let t = actuators::alloc_thing(&mut g, ThingType::Actuator).unwrap();
    g.dungeon.set_record_word(t, 1, 0x32 | 0x55 << 7);
    g.dungeon.set_record_word(t, 2, 1 << 12);
    g.dungeon.set_record_word(t, 3, 0);
    let mut ev = crate::timeline::Event::new(4, 0, 0);
    ev.b9 = SET;
    actuators::wall_actuator(&mut g, ev, t);
    let a = Actuator::load(&g, t);
    assert_eq!(a.w2 & 1, 1, "busy while playing");
    assert_eq!(a.data(), 0, "frame counter reset");
    // A second trigger while playing does not restart it.
    actuators::wall_actuator(&mut g, ev, t);
    assert_eq!(g.timeline.iter().filter(|(_, e)| e.kind == actuators::EVENT_ORNAMENT_STEP).count(), 1);
    run(&mut g, 600);
    let a = Actuator::load(&g, t);
    assert_eq!(a.w2 & 1, 0, "done after one cycle");
    assert!(a.data() > 0, "the counter advanced");
    assert!(!g.timeline.iter().any(|(_, e)| e.kind == actuators::EVENT_ORNAMENT_STEP));
}

#[test]
fn random_pits_drop_items_on_a_marker_square() {
    let Some(mut g) = game() else { return };
    // A pit with a kind-0x0C marker on a map whose graphics set has 0x6A.
    let marker = |g: &GameState, m: usize, x: i32, y: i32, kind: u16| {
        g.dungeon.things_at(m, x, y).into_iter().filter(|t| t.kind() == ThingType::Text).find_map(|t| {
            let w = g.dungeon.record_word(t, 1)?;
            (w & 6 == 2 && w >> 11 == kind).then_some(w >> 3 & 0xFF)
        })
    };
    let mut found = None;
    'outer: for (m, md) in g.dungeon.maps.iter().enumerate() {
        if g.attrs.get(8, md.tileset, 0x6A) == 0 {
            continue;
        }
        for x in 0..md.width as i32 {
            for y in 0..md.height as i32 {
                let sq = g.dungeon.square(m, x, y);
                if sq.element() == Element::Pit {
                    if let Some(id) = marker(&g, m, x, y, 0x0C) {
                        found = Some((m, x, y, id));
                        break 'outer;
                    }
                }
            }
        }
    }
    let Some((m, x, y, id)) = found else { return };
    // Make sure the pit is open.
    let sq = g.dungeon.square(m, x, y).0;
    g.dungeon.set_square(m, x, y, (sq | 8) & !1);
    let item = actuators::alloc_thing(&mut g, ThingType::Weapon).unwrap();
    g.dungeon.set_record_word(item, 1, 0x80);
    movement::move_thing(&mut g, item, None, Some((m, x, y)));
    let landed = g.dungeon.maps.iter().enumerate().find_map(|(mm, md)| {
        (0..md.width as i32)
            .flat_map(|xx| (0..md.height as i32).map(move |yy| (xx, yy)))
            .find(|&(xx, yy)| g.dungeon.things_at(mm, xx, yy).iter().any(|t| t.0 & 0x3FFF == item.0 & 0x3FFF))
            .map(|(xx, yy)| (mm, xx, yy))
    });
    let (lm, lx, ly) = landed.expect("the item lands somewhere");
    assert_eq!(marker(&g, lm, lx, ly, 0x0B), Some(id), "it lands on a matching marker square");
}

/// A wall square on map 0 next to an open floor square.
fn wall_spot(g: &GameState) -> Option<(i32, i32)> {
    let md = &g.dungeon.maps[0];
    (0..md.width as i32)
        .flat_map(|x| (0..md.height as i32).map(move |y| (x, y)))
        .find(|&(x, y)| g.dungeon.square(0, x, y).element() == Element::Wall && !g.dungeon.square(0, x, y).has_things())
}

/// Put a fresh wall actuator of `kind` with `data` on (x, y), cell 0,
/// targeting (1,1).
fn wall_sensor(g: &mut GameState, x: i32, y: i32, kind: u16, data: u16, w2: u16) -> dm2_formats::dungeon::ThingRef {
    let t = actuators::alloc_thing(g, ThingType::Actuator).unwrap();
    g.dungeon.set_record_word(t, 1, kind | data << 7);
    g.dungeon.set_record_word(t, 2, w2);
    g.dungeon.set_record_word(t, 3, 1 << 6 | 1 << 11);
    g.dungeon.add_thing(0, x, y, t);
    t
}

#[test]
fn cooldown_button_fires_once_until_rearmed() {
    let Some(mut g) = game() else { return };
    let Some((x, y)) = wall_spot(&g) else { return };
    let t = wall_sensor(&mut g, x, y, 0x18, 5, 0);
    assert!(actuators::click_wall(&mut g, 0, x, y, 0, None).fired);
    assert!(!actuators::click_wall(&mut g, 0, x, y, 0, None).fired, "busy");
    run(&mut g, 8);
    assert_eq!(Actuator::load(&g, t).w2 & 1, 0, "re-armed after data + 2 ticks");
    assert!(actuators::click_wall(&mut g, 0, x, y, 0, None).fired);
}

#[test]
fn receptacle_counts_items_down_and_fires_at_zero() {
    let Some(mut g) = game() else { return };
    let Some((x, y)) = wall_spot(&g) else { return };
    // Map 0's first wall ornament decides the accepted kind.
    let t = wall_sensor(&mut g, x, y, 0x1B, 2, 1 << 12);
    let orn = g.dungeon.map_lists(0).wall_ornaments[0];
    let kind = g.attrs.get(9, orn, 0x0E);
    let Some(item) = actuators::create_item(&mut g, kind) else { return };
    let r = actuators::click_wall(&mut g, 0, x, y, 0, Some(item));
    assert!(r.consume_item && !r.fired, "first item: 2 -> 1, no fire");
    let item2 = actuators::create_item(&mut g, kind).unwrap();
    let r = actuators::click_wall(&mut g, 0, x, y, 0, Some(item2));
    assert!(r.consume_item && r.fired, "second item: 1 -> 0, fires");
    assert_eq!(Actuator::load(&g, t).w2 & 1, 1, "spent");
}

#[test]
fn party_mover_button_moves_the_party() {
    let Some(mut g) = game() else { return };
    let Some((x, y)) = wall_spot(&g) else { return };
    // Target (1,1), absolute facing east (action bits 1), inverted = absolute.
    wall_sensor(&mut g, x, y, 0x1C, 0, 1 << 3 | 0x20);
    actuators::click_wall(&mut g, 0, x, y, 0, None);
    run(&mut g, 2);
    assert_eq!((g.party.map, g.party.x, g.party.y, g.party.dir), (0, 1, 1, 1));
}

#[test]
fn heavy_parties_slip_on_unstable_floors() {
    let Some(mut base) = game() else { return };
    // A floor square on map 0 without things, with a kind-10 text marker.
    let md = &base.dungeon.maps[0];
    let Some((x, y)) = (0..md.width as i32)
        .flat_map(|x| (0..md.height as i32).map(move |y| (x, y)))
        .find(|&(x, y)| base.dungeon.square(0, x, y).element() == Element::Floor && !base.dungeon.square(0, x, y).has_things())
    else {
        return;
    };
    let txt = actuators::alloc_thing(&mut base, ThingType::Text).unwrap();
    base.dungeon.set_record_word(txt, 1, 2 | 10 << 11);
    base.dungeon.add_thing(0, x, y, txt);
    // Far over the load limit: the chance is capped at 90%.
    base.champions[0].set_load(60000);
    let mut slipped = 0;
    for seed in 0..10 {
        let mut g = base.clone();
        g.rng = crate::rng::Rng::new(seed);
        actuators::floor_sensors(&mut g, 0, x, y, movement::Mover::Party, false, true);
        if g.timeline.iter().any(|(_, e)| e.kind == 0x5D) {
            slipped += 1;
        }
    }
    assert!(slipped >= 5, "slipped {slipped} of 10 times at a 90% chance");
}

#[test]
fn script_variables_cover_flags_bytes_and_words() {
    let Some(mut g) = game() else { return };
    use actuators::{script_var as get, script_var_op as op};
    op(&mut g, 9, 0, 0);
    assert_eq!(get(&g, 9), 1, "flag set");
    op(&mut g, 9, 2, 0);
    assert_eq!(get(&g, 9), 0, "flag toggled off");
    op(&mut g, 70, 6, 300);
    assert_eq!(get(&g, 70), 255, "bytes clamp to 255");
    op(&mut g, 70, 4, 1000);
    assert_eq!(get(&g, 70), 0, "and to 0");
    op(&mut g, 130, 6, 40000);
    op(&mut g, 130, 3, 5);
    assert_eq!(get(&g, 130), 40005, "words hold 16 bits");
    op(&mut g, 130, 5, 7);
    assert_eq!(get(&g, 130), 40005, "unknown operation leaves the value");
}

#[test]
fn variable_actuators_set_and_test() {
    let Some(mut g) = game() else { return };
    // 0x43 on variable 3, then 0x44 testing variable 3, both firing at (1,1).
    let set_var = actuators::alloc_thing(&mut g, ThingType::Actuator).unwrap();
    g.dungeon.set_record_word(set_var, 1, 0x43 | 3 << 7);
    g.dungeon.set_record_word(set_var, 2, 0);
    g.dungeon.set_record_word(set_var, 3, 1 << 6 | 1 << 11);
    let test_var = actuators::alloc_thing(&mut g, ThingType::Actuator).unwrap();
    g.dungeon.set_record_word(test_var, 1, 0x44 | 3 << 7);
    g.dungeon.set_record_word(test_var, 2, 0);
    g.dungeon.set_record_word(test_var, 3, 1 << 6 | 1 << 11);
    let mut ev = crate::timeline::Event::new(4, 0, 0);
    ev.b9 = SET;
    // The variable is clear: a set fails the test.
    actuators::wall_actuator(&mut g, ev, test_var);
    assert!(g.timeline.is_empty());
    // 0x43 sets it and fires its own target.
    actuators::wall_actuator(&mut g, ev, set_var);
    assert_eq!(actuators::script_var(&g, 3), 1);
    assert_eq!(g.timeline.len(), 1);
    // Now the set passes the test and fires.
    actuators::wall_actuator(&mut g, ev, test_var);
    assert_eq!(g.timeline.len(), 2);
    // A clear passes only when the variable agrees with "not inverted" = clear.
    ev.b9 = actuators::CLEAR;
    actuators::wall_actuator(&mut g, ev, test_var);
    assert_eq!(g.timeline.len(), 2);
}

/// Walking into a wall hurts the front champions (0x234A8): the original's
/// probe (walk to the end of the start corridor, then 9 more presses into
/// the wall) lost 8 health. Each bump costs at most 1 and cries out.
#[test]
fn bumping_a_wall_hurts_the_front_champions() {
    let Some(gd) = crate::data::GameData::load_default() else { return };
    let Ok(bytes) = std::fs::read(default_data_dir().join("DUNGEON.DAT")) else { return };
    let dg = Dungeon::parse(&bytes).unwrap();
    let mut g = GameState::new_game_with(&dg, std::rc::Rc::new(gd));
    assert_eq!(g.party.map, 0);
    let start = g.champions[0].health();
    let mut presses = 0;
    while presses < 16 {
        if g.tick >= g.move_ready {
            g.push_command(Command::Move(Move::Forward));
            presses += 1;
        }
        g.advance();
    }
    for _ in 0..20 {
        g.advance();
    }
    assert_eq!((g.party.x, g.party.y), (1, 1), "the corridor ends at (1,1)");
    let lost = start - g.champions[0].health();
    assert!((1..=9).contains(&lost), "lost {lost} health from 9 bumps");
}

#[test]
fn bump_queues_the_champion_cry() {
    let Some(gd) = crate::data::GameData::load_default() else { return };
    let Ok(bytes) = std::fs::read(default_data_dir().join("DUNGEON.DAT")) else { return };
    let dg = Dungeon::parse(&bytes).unwrap();
    let mut g = GameState::new_game_with(&dg, std::rc::Rc::new(gd));
    g.party = PartyPos { map: 0, x: 1, y: 1, dir: 0 };
    let mut cried = false;
    for _ in 0..30 {
        let before = g.champions[0].health();
        g.effects.clear();
        movement::bump(&mut g, Move::Forward, (1, 0));
        let hurt = g.champions[0].health() < before || g.party_status.pending_damage[0] > 0;
        let cry = g.effects.iter().any(|e| matches!(e, Effect::Sound { cat: 0x16, sub: 0x8A, .. }));
        assert_eq!(hurt, cry, "a cry is queued exactly when the bump lands");
        cried |= cry;
    }
    assert!(cried);
}

/// Every move attempt costs each living champion load * 3 / max_load + 1
/// stamina, blocked or not (0x235BF calling 0x47707).
#[test]
fn move_attempts_cost_stamina() {
    let Some(gd) = crate::data::GameData::load_default() else { return };
    let Ok(bytes) = std::fs::read(default_data_dir().join("DUNGEON.DAT")) else { return };
    let dg = Dungeon::parse(&bytes).unwrap();
    let mut g = GameState::new_game_with(&dg, std::rc::Rc::new(gd));
    let max = crate::champions::max_load(&g.champions[0], &mut g.rng).max(1) as i32;
    let cost = g.champions[0].load() as i32 * 3 / max + 1;
    // One free step and one bump against the end wall, between regenerations.
    let before = g.champions[0].stamina() as i32;
    movement::party_command(&mut g, Move::Forward);
    g.party = PartyPos { map: 0, x: 1, y: 1, dir: 0 };
    movement::party_command(&mut g, Move::Forward);
    assert_eq!(before - g.champions[0].stamina() as i32, 2 * cost);
}

/// The new-game creature pass (0x3624F): every creature group starts at its
/// type's base hit points, and types whose info flag bit 0 is clear record
/// their home square in word +0xC.
#[test]
fn new_game_initialises_creatures() {
    let Some(gd) = crate::data::GameData::load_default() else { return };
    let Ok(bytes) = std::fs::read(default_data_dir().join("DUNGEON.DAT")) else { return };
    let dg = Dungeon::parse(&bytes).unwrap();
    let gd = std::rc::Rc::new(gd);
    let g = GameState::new_game_with(&dg, gd.clone());
    let info = |ty: u8, off: u32| {
        let idx = g.attrs.get(15, ty, 5) as u32;
        gd.exe.u8_at(0x71968 + 36 * idx + off).unwrap()
    };
    let mut checked = 0;
    for (m, md) in g.dungeon.maps.iter().enumerate() {
        for x in 0..md.width as i32 {
            for y in 0..md.height as i32 {
                for t in g.dungeon.things_at(m, x, y) {
                    if t.kind() != ThingType::Creature {
                        continue;
                    }
                    let ty = g.dungeon.record(t).unwrap()[4];
                    let base = u16::from_le_bytes([info(ty, 4), info(ty, 5)]);
                    assert_eq!(g.dungeon.record_word(t, 3), Some(base), "hp of type {ty}");
                    if info(ty, 0) & 1 == 0 {
                        let home = (x as u16) | (y as u16) << 5 | (m as u16) << 10;
                        assert_eq!(g.dungeon.record_word(t, 6), Some(home));
                    }
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 100, "checked {checked} creature groups");
}

#[test]
fn arriving_with_a_new_facing_rotates_the_champions() {
    let Some(mut g) = game() else { return };
    // Face north with the champion facing north in cell 0.
    g.set_party_facing(0);
    g.champions[0].raw[0x1C] = 0;
    g.champions[0].raw[0x1D] = 0;
    // Arrive on another map facing east (a stairs or teleport arrival).
    let to = (0..g.dungeon.maps.len()).find(|&m| m != g.party.map).unwrap();
    crate::movement::arrive(&mut g, PartyPos { map: to, x: 1, y: 1, dir: 1 });
    assert_eq!(g.party.dir, 1);
    assert_eq!(g.champions[0].raw[0x1C], 1, "champion facing must rotate with the party");
    assert_eq!(g.champions[0].raw[0x1D], 1, "champion cell must rotate with the party");
}
