//! Timeline event handlers (docs/05-timeline.md, "Event types").

use crate::actuators;
use crate::creatures;
use crate::doors;
use crate::movement;
use crate::squares;
use crate::state::GameState;
use crate::timeline::Event;

/// Run one due event (the processor at 0x59A5C dispatches on the type).
pub fn dispatch(g: &mut GameState, ev: Event) {
    if ev.map as usize >= g.dungeon.maps.len() {
        return;
    }
    match ev.kind {
        0x01 => doors::step(g, ev),
        0x02 => doors::destroy(g, ev),
        0x04 => squares::square_action(g, ev),
        0x56 => actuators::clock_tick(g, ev),
        0x57 | 0x5B => actuators::rearm(g, ev),
        0x59 => actuators::release(g, ev),
        actuators::EV_ORNAMENT_SOUND => actuators::ornament_sound_event(g, ev),
        0x5C => actuators::set_visible(g, ev),
        0x5D => delayed_teleport(g, ev),
        0x4B => crate::champions::poison_event(g, &ev),
        0x0C | 0x0D | 0x47 | 0x48 => crate::party::event(g, ev),
        0x19 => crate::missiles::cloud_event(g, ev),
        0x1D | 0x1E => crate::missiles::flight_event(g, ev),
        0x46 => crate::apply::light_expired(g, ev),
        crate::weather::EV_WEATHER => crate::weather::event(g, ev),
        creatures::EV_CONTINUE | creatures::EV_STEP => creatures::event(g, ev),
        0x5E => creatures::text_spawn_event(g, ev.map as usize, ev.x as i32, ev.y as i32, ev.b9),
        actuators::EVENT_ORNAMENT_STEP => actuators::ornament_step(g, ev),
        crate::sound_queue::EV_DELAYED_SOUND => crate::sound_queue::event(g, ev),
        // TODO: 0x3C/0x3D (deferred arrival).
        _ => {}
    }
}

/// Event 0x5D: if the party is on the event's map, move it to the square in
/// bytes 6-7 (x bits 0-4, y bits 5-9) and face direction bits 10-11.
fn delayed_teleport(g: &mut GameState, ev: Event) {
    if ev.map as usize != g.party.map {
        return;
    }
    let w = u16::from_le_bytes([ev.x, ev.y]);
    let (x, y, dir) = ((w & 0x1F) as i32, (w >> 5 & 0x1F) as i32, (w >> 10 & 3) as u8);
    movement::teleport_party(g, x, y, g.party.map, dir);
}
