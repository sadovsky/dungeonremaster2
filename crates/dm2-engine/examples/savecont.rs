//! Where does continuing after a save part from loading it? Replays the save
//! test's setup (save/tests.rs `save_then_continue_matches_load_then_continue`)
//! and prints the first tick at which the random state differs, with the
//! creature slots that differ at that point.
use std::rc::Rc;

use dm2_engine::state::{Command, GameState};
use dm2_engine::world::{Move, PartyPos};
use dm2_engine::{assets, creatures, creatures::data::CreatureData, data::GameData, save};
use dm2_formats::gdat::Gdat;

fn step(g: &mut GameState) {
    match g.tick % 23 {
        0 => g.push_command(Command::Move(Move::Forward)),
        7 => g.push_command(Command::TurnRight),
        13 => g.push_command(Command::Move(Move::Left)),
        19 => g.push_command(Command::TurnLeft),
        _ => {}
    }
    g.advance();
}

fn slots(g: &GameState) -> Vec<(u16, String)> {
    let mut v: Vec<(u16, String)> = g
        .creature_slots
        .iter()
        .flatten()
        .map(|s| {
            (
                s.thing.0 & 0x3FFF,
                format!(
                    "action {:#x} stage {} armed {} seq {:#x}/{} queued {:#x} at {:?}",
                    s.action, s.stage, s.armed, s.seq_start, s.seq_off, s.queued, (s.pos.map(), s.pos.x(), s.pos.y())
                ),
            )
        })
        .collect();
    v.sort();
    v
}

fn main() {
    let a = assets::Assets::load(&assets::default_data_dir()).expect("game data");
    let gd = Rc::new(GameData::load_default().expect("SKULL.EXE"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).expect("SKULL.EXE");
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).expect("GRAPHICS.DAT"));
    let cd = Rc::new(CreatureData::load(gdat, &exe).expect("creature data"));
    let mut g1 = GameState::new_game_with(&a.dungeon, gd.clone());
    creatures::set_data(&mut g1, cd.clone());
    g1.party = PartyPos { map: 1, x: 2, y: 9, dir: 0 };
    while g1.tick < 250 {
        step(&mut g1);
    }
    let dir = std::env::temp_dir().join(format!("dm2-savecont-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = save::slot_path(&dir, 1);
    save::save(&mut g1, &path, "DETERMINISM").unwrap();
    let mut g2 = save::load_slot(&dir, 1, gd, Some(cd)).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let ev = |g: &GameState| g.timeline.slot_events().into_iter().map(|(s, e)| (s, e.kind, e.tick, e.map, e.x, e.y)).collect::<Vec<_>>();
    if ev(&g1) != ev(&g2) {
        println!("timeline slots differ right after the save/load");
        let (a, b) = (ev(&g1), ev(&g2));
        for (x, y) in a.iter().zip(b.iter()).filter(|(x, y)| x != y).take(4) {
            println!("  continue {x:?}\n  load     {y:?}");
        }
        println!("  lengths {} {}", a.len(), b.len());
    }
    for _ in 250..400 {
        let (before1, before2) = (slots(&g1), slots(&g2));
        step(&mut g1);
        step(&mut g2);
        if ev(&g1) != ev(&g2) {
            println!("timeline slots part during tick {}", g1.tick - 1);
            let (a, b) = (ev(&g1), ev(&g2));
            for (x, y) in a.iter().zip(b.iter()).filter(|(x, y)| x != y).take(4) {
                println!("  continue {x:?}\n  load     {y:?}");
            }
            println!("  lengths {} {}", a.len(), b.len());
            return;
        }
        if g1.rng.state != g2.rng.state {
            println!("random state parts during tick {}", g1.tick - 1);
            for (x, y) in before1.iter().zip(before2.iter()) {
                if x != y {
                    println!("  before: {:#06x} continue: {}\n                 load:     {}", x.0, x.1, y.1);
                }
            }
            if before1.len() != before2.len() {
                println!("  slot counts differ: {} against {}", before1.len(), before2.len());
            }
            return;
        }
    }
    println!("no divergence through tick 400");
}
