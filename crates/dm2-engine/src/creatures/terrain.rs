//! Creature movement legality (docs/08 "Movement legality"; 0x2D792).
//!
//! Each square is classified into terrain bits; a creature may enter it
//! when any of those bits is in its terrain mask (info +0x0A). Simplified
//! from the original: no group merging and no in-square positions yet.

use dm2_formats::dungeon::{Element, ThingType};

use crate::state::GameState;

pub const WALL: u16 = 0x0001;
pub const FLOOR: u16 = 0x0002;
pub const PIT_CLOSED: u16 = 0x0006;
pub const PIT_OPEN: u16 = 0x000C;
pub const PIT_HIDDEN: u16 = 0x8024;
pub const STAIRS: u16 = 0x0100;
pub const DOOR_MISSING: u16 = 0x0200;
pub const DOOR_SHUT: u16 = 0x4200;
pub const TELEPORT_IDLE: u16 = 0x0402;
pub const TELEPORT_ACTIVE: u16 = 0x2000;
pub const TRICK_WALL_A: u16 = 0x0040;
pub const TRICK_WALL_B: u16 = 0x0080;

/// Terrain class of a square for a creature with door size `size`.
/// Returns 0 for squares nothing can enter (solid rock, out of map).
pub fn class(g: &GameState, map: usize, x: i32, y: i32, size: u16) -> u16 {
    let sq = g.dungeon.square(map, x, y);
    let b = sq.0;
    match sq.element() {
        Element::Wall => WALL,
        Element::Floor => FLOOR,
        Element::Pit => {
            if b & 8 == 0 {
                PIT_CLOSED
            } else if b & 1 != 0 {
                PIT_HIDDEN
            } else {
                PIT_OPEN
            }
        }
        Element::Stairs => STAIRS,
        Element::Door => {
            let state = b & 7;
            // State 0 open, 5 destroyed; 1-3 partly open; 4 closed.
            if state == 0 || state == 5 {
                FLOOR
            } else if state == 4 || (4 - state as u16) < size.max(1) {
                DOOR_SHUT
            } else {
                FLOOR
            }
        }
        Element::Teleporter => {
            let active = b & 8 != 0;
            let has_dest = g.dungeon.things_at(map, x, y).iter().any(|t| t.kind() == ThingType::Teleporter);
            if active && has_dest {
                TELEPORT_ACTIVE
            } else {
                TELEPORT_IDLE
            }
        }
        Element::TrickWall => {
            if b & 4 != 0 {
                FLOOR
            } else if b & 1 != 0 {
                TRICK_WALL_B
            } else {
                TRICK_WALL_A
            }
        }
        Element::Rock => 0,
    }
}

/// May a creature with this terrain mask and size enter (map, x, y)?
pub fn can_enter(g: &GameState, map: usize, x: i32, y: i32, mask: u16, size: u16) -> bool {
    let c = class(g, map, x, y, size);
    c != 0 && c & mask != 0
}
