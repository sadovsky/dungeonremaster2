//! Print the screen box of each click command, read from the user's
//! SKULL.EXE zone tables: zonefind [MENU_CHOICES] CMD...
//! With MENU_CHOICES > 0 the action menu is taken as open.
use dm2_engine::assets::{self, Assets};
use dm2_engine::exe::Exe;
use dm2_engine::input::{Input, Screen, UiState, BUTTON_LEFT};

fn main() {
    let args: Vec<u16> = std::env::args()
        .skip(1)
        .filter_map(|s| u16::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .collect();
    let (menu, want) = (args.first().copied().unwrap_or(0) as usize, &args[1.min(args.len())..]);
    let dir = assets::default_data_dir();
    let a = Assets::load(&dir).unwrap();
    let exe = Exe::open(&dir.join("../SKULL.EXE")).expect("SKULL.EXE");
    let input = Input::load(&exe).expect("zone tables");
    let ui = UiState {
        screen: Screen::Game,
        champions: [true, false, false, false],
        inventory_open: None,
        leader: Some(0),
        menu_choices: menu,
        container_open: false,
    };
    for &cmd in want {
        let (mut x0, mut y0, mut x1, mut y1, mut n) = (i32::MAX, i32::MAX, -1, -1, 0);
        for y in 0..200 {
            for x in 0..320 {
                if input.click(&a.layout, &ui, x, y, BUTTON_LEFT) == Some(cmd) {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                    n += 1;
                }
            }
        }
        if n == 0 {
            println!("{cmd:#04x}: not on screen");
        } else {
            println!("{cmd:#04x}: x {x0}-{x1} y {y0}-{y1} centre ({},{})", (x0 + x1) / 2, (y0 + y1) / 2);
        }
    }
}
