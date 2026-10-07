//! The pieces of a pit-fall's damage for champion 0 of a save (0x4722A
//! with attack type 2 and parts 0x30): armour of the legs and feet, the
//! level term, and the damage each base roll (17-20) would give.
//!
//! Usage: falldmg SAVE
use std::rc::Rc;

use dm2_engine::{assets, champions, combat, creatures::data::CreatureData, data::GameData, save};
use dm2_formats::gdat::Gdat;

fn main() {
    let path = std::env::args().nth(1).expect("SAVE");
    let gd = Rc::new(GameData::load_default().expect("SKULL.EXE"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).expect("SKULL.EXE");
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).expect("GRAPHICS.DAT"));
    let cd = Rc::new(CreatureData::load(gdat, &exe).expect("creature data"));
    let g = save::read(std::path::Path::new(&path), gd.clone(), Some(cd)).expect("load");
    let c = &g.champions[0];
    let db = gd.item_db(&g.dungeon);
    let mut rng = g.rng.clone();
    let parts: Vec<i16> = [4usize, 5]
        .iter()
        .map(|&p| combat::armour_value(c, &g.party_status, p, false, &db, &gd.tables, &mut rng))
        .collect();
    let avg = parts.iter().map(|&v| v as i32).sum::<i32>() / parts.len() as i32;
    for with_mods in [true, false] {
        let lv = champions::level(c, &g.party_status, champions::skill::NINJA, with_mods) as i32;
        let def = (avg >> 1) + lv;
        let dmg: Vec<i32> = (17..=20).map(|a| (a * (130 - def)) >> 6).collect();
        println!(
            "armour legs {} feet {} avg {avg}; ninja level {lv} (modifiers {with_mods}) -> defence {def}; base 17..20 gives {dmg:?}",
            parts[0], parts[1]
        );
    }
}
