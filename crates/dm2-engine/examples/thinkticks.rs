//! Print "tick creature" for every creature think in an idle new game, to
//! compare with the original's draw log (docs/05, "Random draws").
//!
//! Usage: thinkticks [END_TICK]   (default 158)
use std::rc::Rc;

use dm2_engine::{assets, creatures::data::CreatureData, data::GameData, rng, state::GameState};
use dm2_formats::gdat::Gdat;

fn main() {
    let end: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(158);
    let a = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = Rc::new(CreatureData::load(gdat, &exe).unwrap());
    let mut g = GameState::new_game_full(&a.dungeon, gd, Some(cd));
    while g.tick < end {
        let t = g.tick;
        rng::trace_start();
        g.advance();
        let m = rng::trace_take();
        let draws: u32 = m.iter().filter(|(k, _)| k.0 != "think" && k.0 != "frame").map(|(_, &v)| v).sum();
        println!("tick {t} draws {draws}");
        let mut tags: Vec<(&str, u32, u32)> =
            m.iter().filter(|(k, _)| k.0 == "think" || k.0 == "frame").map(|(k, &v)| (k.0, k.1, v)).collect();
        tags.sort();
        for (kind, id, n) in tags {
            for _ in 0..n {
                println!("{kind} {t} {id}");
            }
        }
    }
}
