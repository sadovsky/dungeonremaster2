//! List the creature groups active after play start, weakest first, to pick
//! an opponent for combat probes: weakcreatures [N]
use std::rc::Rc;

use dm2_engine::{assets, creatures, creatures::data::CreatureData, data::GameData, state::GameState};
use dm2_formats::gdat::Gdat;

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(20);
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
    let mut rows = Vec::new();
    for slot in g.creature_slots.iter().flatten() {
        let c = slot.thing;
        let ty = creatures::creature_type(&g, c);
        let Some((info, _)) = creatures::type_info(&g, &d, ty) else { continue };
        let p = slot.pos;
        rows.push((info.attack(), info.base_hp(), info.defence(), c.0, ty, p.map(), p.x(), p.y()));
    }
    rows.sort();
    println!("attack  hp  def  thing  type  map  x  y");
    for r in rows.iter().take(n) {
        println!("{:6} {:4} {:4}  {:#06x} {:#04x} {:4} {:2} {:2}", r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7);
    }
}
