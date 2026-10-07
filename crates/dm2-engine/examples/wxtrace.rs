//! Per-tick weather state after loading a save as the original does:
//! wxtrace SAVE [TICKS]
use std::rc::Rc;

use dm2_engine::{creatures::data::CreatureData, data::GameData, save};
use dm2_formats::gdat::Gdat;

fn main() {
    let path = std::env::args().nth(1).expect("SAVE");
    let ticks: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(100);
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = Rc::new(CreatureData::load(gdat, &exe).unwrap());
    let mut g = save::read_as_original(std::path::Path::new(&path), gd, Some(cd)).expect("load");
    println!("after load: {:?}", g.weather);
    for _ in 0..=ticks {
        let w = &g.weather;
        println!(
            "tick {} map {} env {} rain {} rain_on {} cloud {} cl {} kind {} step {} pat {} storm {} flash {} bolt {:?} rng {:#010x}",
            g.tick, g.party.map, w.env, w.rain, w.rain_on, w.cloud, w.cloud_level, w.kind, w.step, w.pattern, w.storm,
            w.flash, w.bolt, g.rng.state
        );
        g.advance();
    }
}
