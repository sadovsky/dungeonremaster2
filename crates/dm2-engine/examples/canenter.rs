//! Why can or can't a creature enter a square? canenter THING_HEX MAP X Y
//! Prints the creature's type, terrain mask and door size, and the square's
//! terrain class (docs/08 "Movement legality").
use std::rc::Rc;

use dm2_engine::{assets, creatures, creatures::data::CreatureData, data::GameData, state::GameState};
use dm2_formats::dungeon::ThingRef;
use dm2_formats::gdat::Gdat;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let thing = u16::from_str_radix(args[0].trim_start_matches("0x"), 16).expect("hex thing");
    let (map, x, y): (usize, i32, i32) = (args[1].parse().unwrap(), args[2].parse().unwrap(), args[3].parse().unwrap());
    let a = assets::Assets::load(&assets::default_data_dir()).expect("game data");
    let gd = Rc::new(GameData::load_default().expect("SKULL.EXE"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).expect("SKULL.EXE");
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).expect("GRAPHICS.DAT"));
    let cd = Rc::new(CreatureData::load(gdat, &exe).expect("creature data"));
    let g = GameState::new_game_full(&a.dungeon, gd, Some(cd.clone()));
    let c = ThingRef(thing | 0x1000);
    let ty = creatures::creature_type(&g, c);
    let (info, _) = creatures::type_info(&g, &cd, ty).expect("type info");
    let class = creatures::terrain::class(&g, map, x, y, info.door_size().max(1));
    println!(
        "thing {thing:#x} type {ty} terrain mask {:#06x} door size {} | square class {class:#06x} -> {}",
        info.terrain(),
        info.door_size(),
        if class != 0 && class & info.terrain() != 0 { "can enter" } else { "blocked" }
    );
}
