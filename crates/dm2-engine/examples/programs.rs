//! Print creature AI program rows (opcode letter, jumps, arguments, goal
//! builder) from the user's SKULL.EXE: programs PROG... (default 0).
use std::rc::Rc;

use dm2_engine::creatures::data::CreatureData;
use dm2_engine::{assets, exe_tables};
use dm2_formats::gdat::Gdat;

fn main() {
    let exe = std::fs::read(exe_tables::default_exe_path()).expect("SKULL.EXE");
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).expect("GRAPHICS.DAT"));
    let d = CreatureData::load(gdat, &exe).expect("creature data");
    let progs: Vec<u8> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    for p in if progs.is_empty() { vec![0] } else { progs } {
        println!("program {p}:");
        for step in 0..12i8 {
            let Some(r) = d.row(p, step) else { break };
            let op = r.op as u8;
            let name = if (0x20..0x7f).contains(&op) { op as char } else { '.' };
            println!(
                "  {step:2}: op {name} ({op:#04x}) done {} other {} args {} {} builder {} goal_arg {}",
                r.on_done, r.on_other, r.arg3, r.arg4, r.goal & 0x1f, r.goal_arg
            );
            if op == 0 || r.on_done == -1 && r.on_other == -1 {
                break;
            }
        }
    }
}
