//! Print a new game's starting food/water and RNG state (for aligning the
//! random sequence with the original).
use std::rc::Rc;
use dm2_engine::{assets, creatures, creatures::data::CreatureData, data::GameData, state::GameState};
use dm2_formats::gdat::Gdat;
fn main() {
    let a = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    // Creature data, as the frontend loads it, so creatures act.
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = Rc::new(CreatureData::load(gdat, &exe).unwrap());
    let _ = creatures::set_data;
    let mut g = GameState::new_game_full(&a.dungeon, gd, Some(cd));
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
