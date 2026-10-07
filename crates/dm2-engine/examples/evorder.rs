//! List the timeline events due at a tick, in the order they will run, for a
//! replayed probe: record index, event type, priority and, for creature
//! events, the creature (thing reference & 0x3FFF). Replays the same inputs
//! as `rngseq` (LOAD=SAVE as the original loads it, CMDS=TICK:CODE,...).
//!
//! Usage: [LOAD=SAVE] [CMDS=...] evorder TICK
use std::rc::Rc;

use dm2_engine::{assets, creatures::data::CreatureData, data::GameData, save, state::GameState};
use dm2_formats::gdat::Gdat;

fn main() {
    let at: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).expect("TICK");
    let a = assets::Assets::load(&assets::default_data_dir()).expect("game data");
    let gd = Rc::new(GameData::load_default().expect("SKULL.EXE"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).expect("SKULL.EXE");
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).expect("GRAPHICS.DAT"));
    let cd = Rc::new(CreatureData::load(gdat, &exe).expect("creature data"));
    let mut g = match std::env::var("LOAD") {
        Ok(p) => save::read_as_original(std::path::Path::new(&p), gd, Some(cd)).expect("load"),
        Err(_) => GameState::new_game_full(&a.dungeon, gd, Some(cd)),
    };
    let cmds: Vec<(u32, u16)> = std::env::var("CMDS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|p| {
            let (t, c) = p.split_once(':')?;
            Some((t.parse().ok()?, u16::from_str_radix(c.trim_start_matches("0x"), 16).ok()?))
        })
        .collect();
    while g.tick < at {
        for &(t, c) in &cmds {
            if t == g.tick {
                dm2_engine::hand::dispatch(&mut g, c);
            }
        }
        g.advance();
    }
    // SHOW=all lists every queued event (record order) instead of only those
    // due at TICK.
    if std::env::var("SHOW").as_deref() == Ok("all") {
        let mut evs: Vec<_> = g.timeline.iter().map(|(s, e)| (s, *e)).collect();
        evs.sort_by_key(|(s, _)| *s);
        for (slot, ev) in evs {
            let who = if ev.w10 & 0x8000 != 0 {
                g.creature_slots.get((ev.w10 & 0x7FFF) as usize).and_then(|s| s.as_ref()).map(|s| s.thing.0 & 0x3FFF)
            } else {
                None
            };
            let who = who.map_or("-".to_string(), |c| format!("{c:#06x}"));
            println!("rec {slot:4} kind {:#04x} prio {:#04x} tick {} creature {who}", ev.kind, ev.prio, ev.tick);
        }
        return;
    }
    let mut tl = g.timeline.clone();
    while tl.due(at) {
        let Some(slot) = tl.iter().next().map(|(s, _)| s) else { break };
        let ev = tl.pop().unwrap();
        let who = if ev.w10 & 0x8000 != 0 {
            g.creature_slots.get((ev.w10 & 0x7FFF) as usize).and_then(|s| s.as_ref()).map(|s| s.thing.0 & 0x3FFF)
        } else {
            None
        };
        let who = who.map_or("-".to_string(), |c| format!("{c:#06x}"));
        println!("rec {slot:4} kind {:#04x} prio {:#04x} tick {} creature {who}", ev.kind, ev.prio, ev.tick);
    }
}
