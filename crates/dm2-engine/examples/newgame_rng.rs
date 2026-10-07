//! Print a new game's starting food/water and RNG state (for aligning the
//! random sequence with the original).
use std::rc::Rc;
use dm2_engine::{assets, data::GameData, state::GameState};
fn main() {
    let a = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let mut g = GameState::new_game_with(&a.dungeon, gd);
    let c = &g.champions[0];
    println!("food {} water {} rng {:#x}", c.food(), c.water(), g.rng.state);
    // Optional: run idle to the given ticks and report the draw count there.
    for t in std::env::args().skip(1).filter_map(|s| s.parse::<u32>().ok()) {
        while g.tick < t {
            g.advance();
        }
        println!("tick {} rng {:#x}", g.tick, g.rng.state);
    }
}
