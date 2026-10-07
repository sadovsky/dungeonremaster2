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

/// The bit stream of a save: after the snapshot, before the trailer.
fn stream(b: &[u8], snap_len: usize) -> &[u8] {
    let n = b.len();
    let len = u32::from_le_bytes([b[n - 8], b[n - 7], b[n - 6], b[n - 5]]) as usize;
    &b[HEADER_LEN + snap_len..n - 8 - len]
}

#[test]
fn byte_level_round_trip() {
    let Some((mut g, data, cd)) = new_game() else { return };
    play(&mut g, 0, 300);
    let a = to_bytes(&g, "ROUND TRIP").unwrap();
    eprintln!("save size {} bytes, timers {}", a.len(), g.timeline.len());
    // The snapshot section is the live dungeon (after preparation).
    let mut p = g.clone();
    prepare(&mut p);
    let snap = p.dungeon.to_snapshot();
    assert_eq!(&a[HEADER_LEN..HEADER_LEN + snap.len()], &snap[..]);
    // Loading renumbers dynamic things into stream order, as the original
    // does (0x35B97), so the snapshot may change once; the stream may not.
    let g2 = from_bytes(&a, data.clone(), cd.clone()).unwrap();
    assert_eq!(g2.legacy.name, "ROUND TRIP");
    let b = to_bytes(&g2, "ROUND TRIP").unwrap();
    assert_eq!(a.len(), b.len());
    assert!(stream(&a, snap.len()) == stream(&b, snap.len()), "re-saving a loaded game changed the stream");
    // After one load the numbering is stable: load -> write is a fixpoint.
    let g3 = from_bytes(&b, data, cd).unwrap();
    let c = to_bytes(&g3, "ROUND TRIP").unwrap();
    assert!(b == c, "a second load and save changed the file");
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
    let full = to_bytes(&g, "DOS").unwrap();
    let d = from_bytes(&b, data.clone(), cd.clone()).unwrap();
    // The same save with its trailer goes through the same rebuild.
    let e = from_bytes(&full, data, cd).unwrap();
    assert_eq!(d.party, g.party);
    assert_eq!(d.champions.len(), g.champions.len());
    assert_eq!(d.leader, g.leader);
    assert_eq!(d.tick & 0xFF_FFFF, g.tick & 0xFF_FFFF);
    assert_eq!(d.dungeon.to_snapshot(), e.dungeon.to_snapshot());
    assert_eq!(d.hand.held, e.hand.held);
    for (x, y) in d.champions.iter().zip(&e.champions) {
        assert_eq!((0..30).map(|s| x.inventory(s)).collect::<Vec<_>>(), (0..30).map(|s| y.inventory(s)).collect::<Vec<_>>());
    }
    // Timers come back with the fields the original keeps.
    let (a, b) = (e.timeline.slot_events(), d.timeline.slot_events());
    assert_eq!(a.len(), b.len());
    for ((_, x), (_, y)) in a.iter().zip(&b) {
        assert_eq!((x.tick, x.kind, x.x, x.y, x.b8, x.b9), (y.tick, y.kind, y.x, y.y, y.b8, y.b9));
    }
    // Champion names survive the champion mask.
    for (x, y) in g.champions.iter().zip(&d.champions) {
        assert_eq!(x.name(), y.name());
    }
}

/// Saves written by the DOS game itself (any SKSAVEn.DAT without a remake
/// trailer in the user's DATA directory) reload and rewrite with a
/// bit-identical stream. Skips when there are none.
#[test]
fn original_saves_rewrite_with_identical_streams() {
    let Some((data, cd, _)) = data() else { return };
    for slot in 0..10 {
        let path = slot_path(&default_data_dir(), slot);
        let Ok(orig) = std::fs::read(&path) else { continue };
        if trailer_slice(&orig[HEADER_LEN..]).is_some() {
            continue;
        }
        let g = from_bytes(&orig, data.clone(), cd.clone()).unwrap();
        let mine = to_bytes(&g, &g.legacy.name).unwrap();
        let snap_len = g.dungeon.to_snapshot().len();
        assert!(
            stream(&mine, snap_len) == &orig[HEADER_LEN + snap_len..],
            "slot {slot}: the remake's stream differs from the original's"
        );
    }
}

/// SYSTEM ERROR 71: a save whose party map still has a pending first-entry
/// spawn crashes the original on load. Teleporting the party by assignment
/// produces one; arriving through the map-change path runs the spawns.
#[test]
fn party_arrival_clears_original_load_hazards() {
    let Some((data, cd, dg)) = data() else { return };
    let mut base = GameState::new_game_with(&dg, data);
    if let Some(c) = &cd {
        creatures::set_data(&mut base, c.clone());
    }
    let maps_with_spawns: Vec<usize> =
        (0..base.dungeon.maps.len()).filter(|&m| !crate::map_entry::pending_spawns(&base, m).is_empty()).collect();
    assert!(maps_with_spawns.contains(&2), "map 2 has a first-entry spawn in the shipped dungeon");
    for &m in &maps_with_spawns {
        let spot = crate::map_entry::pending_spawns(&base, m)[0];
        // Assignment only: what the old posave did.
        let mut g = base.clone();
        g.party = crate::world::PartyPos { map: m, x: spot.0, y: spot.1, dir: 0 };
        assert!(!original_load_hazard(&g).is_empty(), "map {m}: teleported party must be flagged");
        // Proper arrival.
        let mut g = base.clone();
        let creatures_before = (0..g.dungeon.maps[m].width as i32)
            .flat_map(|x| (0..g.dungeon.maps[m].height as i32).map(move |y| (x, y)))
            .filter(|&(x, y)| creatures::group_at(&g, m, x, y).is_some())
            .count();
        crate::movement::arrive(&mut g, crate::world::PartyPos { map: m, x: spot.0, y: spot.1, dir: 0 });
        assert!(original_load_hazard(&g).is_empty(), "map {m}: arrival must run the spawns");
        let creatures_after = (0..g.dungeon.maps[m].width as i32)
            .flat_map(|x| (0..g.dungeon.maps[m].height as i32).map(move |y| (x, y)))
            .filter(|&(x, y)| creatures::group_at(&g, m, x, y).is_some())
            .count();
        assert!(creatures_after > creatures_before, "map {m}: first entry creates a creature");
        // Arriving again doesn't spawn twice.
        crate::movement::arrive(&mut g, crate::world::PartyPos { map: 0, x: 1, y: 8, dir: 0 });
        crate::movement::arrive(&mut g, crate::world::PartyPos { map: m, x: spot.0, y: spot.1, dir: 0 });
        assert!(crate::map_entry::pending_spawns(&g, m).is_empty());
    }
}

/// Saves written by the original never carry the hazard.
#[test]
fn original_saves_have_no_load_hazard() {
    let Some((data, cd, _)) = data() else { return };
    let dir = default_data_dir();
    for slot in 0..10u8 {
        let path = slot_path(&dir, slot);
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if bytes.ends_with(TRAILER_MAGIC) {
            continue; // written by the remake
        }
        let g = from_bytes(&bytes, data.clone(), cd.clone()).expect("original save loads");
        assert!(original_load_hazard(&g).is_empty(), "{}", path.display());
    }
}
