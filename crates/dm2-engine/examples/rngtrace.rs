//! Count the remake's random draws per call site for an idle new game, to
//! compare with the original's draw log (docs/05, "Random draws").
//!
//! Usage: rngtrace [END_TICK]   (default 158)
use std::collections::HashMap;
use std::rc::Rc;

use dm2_engine::{assets, creatures, creatures::data::CreatureData, data::GameData, rng, state::GameState};
use dm2_formats::gdat::Gdat;

fn report(label: &str, mut m: HashMap<(&'static str, u32), u32>, ticks: u32) {
    // Per-creature think counts (tagged "think") as a histogram.
    let thinks: Vec<u32> = m.iter().filter(|(k, _)| k.0 == "think").map(|(_, &v)| v).collect();
    if std::env::var_os("THINK_IDS").is_some() {
        let mut ids: Vec<(u32, u32)> = m.iter().filter(|(k, _)| k.0 == "think").map(|(k, &v)| (k.1, v)).collect();
        ids.sort();
        for (id, n) in &ids {
            println!("think {label} {id} {n}");
        }
    }
    m.retain(|k, _| k.0 != "think");
    if !thinks.is_empty() {
        let mut hist = std::collections::BTreeMap::new();
        for t in &thinks {
            *hist.entry(*t).or_insert(0u32) += 1;
        }
        println!("== {label}: {} creatures thinking, {} thinks; thinks -> creatures {:?}",
            thinks.len(), thinks.iter().sum::<u32>(), hist);
    }
    let total: u32 = m.values().sum();
    println!("== {label}: {total} draws over {ticks} ticks ({:.2}/tick)", total as f64 / ticks.max(1) as f64);
    let mut v: Vec<_> = m.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for ((file, line), n) in v.into_iter().take(20) {
        let short = file.rsplit("src/").next().unwrap_or(file);
        println!("  {n:6}  {short}:{line}");
    }
}

fn main() {
    let end: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(158);
    let a = assets::Assets::load(&assets::default_data_dir()).unwrap();
    rng::trace_start();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = Rc::new(CreatureData::load(gdat, &exe).unwrap());
    let _ = creatures::set_data;
    let mut g = GameState::new_game_full(&a.dungeon, gd, Some(cd));
    report("new game", rng::trace_take(), 1);
    rng::trace_start();
    while g.tick < 2 {
        g.advance();
    }
    report("ticks 0-1", rng::trace_take(), 2);
    rng::trace_start();
    while g.tick < end {
        g.advance();
    }
    report("idle ticks", rng::trace_take(), end - 2);
}
