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
