//! What does the remake do on the first tick after loading a save? Prints
//! the active creature slots before and after the tick, whether the
//! play-start pass is pending, and the draws made (docs/05, "Creature pass
//! at play start").
//!
//! Usage: loadpass SAVE
use std::rc::Rc;

use dm2_engine::{assets, creatures::data::CreatureData, data::GameData, rng, save};
use dm2_formats::gdat::Gdat;

fn main() {
    let path = std::env::args().nth(1).expect("SAVE");
    let gd = Rc::new(GameData::load_default().expect("SKULL.EXE"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).expect("SKULL.EXE");
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).expect("GRAPHICS.DAT"));
    let cd = Rc::new(CreatureData::load(gdat, &exe).expect("creature data"));
    let mut g = save::read(std::path::Path::new(&path), gd, Some(cd)).expect("load");
    let active = |g: &dm2_engine::state::GameState| g.creature_slots.iter().flatten().count();
    println!(
        "loaded: tick {} map {} active slots {} play-start pending {} map seen {:?}",
        g.tick,
        g.party.map,
        active(&g),
        g.play_start_pending,
        g.creature_map_seen
    );
    rng::trace_seq_start();
    g.advance();
    let draws = rng::trace_seq_take();
    println!("after one tick: active slots {}, {} draws", active(&g), draws.len());
    let mut sites = std::collections::BTreeMap::new();
    for (_, file, line, _) in &draws {
        *sites.entry(format!("{}:{}", file.rsplit('/').next().unwrap_or(file), line)).or_insert(0) += 1;
    }
    for (s, n) in sites {
        println!("  {n:4}  {s}");
    }
}
