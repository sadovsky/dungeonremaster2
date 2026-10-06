//! Dungeon Master II engine: game state and rendering, independent of the
//! windowing frontend. See docs/ for the reverse-engineering notes each
//! module follows.

pub mod actuators;
pub mod assets;
pub mod attrs;
pub mod champions;
pub mod combat;
pub mod creatures;
pub mod doors;
pub mod effects;
pub mod events;
pub mod exe;
pub mod exe_tables;
pub mod font;
pub mod gfx;
#[cfg(test)]
mod mechanics_tests;
pub mod hooks;
pub mod input;
pub mod items;
pub mod layout;
pub mod magic;
pub mod movement;
pub mod rng;
pub mod squares;
pub mod state;
pub mod timeline;
pub mod ui;
pub mod viewport;
pub mod world;
