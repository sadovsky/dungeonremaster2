//! Dungeon Master II engine: game state and rendering, independent of the
//! windowing frontend. See docs/ for the reverse-engineering notes each
//! module follows.

pub mod actuators;
pub mod assets;
pub mod attrs;
pub mod creatures;
pub mod doors;
pub mod effects;
pub mod events;
pub mod gfx;
pub mod hooks;
pub mod layout;
#[cfg(test)]
mod mechanics_tests;
pub mod movement;
pub mod rng;
pub mod squares;
pub mod state;
pub mod timeline;
pub mod viewport;
pub mod world;
