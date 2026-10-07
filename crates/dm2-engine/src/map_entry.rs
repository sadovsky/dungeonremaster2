//! Map leave/enter pass (SKULL.EXE 0x59785, docs/05 "Map changes").
//!
//! When the party changes map, the original scans the map it leaves
//! (`entering == false`) and then the map it arrives on (`entering == true`).
//! On each square that has things it looks at the leading things of types
//! 0-3 only:
//! - enter/leave actuators (type 0x21) fire their target;
//! - "spawn on first entry" text things (mode bits 2, type 0x15) create
//!   their creature the first time the party arrives and mark themselves
//!   done (bit 0 of word 1).
//!
//! A save whose party map still has a pending first-entry spawn crashes
//! the original on load (SYSTEM ERROR 71), because its game start runs
//! this pass before the creature slot pool is initialised; see
//! `save::original_load_hazard`.

use dm2_formats::dungeon::{ThingRef, ThingType};

use crate::actuators::{self, Actuator};
use crate::creatures;
use crate::state::GameState;

/// Text-thing type for a first-entry creature spawn (word 1 bits 11-15).
pub const SPAWN_TEXT_TYPE: u16 = 0x15;
/// Actuator type fired on map enter/leave.
pub const ENTER_LEAVE_ACTUATOR: u16 = 0x21;

/// A first-entry spawn text: (creature type, already done).
pub fn spawn_text(g: &GameState, t: ThingRef) -> Option<(u16, bool)> {
    if t.kind() != ThingType::Text {
        return None;
    }
    let w1 = g.dungeon.record_word(t, 1)?;
    (w1 & 6 == 2 && w1 >> 11 == SPAWN_TEXT_TYPE).then_some(((w1 >> 3) & 0xFF, w1 & 1 != 0))
}

/// Squares of `map` holding a first-entry spawn that hasn't run yet.
pub fn pending_spawns(g: &GameState, map: usize) -> Vec<(i32, i32)> {
    let m = &g.dungeon.maps[map];
    let mut out = Vec::new();
    for x in 0..m.width as i32 {
        for y in 0..m.height as i32 {
            if leading_things(g, map, x, y).into_iter().any(|t| spawn_text(g, t).is_some_and(|(_, done)| !done)) {
                out.push((x, y));
            }
        }
    }
    out
}

/// The things the pass looks at: the leading run of types 0-3.
fn leading_things(g: &GameState, map: usize, x: i32, y: i32) -> Vec<ThingRef> {
    g.dungeon.things_at(map, x, y).into_iter().take_while(|t| (t.kind() as u8) <= ThingType::Actuator as u8).collect()
}

/// Run the pass over `map` (0x59785). The party's map must already be the
/// one being entered when `entering` is true, as in the original.
pub fn run(g: &mut GameState, map: usize, entering: bool) {
    let (w, h) = (g.dungeon.maps[map].width as i32, g.dungeon.maps[map].height as i32);
    for x in 0..w {
        for y in 0..h {
            for t in leading_things(g, map, x, y) {
                match t.kind() {
                    ThingType::Actuator => {
                        let a = Actuator::load(g, t);
                        if a.kind() != ENTER_LEAVE_ACTUATOR {
                            // TODO(0x59785): type 0x2C (animated ornament)
                            // restarts its animation on entry when word 2
                            // bit 0 is set.
                            continue;
                        }
                        let w2 = a.w2;
                        let action = if w2 & 0x18 == 0x18 {
                            u8::from(((w2 & 0x3F) >> 5 != 0) == entering)
                        } else {
                            if (w2 & 0x20 == 0) != entering {
                                continue;
                            }
                            ((w2 & 0x1F) >> 3) as u8
                        };
                        actuators::fire(g, map, &a, action, 0);
                    }
                    ThingType::Text if entering => {
                        let Some((kind, done)) = spawn_text(g, t) else { continue };
                        if done {
                            continue;
                        }
                        let w1 = g.dungeon.record_word(t, 1).unwrap_or(0);
                        g.dungeon.set_record_word(t, 1, w1 | 1);
                        let dir = g.rng.rand4() as u8;
                        creatures::spawn(g, kind, map, x, y, dir);
                    }
                    _ => {}
                }
            }
        }
    }
}
