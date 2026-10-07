//! Replay a probe scenario in the remake and print the state to compare with
//! the original's save: statecheck PRESSES GAP_TICKS END_TICK [KEY]
//! KEY is forward (default), back, left or right.
use std::rc::Rc;
use dm2_engine::{assets, data::GameData, state::{Command, GameState}, world::Move};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (presses, gap, end): (u32, u32, u32) = (a[0].parse().unwrap(), a[1].parse().unwrap(), a[2].parse().unwrap());
    let mv = match a.get(3).map(String::as_str) {
        Some("back") => Move::Back,
        Some("left") => Move::Left,
        Some("right") => Move::Right,
        _ => Move::Forward,
    };
    let assets = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let mut g = GameState::new_game_with(&assets.dungeon, gd);
    let mut next = 1u32;
    let mut done = 0;
    while g.tick < end {
        if done < presses && g.tick >= next {
            g.push_command(Command::Move(mv));
            done += 1;
            next = g.tick + gap;
        }
        let st = g.champions[0].stamina();
        g.advance();
        if std::env::var_os("TRACE").is_some() && g.champions[0].stamina() != st {
            println!("  tick {} stamina {} -> {}", g.tick, st, g.champions[0].stamina());
        }
    }
    let c = &g.champions[0];
    println!("tick {} pos map {} ({},{}) dir {}  hp {}/{} st {}/{} food {} water {}",
        g.tick, g.party.map, g.party.x, g.party.y, g.party.dir,
        c.health(), c.max_health(), c.stamina(), c.max_stamina(), c.food(), c.water());
}
