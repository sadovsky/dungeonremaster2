//! Mouse zones and key bindings (docs/10-ui-input.md).
//!
//! Both tables are read at runtime from the user's SKULL.EXE data object.
//! The original picks the active lists with a byte-coded condition tree
//! (0x203A3, predicates at 0x722A4) that is not decoded yet; `UiState`
//! selects the same lists from explicit state instead.

use std::collections::BTreeMap;

use crate::exe::Exe;
use crate::layout::Layout;

const ZONES: u32 = 0x72388;
const KEYS: u32 = 0x729B8;
const KEYS_END: u32 = 0x72B9C;

/// Mouse button bits used by the zone records.
pub const BUTTON_LEFT: u8 = 0x02;
pub const BUTTON_RIGHT: u8 = 0x01;

/// Key modifier bits in the key table (added to the BIOS scan code).
pub const MOD_SHIFT: u16 = 0x200;
pub const MOD_ALT: u16 = 0x400;
pub const MOD_CTRL: u16 = 0x800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Screen,
    /// Rectangle is relative to the viewport frame (rect id 7).
    Viewport,
    /// Rectangle is relative to rect id 18.
    Rect18,
}

#[derive(Clone, Copy, Debug)]
pub struct Zone {
    pub cmd: u16,
    pub rect: u16,
    pub anchor: Anchor,
    pub buttons: u8,
    pub flags: u8,
}

/// List start indices (record numbers) of the lists the engine uses.
pub mod lists {
    pub const TITLE: u16 = 0;
    pub const PAUSED: u16 = 12;
    pub const ARROWS: u16 = 39;
    pub const VIEWPORT: u16 = 45;
    pub const INVENTORY_BUTTONS: u16 = 47;
    pub const INVENTORY_SLOTS: u16 = 53;
    pub const PORTRAIT: [u16; 4] = [15, 22, 28, 34];
    pub const PORTRAIT_CLOSE: [u16; 4] = [14, 21, 27, 33];
    pub const ACTION_HANDS: [u16; 4] = [97, 102, 107, 112];
    pub const SPELL_RUNES: u16 = 166;
    pub const SPELL_CAST: u16 = 172;
    /// Action menu with 1, 2 or 3 choices (rows at rects 63-65), cancel and
    /// champion rotation.
    pub const ACTION_MENU: [u16; 3] = [117, 121, 126];
    pub const CONTAINER: u16 = 174;

    pub const KEY_TITLE: u16 = 0;
    pub const KEY_GAME: u16 = 5;
    pub const KEY_PAUSED: u16 = 8;
    pub const KEY_MOVE: u16 = 21;
    pub const KEY_OPEN: [u16; 4] = [10, 13, 16, 19];
    pub const KEY_CLOSE: [u16; 4] = [9, 12, 15, 18];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Title,
    Game,
    Paused,
}

/// What decides which zone and key lists are live.
#[derive(Clone, Copy, Debug)]
pub struct UiState {
    pub screen: Screen,
    pub champions: [bool; 4],
    pub inventory_open: Option<usize>,
    pub leader: Option<usize>,
    /// Number of choices in the open action menu (0 = no menu).
    pub menu_choices: usize,
    /// A container is shown in the open inventory.
    pub container_open: bool,
}

impl UiState {
    fn mouse_lists(&self) -> Vec<u16> {
        use lists::*;
        match self.screen {
            Screen::Title => vec![TITLE],
            Screen::Paused => vec![PAUSED],
            Screen::Game => {
                let mut v = Vec::new();
                for (i, &present) in self.champions.iter().enumerate() {
                    if present {
                        v.push(if self.inventory_open == Some(i) { PORTRAIT_CLOSE[i] } else { PORTRAIT[i] });
                    }
                }
                if self.inventory_open.is_some() {
                    v.extend([INVENTORY_SLOTS, INVENTORY_BUTTONS]);
                    if self.container_open {
                        v.push(CONTAINER);
                    }
                }
                if self.leader.is_some() {
                    if (1..=3).contains(&self.menu_choices) {
                        v.push(ACTION_MENU[self.menu_choices - 1]);
                    } else {
                        for (i, &present) in self.champions.iter().enumerate() {
                            if present {
                                v.push(ACTION_HANDS[i]);
                            }
                        }
                    }
                    v.extend([SPELL_RUNES, SPELL_CAST]);
                }
                v.extend([ARROWS, VIEWPORT]);
                v
            }
        }
    }

    fn key_lists(&self) -> Vec<u16> {
        use lists::*;
        match self.screen {
            Screen::Title => vec![KEY_TITLE],
            Screen::Paused => vec![KEY_PAUSED],
            Screen::Game => {
                let mut v = vec![KEY_MOVE, KEY_GAME];
                for (i, &present) in self.champions.iter().enumerate() {
                    if present {
                        v.push(if self.inventory_open == Some(i) { KEY_CLOSE[i] } else { KEY_OPEN[i] });
                    }
                }
                v
            }
        }
    }
}

pub struct Input {
    zones: BTreeMap<u16, Vec<Zone>>,
    keys: BTreeMap<u16, Vec<(u16, u16)>>,
}

impl Input {
    /// Read the zone and key tables from the user's SKULL.EXE.
    pub fn load(exe: &Exe) -> Option<Input> {
        let mut zones: BTreeMap<u16, Vec<Zone>> = BTreeMap::new();
        let mut cur = None;
        let mut a = ZONES;
        let mut idx = 0u16;
        // The key table follows the zones; the last zone record is a terminator.
        while a + 12 <= KEYS {
            let (w0, w1, w2) = (exe.u16_at(a)?, exe.u16_at(a + 2)?, exe.u16_at(a + 4)?);
            if w0 & 0x8000 != 0 {
                cur = Some(idx);
                zones.insert(idx, Vec::new());
            }
            if let Some(c) = cur {
                let anchor = if w1 & 0x8000 != 0 {
                    Anchor::Viewport
                } else if w1 & 0x4000 != 0 {
                    Anchor::Rect18
                } else {
                    Anchor::Screen
                };
                zones.get_mut(&c)?.push(Zone {
                    cmd: w0 & 0x7FF,
                    rect: w1 & 0x3FFF,
                    anchor,
                    buttons: w2 as u8,
                    flags: (w2 >> 8) as u8,
                });
            }
            a += 6;
            idx += 1;
        }
        let mut keys: BTreeMap<u16, Vec<(u16, u16)>> = BTreeMap::new();
        let (mut a, mut idx, mut cur) = (KEYS, 0u16, None);
        while a + 4 <= KEYS_END {
            let (c, k) = (exe.u16_at(a)?, exe.u16_at(a + 2)?);
            if c == 0x8000 && k == 0 {
                cur = None;
            } else {
                if c & 0x8000 != 0 {
                    cur = Some(idx);
                    keys.insert(idx, Vec::new());
                }
                if let Some(l) = cur {
                    keys.get_mut(&l)?.push((c & 0x7FFF, k));
                }
            }
            a += 4;
            idx += 1;
        }
        (!zones.is_empty() && !keys.is_empty()).then_some(Input { zones, keys })
    }

    pub fn zone_list(&self, start: u16) -> &[Zone] {
        self.zones.get(&start).map_or(&[], |v| v)
    }

    /// Screen box of a zone (x, y, w, h).
    pub fn zone_box(layout: &Layout, z: &Zone) -> Option<(i32, i32, i32, i32)> {
        let (mut x, mut y, w, h) = rect_box(layout, z.rect)?;
        let origin = match z.anchor {
            Anchor::Screen => (0, 0),
            Anchor::Viewport => rect_box(layout, 7).map_or((0, 0), |b| (b.0, b.1)),
            Anchor::Rect18 => rect_box(layout, 18).map_or((0, 0), |b| (b.0, b.1)),
        };
        x += origin.0;
        y += origin.1;
        Some((x, y, w, h))
    }

    /// Command for a click at screen (x, y) with the given button (0x20B01).
    pub fn click(&self, layout: &Layout, ui: &UiState, x: i32, y: i32, button: u8) -> Option<u16> {
        for l in ui.mouse_lists() {
            for z in self.zone_list(l) {
                if z.flags & 0x08 != 0 || z.buttons & button == 0 {
                    continue;
                }
                if let Some((bx, by, bw, bh)) = Self::zone_box(layout, z) {
                    if x >= bx && y >= by && x < bx + bw && y < by + bh {
                        return Some(z.cmd);
                    }
                }
            }
        }
        None
    }

    /// Command bound to a key (BIOS scan code plus MOD_* bits).
    pub fn key(&self, ui: &UiState, code: u16) -> Option<u16> {
        for l in ui.key_lists() {
            if let Some(v) = self.keys.get(&l) {
                if let Some(&(c, _)) = v.iter().find(|&&(_, k)| k == code) {
                    return Some(c);
                }
            }
        }
        None
    }
}

/// Screen box of a layout rectangle id: a record placed inside a size box
/// (its parent) takes that box's size; a size box on its own is placed
/// through a record that uses it.
pub fn rect_box(layout: &Layout, id: u16) -> Option<(i32, i32, i32, i32)> {
    let rec = layout.get(id)?;
    let par = layout.get(rec.parent as u16)?;
    if par.kind == 9 && rec.kind != 9 {
        let p = layout.resolve(id, par.x as i32, par.y as i32, (par.x as i32, par.y as i32))?;
        return Some((p.x, p.y, p.w, p.h));
    }
    None
}

/// Map a dispatcher command number onto a game command, where one exists.
/// Viewport clicks (0x50) need the click position: see `viewport_command`.
pub fn game_command(cmd: u16) -> Option<crate::state::Command> {
    use crate::state::Command;
    use crate::world::Move;
    Some(match cmd {
        1 => Command::TurnLeft,
        2 => Command::TurnRight,
        3 => Command::Move(Move::Forward),
        4 => Command::Move(Move::Right),
        5 => Command::Move(Move::Back),
        6 => Command::Move(Move::Left),
        0x07..=0x0B | 0x14..=0x41 | 0x46 | 0x47 | 0x5F..=0x62 | 0x65..=0x6C | 0x70..=0x7B => Command::Ui(cmd),
        _ => return None,
    })
}

/// Which part of the 3D view a screen click at (x, y) hit (docs/10
/// "Viewport clicks"; regions are the layout ids 0x2F8-0x2FE).
pub fn view_region(layout: &Layout, x: i32, y: i32) -> crate::hand::ViewRegion {
    use crate::hand::ViewRegion;
    let (ox, oy) = crate::viewport::VP_SCREEN_POS;
    let (vx, vy) = (x - ox, y - oy);
    for (rid, r) in ViewRegion::LAYOUT_IDS {
        if let Some((bx, by, bw, bh)) = rect_box(layout, rid) {
            if vx >= bx && vy >= by && vx < bx + bw && vy < by + bh {
                return r;
            }
        }
    }
    ViewRegion::Elsewhere { right: vx >= crate::viewport::VP_W as i32 / 2 }
}

/// The game command for a click: viewport clicks carry their region.
pub fn click_command(layout: &Layout, cmd: u16, x: i32, y: i32) -> Option<crate::state::Command> {
    if cmd == 0x50 {
        return Some(crate::state::Command::Viewport(view_region(layout, x, y)));
    }
    game_command(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load() -> Option<(Input, Layout)> {
        let dir = crate::assets::default_data_dir();
        let exe = Exe::open(&dir.join("../SKULL.EXE"))?;
        let g = dm2_formats::gdat::Gdat::open(dir.join("GRAPHICS.DAT")).ok()?;
        Some((Input::load(&exe)?, Layout::load(&g)?))
    }

    #[test]
    fn arrows_and_keys_from_original() {
        let Some((inp, layout)) = load() else { return };
        let ui = UiState {
            screen: Screen::Game,
            champions: [false; 4],
            inventory_open: None,
            leader: None,
            menu_choices: 0,
            container_open: false,
        };
        // Arrow grid at (229,129) with 29×23 buttons: turn left, forward, turn right.
        assert_eq!(inp.click(&layout, &ui, 230, 130, BUTTON_LEFT), Some(1));
        assert_eq!(inp.click(&layout, &ui, 262, 131, BUTTON_LEFT), Some(3));
        assert_eq!(inp.click(&layout, &ui, 292, 131, BUTTON_LEFT), Some(2));
        assert_eq!(inp.click(&layout, &ui, 262, 155, BUTTON_LEFT), Some(5));
        // Viewport click.
        assert_eq!(inp.click(&layout, &ui, 100, 100, BUTTON_LEFT), Some(0x50));
        // Keypad 5 moves forward, Esc pauses.
        assert_eq!(inp.key(&ui, 0x4C), Some(3));
        assert_eq!(inp.key(&ui, 0x01), Some(0x90));
        let title = UiState { screen: Screen::Title, ..ui };
        assert_eq!(inp.key(&title, 0x1C), Some(0xD7));
    }

    #[test]
    fn inventory_and_viewport_regions() {
        use crate::hand::ViewRegion;
        let Some((inp, layout)) = load() else { return };
        let ui = UiState {
            screen: Screen::Game,
            champions: [true, false, false, false],
            inventory_open: Some(0),
            leader: Some(0),
            menu_choices: 0,
            container_open: false,
        };
        // Inventory slot 0 (rect 507) lies at viewport (6,56), screen (6,96).
        assert_eq!(inp.click(&layout, &ui, 10, 100, BUTTON_LEFT), Some(0x1C));
        // Near floor cells of the view: rects 0x2F8/0x2F9.
        assert_eq!(view_region(&layout, 30, 40 + 120), ViewRegion::NearLeft);
        assert_eq!(view_region(&layout, 150, 40 + 120), ViewRegion::NearRight);
        assert_eq!(view_region(&layout, 2, 42), ViewRegion::Elsewhere { right: false });
        // With a two-choice menu open the menu rows replace the hand icons.
        let menu = UiState { inventory_open: None, menu_choices: 2, ..ui };
        assert_eq!(inp.click(&layout, &menu, 240, 55, BUTTON_LEFT), Some(0x71));
    }
}
