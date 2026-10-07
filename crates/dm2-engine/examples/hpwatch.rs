//! Load a save and run it idle, printing each change in the champions'
//! health and the game-over flag: hpwatch SAVE [TICKS]
use std::rc::Rc;

use dm2_engine::{creatures::data::CreatureData, data::GameData, save};
use dm2_formats::gdat::Gdat;

fn main() {
    let path = std::env::args().nth(1).expect("SAVE");
    let ticks: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(150);
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = CreatureData::load(gdat, &exe).ok().map(Rc::new);
    let mut g = save::read(std::path::Path::new(&path), gd, cd).expect("load");
    let hp = |g: &dm2_engine::state::GameState| g.champions.iter().map(|c| c.health()).collect::<Vec<_>>();
    let mut last = hp(&g);
    println!("tick {} party map {} ({},{}) health {:?}", g.tick, g.party.map, g.party.x, g.party.y, last);
    for _ in 0..ticks {
        g.advance();
        let now = hp(&g);
        if now != last {
            println!("tick {} health {:?}", g.tick, now);
            last = now;
        }
        if g.game_over {
            println!("tick {} game over", g.tick);
            break;
        }
    }
}
