//! Load a save, run it for some ticks and print every sound request:
//! soundlog SAVE [TICKS]
use std::rc::Rc;

use dm2_engine::creatures::{self, data::CreatureData};
use dm2_engine::data::GameData;
use dm2_engine::effects::Effect;
use dm2_engine::save;
use dm2_formats::gdat::Gdat;

fn main() {
    let path = std::env::args().nth(1).expect("SAVE");
    let ticks: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(80);
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = CreatureData::load(gdat, &exe).ok().map(Rc::new);
    let mut g = save::read(std::path::Path::new(&path), gd, cd.clone()).expect("load");
    if let Some(cd) = cd {
        creatures::set_data(&mut g, cd);
    }
    for _ in 0..ticks {
        g.advance();
        for e in g.effects.drain(..) {
            let (cat, idx, sub, map, x, y, vol) = match e {
                Effect::Sound { cat, idx, sub, map, x, y } => (cat, idx, sub, map, x, y, 200),
                Effect::SoundAt { cat, idx, sub, map, x, y, vol, .. } => (cat, idx, sub, map, x, y, vol),
                _ => continue,
            };
            let p = g.party;
            println!(
                "tick {} sound cat {cat:#x} idx {idx:#x} sub {sub:#x} vol {vol} at map {map} ({x},{y}); party map {} ({},{}) dir {}",
                g.tick, p.map, p.x, p.y, p.dir
            );
        }
    }
}
