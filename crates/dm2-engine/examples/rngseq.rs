//! Print the remake's ordered random draws for an idle new game, one line per
//! draw: tick, creature (thing reference & 0x3FFF, 0 outside creature
//! processing) and call site, to compare with the original's draw log
//! creature by creature (docs/05, "Draw log").
//!
//! Usage: rngseq [END_TICK]   (default 2: ticks 0-1)
use std::rc::Rc;

use dm2_engine::{assets, creatures, creatures::data::CreatureData, data::GameData, rng, state::GameState};
use dm2_formats::gdat::Gdat;

fn main() {
    let end: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(2);
    let a = assets::Assets::load(&assets::default_data_dir()).expect("game data");
    let gd = Rc::new(GameData::load_default().expect("SKULL.EXE"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).expect("SKULL.EXE");
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).expect("GRAPHICS.DAT"));
    let cd = Rc::new(CreatureData::load(gdat, &exe).expect("creature data"));
    // Start before the new game so its setup draws (creature pass,
    // activations, recruit) are logged under tick 0, as in the original.
    rng::trace_seq_start();
    // LOAD=SAVE starts from a save instead of a new game; STEP=F@TICK issues a
    // forward step at that tick (to replay a probe of the original).
    let mut g = match std::env::var("LOAD") {
        Ok(path) => load_save(std::path::Path::new(&path), gd, Some(cd.clone())).expect("load"),
        Err(_) => GameState::new_game_full(&a.dungeon, gd, Some(cd.clone())),
    };
    // RNGSTATE=HEX replaces the random state after loading (experiments on
    // what the original keeps across a load); RNGSHOW=1 prints it.
    if let Some(v) = std::env::var("RNGSTATE").ok().and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()) {
        g.rng.state = v;
    }
    if std::env::var_os("HP").is_some() {
        for (i, c) in g.champions.iter().enumerate() {
            eprintln!("champion {i} +0x2E after load: {:#06x}", c.u16_at(0x2E));
        }
    }
    if std::env::var_os("RNGSHOW").is_some() {
        eprintln!("rng state after load {:#010x}", g.rng.state);
    }
    let step: Option<u32> = std::env::var("STEP").ok().and_then(|s| s.strip_prefix("F@").and_then(|t| t.parse().ok()));
    // CMDS=TICK:CODE,... dispatches interface commands (hex codes, as the
    // original's click zones produce) at those ticks, before the tick runs.
    let cmds: Vec<(u32, u16)> = std::env::var("CMDS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|p| {
            let (t, c) = p.split_once(':')?;
            Some((t.parse().ok()?, u16::from_str_radix(c.trim_start_matches("0x"), 16).ok()?))
        })
        .collect();
    let _ = creatures::set_data;
    // TLLOG=PATH writes the timeline's record traffic (op tick record type
    // prio due) to compare with the original's T hook lines.
    let tl = std::env::var_os("TLLOG").is_some();
    let mut tl_out: Vec<String> = Vec::new();
    if tl {
        dm2_engine::timeline::trace_start();
    }
    while g.tick < end {
        if step == Some(g.tick) {
            g.push_command(dm2_engine::state::Command::Move(dm2_engine::world::Move::Forward));
        }
        // Interface commands run between ticks, after the counter moved on,
        // as the original's command drain does: file their draws under the
        // tick they precede (the trace context is otherwise only set at the
        // start of `advance`).
        if cmds.iter().any(|&(t, _)| t == g.tick) {
            rng::trace_context(Some(g.tick), Some(0));
        }
        for &(t, c) in &cmds {
            if t == g.tick {
                dm2_engine::hand::dispatch(&mut g, c);
            }
        }
        // Commands run between ticks, after the counter moved on: file
        // their record traffic under the tick they precede, as above.
        if tl {
            for (op, slot, kind, prio, due) in dm2_engine::timeline::trace_take() {
                tl_out.push(format!("{op} {pre} {slot} {kind:x} {prio:x} {due}", pre = g.tick));
            }
        }
        let hp_before: Vec<i16> = g.champions.iter().map(|c| c.health()).collect();
        g.advance();
        // HP=1 prints every change in the champions' health (to stderr).
        if std::env::var_os("HP").is_some() {
            let hp: Vec<i16> = g.champions.iter().map(|c| c.health()).collect();
            if hp != hp_before {
                eprintln!("hp tick {} {:?} -> {:?}", g.tick - 1, hp_before, hp);
            }
        }
        if tl {
            for (op, slot, kind, prio, due) in dm2_engine::timeline::trace_take() {
                tl_out.push(format!("{op} {pre} {slot} {kind:x} {prio:x} {due}", pre = g.tick - 1));
            }
        }
    }
    if tl {
        let p = std::env::var("TLLOG").unwrap();
        std::fs::write(&p, tl_out.join("\n") + "\n").expect("TLLOG");
    }
    let frames = rng::trace_seq_frames_take();
    for (i, (tick, file, line, cr)) in rng::trace_seq_take().into_iter().enumerate() {
        let file = file.rsplit('/').next().unwrap_or(file);
        let (a, st, off) = frames.get(i).copied().unwrap_or((0xFF, 0xFFFF, 0xFFFF));
        println!("{tick} {cr:#06x} {file}:{line} a={a:x} seq={st:x}/{off:x}");
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
