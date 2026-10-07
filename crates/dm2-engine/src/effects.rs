//! Things the simulation asks the presentation layer to do (sounds, text),
//! plus a record of outcomes that other modules will consume (damage dealt
//! to the party or creatures). The frontend drains `GameState::effects`.

use dm2_formats::dungeon::ThingRef;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Play sound (cat, idx, 2, sub) heard from square (x, y) of `map`.
    Sound { cat: u8, idx: u8, sub: u8, map: usize, x: i32, y: i32 },
    /// As `Sound`, with the original's volume argument when it differs from
    /// the usual 200 (creature frame sounds use 0x80; 0x15CA9 argument 5).
    SoundAt { cat: u8, idx: u8, sub: u8, map: usize, x: i32, y: i32, vol: u8 },
    /// Show a text thing's message (floor text, wall text).
    ShowText { map: usize, thing: ThingRef },
    /// Champions took damage (`mask` = champions hit, as returned by 0x4766B).
    PartyDamaged { amount: u16, mask: u16 },
    /// The party fell `falls` levels this step.
    PartyFell { falls: u16 },
    /// A creature group was damaged by the dungeon (doors, falls, bumps).
    CreatureDamaged { thing: ThingRef, amount: u16 },
    /// A creature group's damage reached its HP; it plays its death action.
    CreatureDied { thing: ThingRef, map: usize, x: i32, y: i32 },
    /// A shooter actuator fired (missile creation belongs to the combat code).
    Shoot { map: usize, x: i32, y: i32, cell: u8, dir: u8, actuator: ThingRef },
    /// End-game actuator (0x12).
    EndGame,
}
