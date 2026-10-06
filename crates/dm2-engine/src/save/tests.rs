use std::rc::Rc;

use dm2_formats::dungeon::Dungeon;
use dm2_formats::gdat::Gdat;

use super::*;
use crate::assets::default_data_dir;
use crate::exe_tables::default_exe_path;
use crate::state::Command;
use crate::world::Move;

#[test]
fn bit_writer_is_msb_first_and_masked() {
    let mut w = BitWriter::default();
    w.put(&[0b1010_1100, 0xFF, 0x12], &[0xF0, 0x00, 0x0F]);
    // 1010 then 0010 -> one full byte
    assert_eq!(w.out, vec![0b1010_0010]);
    w.bit(true);
    w.bit(true);
    w.flush();
    assert_eq!(w.out, vec![0b1010_0010, 0b1100_0000]);
    let mut r = BitReader::new(&w.out);
    let mut d = [0u8, 0x55, 0];
    r.get(&mut d, &[0xF0, 0x00, 0x0F]).unwrap();
    // Bits outside the mask keep their previous values.
    assert_eq!(d, [0b1010_0000, 0x55, 0x02]);
    assert!(r.bit().unwrap() && r.bit().unwrap());
}

fn data() -> Option<(Rc<GameData>, Option<Rc<CreatureData>>, Dungeon)> {
    let dir = default_data_dir();
    let data = Rc::new(GameData::load_default()?);
    let exe = std::fs::read(default_exe_path()).ok()?;
    let gdat = Rc::new(Gdat::open(dir.join("GRAPHICS.DAT")).ok()?);
    let cd = CreatureData::load(gdat, &exe).ok().map(Rc::new);
    let dg = Dungeon::parse(&std::fs::read(dir.join("DUNGEON.DAT")).ok()?).ok()?;
    Some((data, cd, dg))
}

fn new_game() -> Option<(GameState, Rc<GameData>, Option<Rc<CreatureData>>)> {
    let (data, cd, dg) = data()?;
    let mut g = GameState::new_game_with(&dg, data.clone());
    let c = cd.as_ref().expect("creature tables load from SKULL.EXE");
    creatures::set_data(&mut g, c.clone());
    assert!(!g.champions.is_empty(), "new game recruits the starting champion");
    // Start next to creatures (map 1) so slots and timers are in play.
    g.party = crate::world::PartyPos { map: 1, x: 2, y: 9, dir: 0 };
    Some((g, data, cd))
}

/// A fixed input script so two runs see the same commands on the same ticks.
fn play(g: &mut GameState, from: u32, ticks: u32) {
    for t in from..from + ticks {
        match t % 23 {
            0 => g.push_command(Command::Move(Move::Forward)),
            7 => g.push_command(Command::TurnRight),
            13 => g.push_command(Command::Move(Move::Left)),
            19 => g.push_command(Command::TurnLeft),
            _ => {}
        }
        g.advance();
    }
}

#[test]
fn tables_load_from_skull_exe() {
    let Some((data, _, _)) = data() else { return };
    let t = SaveTables::from_exe(&data.exe).unwrap();
    assert_eq!(t.globals.len(), 60);
    assert_eq!(t.champion.len(), RECORD_SIZE);
    // The `next` link of every saved thing type is never written.
    for m in t.types.iter().flatten() {
        assert_eq!(&m[..2], &[0, 0]);
    }
    // Teleporters (type 1) have no saved record.
    assert!(t.types[1].is_none());
}

#[test]
fn byte_level_round_trip() {
    let Some((mut g, data, cd)) = new_game() else { return };
    play(&mut g, 0, 300);
    let a = to_bytes(&g, "ROUND TRIP").unwrap();
    eprintln!("save size {} bytes, timers {}", a.len(), g.timeline.len());
    let g2 = from_bytes(&a, data, cd).unwrap();
    assert_eq!(g2.legacy.name, "ROUND TRIP");
    let b = to_bytes(&g2, "ROUND TRIP").unwrap();
    assert_eq!(a.len(), b.len());
    assert!(a == b, "re-saving a loaded game changed the file");
    // The snapshot section is the live dungeon (after preparation).
    let mut p = g.clone();
    prepare(&mut p);
    let snap = p.dungeon.to_snapshot();
    assert_eq!(&a[HEADER_LEN..HEADER_LEN + snap.len()], &snap[..]);
}

#[test]
fn save_then_continue_matches_load_then_continue() {
    let Some((mut g1, data, cd)) = new_game() else { return };
    play(&mut g1, 0, 250);
    assert!(!g1.timeline.is_empty(), "something is scheduled at save time");
    assert!(g1.creature_slots.iter().any(|s| s.is_some()), "creatures are active at save time");
    let dir = std::env::temp_dir().join(format!("dm2-save-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = slot_path(&dir, 1);
    save(&mut g1, &path, "DETERMINISM").unwrap();
    let mut g2 = load_slot(&dir, 1, data, cd).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(g1.tick, g2.tick);
    play(&mut g1, 250, 400);
    play(&mut g2, 250, 400);
    assert_eq!(g1.rng.state, g2.rng.state);
    assert_eq!(g1.party, g2.party);
    assert_eq!(g1.timeline.slot_events(), g2.timeline.slot_events());
    let raw = |g: &GameState| g.champions.iter().map(|c| c.raw.to_vec()).collect::<Vec<_>>();
    assert_eq!(raw(&g1), raw(&g2));
    assert_eq!(g1.dungeon.to_snapshot(), g2.dungeon.to_snapshot());
    let slots = |g: &GameState| g.creature_slots.iter().filter(|s| s.is_some()).count();
    assert_eq!(slots(&g1), slots(&g2));
    assert_eq!(to_bytes(&g1, "X").unwrap(), to_bytes(&g2, "X").unwrap());
}

#[test]
fn reads_a_save_without_the_engine_trailer() {
    let Some((mut g, data, cd)) = new_game() else { return };
    play(&mut g, 0, 120);
    prepare(&mut g);
    let mut b = to_bytes(&g, "DOS").unwrap();
    // Strip the trailer, leaving what the DOS game would have written.
    let n = b.len();
    let len = u32::from_le_bytes([b[n - 8], b[n - 7], b[n - 6], b[n - 5]]) as usize;
    b.truncate(n - 8 - len);
    let d = from_bytes(&b, data, cd).unwrap();
    assert_eq!(d.party, g.party);
    assert_eq!(d.champions.len(), g.champions.len());
    assert_eq!(d.leader, g.leader);
    assert_eq!(d.tick & 0xFF_FFFF, g.tick & 0xFF_FFFF);
    assert_eq!(d.dungeon.to_snapshot(), g.dungeon.to_snapshot());
    // Timers come back with the fields the original keeps.
    let (a, b) = (g.timeline.slot_events(), d.timeline.slot_events());
    assert_eq!(a.len(), b.len());
    for ((_, x), (_, y)) in a.iter().zip(&b) {
        assert_eq!((x.tick, x.kind, x.x, x.y, x.b8, x.b9), (y.tick, y.kind, y.x, y.y, y.b8, y.b9));
    }
    // Champion names survive the champion mask.
    for (x, y) in g.champions.iter().zip(&d.champions) {
        assert_eq!(x.name(), y.name());
    }
}
