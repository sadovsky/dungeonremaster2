//! The 3D dungeon view (docs/04-rendering.md). Port of tools/viewport.py:
//! walls, floor and ceiling only so far.

use dm2_formats::dungeon::Dungeon;
use dm2_formats::gdat::Key;

use crate::assets::Assets;
use crate::gfx::Bitmap;

pub const VP_W: usize = 224;
pub const VP_H: usize = 136;
/// Where the viewport sits on the 320×200 screen (layout record 7).
pub const VP_SCREEN_POS: (i32, i32) = (0, 40);

/// View cone: cell -> (lateral, forward); lateral > 0 is to the party's right.
const CELLS: [(i32, i32); 23] = [
    (0, 0), (-1, 0), (1, 0), (0, 1), (-1, 1), (1, 1), (0, 2), (-1, 2), (1, 2),
    (-2, 2), (2, 2), (0, 3), (-1, 3), (1, 3), (-2, 3), (2, 3), (0, 4), (-1, 4),
    (1, 4), (-2, 4), (2, 4), (-3, 4), (3, 4),
];
/// Back-to-front order in which wall cells are drawn.
const DRAW_ORDER: [usize; 20] = [19, 20, 17, 18, 16, 14, 15, 12, 13, 11, 9, 10, 7, 8, 6, 4, 5, 3, 1, 2];
/// Left/right partner of each near cell, used when the parity bit is set.
const SWAP: [usize; 16] = [0, 2, 1, 3, 5, 4, 6, 8, 7, 10, 9, 11, 13, 12, 15, 14];

pub const DX: [i32; 4] = [0, 1, 0, -1];
pub const DY: [i32; 4] = [-1, 0, 1, 0];

const LAYOUT_CEILING: u16 = 700;
const LAYOUT_FLOOR: u16 = 701;
const LAYOUT_WALL0: u16 = 702;

/// Wall/floor alternation bit (0x54874).
fn parity(dg: &Dungeon, map: usize, x: i32, y: i32, dir: u8) -> u8 {
    let m = &dg.maps[map];
    ((m.depth as i32 + m.origin_x as i32 + m.origin_y as i32 + x + y + dir as i32) & 1) as u8
}

/// True if a square is drawn as a wall face.
fn is_wall_face(sq: u8) -> bool {
    let e = sq >> 5;
    e == 0 || (e == 6 && sq & 4 == 0)
}

pub fn render(a: &mut Assets, map: usize, px: i32, py: i32, dir: u8) -> Bitmap {
    let mut buf = Bitmap::new(VP_W, VP_H);
    let tileset = a.dungeon.maps[map].tileset;
    let par = parity(&a.dungeon, map, px, py, dir);
    let key = a.gdat.lookup(Key::new(8, tileset, 11, 100)).unwrap_or(0) as u8;
    let flags = a.gdat.lookup(Key::new(8, tileset, 11, 0x65)).unwrap_or(0);
    // Ceiling and floor flips: only the parity-driven modes are modelled.
    let ceil_flip = if flags & 2 != 0 && flags & 4 == 0 { 1 - par } else { 0 };
    let floor_flip = if flags & 8 != 0 && flags & 0x10 == 0 { par } else { 0 };
    for (sub, rid, fl) in [(1u8, LAYOUT_CEILING, ceil_flip), (0, LAYOUT_FLOOR, floor_flip)] {
        a.draw(&mut buf, 8, tileset, sub, rid, fl, None);
    }
    let d = dir as usize;
    for &c in DRAW_ORDER.iter() {
        let (lat, fwd) = CELLS[c];
        let x = px + DX[d] * fwd + DX[(d + 1) & 3] * lat;
        let y = py + DY[d] * fwd + DY[(d + 1) & 3] * lat;
        if !is_wall_face(a.dungeon.square(map, x, y).0) {
            continue;
        }
        let mut flip = u8::from(lat > 0);
        let sub;
        if c >= 16 {
            if lat.abs() == 2 {
                flip = 0;
            }
            flip ^= par;
            sub = 50;
        } else if par == 0 {
            sub = 34 + c as u8;
        } else {
            sub = 34 + SWAP[c] as u8;
            if lat == 0 {
                flip = 1;
            }
        }
        a.draw(&mut buf, 8, tileset, sub, LAYOUT_WALL0 + c as u16, flip, Some(key));
    }
    buf
}
