//! Follow one creature through an idle new game, one line per tick while it
//! holds a slot: action, turn target, stage, facing, square and the element
//! of the square ahead. Used to find where a creature's state parts from the
//! original's draw log (docs/05, "Draw log").
//!
//! Usage: [LOAD=SAVE] creaturelog THING_HEX [END_TICK]   (thing reference & 0x3FFF)
use std::rc::Rc;

use dm2_engine::{assets, creatures, creatures::data::CreatureData, data::GameData, state::GameState};
use dm2_formats::gdat::Gdat;

const DX: [i32; 4] = [0, 1, 0, -1];
const DY: [i32; 4] = [-1, 0, 1, 0];

fn main() {
    let want = u16::from_str_radix(std::env::args().nth(1).expect("THING_HEX").trim_start_matches("0x"), 16)
        .expect("hex thing reference");
    let end: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let a = assets::Assets::load(&assets::default_data_dir()).expect("game data");
    let gd = Rc::new(GameData::load_default().expect("SKULL.EXE"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).expect("SKULL.EXE");
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).expect("GRAPHICS.DAT"));
    let cd = Rc::new(CreatureData::load(gdat, &exe).expect("creature data"));
    // LOAD=SAVE follows the creature from a save instead of a new game.
    let mut g = match std::env::var("LOAD") {
        Ok(path) => load_save(std::path::Path::new(&path), gd, Some(cd)).expect("load"),
        Err(_) => GameState::new_game_full(&a.dungeon, gd, Some(cd)),
    };
    let mut last = String::new();
    while g.tick < end {
        let tick = g.tick;
        g.advance();
        let slot = g.creature_slots.iter().flatten().find(|s| s.thing.0 & 0x3FFF == want);
        let line = match slot {
            None => "no slot".to_string(),
            Some(s) => {
                let f = creatures::facing(&g, s.thing);
                let (m, x, y) = (s.pos.map(), s.pos.x(), s.pos.y());
                let ahead = g.dungeon.square(m, x + DX[f as usize], y + DY[f as usize]).element();
                format!(
                    "action {:#04x} turn_to {} stage {} facing {} at map {} ({},{}) ahead {:?} seq {:#x}/{}",
                    s.action, s.turn_to, s.stage, f, m, x, y, ahead, s.seq_start, s.seq_off
                )
            }
        };
        if line != last {
            println!("tick {tick:3}: {line}");
            last = line;
        }
    }
}

/// Load a save as the original would (no remake trailer), unless
/// KEEP_TRAILER=1: comparisons against the original's draw log must start
/// from the state the original loads (docs/05, "Comparing from a save").
fn load_save(
    path: &std::path::Path,
    gd: std::rc::Rc<dm2_engine::data::GameData>,
    cd: Option<std::rc::Rc<dm2_engine::creatures::data::CreatureData>>,
) -> Result<dm2_engine::state::GameState, dm2_engine::save::SaveError> {
    if std::env::var_os("KEEP_TRAILER").is_some() {
        dm2_engine::save::read(path, gd, cd)
    } else {
        dm2_engine::save::read_as_original(path, gd, cd)
    }
}
