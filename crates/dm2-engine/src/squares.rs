//! Square actions: timeline event 0x04, dispatched on the target square's
//! element (docs/05-timeline.md, "Square actions").

use dm2_formats::dungeon::{Element, ThingType};

use crate::actuators::{self, resolve_action, CLEAR, TOGGLE};
use crate::creatures;
use crate::doors;
use crate::movement;
use crate::state::GameState;
use crate::timeline::Event;

pub fn square_action(g: &mut GameState, ev: Event) {
    let (map, x, y) = (ev.map as usize, ev.x as i32, ev.y as i32);
    if map >= g.dungeon.maps.len() {
        return;
    }
    match g.dungeon.square(map, x, y).element() {
        Element::Wall => actuators::wall_handler(g, ev),
        Element::Floor => actuators::floor_handler(g, ev),
        Element::Pit => pit(g, ev),
        Element::Door => doors::action(g, ev),
        Element::Teleporter => teleporter(g, ev),
        Element::TrickWall => trick_wall(g, ev),
        Element::Stairs | Element::Rock => {}
    }
}

/// Bit 3 follows the action (toggle inverts it); returns whether it is now set.
fn toggle_bit3(g: &mut GameState, ev: &Event) -> bool {
    let (map, x, y) = (ev.map as usize, ev.x as i32, ev.y as i32);
    let sq = g.dungeon.square(map, x, y).0;
    let on = resolve_action(ev.b9, sq & 8 != 0);
    g.dungeon.set_square(map, x, y, sq & !8 | (on as u8) << 3);
    on
}

/// Pit (0x58F5C): set opens it and drops what stands there.
fn pit(g: &mut GameState, ev: Event) {
    if toggle_bit3(g, &ev) {
        movement::drop_square(g, ev.map as usize, ev.x as i32, ev.y as i32);
    }
    actuators::floor_handler(g, ev);
}

/// Teleporter (0x58EDB): set activates it and teleports what stands there,
/// unless the record's word 2 bits 1-2 are both set.
fn teleporter(g: &mut GameState, ev: Event) {
    let (map, x, y) = (ev.map as usize, ev.x as i32, ev.y as i32);
    let locked = g
        .dungeon
        .things_at(map, x, y)
        .into_iter()
        .find(|t| t.kind() == ThingType::Teleporter)
        .and_then(|t| g.dungeon.record_word(t, 2))
        .is_some_and(|w| w & 6 == 6);
    if !locked && toggle_bit3(g, &ev) {
        movement::drop_square(g, map, x, y);
    }
    actuators::floor_handler(g, ev);
}

/// Trick wall (0x56A03): set opens it (bit 2); a close is retried next tick
/// while the party or a solid creature is inside.
fn trick_wall(g: &mut GameState, mut ev: Event) {
    let (map, x, y) = (ev.map as usize, ev.x as i32, ev.y as i32);
    let sq = g.dungeon.square(map, x, y).0;
    let action = if ev.b9 == TOGGLE { u8::from(sq & 4 != 0) } else { ev.b9 };
    if action == CLEAR {
        let party_in = g.party.map == map && g.party.x == x && g.party.y == y;
        let solid = creatures::group_at(g, map, x, y).is_some_and(|c| !creatures::is_non_material(g, c));
        if party_in || solid {
            ev.tick = ev.tick.wrapping_add(1);
            g.schedule(ev);
        } else {
            g.dungeon.set_square(map, x, y, sq & !4);
        }
    } else {
        g.dungeon.set_square(map, x, y, sq | 4);
    }
}
