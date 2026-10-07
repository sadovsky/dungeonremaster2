//! Explain why given creature records are or aren't active after play
//! start: creaturewhy INDEX...
use std::rc::Rc;

use dm2_engine::{assets, creatures, creatures::data::CreatureData, data::GameData, state::GameState};
use dm2_formats::dungeon::{ThingRef, ThingType};
use dm2_formats::gdat::Gdat;

fn main() {
    let want: Vec<u16> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let a = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let mut g = GameState::new_game_with(&a.dungeon, gd);
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let d = Rc::new(CreatureData::load(gdat, &exe).unwrap());
    creatures::set_data(&mut g, d.clone());
    while g.tick < 2 {
        g.advance();
    }
    for (m, md) in g.dungeon.maps.iter().enumerate() {
        for x in 0..md.width as i32 {
            for y in 0..md.height as i32 {
                let things = g.dungeon.things_at(m, x, y);
                let first_creature = things.iter().find(|t| t.kind() == ThingType::Creature).map(|t| t.0 & 0x3FF);
                for t in &things {
                    if t.kind() == ThingType::Creature && want.contains(&(t.0 & 0x3FF)) {
                        let c = ThingRef(t.0 & 0x3FFF);
                        let ty = creatures::creature_type(&g, c);
                        let info = creatures::type_info(&g, &d, ty).map(|(i, _)| i.inanimate());
                        println!(
                            "creature {} map {m} ({x},{y}) type {ty} dormant {info:?} slot byte {:#x} first creature on square {:?} things on square {}",
                            t.0 & 0x3FF,
                            creatures::rec_u8(&g, c, 5),
                            first_creature,
                            things.len()
                        );
                    }
                }
            }
        }
    }
}
