//! List the pending timeline events in a save: timers PATH
use std::rc::Rc;

use dm2_engine::{creatures::data::CreatureData, data::GameData, save};
use dm2_formats::gdat::Gdat;

fn main() {
    let path = std::env::args().nth(1).expect("PATH");
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = CreatureData::load(gdat, &exe).ok().map(Rc::new);
    let g = save::read(std::path::Path::new(&path), gd, cd).expect("load");
    println!("tick {}  party map {} ({},{})", g.tick, g.party.map, g.party.x, g.party.y);
    let mut evs: Vec<_> = g.timeline.iter().map(|(s, e)| (s, *e)).collect();
    evs.sort_by_key(|(_, e)| e.tick);
    for (slot, e) in evs {
        println!(
            "slot {slot:3}  due {:6} (+{:4})  kind {:#04x}  map {:2}  prio {:3}  x {:2} y {:2}  b8 {:#04x} b9 {:#04x} w10 {:#06x}",
            e.tick,
            e.tick as i64 - g.tick as i64,
            e.kind,
            e.map,
            e.prio,
            e.x,
            e.y,
            e.b8,
            e.b9,
            e.w10
        );
    }
}
