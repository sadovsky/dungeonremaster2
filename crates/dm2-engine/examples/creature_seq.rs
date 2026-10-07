//! Per-tick animation state of one creature in an idle new game:
//! creature_seq INDEX [END_TICK]. Prints a line whenever its slot changes.
use std::rc::Rc;

use dm2_engine::{assets, creatures::data::CreatureData, data::GameData, rng, state::GameState};
use dm2_formats::gdat::Gdat;

fn main() {
    let mut args = std::env::args().skip(1);
    let idx: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);
    let end: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(60);
    let a = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = Rc::new(CreatureData::load(gdat, &exe).unwrap());
    let mut g = GameState::new_game_full(&a.dungeon, gd, Some(cd));
    let mut last = String::new();
    while g.tick < end {
        let t = g.tick;
        rng::trace_start();
        g.advance();
        let m = rng::trace_take();
        let thought = m.get(&("think", idx as u32)).copied().unwrap_or(0);
        let frames = m.get(&("frame", idx as u32)).copied().unwrap_or(0);
        let slot = g.creature_slots.iter().flatten().find(|s| s.thing.0 & 0x3FF == idx);
        if let Some(s) = slot {
            let now = format!(
                "action {:#04x} queued {:#04x} seq {} off {:#06x} prog {} step {}",
                s.action, s.queued, s.seq_start, s.seq_off, s.program, s.step
            );
            if now != last || thought > 0 || frames > 0 {
                println!("tick {t:3} think {thought} frame {frames} | {now}");
                last = now;
            }
        }
    }
}
