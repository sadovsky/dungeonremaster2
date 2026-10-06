//! Compare a save written by the original game with the remake's rewrite
//! of the same state, bit by bit through the masked stream, and name the
//! field where they first diverge: savediff SKSAVEn.DAT
use std::rc::Rc;

use dm2_engine::{creatures::data::CreatureData, data::GameData, save};
use dm2_formats::gdat::Gdat;

fn bit(b: &[u8], i: usize) -> Option<bool> {
    b.get(i / 8).map(|v| v >> (7 - i % 8) & 1 != 0)
}

fn main() {
    let path = std::env::args().nth(1).expect("SKSAVEn.DAT");
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = CreatureData::load(gdat, &exe).ok().map(Rc::new);
    let orig = std::fs::read(&path).unwrap();
    let g = save::from_bytes(&orig, gd, cd).expect("load");
    let (mine, start, labels) = save::to_bytes_traced(&g, &g.legacy.name, true).expect("write");
    std::fs::write("target/savecompat/out.dat", &mine).ok();
    let a = &orig[start..];
    // The remake's stream ends where its trailer begins.
    let tlen = u32::from_le_bytes(mine[mine.len() - 8..mine.len() - 4].try_into().unwrap()) as usize;
    let b = &mine[start..mine.len() - 8 - tlen];
    println!("snapshot identical: {}", orig[42..start] == mine[42..start]);
    println!("stream bytes: original {}, remake {}", a.len(), b.len());
    let n = a.len().max(b.len()) * 8;
    let first = (0..n).find(|&i| bit(a, i) != bit(b, i));
    let Some(i) = first else {
        println!("streams identical");
        return;
    };
    let k = labels.iter().rposition(|(p, _)| *p <= i).unwrap_or(0);
    println!("first divergence at stream bit {i} (byte {}):", i / 8);
    for (p, l) in &labels[k.saturating_sub(6)..(k + 3).min(labels.len())] {
        println!("{} {:6} {l}", if *p <= i && labels.get(k).map(|x| x.0) == Some(*p) { ">" } else { " " }, p);
    }
    let show = |s: &[u8]| (i.saturating_sub(16)..i + 32).map(|j| match bit(s, j) { Some(true) => '1', Some(false) => '0', None => '.' }).collect::<String>();
    println!("original: {}", show(a));
    println!("remake:   {}", show(b));
    println!("           {}^", " ".repeat(i.min(16)));
}
