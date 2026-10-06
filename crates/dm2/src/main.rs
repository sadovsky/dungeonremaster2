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
fn first_open(a: &Assets, map: usize) -> Option<(i32, i32)> {
    let m = &a.dungeon.maps[map];
    for x in 0..m.width as i32 {
        for y in 0..m.height as i32 {
            if !dm2_engine::world::blocks(&a.dungeon, map, x, y) {
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
    let s = &a.dungeon.start;
    let mut party = PartyPos { map: 0, x: s.x as i32, y: s.y as i32, dir: s.facing };
    let mut screen = Bitmap::new(SCREEN_W, SCREEN_H);
    let mut rgba = vec![0u8; SCREEN_W * SCREEN_H * 4];
    let tex = Texture2D::from_rgba8(SCREEN_W as u16, SCREEN_H as u16, &rgba);
    tex.set_filter(FilterMode::Nearest);
    let mut debug = true;

    loop {
        if is_key_pressed(KeyCode::W) || is_key_pressed(KeyCode::Up) {
            party.step(&a.dungeon, Move::Forward);
        }
        if is_key_pressed(KeyCode::S) || is_key_pressed(KeyCode::Down) {
            party.step(&a.dungeon, Move::Back);
        }
        if is_key_pressed(KeyCode::A) {
            party.step(&a.dungeon, Move::Left);
        }
        if is_key_pressed(KeyCode::D) {
            party.step(&a.dungeon, Move::Right);
        }
        if is_key_pressed(KeyCode::Q) || is_key_pressed(KeyCode::Left) {
            party.turn_left();
        }
        if is_key_pressed(KeyCode::E) || is_key_pressed(KeyCode::Right) {
            party.turn_right();
        }
        let n = a.dungeon.maps.len();
        for (key, delta) in [(KeyCode::PageDown, 1), (KeyCode::PageUp, n - 1)] {
            if is_key_pressed(key) {
                let mut m = party.map;
                for _ in 0..n {
                    m = (m + delta) % n;
                    if let Some((x, y)) = first_open(&a, m) {
                        party = PartyPos { map: m, x, y, dir: party.dir };
                        break;
                    }
                }
            }
        }
        if is_key_pressed(KeyCode::Tab) {
            debug = !debug;
        }

        screen.fill(0);
        let vp = viewport::render(&mut a, party.map, party.x, party.y, party.dir);
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
                "map {} ({},{}) facing {}  layer {}",
                party.map,
                party.x,
                party.y,
                ["N", "E", "S", "W"][party.dir as usize],
                a.dungeon.maps[party.map].depth
            );
            draw_text(&label, 8.0, 20.0, 22.0, YELLOW);
        }
        next_frame().await;
    }
}
