//! Creature hooks used by the dungeon mechanics (stubs).
//!
//! The creatures work (docs/08) will replace these bodies. Signatures follow
//! the original routines named in each comment.

use dm2_formats::dungeon::{ThingRef, ThingType};

use crate::effects::Effect;
use crate::state::GameState;

/// The creature group on a square, if any (0x2FBA9).
pub fn group_at(g: &GameState, map: usize, x: i32, y: i32) -> Option<ThingRef> {
    g.dungeon.things_at(map, x, y).into_iter().find(|t| t.kind() == ThingType::Creature)
}

/// Non-material creatures (attribute bit 0x20 via 0x1F9A3) ignore doors and
/// don't block trick walls.
pub fn is_non_material(_g: &GameState, _c: ThingRef) -> bool {
    false
}

/// Size class used by closing doors (attribute bits 6-7).
pub fn door_size(_g: &GameState, _c: ThingRef) -> u16 {
    1
}

/// Creatures whose info flag 0x19 bit 0x10 halves door damage.
pub fn halves_door_damage(_g: &GameState, _c: ThingRef) -> bool {
    false
}

/// Damage a creature group (0x24E62).
pub fn damage(g: &mut GameState, c: ThingRef, _map: usize, _x: i32, _y: i32, amount: u16) {
    g.effects.push(Effect::CreatureDamaged { thing: c, amount });
}

/// Teleporter scope class: 2 if the creature has attribute 0x1E, else 1.
pub fn teleport_class(_g: &GameState, _c: ThingRef) -> u8 {
    1
}

/// Things that don't fall into pits (0x49FCB): flying creatures, missiles,
/// clouds.
pub fn is_airborne(_g: &GameState, t: ThingRef) -> bool {
    matches!(t.kind(), ThingType::Missile | ThingType::Cloud)
}

/// Create a creature of `kind` facing `dir` (0x30BA6). Returns the new group.
pub fn spawn(_g: &mut GameState, _kind: u16, _map: usize, _x: i32, _y: i32, _dir: u8) -> Option<ThingRef> {
    None
}

/// Floor actuator types 0x0B/0x28 (0x56BA5): creature-affecting trap.
pub fn floor_trap(_g: &mut GameState, _map: usize, _ev_x: i32, _ev_y: i32, _actuator: ThingRef, _action: u8) {}

/// Floor actuator type 0x3A (0x2538C).
pub fn floor_signal(_g: &mut GameState, _map: usize, _x: i32, _y: i32, _set: bool) {}

/// Event 0x5E (0x30BA6 path from text kinds 0x13/0x16).
pub fn text_spawn_event(_g: &mut GameState, _map: usize, _x: i32, _y: i32, _param: u8) {}

/// Defence values a missile or melee hit needs (type info record, docs/08).
/// None until creature types are loaded; callers then apply raw damage.
pub fn defence(_g: &GameState, _c: ThingRef) -> Option<crate::combat::CreatureDefence> {
    None
}

/// Creature type flag 0x02: deflects spell missiles (0x17A7B).
pub fn reflects_spells(_g: &GameState, _c: ThingRef) -> bool {
    false
}

/// Facing of the group, used for the reflection table (parity only).
pub fn facing(_g: &GameState, _c: ThingRef) -> u8 {
    0
}

/// Fire/explosion resistance nibble (type info word +0x18 bits 4-7); 15
/// means immune.
pub fn resistance(_g: &GameState, _c: ThingRef) -> u8 {
    0
}

/// Tell a group something is coming (0x24E62 with code 0x2006); how
/// creatures notice incoming missiles.
pub fn alert(_g: &mut GameState, _c: ThingRef, _map: usize, _x: i32, _y: i32) {}
