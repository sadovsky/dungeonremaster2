//! Print a save's weather state and the darkness step after loading it as
//! the original does: wxdump SAVE [TICKS]
use std::rc::Rc;
use dm2_engine::{creatures::{self, data::CreatureData}, data::GameData, save};
use dm2_formats::gdat::Gdat;
fn main() {
    let path = std::env::args().nth(1).expect("SAVE");
    let ticks: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(25);
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = Rc::new(CreatureData::load(gdat, &exe).unwrap());
    let mut g = save::read_as_original(std::path::Path::new(&path), gd, Some(cd.clone())).expect("load");
    println!("weather after load: {:?}", g.weather);
    for t in 0..=ticks {
        let d = creatures::fight::darkness_level(&g, &cd);
        if t < 3 || t % 5 == 0 { println!("tick {} darkness {} env {} hour_light {}", g.tick, d, g.weather.env, g.weather.hour_light); }
        g.advance();
    }
}
