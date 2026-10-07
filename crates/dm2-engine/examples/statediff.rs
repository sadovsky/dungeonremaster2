//! Compare two saves field by field, both loaded the way the original loads
//! them (no remake trailer): statediff A.DAT B.DAT
//!
//! Prints the tick, the random state's low 16 bits (all the original keeps),
//! the party, each champion's record bytes, the pending events, square bytes,
//! every thing record and every square's thing list, naming what differs.
//! Used to check the remake's state at a tick against a save the original
//! wrote at that tick after the same inputs (examples/rngseq SAVEOUT=).
use std::collections::BTreeMap;
use std::rc::Rc;

use dm2_engine::{creatures::data::CreatureData, data::GameData, save, state::GameState};
use dm2_formats::dungeon::ThingType;
use dm2_formats::gdat::Gdat;

fn load(path: &str, gd: &Rc<GameData>, cd: &Option<Rc<CreatureData>>) -> GameState {
    save::read_as_original(std::path::Path::new(path), gd.clone(), cd.clone()).expect("load")
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = CreatureData::load(gdat, &exe).ok().map(Rc::new);
    let (x, y) = (load(&a[0], &gd, &cd), load(&a[1], &gd, &cd));
    let mut diffs = 0;
    let mut note = |what: String| {
        diffs += 1;
        println!("DIFF {what}");
    };
    if x.tick != y.tick {
        note(format!("tick {} vs {}", x.tick, y.tick));
    }
    if x.rng.state & 0xFFFF != y.rng.state & 0xFFFF {
        note(format!("rng low16 {:#06x} vs {:#06x}", x.rng.state & 0xFFFF, y.rng.state & 0xFFFF));
    }
    if (x.party.map, x.party.x, x.party.y, x.party.dir) != (y.party.map, y.party.x, y.party.y, y.party.dir) {
        note(format!("party {:?} vs {:?}", x.party, y.party));
    }
    if x.leader != y.leader {
        note(format!("leader {:?} vs {:?}", x.leader, y.leader));
    }
    if x.champions.len() != y.champions.len() {
        note(format!("champion count {} vs {}", x.champions.len(), y.champions.len()));
    }
    for (i, (c, d)) in x.champions.iter().zip(&y.champions).enumerate() {
        let offs: Vec<String> = (0..c.raw.len())
            .filter(|&o| c.raw[o] != d.raw[o])
            .map(|o| format!("+{o:#x}:{:02x}/{:02x}", c.raw[o], d.raw[o]))
            .collect();
        if !offs.is_empty() {
            note(format!("champion {i} bytes {}", offs.join(" ")));
        }
    }
    // Pending events as a multiset (record numbers can legitimately differ).
    let events = |g: &GameState| {
        let mut m: BTreeMap<[u8; 12], u32> = BTreeMap::new();
        for (_, e) in g.timeline.iter() {
            *m.entry(e.to_bytes()).or_default() += 1;
        }
        m
    };
    let (ex, ey) = (events(&x), events(&y));
    for (k, n) in &ex {
        if ey.get(k) != Some(n) {
            note(format!("event only in A (x{n}): {:?}", dm2_engine::timeline::Event::from_bytes(k)));
        }
    }
    for (k, n) in &ey {
        if ex.get(k) != Some(n) {
            note(format!("event only in B (x{n}): {:?}", dm2_engine::timeline::Event::from_bytes(k)));
        }
    }
    let (dx, dy) = (&x.dungeon, &y.dungeon);
    let sq: Vec<usize> = (0..dx.map_data.len().min(dy.map_data.len())).filter(|&i| dx.map_data[i] != dy.map_data[i]).collect();
    if !sq.is_empty() {
        let n = if std::env::var_os("ALL").is_some() { sq.len() } else { sq.len().min(8) };
        note(format!("{} map-data bytes differ, first at {:?}", sq.len(), &sq[..n]));
    }
    for t in 0..16 {
        let size = ThingType::from_index(t as u16).record_size();
        if size == 0 {
            continue;
        }
        let (rx, ry) = (&dx.things[t], &dy.things[t]);
        let n = rx.len().min(ry.len()) / size;
        let bad: Vec<String> = (0..n)
            .filter(|&i| rx[i * size..(i + 1) * size] != ry[i * size..(i + 1) * size])
            .map(|i| format!("{i}:{:02x?}/{:02x?}", &rx[i * size..(i + 1) * size], &ry[i * size..(i + 1) * size]))
            .collect();
        if !bad.is_empty() {
            note(format!("{:?} records differ ({}): {}", ThingType::from_index(t as u16), bad.len(), bad.iter().take(if std::env::var_os("ALL").is_some() { usize::MAX } else { 6 }).cloned().collect::<Vec<_>>().join("  ")));
        }
    }
    for (mi, m) in dx.maps.iter().enumerate() {
        for sx in 0..m.width as i32 {
            for sy in 0..m.height as i32 {
                let (lx, ly) = (dx.things_at(mi, sx, sy), dy.things_at(mi, sx, sy));
                if lx != ly {
                    note(format!("map {mi} ({sx},{sy}) things {:x?} vs {:x?}", lx.iter().map(|r| r.0).collect::<Vec<_>>(), ly.iter().map(|r| r.0).collect::<Vec<_>>()));
                }
            }
        }
    }
    println!("{diffs} differences");
}
