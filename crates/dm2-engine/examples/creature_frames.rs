//! Print active creatures' animation state over ticks (debug aid):
//! creature_frames MAP X Y DIR TICKS
use std::rc::Rc;

use dm2_engine::{assets, creatures, data::GameData, state::GameState, world::PartyPos};
use dm2_formats::{dungeon::Dungeon, gdat::Gdat};

fn main() {
    let a: Vec<u32> = std::env::args().skip(1).map(|s| s.parse().unwrap()).collect();
    let dir = assets::default_data_dir();
    let dg = Dungeon::parse(&std::fs::read(dir.join("DUNGEON.DAT")).unwrap()).unwrap();
    let mut g = GameState::new_game_with(&dg, Rc::new(GameData::load_default().unwrap()));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let cd = creatures::data::CreatureData::load(Rc::new(Gdat::open(dir.join("GRAPHICS.DAT")).unwrap()), &exe).unwrap();
    creatures::set_data(&mut g, Rc::new(cd));
    g.party = PartyPos { map: a[0] as usize, x: a[1] as i32, y: a[2] as i32, dir: a[3] as u8 };
    for t in 0..a[4] {
        g.advance();
        let v: Vec<String> = g
            .creature_slots
            .iter()
            .flatten()
            .filter_map(|s| {
                creatures::view(&g, s.thing)
                    .map(|cv| format!("{:04x}:a{}f{}@{},{}", s.thing.0, cv.action, cv.frame, s.pos.x(), s.pos.y()))
            })
            .collect();
        if t % 6 == 0 {
            println!("t{t:3} {}", v.join(" "));
        }
    }
}
