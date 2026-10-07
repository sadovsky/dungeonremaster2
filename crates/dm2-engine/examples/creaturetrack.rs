//! Follow one creature through a replayed probe: after each tick in a
//! range, print its facing, action, queued action, turn target, stage,
//! program and square. Replays the same inputs as `rngseq` (LOAD=SAVE,
//! CMDS=TICK:CODE,..., STEP=F@TICK), loading saves as the original does.
//!
//! Usage: [LOAD=SAVE] [CMDS=...] creaturetrack THING_HEX FROM_TICK TO_TICK
use std::rc::Rc;

use dm2_engine::{assets, creatures, creatures::data::CreatureData, data::GameData, state::GameState};
use dm2_formats::dungeon::ThingRef;
use dm2_formats::gdat::Gdat;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let want = u16::from_str_radix(args[0].trim_start_matches("0x"), 16).expect("THING_HEX") & 0x3FFF;
    let from: u32 = args[1].parse().expect("FROM_TICK");
    let to: u32 = args[2].parse().expect("TO_TICK");
    let a = assets::Assets::load(&assets::default_data_dir()).expect("game data");
    let gd = Rc::new(GameData::load_default().expect("SKULL.EXE"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).expect("SKULL.EXE");
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).expect("GRAPHICS.DAT"));
    let cd = Rc::new(CreatureData::load(gdat, &exe).expect("creature data"));
    let mut g = match std::env::var("LOAD") {
        Ok(path) => dm2_engine::save::read_as_original(std::path::Path::new(&path), gd, Some(cd.clone())).expect("load"),
        Err(_) => GameState::new_game_full(&a.dungeon, gd, Some(cd.clone())),
    };
    let step: Option<u32> = std::env::var("STEP").ok().and_then(|s| s.strip_prefix("F@").and_then(|t| t.parse().ok()));
    let cmds: Vec<(u32, u16)> = std::env::var("CMDS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|p| {
            let (t, c) = p.split_once(':')?;
            Some((t.parse().ok()?, u16::from_str_radix(c.trim_start_matches("0x"), 16).ok()?))
        })
        .collect();
    while g.tick <= to {
        if step == Some(g.tick) {
            g.push_command(dm2_engine::state::Command::Move(dm2_engine::world::Move::Forward));
        }
        for &(t, c) in &cmds {
            if t == g.tick {
                dm2_engine::hand::dispatch(&mut g, c);
            }
        }
        let tick = g.tick;
        g.advance();
        if tick < from {
            continue;
        }
        let Some(c) = g
            .creature_slots
            .iter()
            .flatten()
            .map(|s| s.thing)
            .find(|t| t.0 & 0x3FFF == want)
            .or_else(|| Some(ThingRef(want)))
        else {
            continue;
        };
        let facing = creatures::facing(&g, c);
        match creatures::slot_of(&g, c).and_then(|si| g.creature_slots[si].as_ref()) {
            Some(s) => println!(
                "{tick} facing {facing} action {:#04x} queued {:#04x} turn_to {} stage {} program {} step {} set {} act_tick {} alert {} at ({},{}) map {}",
                s.action,
                s.queued,
                s.turn_to,
                s.stage,
                s.program,
                s.step,
                s.set,
                s.act_tick,
                g.creature_alert_roll,
                s.pos.x(),
                s.pos.y(),
                s.pos.map()
            ),
            None => println!("{tick} facing {facing} (no slot)"),
        }
    }
}
