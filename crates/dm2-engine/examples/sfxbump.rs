//! Replay a `TICK CODE` script from a new game and print every sound request
//! and the leader's health and position after each command:
//! sfxbump SCRIPT
//! Used to check that a scripted wall bump reaches the wall and requests the
//! champion's cry (docs/11).
use std::rc::Rc;

use dm2_engine::{assets, creatures::data::CreatureData, data::GameData, effects::Effect, input, state::GameState};
use dm2_formats::gdat::Gdat;

fn main() {
    let script = std::fs::read_to_string(std::env::args().nth(1).expect("SCRIPT")).unwrap();
    let mut cmds: Vec<(u32, u16)> = Vec::new();
    let mut end = 0;
    for line in script.lines() {
        let mut it = line.split_whitespace();
        match (it.next(), it.next()) {
            (Some("end"), Some(t)) => end = t.parse().unwrap(),
            (Some(t), Some(c)) => cmds.push((t.parse().unwrap(), u16::from_str_radix(c.trim_start_matches("0x"), 16).unwrap())),
            _ => {}
        }
    }
    let a = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = CreatureData::load(gdat, &exe).ok().map(Rc::new);
    let mut g = GameState::new_game_full(&a.dungeon, gd, cd);
    let mut next = cmds.iter().peekable();
    for tick in 0..end {
        while let Some(&&(t, c)) = next.peek() {
            if t > tick {
                break;
            }
            next.next();
            if let Some(gc) = input::game_command(c) {
                g.push_command(gc);
            }
        }
        g.advance();
        for e in &g.effects {
            match e {
                Effect::Sound { cat, idx, sub, x, y, .. } => {
                    println!("tick {tick:3}: Sound ({cat:#04x},{idx},{sub:#04x}) at ({x},{y})")
                }
                Effect::SoundAt { cat, idx, sub, x, y, vol, mode, .. } => {
                    println!("tick {tick:3}: SoundAt ({cat:#04x},{idx},{sub:#04x}) at ({x},{y}) vol {vol} mode {mode}")
                }
                _ => {}
            }
        }
        g.effects.clear();
        if cmds.iter().any(|&(t, _)| t == tick) {
            let c = &g.champions[0];
            println!(
                "tick {tick:3}: party ({},{}) dir {}  health {}/{}",
                g.party.x, g.party.y, g.party.dir, c.health(), c.max_health()
            );
        }
    }
}
