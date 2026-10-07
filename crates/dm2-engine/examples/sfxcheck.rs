//! Run a save for some ticks and show which sound requests the original's
//! play rules accept, with their gain: sfxcheck SAVE [TICKS]
use std::rc::Rc;

use dm2_engine::audio::registry::Registry;
use dm2_engine::audio::sfx::{self, SoundGrid};
use dm2_engine::creatures::{self, data::CreatureData};
use dm2_engine::data::GameData;
use dm2_engine::save;
use dm2_formats::gdat::Gdat;

fn main() {
    let path = std::env::args().nth(1).expect("SAVE");
    let ticks: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(83);
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = CreatureData::load(gdat.clone(), &exe).ok().map(Rc::new);
    let mut g = save::read(std::path::Path::new(&path), gd, cd.clone()).expect("load");
    if let Some(cd) = cd {
        creatures::set_data(&mut g, cd);
    }
    let portraits: Vec<u8> = g.champions.iter().map(|c| c.portrait()).collect();
    let (mut total, mut kept) = (0, 0);
    let mut energy = 0.0f64;
    for _ in 0..ticks {
        g.advance();
        let reqs = sfx::drain_sounds(&mut g.effects);
        let reg = Registry::for_map(&gdat, &g.dungeon, g.party.map, &portraits);
        let grid = SoundGrid::build(&g.dungeon, &g.party);
        for r in reqs {
            total += 1;
            let why = if r.mode >= 1 && r.map != g.party.map {
                "other map"
            } else if !reg.contains(r.cat, r.idx, r.sub) {
                "not registered"
            } else {
                ""
            };
            let (mut right, mut forward) = sfx::relative(&g.party, r.x, r.y);
            let dist = right.abs() + forward.abs();
            let mut why = why.to_string();
            if why.is_empty() && dist > 1 {
                match grid.distance(r.x, r.y) {
                    None => why = "unreachable".into(),
                    Some(p) => (right, forward) = sfx::stretch(right, forward, dist, p),
                }
            }
            let gain = sfx::attenuate(r.vol, right, forward);
            if why.is_empty() {
                kept += 1;
                energy += (gain as f64).powi(2);
            }
            println!(
                "tick {} ({:#x},{:#x},{:#x}) vol {} at ({},{}) dist {dist} -> {}",
                g.tick, r.cat, r.idx, r.sub, r.vol, r.x, r.y,
                if why.is_empty() { format!("plays, gain {gain:.3} at ({right},{forward})") } else { format!("dropped: {why}") }
            );
        }
    }
    println!("{kept} of {total} requests play; summed gain^2 {energy:.3}");
}
