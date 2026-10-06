//! Write a save with the party moved to a chosen viewpoint, for comparing
//! views with the original game: posave IN OUT MAP X Y DIR [NAME]
use std::rc::Rc;

use dm2_engine::{creatures::data::CreatureData, data::GameData, save, world::PartyPos};
use dm2_formats::gdat::Gdat;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let n: Vec<i32> = a[2..6].iter().map(|s| s.parse().expect("MAP X Y DIR")).collect();
    let name = a.get(6).map(String::as_str).unwrap_or("VIEW");
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = CreatureData::load(gdat, &exe).ok().map(Rc::new);
    let mut g = save::read(std::path::Path::new(&a[0]), gd, cd).expect("load");
    g.party = PartyPos { map: n[0] as usize, x: n[1], y: n[2], dir: n[3] as u8 };
    let bytes = save::to_bytes(&mut g, name).expect("save");
    std::fs::write(&a[1], &bytes).unwrap();
    println!("wrote {} at map {} ({},{}) dir {}", a[1], n[0], n[1], n[2], n[3]);
}
