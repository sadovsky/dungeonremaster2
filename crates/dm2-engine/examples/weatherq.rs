//! Print the outdoor clock and weather state of a save after one tick, and
//! the darkness step it produces: weatherq SAVE
use std::rc::Rc;

use dm2_engine::{creatures, creatures::data::CreatureData, data::GameData, save, weather};
use dm2_formats::gdat::{Gdat, Key};

fn main() {
    let path = std::env::args().nth(1).expect("SAVE");
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = Rc::new(CreatureData::load(gdat.clone(), &exe).unwrap());
    let mut g = save::read(std::path::Path::new(&path), gd, Some(cd.clone())).expect("load");
    g.advance();
    let set = g.dungeon.maps[g.party.map].tileset;
    println!("map {} tileset {} tick {}", g.party.map, set, g.tick);
    println!("state attr 0x66 = {:?}", gdat.lookup(Key::new(8, set, 11, 0x66)));
    println!("env-state table bytes: {:?}", exe.len());
    println!("{:#?}", g.weather);
    println!("darkness step {}", creatures::fight::darkness_level(&g, &cd));
    println!("view {:?}", weather::view(&g));
}
