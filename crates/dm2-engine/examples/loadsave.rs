//! Load a save file and print a summary: loadsave PATH
use std::rc::Rc;
use dm2_engine::{creatures::data::CreatureData, data::GameData, save};
use dm2_formats::gdat::Gdat;
fn main() {
    let path = std::env::args().nth(1).expect("PATH");
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = CreatureData::load(gdat, &exe).ok().map(Rc::new);
    let bytes = std::fs::read(&path).unwrap();
    println!("{} bytes, trailer: {}", bytes.len(), bytes.ends_with(b"DM2R") || bytes.windows(4).any(|w| w == b"DM2R"));
    match save::from_bytes(&bytes, gd, cd) {
        Ok(mut g) => {
            println!("party map {} ({},{}) dir {}  tick {}  rng {:#x}", g.party.map, g.party.x, g.party.y, g.party.dir, g.tick, g.rng.state);
            for c in &g.champions {
                println!("champion {:?} hp {}/{} st {}/{} mana {}/{} food {} water {}", c.name(), c.health(), c.max_health(), c.stamina(), c.max_stamina(), c.mana(), c.max_mana(), c.food(), c.water());
            }
            println!("leader {:?}  timers {}", g.leader, g.timeline.len());
            for _ in 0..200 { g.advance(); }
            println!("ran 200 ticks ok: tick {}", g.tick);
        }
        Err(e) => println!("load failed: {e}"),
    }
}
