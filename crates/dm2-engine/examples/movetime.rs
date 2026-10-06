//! Print the party's move time (ticks per step) at the start of a new game.
use std::rc::Rc;
use dm2_engine::{assets, champions, data::GameData, state::GameState};
fn main() {
    let a = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let mut g = GameState::new_game_with(&a.dungeon, gd);
    for _ in 0..5 {
        let t = champions::party_move_time(&g.champions, &g.party_status, &mut g.rng);
        println!("move time {t} ticks");
    }
}
