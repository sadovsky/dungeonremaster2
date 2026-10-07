//! Replay a scripted route in the remake and print the state to compare with
//! a save the original wrote after the same inputs (tools/state_probe.py):
//!
//!   routecheck ROUTE GAP_TICKS END_TICK
//!
//! ROUTE is a space-separated list: F/B/L/R[n] steps forward, back, left or
//! right n times; TL/TR[n] turn; W[n] waits n ticks. Commands are issued
//! GAP_TICKS apart, starting at tick 1, and the game runs to END_TICK (the
//! tick the original saved at).
use std::rc::Rc;

use dm2_engine::{assets, creatures::data::CreatureData, data::GameData, state::{Command, GameState}, world::Move};
use dm2_formats::gdat::Gdat;

enum Step {
    Cmd(Command),
    Wait(u32),
    /// A command issued at an exact tick ("F@34", "TR@79"), e.g. from the
    /// DOSBox hook's log of the original's party moves.
    At(u32, Command),
}

fn command(head: &str) -> Command {
    match head {
        "F" => Command::Move(Move::Forward),
        "B" => Command::Move(Move::Back),
        "L" => Command::Move(Move::Left),
        "R" => Command::Move(Move::Right),
        "TL" => Command::TurnLeft,
        "TR" => Command::TurnRight,
        other => panic!("unknown route token {other}"),
    }
}

fn parse(route: &str) -> Vec<Step> {
    let mut out = Vec::new();
    for tok in route.split_whitespace() {
        if let Some((head, tick)) = tok.split_once('@') {
            out.push(Step::At(tick.parse().unwrap(), command(head)));
            continue;
        }
        let (head, n) = match tok.find(|c: char| c.is_ascii_digit()) {
            Some(i) => (&tok[..i], tok[i..].parse().unwrap()),
            None => (tok, 1u32),
        };
        let step = match head {
            "F" => Step::Cmd(Command::Move(Move::Forward)),
            "B" => Step::Cmd(Command::Move(Move::Back)),
            "L" => Step::Cmd(Command::Move(Move::Left)),
            "R" => Step::Cmd(Command::Move(Move::Right)),
            "TL" => Step::Cmd(Command::TurnLeft),
            "TR" => Step::Cmd(Command::TurnRight),
            "W" => {
                out.push(Step::Wait(n));
                continue;
            }
            other => panic!("unknown route token {other}"),
        };
        for _ in 0..n {
            out.push(match &step {
                Step::Cmd(c) => Step::Cmd(*c),
                Step::Wait(w) => Step::Wait(*w),
                Step::At(t, c) => Step::At(*t, *c),
            });
        }
    }
    out
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let route = parse(&a[0]);
    let (gap, end): (u32, u32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let assets = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = Rc::new(CreatureData::load(gdat, &exe).unwrap());
    // LOAD=SAVE starts from a save (e.g. one state_probe.py --load started
    // the original from); ticks in the route and END_TICK stay absolute.
    let mut g = match std::env::var_os("LOAD") {
        Some(p) => dm2_engine::save::read(std::path::Path::new(&p), gd, Some(cd)).expect("load save"),
        None => GameState::new_game_full(&assets.dungeon, gd, Some(cd)),
    };
    let mut steps = route.into_iter().peekable();
    let mut next = g.tick + 1;
    while g.tick < end {
        // Exact-tick commands: issue every one due on this tick.
        while let Some(Step::At(t, c)) = steps.peek() {
            if *t > g.tick {
                break;
            }
            g.push_command(*c);
            steps.next();
        }
        if g.tick >= next && !matches!(steps.peek(), Some(Step::At(..))) {
            match steps.next() {
                Some(Step::Cmd(c)) => {
                    g.push_command(c);
                    next = g.tick + gap;
                }
                Some(Step::Wait(w)) => next = g.tick + w,
                Some(Step::At(..)) => unreachable!(),
                None => next = u32::MAX,
            }
        }
        let before = g.champions.first().map(|c| (c.stamina(), c.food(), c.water()));
        let t = g.tick;
        g.advance();
        if std::env::var_os("TRACE").is_some() {
            let after = g.champions.first().map(|c| (c.stamina(), c.food(), c.water()));
            if before.map(|b| (b.1, b.2)) != after.map(|a| (a.1, a.2)) {
                println!("  tick {t}: stamina/food/water {before:?} -> {after:?}");
            }
        }
    }
    let p = g.party;
    println!("tick {}  map {} ({},{}) dir {}  rng {:#06x}", g.tick, p.map, p.x, p.y, p.dir, g.rng.state & 0xFFFF);
    for c in &g.champions {
        println!(
            "{:?} hp {}/{} st {}/{} mana {}/{} food {} water {} wounds {:#x}",
            c.name(), c.health(), c.max_health(), c.stamina(), c.max_stamina(),
            c.mana(), c.max_mana(), c.food(), c.water(), c.wounds()
        );
    }
}
