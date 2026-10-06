//! Dungeon Master II remake: windowed frontend.
//!
//! Usage: dm2 [DATA_DIR]   (default: $DM2_DATA, then original/dumast2/DATA)
//!
//! Keys: W/Up forward, S/Down back, A/D strafe, Q/E or Left/Right turn,
//! PageUp/PageDown cycle maps (debug), Tab toggle debug overlay.

use std::path::PathBuf;

use dm2_engine::assets::{self, Assets};
use dm2_engine::gfx::{Bitmap, SCREEN_H, SCREEN_W};
use dm2_engine::viewport::{self, VP_SCREEN_POS};
use dm2_engine::state::{Command, GameState};
use dm2_engine::world::{Move, PartyPos};
use macroquad::prelude::*;

const SCALE: i32 = 3;

fn window_conf() -> Conf {
    Conf {
        window_title: "Dungeon Master II remake".to_owned(),
        window_width: SCREEN_W as i32 * SCALE,
        window_height: SCREEN_H as i32 * SCALE,
        ..Default::default()
    }
}

fn data_dir() -> PathBuf {
    std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("DM2_DATA").map(PathBuf::from))
        .unwrap_or_else(assets::default_data_dir)
}

/// First walkable square of a map, for debug map cycling.
fn first_open(dg: &dm2_formats::dungeon::Dungeon, map: usize) -> Option<(i32, i32)> {
    let m = &dg.maps[map];
    for x in 0..m.width as i32 {
        for y in 0..m.height as i32 {
            if !dm2_engine::world::blocks(dg, map, x, y) {
                return Some((x, y));
            }
        }
    }
    None
}

#[macroquad::main(window_conf)]
async fn main() {
    let dir = data_dir();
    let mut a = match Assets::load(&dir) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\nPoint dm2 at the DATA directory of your Dungeon Master II install.");
            std::process::exit(1);
        }
    };
    let mut game = GameState::new_game(&a.dungeon);
    // Real-time tick length is not yet known (docs/05); configurable.
    let tick_secs = std::env::var("DM2_TICK_MS").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(166.0) / 1000.0;
    let mut acc = 0.0f64;
    let mut screen = Bitmap::new(SCREEN_W, SCREEN_H);
    let mut rgba = vec![0u8; SCREEN_W * SCREEN_H * 4];
    let tex = Texture2D::from_rgba8(SCREEN_W as u16, SCREEN_H as u16, &rgba);
    tex.set_filter(FilterMode::Nearest);
    let mut debug = true;

    loop {
        let keys = [
            (KeyCode::W, Command::Move(Move::Forward)),
            (KeyCode::Up, Command::Move(Move::Forward)),
            (KeyCode::S, Command::Move(Move::Back)),
            (KeyCode::Down, Command::Move(Move::Back)),
            (KeyCode::A, Command::Move(Move::Left)),
            (KeyCode::D, Command::Move(Move::Right)),
            (KeyCode::Q, Command::TurnLeft),
            (KeyCode::Left, Command::TurnLeft),
            (KeyCode::E, Command::TurnRight),
            (KeyCode::Right, Command::TurnRight),
        ];
        for (k, c) in keys {
            if is_key_pressed(k) {
                game.push_command(c);
            }
        }
        acc += get_frame_time() as f64;
        while acc >= tick_secs {
            acc -= tick_secs;
            game.advance();
        }
        let n = a.dungeon.maps.len();
        for (key, delta) in [(KeyCode::PageDown, 1), (KeyCode::PageUp, n - 1)] {
            if is_key_pressed(key) {
                let mut m = game.party.map;
                for _ in 0..n {
                    m = (m + delta) % n;
                    if let Some((x, y)) = first_open(&game.dungeon, m) {
                        game.party = PartyPos { map: m, x, y, dir: game.party.dir };
                        break;
                    }
                }
            }
        }
        if is_key_pressed(KeyCode::Tab) {
            debug = !debug;
        }

        screen.fill(0);
        let vp = viewport::render(&mut a, &game.dungeon, game.party.map, game.party.x, game.party.y, game.party.dir);
        screen.paste(&vp, VP_SCREEN_POS.0, VP_SCREEN_POS.1);
        for (i, &c) in screen.px.iter().enumerate() {
            let [r, g, b] = a.palette[c as usize];
            rgba[i * 4..i * 4 + 4].copy_from_slice(&[r, g, b, 255]);
        }
        tex.update_from_bytes(SCREEN_W as u32, SCREEN_H as u32, &rgba);

        clear_background(BLACK);
        draw_texture_ex(
            &tex,
            0.0,
            0.0,
            WHITE,
            DrawTextureParams { dest_size: Some(vec2(screen_width(), screen_height())), ..Default::default() },
        );
        if debug {
            let label = format!(
                "map {} ({},{}) facing {}  layer {}  tick {}",
                game.party.map,
                game.party.x,
                game.party.y,
                ["N", "E", "S", "W"][game.party.dir as usize],
                game.dungeon.maps[game.party.map].depth,
                game.tick
            );
            draw_text(&label, 8.0, 20.0, 22.0, YELLOW);
        }
        next_frame().await;
    }
}
