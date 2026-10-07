//! Dump a creature's animation frames: anim_dump INDEX [START] [COUNT]
//! (INDEX = creature thing index in an idle new game).
use std::rc::Rc;

use dm2_engine::{assets, creatures, creatures::data::CreatureData, data::GameData, state::GameState};
use dm2_formats::dungeon::ThingRef;
use dm2_formats::gdat::Gdat;

fn main() {
    let mut args = std::env::args().skip(1);
    let idx: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);
    let start: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let count: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(12);
    let a = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = Rc::new(CreatureData::load(gdat, &exe).unwrap());
    let g = GameState::new_game_full(&a.dungeon, gd, Some(cd.clone()));
    let thing = ThingRef(0x1000 | idx);
    let ty = creatures::creature_type(&g, thing);
    println!("creature {idx}: type {ty:#04x}");
    let Some(an) = cd.anim(ty) else { return println!("no animation data") };
    for action in 0..8u8 {
        println!("action {action:#04x} -> start {}", an.seq_start(action));
    }
    for off in 0..count {
        let f = an.frame(start, off);
        println!(
            "frame {:4} bytes {:02x} {:02x} {:02x} {:02x} | cont {} branch {:#x} jump {} chain {} event {} base {} extra {}",
            start + off, f.0[0], f.0[1], f.0[2], f.0[3], f.cont(), f.branch_chance(), f.jump(),
            f.chain() as u8, f.event() as u8, f.base_ticks(), f.extra_ticks()
        );
    }
}
