//! Print the light state at a new game's start: darkness step and inputs.
use std::rc::Rc;
use dm2_engine::{assets, creatures, data::GameData, exe_tables, state::GameState};
use dm2_formats::{dungeon::Dungeon, gdat::Gdat};
fn main() {
    let dir = assets::default_data_dir();
    let dg = Dungeon::parse(&std::fs::read(dir.join("DUNGEON.DAT")).unwrap()).unwrap();
    let gd = Rc::new(GameData::load_default().unwrap());
    let exe = std::fs::read(exe_tables::default_exe_path()).unwrap();
    let cd = Rc::new(creatures::data::CreatureData::load(Rc::new(Gdat::open(dir.join("GRAPHICS.DAT")).unwrap()), &exe).unwrap());
    let mut g = GameState::new_game_with(&dg, gd);
    creatures::set_data(&mut g, cd.clone());
    let m = &g.dungeon.maps[g.party.map];
    println!("map {} difficulty {} tileset {} light {} step {}", g.party.map, m.difficulty, m.tileset, g.light, creatures::fight::darkness_level(&g, &cd));
}
