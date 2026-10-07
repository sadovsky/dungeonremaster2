//! Print the remake's ordered random draws for an idle new game, one line per
//! draw: tick, creature (thing reference & 0x3FFF, 0 outside creature
//! processing) and call site, to compare with the original's draw log
//! creature by creature (docs/05, "Draw log").
//!
//! Usage: rngseq [END_TICK]   (default 2: ticks 0-1)
use std::rc::Rc;

use dm2_engine::{assets, creatures, creatures::data::CreatureData, data::GameData, rng, state::GameState};
use dm2_formats::gdat::Gdat;

fn main() {
    let end: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(2);
    let a = assets::Assets::load(&assets::default_data_dir()).expect("game data");
    let gd = Rc::new(GameData::load_default().expect("SKULL.EXE"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).expect("SKULL.EXE");
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).expect("GRAPHICS.DAT"));
    let cd = Rc::new(CreatureData::load(gdat, &exe).expect("creature data"));
    // Start before the new game so its setup draws (creature pass,
    // activations, recruit) are logged under tick 0, as in the original.
    rng::trace_seq_start();
    let mut g = GameState::new_game_full(&a.dungeon, gd, Some(cd.clone()));
    let _ = creatures::set_data;
    while g.tick < end {
        g.advance();
    }
    for (tick, file, line, cr) in rng::trace_seq_take() {
        let file = file.rsplit('/').next().unwrap_or(file);
        println!("{tick} {cr:#06x} {file}:{line}");
    }
}
