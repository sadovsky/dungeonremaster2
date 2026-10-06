//! Dungeon Master II engine: game state and rendering, independent of the
//! windowing frontend. See docs/ for the reverse-engineering notes each
//! module follows.

pub mod assets;
pub mod events;
pub mod exe;
pub mod font;
pub mod gfx;
pub mod input;
pub mod layout;
pub mod rng;
pub mod state;
pub mod timeline;
pub mod ui;
pub mod viewport;
pub mod world;
