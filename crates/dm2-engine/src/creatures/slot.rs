//! Active-creature slots (docs/08 "Active creature slots"; pool at 0x7F898,
//! activation 0x306A8).

use dm2_formats::dungeon::ThingRef;

use super::anim::NO_FRAME;

/// Slot pool size. The original sizes it from the dungeon's spare creature
/// count (docs/03); this matches that count.
pub const POOL_SIZE: usize = 75;

pub const NO_ACTION: u8 = 0xFF;

/// Packed square: x bits 0-4, y bits 5-9, map bits 10-15 (slot +0x0C, +0x18).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Packed(pub u16);

impl Packed {
    pub fn new(map: usize, x: i32, y: i32) -> Packed {
        Packed((x as u16 & 0x1F) | (y as u16 & 0x1F) << 5 | (map as u16 & 0x3F) << 10)
    }
    pub fn x(self) -> i32 {
        (self.0 & 0x1F) as i32
    }
    pub fn y(self) -> i32 {
        (self.0 >> 5 & 0x1F) as i32
    }
    pub fn map(self) -> usize {
        (self.0 >> 10) as usize
    }
}

/// The 34-byte slot, as named fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    /// +0x00: the creature group.
    pub thing: ThingRef,
    /// +0x02: pending timeline record.
    pub event: Option<u16>,
    /// +0x04: low byte of the tick of the last completed action.
    pub act_tick: u8,
    /// +0x06: regeneration timestamp, (tick / 4) − 1 at activation.
    pub regen_stamp: u8,
    /// +0x07: drawing jitter (bits 0-2 x, 3-5 y, bit 6 flip).
    pub jitter: u8,
    /// +0x08 / +0x0A: current sequence start frame and offset.
    pub seq_start: u16,
    pub seq_off: u16,
    /// +0x0C: home square.
    pub home: Packed,
    /// +0x0E / +0x10: program variables.
    pub vars: [u16; 2],
    /// +0x12: current program (−1 none) and +0x13 step.
    pub program: i8,
    pub step: i8,
    /// +0x14: damage taken but not yet applied.
    pub pending_damage: u16,
    /// +0x16: chosen behaviour set (index into the class's sets).
    pub set: i8,
    /// +0x17: queued action.
    pub queued: u8,
    /// +0x18: target square.
    pub target: Packed,
    /// +0x1A: current action.
    pub action: u8,
    /// +0x1B: direction argument; +0x1C cell argument.
    pub dir_arg: u8,
    pub cell_arg: u8,
    /// +0x1D: facing to adopt when a turn finishes.
    pub turn_to: u8,
    /// +0x1E: transform-into type or other argument; +0x1F stage counter.
    pub arg: u8,
    pub stage: u8,
    /// +0x20: movement mode used for the current move.
    pub mode: u8,
    /// +0x21: frame events stay armed.
    pub armed: u8,
    /// Default item kinds of the running behaviour (goal data words +8 and
    /// +10, globals 0x7F7D8/0x7F7DA): used by `N` and `]` when their
    /// arguments are −1. 0xFFFF = none.
    pub kind_a: u16,
    pub kind_b: u16,
    /// Where the group currently stands (the original finds this from the
    /// event's square; kept here for convenience).
    pub pos: Packed,
}

impl Slot {
    pub fn new(thing: ThingRef, map: usize, x: i32, y: i32, tick: u32) -> Slot {
        Slot {
            thing,
            event: None,
            act_tick: (tick as u8).wrapping_sub(0x7F),
            regen_stamp: ((tick >> 2) as u8).wrapping_sub(1),
            jitter: 0,
            seq_start: 0,
            seq_off: NO_FRAME,
            home: Packed::new(map, x, y),
            vars: [0; 2],
            program: -1,
            step: 0,
            pending_damage: 0,
            set: -1,
            queued: NO_ACTION,
            target: Packed::new(map, x, y),
            action: 0,
            dir_arg: 0,
            cell_arg: 0,
            turn_to: 0,
            arg: 0,
            stage: 0,
            mode: 0,
            armed: 0,
            kind_a: 0xFFFF,
            kind_b: 0xFFFF,
            pos: Packed::new(map, x, y),
        }
    }
}
