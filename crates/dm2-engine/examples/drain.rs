//! Idle a new game and print the starting champion's upkeep over time
//! (debugging aid for the regeneration and food/water formulas).
use std::rc::Rc;

use dm2_engine::{assets::default_data_dir, data::GameData, state::GameState};
use dm2_formats::dungeon::Dungeon;

fn main() {
    let ticks: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1200);
    let dg = Dungeon::parse(&std::fs::read(default_data_dir().join("DUNGEON.DAT")).unwrap()).unwrap();
    let mut g = GameState::new_game_with(&dg, Rc::new(GameData::load_default().unwrap()));
    for t in 0..=ticks {
        if t % (ticks / 12).max(1) == 0 {
            let c = &g.champions[0];
            println!(
                "tick {t:6} hp {}/{} st {}/{} mana {}/{} food {} water {}",
                c.health(),
                c.max_health(),
                c.stamina(),
                c.max_stamina(),
                c.mana(),
                c.max_mana(),
                c.food(),
                c.water()
            );
        }
        g.advance();
    }
}
