//! Deterministic playthrough harness: a seeded controller drives a real new
//! game (moves, stairs, doors, items, eating, combat, spells, throwing,
//! save/load and map jumps) and the engine's invariants are checked after
//! every tick. Needs the user's game data; skips when it is absent.
//!
//! `cargo test -p dm2-engine --test playthrough` runs the quick variant;
//! `cargo test -p dm2-engine --test playthrough -- --ignored` runs the soak.

use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dm2_engine::assets::default_data_dir;
use dm2_engine::champions::{EMPTY, INVENTORY_SLOTS};
use dm2_engine::creatures::{self, data::CreatureData};
use dm2_engine::data::GameData;
use dm2_engine::exe_tables::default_exe_path;
use dm2_engine::hand::ViewRegion;
use dm2_engine::magic::RUNE_BASE;
use dm2_engine::save;
use dm2_engine::state::{Command, GameState};
use dm2_engine::world::{self, Move, PartyPos};
use dm2_engine::viewport::{DX, DY};
use dm2_formats::dungeon::{Dungeon, Element, ThingRef, ThingType};
use dm2_formats::gdat::Gdat;

/// Everything the harness needs to start or reload games.
struct World {
    dungeon: Dungeon,
    data: Rc<GameData>,
    creatures: Option<Rc<CreatureData>>,
}

fn load_world() -> Option<World> {
    let dir = default_data_dir();
    let dungeon = Dungeon::parse(&std::fs::read(dir.join("DUNGEON.DAT")).ok()?).ok()?;
    let data = Rc::new(GameData::load_default()?);
    let exe = std::fs::read(default_exe_path()).ok()?;
    let gdat = Rc::new(Gdat::open(dir.join("GRAPHICS.DAT")).ok()?);
    let creatures = CreatureData::load(gdat, &exe).ok().map(Rc::new);
    Some(World { dungeon, data, creatures })
}

fn new_game(w: &World) -> GameState {
    let mut g = GameState::new_game_with(&w.dungeon, w.data.clone());
    if let Some(cd) = &w.creatures {
        creatures::set_data(&mut g, cd.clone());
    }
    g
}

/// The controller's own generator, independent of the game's RNG.
struct Ctl(u64);

impl Ctl {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
    fn chance(&mut self, pct: u32) -> bool {
        self.below(100) < pct
    }
}

/// Global column index of each map's first column (maps are stored in order).
fn first_columns(dg: &Dungeon) -> Vec<usize> {
    let mut v = Vec::with_capacity(dg.maps.len());
    let mut c = 0;
    for m in &dg.maps {
        v.push(c);
        c += m.width as usize;
    }
    v
}

/// All engine invariants. Returns a description of the first violation.
fn check(g: &GameState) -> Result<(), String> {
    let dg = &g.dungeon;
    // Timeline structure.
    g.timeline.check().map_err(|e| format!("timeline: {e}"))?;

    // Square lists: acyclic, records live and in range, nothing on two
    // squares, and the column table matches the "has things" bits.
    let mut on_square: HashMap<u16, (usize, i32, i32)> = HashMap::new();
    let cols = first_columns(dg);
    for (mi, m) in dg.maps.iter().enumerate() {
        for x in 0..m.width as i32 {
            let mut flagged = 0usize;
            for y in 0..m.height as i32 {
                if !dg.square(mi, x, y).has_things() {
                    continue;
                }
                flagged += 1;
                let mut t = dg.first_thing(mi, x, y);
                if !t.is_thing() {
                    return Err(format!("map {mi} ({x},{y}) flagged but list is empty ({:#06x})", t.0));
                }
                let mut seen = HashSet::new();
                while t.is_thing() {
                    let key = t.0 & 0x3FFF;
                    if !seen.insert(key) {
                        return Err(format!("map {mi} ({x},{y}) list cycles at {:#06x}", t.0));
                    }
                    if t.index() >= dg.thing_count(t.kind()) {
                        return Err(format!("map {mi} ({x},{y}) thing {:#06x} out of range", t.0));
                    }
                    if let Some(prev) = on_square.insert(key, (mi, x, y)) {
                        return Err(format!("thing {key:#06x} on map {mi} ({x},{y}) and also on {prev:?}"));
                    }
                    let next = dg.record_word(t, 0).unwrap_or(ThingRef::END.0);
                    if next == Dungeon::FREE {
                        return Err(format!("freed thing {key:#06x} still listed on map {mi} ({x},{y})"));
                    }
                    t = ThingRef(next);
                }
            }
            let c = cols[mi] + x as usize;
            if c + 1 < dg.column_first.len() {
                let span = dg.column_first[c + 1] as usize - dg.column_first[c] as usize;
                if span != flagged {
                    return Err(format!("map {mi} column {x}: column table spans {span}, {flagged} squares flagged"));
                }
            }
        }
    }

    // Things held by champions or the hand are not also on the floor, and no
    // item is held twice.
    let mut held: HashSet<u16> = HashSet::new();
    let mut note = |t: u16, what: String| -> Result<(), String> {
        if t == EMPTY {
            return Ok(());
        }
        let key = t & 0x3FFF;
        if let Some(sq) = on_square.get(&key) {
            return Err(format!("{what} holds {key:#06x}, which also lies on {sq:?}"));
        }
        if !held.insert(key) {
            return Err(format!("{what} holds {key:#06x}, which is held elsewhere too"));
        }
        Ok(())
    };
    for (ci, c) in g.champions.iter().enumerate() {
        for s in 0..INVENTORY_SLOTS {
            note(c.inventory(s), format!("champion {ci} slot {s}"))?;
        }
    }
    note(g.hand.held, "the leader hand".into())?;

    // Things inside containers and carried by creatures: well-formed chains,
    // and nothing in two places at once.
    let mut owners: Vec<(ThingRef, String)> = Vec::new();
    for &key in on_square.keys() {
        let t = ThingRef(key);
        if matches!(t.kind(), ThingType::Container | ThingType::Creature) {
            owners.push((t, format!("{:?} {key:#06x} at {:?}", t.kind(), on_square[&key])));
        }
    }
    let carried = g.champions.iter().flat_map(|c| (0..INVENTORY_SLOTS).map(move |s| c.inventory(s)));
    for t in carried.chain(std::iter::once(g.hand.held)) {
        if t != EMPTY && ThingRef(t).kind() == ThingType::Container {
            owners.push((ThingRef(t & 0x3FFF), format!("carried container {:#06x}", t & 0x3FFF)));
        }
    }
    let mut i = 0;
    while i < owners.len() {
        let (owner, what) = owners[i].clone();
        i += 1;
        let mut t = ThingRef(dg.record_word(owner, 1).unwrap_or(ThingRef::END.0));
        let mut n = 0;
        while t.is_thing() {
            n += 1;
            if n > 256 {
                return Err(format!("{what}: contents chain does not end"));
            }
            if t.index() >= dg.thing_count(t.kind()) {
                return Err(format!("{what}: content {:#06x} out of range", t.0));
            }
            note(t.0, format!("{what} (contents)"))?;
            if t.kind() == ThingType::Container {
                owners.push((t, format!("container {:#06x} inside {what}", t.0 & 0x3FFF)));
            }
            let next = dg.record_word(t, 0).unwrap_or(ThingRef::END.0);
            if next == Dungeon::FREE {
                return Err(format!("{what}: freed thing {:#06x} still in contents", t.0 & 0x3FFF));
            }
            t = ThingRef(next);
        }
    }

    // The party stands somewhere it may stand.
    let p = g.party;
    if p.map >= dg.maps.len() || world::blocks(dg, p.map, p.x, p.y) {
        return Err(format!("party stands on a blocked square: {p:?} ({:?})", dg.square(p.map, p.x, p.y).element()));
    }

    // Square-addressed events point at real squares.
    for (slot, e) in g.timeline.iter() {
        if matches!(e.kind, 0x01 | 0x02 | 0x04) {
            let m = e.map as usize;
            let ok = m < dg.maps.len() && (e.x as i32) < dg.maps[m].width as i32 && (e.y as i32) < dg.maps[m].height as i32;
            if !ok {
                return Err(format!("event slot {slot} type {:#04x} points outside its map: {e:?}", e.kind));
            }
        }
    }

    // Each champion's cached load matches its inventory.
    if let Some(data) = &g.data {
        let db = data.item_db(dg);
        for (ci, c) in g.champions.iter().enumerate() {
            if !c.is_alive() {
                continue;
            }
            let mut fresh = c.clone();
            dm2_engine::champions::recompute_load(&mut fresh, &db);
            if fresh.load() != c.load() {
                return Err(format!("champion {ci} cached load {} but inventory weighs {}", c.load(), fresh.load()));
            }
        }
    }

    // Champion stats.
    for (ci, c) in g.champions.iter().enumerate() {
        let (h, mh) = (c.health(), c.max_health());
        let (s, ms) = (c.stamina(), c.max_stamina());
        let (n, mn) = (c.mana(), c.max_mana());
        let bad = |w: &str| Err(format!("champion {ci} {w}: hp {h}/{mh} st {s}/{ms} mana {n}/{mn} food {} water {}", c.food(), c.water()));
        if !(0..=999).contains(&mh) || !(0..=mh).contains(&h) {
            return bad("health");
        }
        if !(0..=9999).contains(&ms) || !(0..=ms).contains(&s) {
            return bad("stamina");
        }
        if !(0..=900).contains(&mn) || n < 0 {
            return bad("mana");
        }
        if !(-1024..=2048).contains(&c.food()) || !(-1024..=2048).contains(&c.water()) {
            return bad("food/water");
        }
    }
    if let Some(l) = g.leader {
        if l >= g.champions.len() || !g.champions[l].is_alive() {
            return Err(format!("leader {l} is not a living champion"));
        }
    }

    // Creature slots point at real, listed creature groups, once each, and
    // their pending events exist.
    let mut slotted = HashSet::new();
    for (si, s) in g.creature_slots.iter().enumerate() {
        let Some(s) = s else { continue };
        let t = s.thing;
        if t.kind() != ThingType::Creature {
            return Err(format!("creature slot {si} holds non-creature {:#06x}", t.0));
        }
        let key = t.0 & 0x3FFF;
        if !slotted.insert(key) {
            return Err(format!("creature {key:#06x} has two slots"));
        }
        if !on_square.contains_key(&key) {
            return Err(format!("creature slot {si} points at {key:#06x}, which is on no square"));
        }
        if let Some(e) = s.event {
            if g.timeline.get(e).is_none() {
                return Err(format!("creature slot {si} waits on dead event slot {e}"));
            }
        }
    }
    Ok(())
}

/// A random walkable square on a random map, for map jumps.
fn random_open(ctl: &mut Ctl, dg: &Dungeon) -> Option<PartyPos> {
    for _ in 0..50 {
        let map = ctl.below(dg.maps.len() as u32) as usize;
        let m = &dg.maps[map];
        let (x, y) = (ctl.below(m.width as u32) as i32, ctl.below(m.height as u32) as i32);
        if !world::blocks(dg, map, x, y) {
            return Some(PartyPos { map, x, y, dir: ctl.below(4) as u8 });
        }
    }
    None
}

/// Pick this tick's player input: mostly purposeful exploring and
/// fighting, with random interface actions mixed in.
fn drive(ctl: &mut Ctl, plan: &mut VecDeque<Command>, g: &GameState) -> Command {
    if let Some(c) = plan.pop_front() {
        return c;
    }
    let d = g.party.dir as usize;
    let (map, x, y) = (g.party.map, g.party.x, g.party.y);
    let ahead = (x + DX[d], y + DY[d]);
    // Now and then cast a real spell from the user's spell table: its runes
    // (column = symbol - base - 6 * position), then Cast.
    if ctl.chance(3) {
        if let Some(spells) = g.data.as_ref().map(|d| d.tables.spells.clone()) {
            if !spells.is_empty() {
                let s = &spells[ctl.below(spells.len() as u32) as usize];
                for n in 0..4u32 {
                    let b = (s.key >> (24 - 8 * n)) as u8;
                    let col = match (n, b) {
                        (0, 0) => ctl.below(3) as u8,
                        (_, 0) => break,
                        _ => b.wrapping_sub(RUNE_BASE).wrapping_sub(6 * n as u8),
                    };
                    plan.push_back(Command::Ui(0x65 + (col % 6) as u16));
                }
                plan.push_back(Command::Ui(0x6C));
                return plan.pop_front().unwrap();
            }
        }
    }
    // Walk into a closed door now and then: bashing.
    let door_shut = g.dungeon.square(map, ahead.0, ahead.1).element() == Element::Door
        && world::blocks(&g.dungeon, map, ahead.0, ahead.1);
    if door_shut && ctl.chance(30) {
        return Command::Move(Move::Forward);
    }
    // A creature group directly ahead: attack with one of the leader's hands.
    if creatures::group_at(g, map, ahead.0, ahead.1).is_some() && ctl.chance(70) {
        let leader = g.leader.unwrap_or(0) as u16;
        let cmd = if g.hand.menu.is_some() { 0x71 + ctl.below(3) as u16 } else { 0x74 + leader * 2 + ctl.below(2) as u16 };
        return Command::Ui(cmd);
    }
    if ctl.chance(70) {
        let dg = &g.dungeon;
        let open = |dir: usize| !world::blocks(dg, map, x + DX[dir], y + DY[dir]);
        let stairs = |dir: usize| dg.square(map, x + DX[dir], y + DY[dir]).element() == Element::Stairs;
        let cmd = if stairs(d) {
            Command::Move(Move::Forward)
        } else if stairs((d + 1) & 3) {
            Command::TurnRight
        } else if stairs((d + 3) & 3) {
            Command::TurnLeft
        } else if open(d) && ctl.chance(85) {
            Command::Move(Move::Forward)
        } else if open((d + 1) & 3) && (ctl.chance(50) || !open((d + 3) & 3)) {
            Command::TurnRight
        } else {
            Command::TurnLeft
        };
        return cmd;
    }
    random_input(ctl, g)
}

/// A random player input from the whole command set.
fn random_input(ctl: &mut Ctl, g: &GameState) -> Command {
    let r = ctl.below(100);
    let cmd = match r {
        0..=29 => Command::Move(Move::Forward),
        30..=34 => Command::Move(Move::Back),
        35..=39 => Command::Move(if ctl.chance(50) { Move::Left } else { Move::Right }),
        40..=49 => if ctl.chance(50) { Command::TurnLeft } else { Command::TurnRight },
        // Floor and wall clicks: take, drop, alcoves, buttons.
        50..=59 => Command::Viewport(match ctl.below(6) {
            0 => ViewRegion::NearLeft,
            1 => ViewRegion::NearRight,
            2 => ViewRegion::AheadLeft,
            3 => ViewRegion::AheadRight,
            4 => ViewRegion::WallLeft,
            _ => ViewRegion::WallRight,
        }),
        // Throwing whatever is held.
        60..=62 => Command::Viewport(ViewRegion::Elsewhere { right: ctl.chance(50) }),
        // Inventory: open, swap slots, eat, close.
        63..=64 => Command::Ui(0x07 + ctl.below(g.champions.len().max(1) as u32) as u16),
        65..=72 => Command::Ui(0x1C + ctl.below(30) as u16),
        73..=74 => Command::Ui(0x14 + ctl.below(8) as u16),
        75..=76 => Command::Ui(0x46),
        77 => Command::Ui(0x0B),
        // Actions: open a hand's menu, pick a row, close.
        78..=81 => Command::Ui(0x74 + ctl.below(8) as u16),
        82..=85 => Command::Ui(0x71 + ctl.below(3) as u16),
        86 => Command::Ui(0x70),
        // Spells: runes, delete, cast.
        87..=91 => Command::Ui(0x65 + ctl.below(6) as u16),
        92 => Command::Ui(0x6B),
        93..=95 => Command::Ui(0x6C),
        // Leader choice by party cell.
        96 => Command::Ui(0x5F + ctl.below(4) as u16),
        _ => Command::Move(Move::Forward),
    };
    cmd
}

struct Stats {
    ticks: u32,
    saves: u32,
    jumps: u32,
    restarts: u32,
    maps: HashSet<usize>,
    slowest: Duration,
    /// Coverage: effect kinds seen, items taken into and out of the hand,
    /// map changes not caused by a harness jump, and missiles alive.
    effects: HashMap<String, u32>,
    picked: u32,
    put_down: u32,
    walked_maps: u32,
}

/// Run `ticks` ticks from seed `seed`, checking invariants every tick.
fn run(w: &World, seed: u64, ticks: u32, save_every: u32, jump_every: u32) -> (Stats, GameState) {
    let mut ctl = Ctl(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut g = new_game(w);
    let mut plan = VecDeque::new();
    let mut recent: VecDeque<(u32, Command)> = VecDeque::new();
    let mut st = Stats {
        ticks: 0,
        saves: 0,
        jumps: 0,
        restarts: 0,
        maps: HashSet::new(),
        slowest: Duration::ZERO,
        effects: HashMap::new(),
        picked: 0,
        put_down: 0,
        walked_maps: 0,
    };
    check(&g).unwrap_or_else(|e| panic!("seed {seed}: invariant broken in the new game: {e}"));
    for n in 1..=ticks {
        if g.game_over || g.champions.iter().all(|c| !c.is_alive()) {
            if std::env::var_os("DM2_SOAK_VERBOSE").is_some() {
                eprintln!("seed {seed} tick {n}: restart (game_over {}, party {:?}, champions {:?})", g.game_over, g.party, g.champions.iter().map(|c| (c.health(), c.max_health())).collect::<Vec<_>>());
            }
            g = new_game(w);
            st.restarts += 1;
        }
        let mut jumped = false;
        if jump_every > 0 && n % jump_every == 0 {
            if let Some(p) = random_open(&mut ctl, &g.dungeon) {
                g.pending_map = Some(p);
                st.jumps += 1;
                jumped = true;
            }
        }
        if save_every > 0 && n % save_every == 0 {
            let bytes = save::to_bytes(&g, "SOAK").unwrap_or_else(|e| panic!("seed {seed} tick {n}: save failed: {e}"));
            g = save::from_bytes(&bytes, w.data.clone(), w.creatures.clone())
                .unwrap_or_else(|e| panic!("seed {seed} tick {n}: load failed: {e}"));
            st.saves += 1;
            check(&g).unwrap_or_else(|e| panic!("seed {seed} tick {n}: invariant broken after load: {e}"));
        }
        let cmd = drive(&mut ctl, &mut plan, &g);
        g.push_command(cmd);
        recent.push_back((g.tick, cmd));
        if recent.len() > 8 {
            recent.pop_front();
        }
        let (held_before, map_before) = (g.hand.held, g.party.map);
        let start = Instant::now();
        let r = catch_unwind(AssertUnwindSafe(|| g.advance()));
        if let Err(p) = r {
            eprintln!("seed {seed}: panic during tick {n} (game tick {}), party {:?}", g.tick, g.party);
            resume_unwind(p);
        }
        let dt = start.elapsed();
        st.slowest = st.slowest.max(dt);
        assert!(dt < Duration::from_secs(5), "seed {seed} tick {n}: a tick took {dt:?}");
        for e in g.effects.drain(..) {
            let name = format!("{e:?}");
            let name = name.split([' ', '{', '(']).next().unwrap_or("").to_string();
            *st.effects.entry(name).or_default() += 1;
        }
        if g.timeline.iter().any(|(_, e)| matches!(e.kind, 0x1D | 0x1E)) {
            *st.effects.entry("(missile in flight)".into()).or_default() += 1;
        }
        match (held_before == EMPTY, g.hand.held == EMPTY) {
            (true, false) => st.picked += 1,
            (false, true) => st.put_down += 1,
            _ => {}
        }
        if g.party.map != map_before && !jumped {
            st.walked_maps += 1;
        }
        st.maps.insert(g.party.map);
        st.ticks = n;
        if let Err(e) = check(&g) {
            panic!("seed {seed} tick {n} (game tick {}, party {:?}): {e}\nlast commands (game tick, command): {recent:?}", g.tick, g.party);
        }
    }
    (st, g)
}

#[test]
fn quick_playthrough() {
    let Some(w) = load_world() else { return };
    for seed in 1..=2 {
        let (st, _) = run(&w, seed, 700, 300, 150);
        assert!(st.maps.len() > 3, "seed {seed}: visited only {:?}", st.maps);
        report(seed, &st);
    }
}

/// The same seed must give the same game: the save bytes of two runs match.
#[test]
fn playthrough_is_deterministic() {
    let Some(w) = load_world() else { return };
    let (_, a) = run(&w, 7, 400, 150, 100);
    let (_, b) = run(&w, 7, 400, 150, 100);
    let (a, b) = (save::to_bytes(&a, "A").unwrap(), save::to_bytes(&b, "A").unwrap());
    assert!(a == b, "two runs of the same seed diverged");
}

/// Once nothing is in flight, missile and cloud (explosion) records must
/// all be free again: anything else is a leak.
#[test]
fn missiles_and_explosions_do_not_leak() {
    let Some(w) = load_world() else { return };
    let fresh = new_game(&w);
    let (_, mut g) = run(&w, 11, 1200, 0, 200);
    // Let everything in flight land and every explosion expire.
    for _ in 0..400 {
        g.advance();
        g.effects.clear();
    }
    for kind in [ThingType::Missile, ThingType::Cloud] {
        let live = (0..g.dungeon.maps.len())
            .flat_map(|m| {
                let md = &g.dungeon.maps[m];
                let dg = &g.dungeon;
                (0..md.width as i32).flat_map(move |x| (0..md.height as i32).map(move |y| (m, x, y))).flat_map(move |(m, x, y)| dg.things_at(m, x, y))
            })
            .filter(|t| t.kind() == kind)
            .count();
        let free = g.dungeon.free_count(kind);
        assert_eq!(
            free + live,
            fresh.dungeon.free_count(kind) + fresh_live(&fresh, kind),
            "{kind:?}: {free} free + {live} placed now, against the fresh game's records"
        );
    }
}

fn fresh_live(g: &GameState, kind: ThingType) -> usize {
    (0..g.dungeon.maps.len())
        .map(|m| {
            let md = &g.dungeon.maps[m];
            (0..md.width as i32)
                .flat_map(|x| (0..md.height as i32).map(move |y| (x, y)))
                .map(|(x, y)| g.dungeon.things_at(m, x, y).into_iter().filter(|t| t.kind() == kind).count())
                .sum::<usize>()
        })
        .sum()
}

fn report(seed: u64, st: &Stats) {
    let mut fx: Vec<_> = st.effects.iter().collect();
    fx.sort();
    eprintln!(
        "seed {seed}: {} ticks, {} saves, {} jumps, {} restarts, {} maps ({} walked), held +{} -{}, slowest {:?}, effects {fx:?}",
        st.ticks, st.saves, st.jumps, st.restarts, st.maps.len(), st.walked_maps, st.picked, st.put_down, st.slowest
    );
}

#[test]
#[ignore]
fn soak_playthrough() {
    let Some(w) = load_world() else { return };
    let seeds: u64 = std::env::var("DM2_SOAK_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    let ticks: u32 = std::env::var("DM2_SOAK_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000);
    for seed in 1..=seeds {
        let (st, _) = run(&w, seed, ticks, 1000, 600);
        report(seed, &st);
    }
}
