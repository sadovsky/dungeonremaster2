//! Timeline event handlers (docs/05-timeline.md, "Event types").

use crate::state::GameState;
use crate::timeline::Event;
use crate::world::PartyPos;

/// Run one due event (the processor at 0x59A5C dispatches on the type).
pub fn dispatch(_g: &mut GameState, ev: Event) {
    match ev.kind {
        // TODO: implement each event type from the docs/05 table.
        _ => {}
    }
}

/// Hook after the party changes square: pressure plates, pits, teleporters.
pub fn party_moved(_g: &mut GameState, _from: PartyPos) {
    // TODO: floor sensors (0x4A0FE / move routine 0x4B108).
}
