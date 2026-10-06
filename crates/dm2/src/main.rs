//! Dungeon Master II remake: windowed frontend.
//!
//! Usage:
//!   dm2 [DATA_DIR]                          play (default data: $DM2_DATA,
//!                                           then original/dumast2/DATA)
//!   dm2 --screenshot OUT.png [MAP X Y DIR] [--ticks N] [--cmd C]...
//!                                           render one game frame headless,
//!                                           after N game ticks and the given
//!                                           interface commands (hex, e.g. 0x07)
//!   dm2 --screenshot-title OUT.png [SUB]    render title image (5,0,1,SUB) headless
//!
//! Controls follow the original's tables (read from SKULL.EXE): keypad
//! 4/5/6 turn left / forward / turn right, 1/2/3 strafe left / back /
//! strafe right, Esc pauses, mouse clicks on the arrows and panels.
//! Extra keys: W/A/S/D and Q/E or the cursor keys, PageUp/PageDown cycle
//! maps (debug), Tab toggles the debug overlay.

mod png;
mod sound;

use std::path::{Path, PathBuf};

use dm2_engine::assets::{self, Assets};
use dm2_engine::creatures::{self, data::CreatureData};
use dm2_engine::data::GameData;
use dm2_engine::missiles;
use dm2_engine::save;
use dm2_formats::dungeon::ThingType;
use dm2_formats::gdat::Gdat;
use dm2_engine::exe::Exe;
use dm2_engine::font::Font;
use dm2_engine::gfx::{Bitmap, SCREEN_H, SCREEN_W};
use dm2_engine::input::{self, Input, Screen, UiState, BUTTON_LEFT, BUTTON_RIGHT, MOD_ALT, MOD_CTRL, MOD_SHIFT};
use dm2_engine::state::{Command, GameState};
use dm2_engine::hand;
use dm2_engine::ui::{self, ChampionView, Icon, InventoryView, MenuView, UiTables, UiView};
use dm2_engine::viewport;
use dm2_engine::world::{Move, PartyPos};
use dm2_formats::dungeon::ThingRef;
use macroquad::prelude::*;

const SCALE: i32 = 3;
/// Sub-index of the title-screen image in category 5.
const TITLE_FRAME: u8 = 4;

fn window_conf() -> Conf {
    Conf {
        window_title: "Dungeon Master II remake".to_owned(),
        window_width: SCREEN_W as i32 * SCALE,
        window_height: SCREEN_H as i32 * SCALE,
        ..Default::default()
    }
}

fn data_dir(arg: Option<String>) -> PathBuf {
    arg.map(PathBuf::from)
        .or_else(|| std::env::var_os("DM2_DATA").map(PathBuf::from))
        .unwrap_or_else(assets::default_data_dir)
}

/// Everything loaded from the user's game files.
struct Data {
    assets: Assets,
    font: Font,
    input: Option<Input>,
    tables: UiTables,
    /// Archive and executable tables the simulation needs; None if
    /// SKULL.EXE is missing (the game then runs without champions).
    game_data: Option<std::rc::Rc<GameData>>,
    /// Creature tables; creatures stay inert without them.
    creature_data: Option<std::rc::Rc<CreatureData>>,
}

/// Where save games go: $DM2_SAVE_DIR, else ./saves (kept out of the
/// user's original install).
fn save_dir() -> PathBuf {
    std::env::var_os("DM2_SAVE_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("saves"))
}

/// Title-screen Resume: load save slot 0 (SKSAVE0.DAT, then .BAK).
fn resume(d: &Data) -> Result<GameState, String> {
    let gd = d.game_data.clone().ok_or("SKULL.EXE is needed to load saves")?;
    save::load_slot(&save_dir(), 0, gd, d.creature_data.clone()).map_err(|e| e.to_string())
}

/// Start a new game the way the original does (recruits the starting
/// champion when the game data is available).
fn new_game(d: &Data) -> GameState {
    let mut g = match &d.game_data {
        Some(gd) => GameState::new_game_with(&d.assets.dungeon, gd.clone()),
        None => GameState::new_game(&d.assets.dungeon),
    };
    if let Some(cd) = &d.creature_data {
        creatures::set_data(&mut g, cd.clone());
    }
    g
}

fn load(dir: &Path) -> Data {
    let assets = match Assets::load(dir) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\nPoint dm2 at the DATA directory of your Dungeon Master II install.");
            std::process::exit(1);
        }
    };
    let font = Font::load(&assets.gdat).unwrap_or_else(|| {
        eprintln!("GRAPHICS.DAT has no interface font");
        std::process::exit(1);
    });
    // SKULL.EXE sits next to the DATA directory.
    let exe = Exe::open(&dir.join("../SKULL.EXE"));
    let input = exe.as_ref().and_then(Input::load);
    let tables = exe.as_ref().and_then(UiTables::load).unwrap_or_default();
    if input.is_none() {
        eprintln!("warning: SKULL.EXE not found next to {}; mouse zones disabled", dir.display());
    }
    let exe_path = dir.join("../SKULL.EXE");
    let game_data = GameData::load(dir, &exe_path).map(std::rc::Rc::new);
    let creature_data = std::fs::read(&exe_path).ok().and_then(|exe| {
        let gdat = std::rc::Rc::new(Gdat::open(dir.join("GRAPHICS.DAT")).ok()?);
        CreatureData::load(gdat, &exe).ok().map(std::rc::Rc::new)
    });
    Data { assets, font, input, tables, game_data, creature_data }
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

/// The interface view of the current game: the real party, or a demo
/// champion (DM2_DEMO_CHAMPION) when no champion has been recruited.
fn ui_view(g: &GameState, demo: bool) -> UiView {
    let mut v = UiView::default();
    for (i, c) in g.champions.iter().take(4).enumerate() {
        v.champions[i] = Some(ChampionView {
            name: c.name().into_bytes(),
            portrait: c.portrait(),
            rune_set: c.raw[0x1E],
            dead: !c.is_alive(),
            bars: [
                (c.health().max(0) as u16, c.max_health().max(0) as u16),
                (c.stamina().max(0) as u16, c.max_stamina().max(0) as u16),
                (c.mana().max(0) as u16, c.max_mana().max(0) as u16),
            ],
            ..Default::default()
        });
    }
    v.leader = g.leader;
    // Icon frame from attribute 6 (0x37F76); `slot` is where the item sits,
    // for the "animate only while equipped" gate.
    let icon = |t: u16, slot: Option<usize>| -> Option<Icon> {
        if t == 0xFFFF {
            return None;
        }
        let r = ThingRef(t);
        let (c, i) = hand::item_key(g, r)?;
        let sub = match g.data.as_ref() {
            Some(data) => {
                let equipped = slot.is_some_and(|s| dm2_engine::party::slot_fits(g, r, s));
                let visual = g.tick.wrapping_mul(2_654_435_761) ^ t as u32;
                data.item_db(&g.dungeon).icon_sub(r, equipped, g.tick, g.party.dir, visual >> 16)
            }
            None => ITEM_ICON,
        };
        Some((c, i, sub))
    };
    for (i, c) in g.champions.iter().take(4).enumerate() {
        v.hands[i] = [icon(c.inventory(0), Some(0)), icon(c.inventory(1), Some(1))];
        v.busy[i] = [hand::hand_busy(g, i, 0), hand::hand_busy(g, i, 1)];
        v.cells[i] = (c.cell() + 4 - g.party.dir) & 3;
    }
    v.held = icon(g.hand.held, None);
    v.inventory_open = g.hand.inventory_open;
    if let Some(ci) = g.hand.inventory_open {
        let c = &g.champions[ci];
        let mut rng = g.rng.clone();
        let container = hand::open_container(g).map(|_| {
            (hand::CONTAINER_FIRST..hand::CONTAINER_FIRST + hand::CONTAINER_CELLS)
                .map(|s| icon(hand::slot_item(g, ci, s), None))
                .collect()
        });
        let info = (g.hand.show_info && g.hand.held != 0xFFFF)
            .then(|| hand::item_key(g, ThingRef(g.hand.held)))
            .flatten()
            .and_then(|(cat, idx)| {
                let gd = g.data.as_ref()?;
                dm2_engine::font::text(&gd.gdat, cat, idx, 24, &Default::default())
            });
        v.inventory = Some(InventoryView {
            champion: ci,
            slots: (0..30).map(|s| icon(c.inventory(s), Some(s))).collect(),
            container,
            name: c.name().into_bytes(),
            stats: [
                (c.health().max(0) as u16, c.max_health().max(0) as u16),
                (c.stamina().max(0) as u16, c.max_stamina().max(0) as u16),
                (c.mana().max(0) as u16, c.max_mana().max(0) as u16),
            ],
            food: c.food(),
            water: c.water(),
            poisoned: c.poison_pool() > 0,
            load: (c.load(), dm2_engine::champions::max_load(c, &mut rng)),
            info,
        });
    }
    if let Some(m) = &g.hand.menu {
        v.menu = Some(MenuView { champion: m.champion, names: m.actions.iter().map(|a| a.name.clone().into_bytes()).collect() });
    }
    if demo && g.champions.is_empty() {
        v.champions[0] = Some(ChampionView {
            name: b"TESTER".to_vec(),
            portrait: 0,
            bars: [(80, 100), (50, 100), (20, 100)],
            ..Default::default()
        });
        v.leader = Some(0);
    }
    v
}

/// Sub-index of an item's 16×16 icon (0x37F76 default).
const ITEM_ICON: u8 = 24;

fn ui_state(screen: Screen, view: &UiView) -> UiState {
    UiState {
        screen,
        champions: std::array::from_fn(|i| view.champions[i].is_some()),
        inventory_open: view.inventory_open,
        leader: view.leader,
        menu_choices: view.menu.as_ref().map_or(0, |m| m.names.len()),
        container_open: view.inventory.as_ref().is_some_and(|i| i.container.is_some()),
    }
}

/// Viewport inputs from the simulation: each active creature's frame,
/// position byte and facing rule.
fn view_extras(g: &GameState) -> viewport::ViewExtras {
    let mut ex = viewport::ViewExtras { tick: g.tick, ..Default::default() };
    for slot in g.creature_slots.iter().flatten() {
        if let Some(cv) = creatures::view(g, slot.thing) {
            let key = slot.thing.0 & 0x3FFF;
            ex.creature_frames.insert(key, cv.frame);
            // Type info word 0 bit 2: always drawn facing the party.
            let faces_party = g.creature_data.as_ref().is_some_and(|d| {
                creatures::type_info(g, d, creatures::creature_type(g, slot.thing)).is_some_and(|(i, _)| i.raw[0] & 4 != 0)
            });
            ex.creatures.insert(
                key,
                viewport::CreatureDraw { position: cv.jitter, faces_party, alt_frame: cv.alt_frame, ..Default::default() },
            );
        }
    }
    // Missiles in flight: direction from each one's flight event.
    let n = g.dungeon.thing_count(ThingType::Missile);
    for i in 0..n {
        let m = ThingRef((ThingType::Missile as u16) << 10 | i as u16);
        if let Some(dir) = missiles::view_dir(g, m) {
            ex.missile_dirs.insert(m.0 & 0x3FFF, dir);
        }
    }
    ex.mid_step = g.walk.is_some();
    if let Some(d) = &g.creature_data {
        // 0x802CC's high word (0x802CE) is the ambient level: the darkness
        // step (0x7F282) × 10, set each frame at 0x54015.
        ex.darkness_step = creatures::fight::darkness_level(g, d) as i32;
        ex.ambient = ex.darkness_step * 10;
    }
    ex
}

/// Where the view is drawn from: the square just left while the in-between
/// walking frame plays, otherwise the party's square.
fn view_pos(g: &GameState) -> PartyPos {
    g.walk.map_or(g.party, |(from, _)| from)
}

fn game_frame(d: &mut Data, g: &GameState, view: &UiView) -> Bitmap {
    let ex = view_extras(g);
    let p = view_pos(g);
    let vp = viewport::render_ex(&mut d.assets, &g.dungeon, p.map, p.x, p.y, p.dir, &ex);
    ui::compose(&mut d.assets, &d.font, &d.tables, view, &vp)
}

fn to_rgb(pal: &[[u8; 3]; 256], s: &Bitmap) -> Vec<u8> {
    s.px.iter().flat_map(|&c| pal[c as usize]).collect()
}

fn screenshot(args: &[String], title: bool) {
    let out = &args[0];
    let mut d = load(&data_dir(None));
    let frame = if title {
        let f = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(TITLE_FRAME);
        ui::title(&mut d.assets, f)
    } else {
        let mut g = new_game(&d);
        // Positional MAP X Y DIR, then --ticks N and --cmd C options.
        let pos: Vec<&String> = args[1..].iter().take_while(|a| !a.starts_with("--")).collect();
        if pos.len() >= 4 {
            let n: Vec<i32> = pos[..4].iter().map(|s| s.parse().expect("MAP X Y DIR must be numbers")).collect();
            g.party = PartyPos { map: n[0] as usize, x: n[1], y: n[2], dir: n[3] as u8 };
        }
        let mut ticks = 0u32;
        let mut cmds = Vec::new();
        let mut it = args[1..].iter().skip_while(|a| !a.starts_with("--"));
        while let Some(a) = it.next() {
            let v = it.next().map(String::as_str).unwrap_or("0");
            let num = |v: &str| u32::from_str_radix(v.trim_start_matches("0x"), if v.starts_with("0x") { 16 } else { 10 });
            match a.as_str() {
                "--ticks" => ticks = num(v).expect("--ticks N"),
                "--cmd" => cmds.push(num(v).expect("--cmd C") as u16),
                _ => panic!("unknown option {a}"),
            }
        }
        for c in cmds {
            if let Some(gc) = input::game_command(c) {
                g.push_command(gc);
            }
            g.advance();
        }
        for _ in 0..ticks {
            g.advance();
        }
        let demo = std::env::var_os("DM2_DEMO_CHAMPION").is_some();
        game_frame(&mut d, &g, &ui_view(&g, demo))
    };
    let png = png::encode_rgb(SCREEN_W as u32, SCREEN_H as u32, &to_rgb(&d.assets.palette, &frame));
    std::fs::write(out, png).expect("write screenshot");
    println!("{out}");
}

/// BIOS scan code (plus modifier bits) for a macroquad key, as used by the
/// original key table. Only keys the game binds are listed.
fn scan_code(k: KeyCode) -> Option<u16> {
    use KeyCode::*;
    let code = match k {
        Escape => 0x01,
        Key1 => 0x02,
        Key2 => 0x03,
        Key3 => 0x04,
        Key4 => 0x05,
        Q => 0x10,
        S => 0x1F,
        Enter | KpEnter => 0x1C,
        Space => 0x39,
        Kp7 => 0x47,
        Kp8 => 0x48,
        Kp9 => 0x49,
        Kp4 => 0x4B,
        Kp5 => 0x4C,
        Kp6 => 0x4D,
        Kp1 => 0x4F,
        Kp2 => 0x50,
        Kp3 => 0x51,
        _ => return None,
    };
    let mut m = 0;
    if is_key_down(LeftShift) || is_key_down(RightShift) {
        m |= MOD_SHIFT;
    }
    if is_key_down(LeftAlt) || is_key_down(RightAlt) {
        m |= MOD_ALT;
    }
    if is_key_down(LeftControl) || is_key_down(RightControl) {
        m |= MOD_CTRL;
    }
    Some(code | m)
}

/// Remake-only convenience keys (not in the original tables).
const EXTRA_KEYS: [(KeyCode, Command); 8] = [
    (KeyCode::W, Command::Move(Move::Forward)),
    (KeyCode::Up, Command::Move(Move::Forward)),
    (KeyCode::Down, Command::Move(Move::Back)),
    (KeyCode::A, Command::Move(Move::Left)),
    (KeyCode::D, Command::Move(Move::Right)),
    (KeyCode::Left, Command::TurnLeft),
    (KeyCode::E, Command::TurnRight),
    (KeyCode::Right, Command::TurnRight),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Screenshot modes run without creating a window.
    match args.first().map(String::as_str) {
        Some("--screenshot") => screenshot(&args[1..], false),
        Some("--screenshot-title") => screenshot(&args[1..], true),
        _ => macroquad::Window::from_config(window_conf(), play(args)),
    }
}

async fn play(args: Vec<String>) {
    let mut d = load(&data_dir(args.first().cloned()));
    let mut game = new_game(&d);
    let mut sound = sound::Sound::start(&data_dir(args.first().cloned()));
    let demo = std::env::var_os("DM2_DEMO_CHAMPION").is_some();
    // Real-time tick length is not yet known (docs/05); configurable.
    let tick_secs = std::env::var("DM2_TICK_MS").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(133.3) / 1000.0;
    let mut acc = 0.0f64;
    let mut screen = Screen::Title;
    let mut debug = false;
    let mut rgba = vec![0u8; SCREEN_W * SCREEN_H * 4];
    let tex = Texture2D::from_rgba8(SCREEN_W as u16, SCREEN_H as u16, &rgba);
    tex.set_filter(FilterMode::Nearest);

    loop {
        let view = ui_view(&game, demo);
        // Commands from the original key and zone tables.
        let mut cmds: Vec<u16> = Vec::new();
        let mut game_cmds: Vec<Command> = Vec::new();
        if let Some(inp) = &d.input {
            let st = ui_state(screen, &view);
            for k in get_keys_pressed() {
                if let Some(code) = scan_code(k) {
                    cmds.extend(inp.key(&st, code));
                }
            }
            let (mx, my) = mouse_position();
            let (sx, sy) = (
                (mx / screen_width() * SCREEN_W as f32) as i32,
                (my / screen_height() * SCREEN_H as f32) as i32,
            );
            for (b, mask) in [(MouseButton::Left, BUTTON_LEFT), (MouseButton::Right, BUTTON_RIGHT)] {
                if is_mouse_button_pressed(b) {
                    if let Some(c) = inp.click(&d.assets.layout, &st, sx, sy, mask) {
                        if c == 0x50 && screen == Screen::Game {
                            game_cmds.extend(input::click_command(&d.assets.layout, c, sx, sy));
                        } else {
                            cmds.push(c);
                        }
                    }
                }
            }
        } else if screen == Screen::Title && is_key_pressed(KeyCode::Enter) {
            cmds.push(0xD7);
        }
        for c in cmds {
            match (screen, c) {
                (Screen::Title, 0xE0) => std::process::exit(0),
                (Screen::Title, 0xD7) => {
                    game = new_game(&d);
                    screen = Screen::Game;
                }
                (Screen::Title, 0xD9) => match resume(&d) {
                    Ok(g) => {
                        game = g;
                        screen = Screen::Game;
                    }
                    Err(e) => eprintln!("resume: {e}"),
                },
                (Screen::Game, 0x8C) => {
                    let dir = save_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    match save::save(&mut game, &save::slot_path(&dir, 0), "DM2 REMAKE") {
                        Ok(()) => eprintln!("saved to {}", save::slot_path(&dir, 0).display()),
                        Err(e) => eprintln!("save failed: {e}"),
                    }
                }
                (Screen::Game, 0x90) => screen = Screen::Paused,
                (Screen::Paused, 0x91) => screen = Screen::Game,
                (Screen::Game, c) => {
                    if let Some(gc) = input::game_command(c) {
                        game.push_command(gc);
                    }
                }
                _ => {}
            }
        }
        for c in game_cmds {
            game.push_command(c);
        }
        if screen == Screen::Game {
            for (k, c) in EXTRA_KEYS {
                if is_key_pressed(k) {
                    game.push_command(c);
                }
            }
            let n = game.dungeon.maps.len();
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
            acc += get_frame_time() as f64;
            while acc >= tick_secs {
                acc -= tick_secs;
                game.advance();
            }
            if let Some(s) = sound.as_mut() {
                s.update(&mut game);
            }
        }
        if is_key_pressed(KeyCode::Tab) {
            debug = !debug;
        }

        let mut frame = match screen {
            Screen::Title => ui::title(&mut d.assets, TITLE_FRAME),
            _ => game_frame(&mut d, &game, &view),
        };
        // The held item follows the mouse (the original's cursor, 0x7FBB4).
        let (mx, my) = mouse_position();
        let (cx, cy) = ((mx / screen_width() * SCREEN_W as f32) as i32, (my / screen_height() * SCREEN_H as f32) as i32);
        if screen == Screen::Game {
            ui::draw_cursor(&mut d.assets, &mut frame, view.held, cx, cy);
        }
        show_mouse(view.held.is_none() || screen != Screen::Game);
        for (i, &c) in frame.px.iter().enumerate() {
            let [r, g, b] = d.assets.palette[c as usize];
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
        if screen == Screen::Paused {
            draw_text("PAUSED (Esc)", 8.0, screen_height() - 12.0, 22.0, YELLOW);
        }
        if debug && screen != Screen::Title {
            let label = format!(
                "map {} ({},{}) facing {}  layer {}  tick {}",
                game.party.map,
                game.party.x,
                game.party.y,
                ["N", "E", "S", "W"][game.party.dir as usize],
                game.dungeon.maps[game.party.map].depth,
                game.tick
            );
            draw_text(&label, 8.0, screen_height() - 12.0, 22.0, YELLOW);
        }
        next_frame().await;
    }
}
