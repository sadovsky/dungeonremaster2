//! The 3D dungeon view (docs/04-rendering.md, section 3).
//!
//! Every image is loaded at runtime from the user's own GRAPHICS.DAT; this
//! module only holds the traversal and placement rules.

pub mod light;

use std::collections::HashMap;

use dm2_formats::dungeon::{Dungeon, Element, ThingRef};
use dm2_formats::gdat::Key;

use crate::assets::Assets;
use crate::gfx::{Bitmap, Sprite};

pub const VP_W: usize = 224;
pub const VP_H: usize = 136;
/// Where the viewport sits on the 320×200 screen (layout record 7).
pub const VP_SCREEN_POS: (i32, i32) = (0, 40);

pub const DX: [i32; 4] = [0, 1, 0, -1];
pub const DY: [i32; 4] = [-1, 0, 1, 0];

/// View cone: cell -> (lateral, forward); lateral > 0 is to the party's right.
const CELLS: [(i32, i32); 23] = [
    (0, 0), (-1, 0), (1, 0), (0, 1), (-1, 1), (1, 1), (0, 2), (-1, 2), (1, 2),
    (-2, 2), (2, 2), (0, 3), (-1, 3), (1, 3), (-2, 3), (2, 3), (0, 4), (-1, 4),
    (1, 4), (-2, 4), (2, 4), (-3, 4), (3, 4),
];
/// Back-to-front cell order (0x76025).
const DRAW_ORDER: [usize; 20] = [19, 20, 17, 18, 16, 14, 15, 12, 13, 11, 9, 10, 7, 8, 6, 4, 5, 3, 1, 2];
/// Left/right partner of each near cell (0x75B28).
const SWAP: [usize; 16] = [0, 2, 1, 3, 5, 4, 6, 8, 7, 10, 9, 11, 13, 12, 15, 14];
/// View depth per cell (0x75B11).
const DEPTH: [usize; 23] = [0, 0, 0, 1, 1, 1, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4];
/// Which wall faces a cell shows: 1 front, 2 side, 3 both (0x7600E).
const FACES: [u8; 23] = [0, 2, 2, 1, 3, 3, 1, 3, 3, 2, 2, 1, 3, 3, 3, 3, 1, 1, 1, 1, 1, 0, 0];
/// Scale per depth in 64ths (0x75B6D).
const DEPTH_SCALE: [i32; 5] = [96, 64, 43, 28, 19];
/// Does a cell get a contents pass (0x75DCF)?
const HAS_CONTENTS: [bool; 16] = [true, true, true, true, true, true, true, true, true, false, false, true, true, true, true, true];
/// Sub-square visiting orders for left, right and centre cells (0x75D84/9D/B6).
const ORDER_LEFT: [u8; 25] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24];
const ORDER_RIGHT: [u8; 25] = [4, 3, 2, 1, 0, 9, 8, 7, 6, 5, 14, 13, 12, 11, 10, 19, 18, 17, 16, 15, 24, 23, 22, 21, 20];
const ORDER_CENTRE: [u8; 25] = [0, 4, 1, 3, 2, 5, 9, 6, 8, 7, 10, 14, 11, 13, 12, 15, 19, 16, 18, 17, 20, 24, 21, 23, 22];

const LAYOUT_CEILING: u16 = 700;
const LAYOUT_FLOOR: u16 = 701;
const LAYOUT_WALL0: u16 = 702;

// Pits (0x75CAC, alternate 0x75C9C, placements 0x75C6C, mirroring 0x75C8C).
const PIT_SUB: [i16; 16] = [107, 108, 108, 110, 111, 111, 113, 114, 114, -1, -1, 118, 119, 119, 121, 121];
const PIT_SUB_ALT: [i16; 16] = [130, 131, 131, 133, 134, 134, 136, 137, 137, -1, -1, 118, 119, 119, 121, 121];
const PIT_LAYOUT: [i16; 16] = [862, 861, 863, 859, 858, 860, 856, 855, 857, -1, -1, 853, 852, 854, 850, 851];
const PIT_FLIP: [u8; 16] = [0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1];
// Ceiling holes (0x507AB).
const HOLE_SUB: [u8; 9] = [153, 154, 154, 156, 157, 157, 159, 160, 160];
const HOLE_LAYOUT: [u16; 9] = [871, 870, 872, 868, 867, 869, 865, 864, 866];
const HOLE_FLIP: [u8; 9] = [0, 0, 1, 0, 0, 1, 0, 0, 1];
// Stairs seen front-on (0x75F58, partner 0x75F78, placements 0x75F98),
// indexed by cell·2 + square bit 2.
const STAIR_FRONT_SUB: [i16; 32] = [
    -1, -1, -1, -1, -1, -1, 79, 59, 80, 60, 81, 61, 82, 62, 83, 63, 84, 64, -1, -1, -1, -1, 85, 65, 86, 66, 87, 67, 88, 68, 89, 69,
];
const STAIR_FRONT_ALT: [i16; 32] = [
    -1, -1, -1, -1, -1, -1, 79, 59, 80, 60, 80, 60, 82, 62, 83, 63, 83, 63, -1, -1, -1, -1, 85, 65, 86, 66, 86, 66, 88, 68, 88, 68,
];
const STAIR_FRONT_LAYOUT: [i16; 32] = [
    -1, -1, -1, -1, -1, -1, 822, 809, 821, 808, 823, 810, 819, 806, 818, 805, 820, 807, -1, -1, -1, -1, 816, 803, 815, 802, 817, 804,
    800, 800, 801, 801,
];
// Stairs seen side-on (0x53BFB).
const STAIR_SIDE_SUB: [i16; 18] = [-1, -1, 205, 199, 206, 200, -1, -1, 207, 201, 208, 202, -1, -1, 209, 203, 210, 204];
const STAIR_SIDE_LAYOUT: [i16; 18] = [-1, -1, 832, 832, 833, 833, -1, -1, 830, 828, 831, 829, -1, -1, 826, 826, 827, -1];
// Doors (placements 0x75F28, cells 0x75F48).
const DOOR_LAYOUT: [i16; 16] = [3810, -1, -1, 3790, 3780, 3800, 3760, 3750, 3770, -1, -1, 3730, 3720, 3740, 3700, 3710];
/// Floor items: perspective scale by depth·4 + (4 − row) (0x75B73).
const ITEM_SCALE: [i32; 18] = [87, 78, 71, 64, 58, 52, 47, 43, 39, 35, 31, 28, 26, 23, 21, 19, 17, 15];
/// Item quadrant (relative to facing) -> 5×5 slot (0x7168A).
const QUAD_SLOT: [u8; 4] = [6, 8, 18, 16];
/// Missiles: scale by depth·2 − slot row/2 (0x75B8D).
const MISSILE_SCALE: [i32; 7] = [64, 52, 43, 35, 28, 23, 19];
/// Floor ornaments: sub per cell (0x75C1A) and scaled fallback (0x75C31).
const FLOOR_ORN_SUB: [u8; 16] = [0, 1, 1, 2, 3, 3, 4, 5, 5, 6, 6, 7, 8, 8, 8, 8];
const FLOOR_ORN_FALLBACK: [u8; 16] = [2, 3, 3, 2, 3, 3, 2, 3, 3, 3, 3, 2, 3, 3, 3, 3];
/// Side-wall ornament base placement per cell (0x7179C).
const SIDE_ORN_BASE: [i16; 16] = [-1, 4425, 4450, -1, 4500, 4525, -1, 4575, 4600, 4625, 4650, -1, 4700, 4725, 4750, 4775];
/// Teleporter field per cell: phase, mask sub (0x7F none; bit 7 mirror),
/// width, height (0x75CDC).
const TELEPORTER: [(u8, u8, u16, u16); 16] = [
    (59, 255, 224, 136), (63, 5, 33, 136), (63, 133, 33, 136), (61, 255, 160, 111), (63, 4, 60, 111), (63, 132, 60, 111),
    (60, 255, 106, 74), (63, 3, 78, 74), (63, 131, 78, 74), (63, 2, 8, 52), (63, 130, 8, 52), (63, 255, 70, 49),
    (63, 1, 83, 49), (63, 129, 83, 49), (63, 0, 36, 49), (63, 128, 36, 49),
];

/// Feature switches, mainly for comparing against tools/viewport.py.
pub mod layers {
    pub const ORNAMENTS: u32 = 1;
    pub const ITEMS: u32 = 2;
    pub const CREATURES: u32 = 4;
    pub const MISSILES: u32 = 8;
    pub const TELEPORTERS: u32 = 16;
    pub const DOORS: u32 = 32;
    pub const PITS_STAIRS: u32 = 64;
    pub const ALL: u32 = 0x7F;
}

/// Inputs to the view that are not part of the dungeon itself.
#[derive(Clone, Debug)]
pub struct ViewExtras {
    /// Game tick, for animated ornaments.
    pub tick: u32,
    /// Apply depth lighting (docs/04 section 6).
    pub lighting: bool,
    /// Ambient darkness in 64ths (0 = full light).
    pub ambient: i32,
    /// Seed for visual-only randomness (teleporter shimmer). Never the game Rng.
    pub visual_seed: u32,
    /// Which feature layers to draw (`layers::*`).
    pub layers: u32,
    /// Creature drawing-descriptor index per creature thing reference
    /// (raw `ThingRef` value); default 0 (first descriptor).
    pub creature_frames: HashMap<u16, u16>,
    /// Missile flight direction per missile thing reference; default: towards the party.
    pub missile_dirs: HashMap<u16, u8>,
}

impl Default for ViewExtras {
    fn default() -> Self {
        ViewExtras {
            tick: 0,
            lighting: true,
            ambient: 0,
            visual_seed: 0,
            layers: layers::ALL,
            creature_frames: HashMap::new(),
            missile_dirs: HashMap::new(),
        }
    }
}

/// View types from the cell summary (0x1E908).
mod vt {
    pub const WALL: u8 = 0;
    pub const FLOOR: u8 = 1;
    pub const PIT: u8 = 2;
    pub const TELEPORTER: u8 = 5;
    pub const ROCK: u8 = 7;
    pub const DOOR_EDGE: u8 = 0x10;
    pub const DOOR_ACROSS: u8 = 0x11;
    pub const STAIRS_SIDE: u8 = 0x12;
    pub const STAIRS_FRONT: u8 = 0x13;
}

const NO_ORN: u8 = 0xFF;

#[derive(Clone, Debug)]
struct Cell {
    x: i32,
    y: i32,
    vt: u8,
    sq: u8,
    /// Wall ornament per face, indexed by direction relative to the party
    /// facing (2 = the face looking back at the party). 0 = wall writing.
    faces: [u8; 4],
    /// Text thing shown as wall writing on the face towards the party.
    wall_text: Option<ThingRef>,
    floor_orn: u8,
    door: Option<ThingRef>,
    things: Vec<ThingRef>,
}

struct Ctx<'a> {
    dg: &'a Dungeon,
    ex: &'a ViewExtras,
    map: usize,
    set: u8,
    dir: u8,
    par: u8,
    set_flags: u16,
    rng: u32,
}

impl Ctx<'_> {
    fn on(&self, layer: u32) -> bool {
        self.ex.layers & layer != 0
    }

    /// Visual-only random byte (xorshift), independent of the game Rng.
    fn rand(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }
}

/// One draw request (the generic helper at 0x4E502).
struct Req {
    cat: u8,
    idx: u8,
    sub: u8,
    rid: u16,
    flip: u8,
    xs: i32,
    ys: i32,
    xoff: i32,
    yoff: i32,
    /// Light depth (None: no lighting).
    depth: Option<usize>,
    key: Option<u8>,
}

impl Req {
    fn new(cat: u8, idx: u8, sub: u8, rid: u16) -> Req {
        Req { cat, idx, sub, rid, flip: 0, xs: 64, ys: 64, xoff: 0, yoff: 0, depth: None, key: None }
    }
}

fn attr(a: &Assets, cat: u8, idx: u8, n: u8) -> u16 {
    a.gdat.lookup(Key::new(cat, idx, 11, n)).unwrap_or(0)
}

/// Colour key from attribute 4 of an image's category, or `default` when
/// the attribute is absent (or zero, where the original treats 0 as unset).
/// The draw request keeps the key as a 16-bit value, so values above 255
/// (0x8000 occurs) never match a pixel: no key.
fn key_attr(a: &Assets, cat: u8, idx: u8, default: Option<u8>, zero_is_unset: bool) -> Option<u8> {
    match a.gdat.lookup(Key::new(cat, idx, 11, 4)) {
        None => default,
        Some(0) if zero_is_unset => default,
        Some(k) if k > 0xFF => None,
        Some(k) => Some(k as u8),
    }
}

fn scale_v(v: i32, s: i32) -> i32 {
    (v * s + s / 2) >> 6
}

/// Light colour map for a depth, or None.
fn light_map(a: &mut Assets, cx: &Ctx, depth: Option<usize>, key: Option<u8>) -> Option<[u8; 256]> {
    let depth = depth?;
    if !cx.ex.lighting {
        return None;
    }
    let remap = if (1..=4).contains(&depth) {
        a.gdat.get(Key::new(8, cx.set, 7, depth as u8)).map(|t| t.to_vec())
    } else {
        None
    };
    a.light.as_mut()?.for_depth(depth, cx.ex.ambient, key, remap.as_deref())
}

/// Place and draw one sprite with offsets, flip and light (0x1B54A/0x1B8E5).
fn draw_sprite(a: &mut Assets, buf: &mut Bitmap, cx: &Ctx, s: &Sprite, base_off: (i32, i32), r: &Req, xs: i32, ys: i32) -> bool {
    let mut ox = scale_v(base_off.0 + r.xoff, xs);
    let oy = scale_v(base_off.1 + r.yoff, ys);
    if r.flip & 1 != 0 {
        ox = -ox;
    }
    let img = (s.w as i32, s.h as i32);
    let p = if (ox, oy) != (0, 0) {
        a.layout.resolve(r.rid | 0x8000, ox, oy, img)
    } else {
        a.layout.resolve(r.rid, img.0, img.1, img)
    };
    let Some(p) = p else { return false };
    let lm = light_map(a, cx, r.depth, r.key);
    s.blit_mapped(buf, &p, r.flip, r.key, lm.as_ref());
    true
}

fn draw(a: &mut Assets, buf: &mut Bitmap, cx: &Ctx, r: Req) -> bool {
    let mut xs = r.xs;
    // Per-image aspect override at depths 2 and 3 when x and y scales differ.
    if r.xs != r.ys {
        if let Some(n) = match r.depth {
            Some(2) => Some(0x14),
            Some(3) => Some(0x15),
            _ => None,
        } {
            let v = attr(a, r.cat, r.idx, n) as i32;
            if v != 0 && v & 0xFF != 0 {
                xs = (((v >> 8) << 7) / (v & 0xFF) + 1) >> 1;
            }
        }
    }
    let Some(orig) = a.sprite(r.cat, r.idx, r.sub) else { return false };
    let Some(s) = a.sprite_scaled(r.cat, r.idx, r.sub, xs, r.ys) else { return false };
    draw_sprite(a, buf, cx, &s, orig.off, &r, xs, r.ys)
}

/// Wall/floor alternation bit (0x54874).
fn parity(dg: &Dungeon, map: usize, x: i32, y: i32, dir: u8) -> u8 {
    let m = &dg.maps[map];
    ((m.depth as i32 + m.origin_x as i32 + m.origin_y as i32 + x + y + dir as i32) & 1) as u8
}

fn word(dg: &Dungeon, t: ThingRef, n: usize) -> u16 {
    dg.record_word(t, n).unwrap_or(0)
}

/// Is the square directly above (one layer up) an open pit?
fn hole_above(dg: &Dungeon, map: usize, x: i32, y: i32) -> bool {
    let m = &dg.maps[map];
    let (gx, gy) = (x + m.origin_x as i32, y + m.origin_y as i32);
    dg.maps.iter().enumerate().any(|(j, o)| {
        o.depth + 1 == m.depth
            && (0..o.width as i32).contains(&(gx - o.origin_x as i32))
            && (0..o.height as i32).contains(&(gy - o.origin_y as i32))
            && {
                let sq = dg.square(j, gx - o.origin_x as i32, gy - o.origin_y as i32);
                sq.element() == Element::Pit && sq.0 & 8 != 0
            }
    })
}

/// Build a cell summary (0x1E908, 0x1E273, 0x1E4EE).
fn summarise(cx: &Ctx, x: i32, y: i32) -> Cell {
    let dg = cx.dg;
    let sq = dg.square(cx.map, x, y).0;
    let things = dg.things_at(cx.map, x, y);
    let mut c = Cell { x, y, vt: vt::ROCK, sq, faces: [NO_ORN; 4], wall_text: None, floor_orn: NO_ORN, door: None, things };
    let lists = dg.map_lists(cx.map);
    match sq >> 5 {
        0 => c.vt = vt::WALL,
        1 => c.vt = vt::FLOOR,
        2 => c.vt = if sq & 8 != 0 { vt::PIT } else { vt::FLOOR },
        3 => c.vt = if (sq >> 3) & 1 != cx.dir & 1 { vt::STAIRS_FRONT } else { vt::STAIRS_SIDE },
        4 => {
            c.door = c.things.iter().copied().find(|t| t.kind() as u16 == 0);
            c.vt = if (sq >> 3) & 1 == cx.dir & 1 { vt::DOOR_EDGE } else { vt::DOOR_ACROSS };
        }
        5 => c.vt = if sq & 8 != 0 && sq & 4 != 0 { vt::TELEPORTER } else { vt::FLOOR },
        6 => c.vt = if sq & 4 != 0 { vt::FLOOR } else { vt::WALL },
        _ => {}
    }
    if c.vt == vt::WALL {
        // Things on the wall: text (type 2) and actuators (type 3).
        for &t in &c.things {
            let kind = t.kind() as u16;
            let rel = (t.cell().wrapping_sub(cx.dir) & 3) as usize;
            if c.faces[rel] != NO_ORN {
                continue;
            }
            if kind == 2 {
                let w1 = word(dg, t, 1);
                match (w1 & 7) >> 1 {
                    0 => {
                        c.faces[rel] = 0;
                        if rel == 2 && w1 & 1 != 0 {
                            c.wall_text = Some(t);
                        }
                    }
                    1 => c.faces[rel] = (w1 >> 3) as u8,
                    _ => {}
                }
            } else if kind == 3 {
                let nib = (word(dg, t, 2) >> 12) as usize;
                if nib != 0 {
                    if let Some(&o) = lists.wall_ornaments.get(nib - 1) {
                        c.faces[rel] = o;
                    }
                }
            }
        }
        // Faces next to a door square never show ornaments.
        return c;
    }
    // The map set's default floor ornament (attribute 0x6B) is applied by
    // render_ex; things on the square override it here.
    if matches!(c.vt, vt::FLOOR | vt::PIT | vt::TELEPORTER) {
        for &t in &c.things {
            match t.kind() as u16 {
                2 => {
                    let w1 = word(dg, t, 1);
                    if w1 & 6 == 2 {
                        c.floor_orn = (w1 >> 3) as u8;
                    }
                }
                3 => {
                    let nib = (word(dg, t, 2) >> 12) as usize;
                    if nib != 0 {
                        if let Some(&o) = lists.floor_ornaments.get(nib - 1) {
                            c.floor_orn = o;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    c
}

pub fn render(a: &mut Assets, dg: &Dungeon, map: usize, px: i32, py: i32, dir: u8) -> Bitmap {
    render_ex(a, dg, map, px, py, dir, &ViewExtras::default())
}

pub fn render_ex(a: &mut Assets, dg: &Dungeon, map: usize, px: i32, py: i32, dir: u8, ex: &ViewExtras) -> Bitmap {
    let mut buf = Bitmap::new(VP_W, VP_H);
    let set = dg.maps[map].tileset;
    let par = parity(dg, map, px, py, dir);
    let set_flags = a.gdat.lookup(Key::new(8, set, 11, 0x65)).unwrap_or(0);
    let mut cx = Ctx {
        dg,
        ex,
        map,
        set,
        dir,
        par,
        set_flags,
        rng: ex.visual_seed.wrapping_mul(0x9E37_79B9) ^ ex.tick.wrapping_add(1).wrapping_mul(0x85EB_CA6B) | 1,
    };
    // Ceiling and floor (0x4E32A); only the parity-driven flips are modelled.
    let ceil_flip = if set_flags & 2 != 0 && set_flags & 4 == 0 { 1 - par } else { 0 };
    let floor_flip = if set_flags & 8 != 0 && set_flags & 0x10 == 0 { par } else { 0 };
    for (sub, rid, fl) in [(1u8, LAYOUT_CEILING, ceil_flip), (0, LAYOUT_FLOOR, floor_flip)] {
        a.draw(&mut buf, 8, set, sub, rid, fl, None);
    }
    let floor_orn_default = a.gdat.lookup(Key::new(8, set, 11, 0x6B)).unwrap_or(0);
    let d = dir as usize;
    let mut cells: Vec<Cell> = (0..23)
        .map(|c| {
            let (lat, fwd) = CELLS[c];
            let x = px + DX[d] * fwd + DX[(d + 1) & 3] * lat;
            let y = py + DY[d] * fwd + DY[(d + 1) & 3] * lat;
            summarise(&cx, x, y)
        })
        .collect();
    // Default floor ornament of the map set (attribute 0x6B), unless a thing overrides it.
    if floor_orn_default != 0 {
        for c in cells.iter_mut().filter(|c| c.vt == vt::FLOOR && c.floor_orn == NO_ORN) {
            c.floor_orn = floor_orn_default as u8;
        }
    }
    for &c in DRAW_ORDER.iter() {
        draw_cell(a, &mut buf, &mut cx, &cells[c], c);
    }
    draw_party_cell(a, &mut buf, &mut cx, &cells[0]);
    buf
}

fn draw_cell(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell, c: usize) {
    match cell.vt {
        vt::WALL => {
            // Only creatures standing in the wall cell; items on walls appear
            // through alcove ornaments (attribute 99, not yet composed).
            draw_contents(a, buf, cx, cell, c, |_| true, true);
            draw_wall(a, buf, cx, cell, c);
        }
        vt::ROCK => {}
        _ if c >= 16 => {
            if cell.vt != vt::DOOR_EDGE {
                draw_floor_ornament(a, buf, cx, cell, c);
            }
            draw_contents(a, buf, cx, cell, c, |_| true, true);
        }
        vt::FLOOR | vt::PIT | vt::TELEPORTER => {
            draw_floor_ornament(a, buf, cx, cell, c);
            draw_ceiling_hole(a, buf, cx, cell, c);
            if cell.vt == vt::PIT {
                draw_pit(a, buf, cx, cell, c);
            }
            draw_contents(a, buf, cx, cell, c, |_| true, false);
            if cell.vt == vt::TELEPORTER {
                draw_teleporter(a, buf, cx, c);
            }
        }
        vt::DOOR_EDGE => {
            if c == 3 {
                draw_lintel(a, buf, cx, cell);
            }
            draw_contents(a, buf, cx, cell, c, |_| true, false);
        }
        vt::DOOR_ACROSS => {
            // Far half, the panel, then the near half (0x539CB).
            draw_contents(a, buf, cx, cell, c, |s| s < 10, false);
            draw_door(a, buf, cx, cell, c);
            draw_contents(a, buf, cx, cell, c, |s| s >= 10, false);
        }
        vt::STAIRS_SIDE | vt::STAIRS_FRONT => {
            draw_stairs(a, buf, cx, cell, c);
            draw_contents(a, buf, cx, cell, c, |_| true, false);
        }
        _ => {}
    }
}

fn draw_wall(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell, c: usize) {
    let (lat, _) = CELLS[c];
    let key = a.gdat.lookup(Key::new(8, cx.set, 11, 100)).unwrap_or(0) as u8;
    let mut flip = u8::from(lat > 0);
    let sub;
    if c >= 16 {
        if lat.abs() == 2 {
            flip = 0;
        }
        flip ^= cx.par;
        sub = 50;
    } else if cx.par == 0 {
        sub = 34 + c as u8;
    } else {
        // No alternate (176+) art exists in this archive; use the partner.
        sub = 34 + SWAP[c] as u8;
        if lat == 0 {
            flip = 1;
        }
    }
    a.draw(buf, 8, cx.set, sub, LAYOUT_WALL0 + c as u16, flip, Some(key));
    if !cx.on(layers::ORNAMENTS) || c >= 16 {
        return;
    }
    // Ornaments on the visible faces (0x53E41 -> 0x4F3DF).
    let faces = FACES[c];
    if faces & 1 != 0 {
        draw_wall_ornament(a, buf, cx, cell, c, 0);
    }
    if faces & 2 != 0 {
        draw_wall_ornament(a, buf, cx, cell, c, lat);
    }
}

/// One wall ornament (0x4F3DF). `side` is 0 for the face towards the party,
/// negative for a face seen on the left, positive on the right.
fn draw_wall_ornament(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell, c: usize, side: i32) {
    // Face relative to the party: 2 faces back at us; a cell on the left
    // shows its right-hand face (relative direction 1), on the right 3.
    let rel = match side.signum() {
        -1 => 1,
        0 => 2,
        _ => 3,
    };
    let orn = cell.faces[rel];
    if orn == NO_ORN {
        return;
    }
    let depth = DEPTH[c];
    if orn == 0 {
        // Wall writing: the writing panel of the map set. TODO: glyphs from
        // the 8×8 wall font (8, set, 3) laid out per docs/04 section 7.
        if side == 0 && cell.wall_text.is_some() {
            let mut r = Req::new(8, cx.set, 0xFC, 3100 + 25 * c as u16 + 12);
            r.xs = DEPTH_SCALE[depth];
            r.ys = DEPTH_SCALE[depth];
            r.depth = Some(depth);
            draw(a, buf, cx, r);
        }
        return;
    }
    let key = key_attr(a, 9, orn, None, true);
    let (slot, _anchor) = match attr(a, 9, orn, 5) {
        0 => (12u16, 0u16),
        v => ((v & 0xFF).saturating_sub(1), v >> 8),
    };
    let rid = if side == 0 {
        3100 + 25 * c as u16 + slot
    } else {
        match SIDE_ORN_BASE.get(c) {
            Some(&b) if b > 0 => b as u16 + slot,
            _ => return,
        }
    };
    let ds = DEPTH_SCALE[depth];
    let mut xs = ds;
    if side.abs() > 1 {
        match depth {
            2 => xs = 114,
            3 => xs = 76,
            _ => {}
        }
    }
    let (sub, flip) = if side == 0 {
        (1u8, 0u8)
    } else if side > 0 {
        if a.has_image(9, orn, 2) { (2, 0) } else { (0, 1) }
    } else {
        (0, 0)
    };
    let mut r = Req::new(9, orn, sub, rid);
    r.flip = flip;
    r.xs = xs;
    r.ys = ds;
    r.depth = Some(depth);
    r.key = key;
    draw(a, buf, cx, r);
}

/// Floor ornament (0x50081).
fn draw_floor_ornament(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell, c: usize) {
    let orn = cell.floor_orn;
    if !cx.on(layers::ORNAMENTS) || orn == NO_ORN || orn == 0 || c >= 16 {
        return;
    }
    if (c == 14 || c == 15) && attr(a, 10, orn, 0x61) != 0 {
        return;
    }
    let depth = DEPTH[c];
    let side = CELLS[c].0;
    let flip = if side == 0 {
        if attr(a, 10, orn, 7) == 0 { cx.par ^ u8::from(depth & 1 == 0) } else { 0 }
    } else {
        u8::from(side > 0)
    };
    let key = key_attr(a, 10, orn, None, true);
    let slot = match attr(a, 10, orn, 5) {
        0 => 12u16,
        v => (v & 0xFF).saturating_sub(1),
    };
    let rid = 5000 + 25 * c as u16 + slot;
    let (sub, sc) = if a.has_image(10, orn, FLOOR_ORN_SUB[c]) {
        (FLOOR_ORN_SUB[c], 64)
    } else {
        (FLOOR_ORN_FALLBACK[c], DEPTH_SCALE[depth])
    };
    let mut r = Req::new(10, orn, sub, rid);
    r.flip = flip;
    r.xs = sc;
    r.ys = sc;
    r.depth = Some(depth);
    r.key = key;
    draw(a, buf, cx, r);
}

/// Ceiling hole under an open pit on the layer above (0x507AB).
fn draw_ceiling_hole(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell, c: usize) {
    if !cx.on(layers::PITS_STAIRS) || c >= 9 || cx.set_flags & 1 == 0 || !hole_above(cx.dg, cx.map, cell.x, cell.y) {
        return;
    }
    let flip = if c == 0 { cx.par } else { HOLE_FLIP[c] };
    let r = Req { flip, ..Req::new(8, cx.set, HOLE_SUB[c], HOLE_LAYOUT[c]) };
    draw(a, buf, cx, r);
}

/// Open pit (0x508FF).
fn draw_pit(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell, c: usize) {
    if !cx.on(layers::PITS_STAIRS) || c >= 16 || PIT_LAYOUT[c] < 0 {
        return;
    }
    let alt = cell.sq & 4 != 0;
    if c >= 11 && alt {
        return;
    }
    let sub = if alt { PIT_SUB_ALT[c] } else { PIT_SUB[c] };
    let flip = if c == 0 { cx.par } else { PIT_FLIP[c] };
    let r = Req { flip, ..Req::new(8, cx.set, sub as u8, PIT_LAYOUT[c] as u16) };
    draw(a, buf, cx, r);
}

/// Stairs, front-on (0x53B1B) or side-on (0x53BFB).
fn draw_stairs(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell, c: usize) {
    if !cx.on(layers::PITS_STAIRS) {
        return;
    }
    let k = c * 2 + ((cell.sq >> 2) & 1) as usize;
    if cell.vt == vt::STAIRS_FRONT {
        if k >= 32 || STAIR_FRONT_SUB[k] < 0 {
            return;
        }
        let (sub, flip) = if a.has_image(8, cx.set, STAIR_FRONT_SUB[k] as u8) {
            (STAIR_FRONT_SUB[k], 0)
        } else {
            (STAIR_FRONT_ALT[k], 1)
        };
        let r = Req { flip, ..Req::new(8, cx.set, sub as u8, STAIR_FRONT_LAYOUT[k] as u16) };
        draw(a, buf, cx, r);
    } else if k < 18 && STAIR_SIDE_SUB[k] >= 0 && STAIR_SIDE_LAYOUT[k] >= 0 {
        draw(a, buf, cx, Req::new(8, cx.set, STAIR_SIDE_SUB[k] as u8, STAIR_SIDE_LAYOUT[k] as u16));
    }
}

/// Door lintel seen edge-on in the cell ahead.
fn draw_lintel(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell) {
    if !cx.on(layers::DOORS) {
        return;
    }
    if let Some(t) = cell.door {
        if attr(a, 14, door_type(cx, t), 0x40) != 0 {
            return;
        }
    }
    draw(a, buf, cx, Req::new(8, cx.set, 6, 5000 + 25 * 3 + 2));
}

/// Door type number of a door thing (word 1 bit 0 picks descriptor type 0 or 1).
fn door_type(cx: &Ctx, t: ThingRef) -> u8 {
    let w1 = word(cx.dg, t, 1);
    let we = cx.dg.maps[cx.map].raw_words[2];
    ((we >> if w1 & 1 != 0 { 12 } else { 8 }) & 15) as u8
}

/// Door panel across the view (0x5346E).
fn draw_door(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell, c: usize) {
    if !cx.on(layers::DOORS) || c >= 16 || DOOR_LAYOUT[c] < 0 {
        return;
    }
    let state = cell.sq & 7;
    let Some(t) = cell.door else { return };
    if state == 0 {
        return;
    }
    let depth = DEPTH[c];
    let dt = door_type(cx, t);
    let w1 = word(cx.dg, t, 1);
    let key = key_attr(a, 14, dt, Some(10), true);
    let (sub, sc) = if depth > 0 && a.has_image(14, dt, depth as u8 - 1) {
        (depth as u8 - 1, 64)
    } else {
        (0, if depth == 0 { 113 } else { DEPTH_SCALE[depth] })
    };
    let Some(panel) = a.sprite(14, dt, sub) else { return };
    // Compose the door ornament and the damage overlay onto a copy of the
    // panel; composition yields an 8-bit image with its own key.
    let mut img = Sprite { w: panel.w, h: panel.h, px: panel.px.clone(), cmap: panel.cmap, off: panel.off };
    let mut img_key = key;
    let orn_bits = ((w1 >> 1) & 15) as usize;
    if orn_bits != 0 {
        if let Some(&orn) = cx.dg.map_lists(cx.map).door_ornaments.get(orn_bits - 1) {
            let osub = if depth > 0 && a.has_image(11, orn, depth as u8 - 1) { depth as u8 - 1 } else { 0 };
            let okey = key_attr(a, 11, orn, Some(9), true);
            let rid = 2000 + 4 * attr(a, 11, orn, 8) + [3u16, 2, 1, 0, 0][depth];
            if let Some((s, k)) = compose(a, &img, img_key, 11, orn, osub, rid, okey) {
                (img, img_key) = (s, k);
            }
        }
    }
    if state == 5 {
        let rid = attr(a, 14, dt, 10);
        if rid != 0 {
            if let Some((s, k)) = compose(a, &img, img_key, 14, dt, 0x41, rid, key) {
                (img, img_key) = (s, k);
            }
        }
    }
    let Some(img) = (if sc == 64 { Some(img) } else { img.scaled(sc, sc) }) else { return };
    let base = DOOR_LAYOUT[c] as u16;
    let r = |rid: u16| Req { depth: Some(depth), key: img_key, ..Req::new(14, dt, sub, rid) };
    if state >= 4 {
        draw_sprite(a, buf, cx, &img, panel.off, &r(base), sc, sc);
    } else if w1 & 0x20 != 0 {
        draw_sprite(a, buf, cx, &img, panel.off, &r(base + state as u16), sc, sc);
    } else {
        // Split door: two half panels sliding apart.
        let half = img.w / 2;
        if half == 0 {
            return;
        }
        let cut = |from: usize, w: usize| Sprite {
            w,
            h: img.h,
            px: (0..img.h).flat_map(|y| img.px[y * img.w + from..y * img.w + from + w].to_vec()).collect(),
            cmap: img.cmap,
            off: (0, 0),
        };
        let left = cut(0, half);
        let right = cut(img.w - half, half);
        draw_sprite(a, buf, cx, &left, (0, 0), &r(base + state as u16 + 3), 64, 64);
        draw_sprite(a, buf, cx, &right, (0, 0), &r(base + state as u16 + 6), 64, 64);
    }
}

/// Copy `base` and draw image (cat, idx, 1, sub) onto it at layout `rid`,
/// resolved in the panel's own frame. Returns an 8-bit sprite plus the
/// palette index now marking its transparent pixels.
#[allow(clippy::too_many_arguments)]
fn compose(
    a: &mut Assets,
    base: &Sprite,
    base_key: Option<u8>,
    cat: u8,
    idx: u8,
    sub: u8,
    rid: u16,
    key: Option<u8>,
) -> Option<(Sprite, Option<u8>)> {
    let over = a.sprite(cat, idx, sub)?;
    // Flatten the base to palette indices, remembering transparency.
    let transparent: Vec<bool> = base.px.iter().map(|&v| Some(v) == base_key).collect();
    let mut bm = Bitmap::new(base.w, base.h);
    for (i, &v) in base.px.iter().enumerate() {
        bm.px[i] = match &base.cmap {
            Some(m) => m[(v & 15) as usize],
            None => v,
        };
    }
    let img = (over.w as i32, over.h as i32);
    let p = if over.off != (0, 0) {
        a.layout.resolve(rid | 0x8000, over.off.0, over.off.1, img)
    } else {
        a.layout.resolve(rid, img.0, img.1, img)
    }?;
    over.blit(&mut bm, &p, 0, key);
    if !transparent.iter().any(|&t| t) {
        return Some((Sprite { w: base.w, h: base.h, px: bm.px, cmap: None, off: base.off }, None));
    }
    // Mark transparent pixels with a palette index no opaque pixel uses.
    let used: std::collections::HashSet<u8> = bm.px.iter().zip(&transparent).filter(|(_, t)| !**t).map(|(v, _)| *v).collect();
    let tkey = (0..=255u8).find(|v| !used.contains(v))?;
    for (v, &t) in bm.px.iter_mut().zip(&transparent) {
        if t {
            *v = tkey;
        }
    }
    Some((Sprite { w: base.w, h: base.h, px: bm.px, cmap: None, off: base.off }, Some(tkey)))
}

/// Teleporter shimmer (0x509E6): the noise texture shown through a per-depth
/// mask at a random offset each frame. Visual randomness only.
fn draw_teleporter(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, c: usize) {
    if !cx.on(layers::TELEPORTERS) || c >= 16 {
        return;
    }
    let (phase, mask, w, h) = TELEPORTER[c];
    let Some(noise) = a.sprite(24, 0, 20) else { return };
    let rx = (cx.rand() & 0xFF) as usize;
    let ry = (((cx.rand() & 0xFF) + phase as u32) * 16) as usize;
    let (w, h) = (w as usize, h as usize);
    let m = if mask & 0x7F == 0x7F { None } else { a.sprite(24, 0, mask & 0x7F) };
    let mirror = mask & 0x80 != 0;
    let key = a.gdat.lookup(Key::new(24, 0, 11, 4)).unwrap_or(0) as u8;
    let mut px = vec![key; w * h];
    for y in 0..h {
        for x in 0..w {
            let visible = match &m {
                None => true,
                Some(ms) => {
                    let mx = if mirror { ms.w.saturating_sub(1 + x) } else { x };
                    mx < ms.w && y < ms.h && ms.px[y * ms.w + mx] != key
                }
            };
            if visible {
                let n = noise.px[((y + ry) % noise.h) * noise.w + (x + rx) % noise.w];
                px[y * w + x] = match &noise.cmap {
                    Some(cm) => cm[(n & 15) as usize],
                    None => n,
                };
            }
        }
    }
    let field = Sprite { w, h, px, cmap: None, off: (0, 0) };
    let r = Req { key: Some(key), ..Req::new(24, 0, 20, LAYOUT_WALL0 + c as u16) };
    draw_sprite(a, buf, cx, &field, (0, 0), &r, 64, 64);
}

/// (category, index) of a floor item (docs/09; types 5-10 only).
fn item_key(dg: &Dungeon, t: ThingRef) -> (u8, u8) {
    let kind = t.kind() as u16;
    let w1 = word(dg, t, 1);
    let idx = match kind {
        5 | 6 | 10 => w1 & 0x7F,
        7 => 0,
        8 => (w1 >> 8) & 0x7F,
        _ => {
            let w2 = word(dg, t, 2);
            (w2 >> 13) | ((w2 >> 1) & 3) << 3
        }
    };
    (16 + kind as u8 - 5, idx as u8)
}

/// Rotate a 5×5 slot by a view direction (0x19B0B).
fn rotate_slot(slot: u8, view: u8) -> u8 {
    let (x, y) = (slot as i32 % 5 - 2, slot as i32 / 5 - 2);
    let (nx, ny) = match view & 3 {
        0 => (x, y),
        1 => (y, -x),
        2 => (-x, -y),
        _ => (-y, x),
    };
    ((nx + 2) + (ny + 2) * 5) as u8
}

/// Things standing in a cell, drawn per sub-square (0x52518).
fn draw_contents(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell, c: usize, filter: impl Fn(u8) -> bool, creatures_only: bool) {
    if c < 16 && !HAS_CONTENTS[c] {
        return;
    }
    let order: &[u8] = match CELLS[c].0.signum() {
        -1 => &ORDER_LEFT,
        1 => &ORDER_RIGHT,
        _ => &ORDER_CENTRE,
    };
    let order = if c == 0 { &order[..15] } else { order };
    let depth = DEPTH[c];
    for &s in order {
        if !filter(s) {
            continue;
        }
        for &t in &cell.things {
            let kind = t.kind() as u16;
            if (5..=10).contains(&kind) && !creatures_only && c < 16 && cx.on(layers::ITEMS) {
                let slot = QUAD_SLOT[(t.cell().wrapping_sub(cx.dir) & 3) as usize];
                if slot == s {
                    draw_item(a, buf, cx, t, c, slot, depth);
                }
            } else if kind == 4 && cx.on(layers::CREATURES) {
                draw_creature(a, buf, cx, t, c, s, depth);
            } else if kind == 14 && !creatures_only && c < 16 && cx.on(layers::MISSILES) {
                let slot = QUAD_SLOT[(t.cell().wrapping_sub(cx.dir) & 3) as usize];
                if slot == s {
                    draw_missile(a, buf, cx, t, c, slot, depth);
                }
            }
        }
    }
}

fn draw_item(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, t: ThingRef, c: usize, slot: u8, depth: usize) {
    let row = slot as usize / 5;
    if c == 0 && 4 - row < 2 {
        return; // behind the camera
    }
    let (cat, idx) = item_key(cx.dg, t);
    let sc = ITEM_SCALE[depth * 4 + 4 - row];
    let key = key_attr(a, cat, idx, Some(10), false);
    let r = Req { xs: sc, ys: sc, depth: Some(depth), key, ..Req::new(cat, idx, 0, 5000 + 25 * c as u16 + slot as u16) };
    draw(a, buf, cx, r);
}

/// Creature (0x51203 -> 0x50DEE), drawn from its drawing descriptor.
fn draw_creature(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, t: ThingRef, c: usize, pass_slot: u8, depth: usize) {
    let Some(rec) = cx.dg.record(t) else { return };
    let ctype = rec[4];
    let facing = ((u16::from_le_bytes([rec[14], rec[15]]) >> 8) & 3) as u8;
    let view = cx.dir.wrapping_sub(facing) & 3;
    let frame = cx.ex.creature_frames.get(&t.0).copied().unwrap_or(0) as usize;
    let Some(desc) = a.gdat.get(Key::new(15, ctype, 7, 253)).and_then(|d| d.get(frame * 8..frame * 8 + 8)).map(|d| d.to_vec()) else {
        return;
    };
    let slot = rotate_slot(desc[4], view);
    if slot != pass_slot {
        return;
    }
    // Image for the view, with the documented fallbacks.
    let mut flip = 0u8;
    let mut sub = desc[view as usize];
    if a.has_image(15, ctype, sub) {
        if (desc[7] >> ((3 - view) * 2)) & 1 != 0 {
            flip = 1;
        }
    } else {
        let opp = (view + 2) & 3;
        sub = desc[opp as usize];
        flip = opp & 1;
        if !a.has_image(15, ctype, sub) {
            sub = desc[2];
            flip = 0;
        }
    }
    if !a.has_image(15, ctype, sub) {
        sub = view.wrapping_sub(6);
        flip = 0;
        if !a.has_image(15, ctype, sub) {
            let alt = ((view + 2) & 3).wrapping_sub(6);
            if view & 1 != 0 && a.has_image(15, ctype, alt) {
                sub = alt;
                flip = 1;
            } else {
                sub = 0xFC;
            }
        }
    }
    let ds = DEPTH_SCALE[depth];
    let per_frame = a
        .gdat
        .get(Key::new(15, ctype, 7, 0xFE))
        .and_then(|tb| tb.get(desc[5] as usize * 4 + view as usize).copied())
        .unwrap_or(64) as i32;
    let sc = scale_v(per_frame, ds);
    let key = key_attr(a, 15, ctype, Some(4), true);
    let r = Req { flip, xs: sc, ys: sc, depth: Some(depth), key, ..Req::new(15, ctype, sub, 5000 + 25 * c as u16 + slot as u16) };
    draw(a, buf, cx, r);
}

/// Missile or spell effect in flight (0x518B0).
fn draw_missile(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, t: ThingRef, c: usize, slot: u8, depth: usize) {
    let carried = ThingRef(word(cx.dg, t, 1));
    let (cat, idx) = if carried.0 >= 0xFF80 {
        (13u8, (carried.0 - 0xFF80) as u8)
    } else if (5..=10).contains(&(carried.kind() as u16)) {
        item_key(cx.dg, carried)
    } else {
        return;
    };
    // Sub by flight direction relative to the view: 8 head-on, 10 across, 12 from behind.
    let mdir = cx.ex.missile_dirs.get(&t.0).copied().unwrap_or((cx.dir + 2) & 3);
    let rel = mdir.wrapping_sub(cx.dir) & 3;
    let sub = match rel {
        2 => 8,
        0 => 12,
        _ => 10,
    };
    let sub = if a.has_image(cat, idx, sub) { sub } else { 8 };
    let si = (depth as i32 * 2 - (slot as i32 / 5) / 2).clamp(0, 6) as usize;
    let mut sc = MISSILE_SCALE[si];
    if cat == 13 {
        let power = cx.dg.record(t).map(|r| r[4] as i32).unwrap_or(255);
        let f = ((power * 128 / 255) + 1) / 2;
        sc = scale_v(sc, f).max(8);
    }
    let fmask = if cat == 13 { attr(a, 13, idx, 1) as u8 } else { 3 };
    let flip = if rel == 1 { 1 & fmask } else { 0 };
    let key = key_attr(a, cat, idx, Some(10), true);
    let r = Req { flip, xs: sc, ys: sc, yoff: -92, depth: Some(depth), key, ..Req::new(cat, idx, sub, 5000 + 25 * c as u16 + slot as u16) };
    draw(a, buf, cx, r);
}

/// The party's own square (0x54117).
fn draw_party_cell(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell) {
    match cell.sq >> 5 {
        3 if cx.on(layers::PITS_STAIRS) => {
            // Stairs underfoot: going down (bit 2 clear) or up.
            let pairs: [(u8, u16); 2] = if cell.sq & 4 == 0 { [(0x4D, 0x338), (0x4E, 0x339)] } else { [(0x39, 0x32B), (0x3A, 0x32C)] };
            for (sub, rid) in pairs {
                draw(a, buf, cx, Req::new(8, cx.set, sub, rid));
            }
        }
        4 if cx.on(layers::DOORS) && cell.vt == vt::DOOR_EDGE => {
            draw(a, buf, cx, Req::new(8, cx.set, 6, LAYOUT_WALL0));
        }
        _ => {}
    }
    if cell.vt == vt::PIT {
        draw_pit(a, buf, cx, cell, 0);
    }
    draw_ceiling_hole(a, buf, cx, cell, 0);
    draw_floor_ornament(a, buf, cx, cell, 0);
    if cell.vt == vt::DOOR_ACROSS {
        draw_door(a, buf, cx, cell, 0);
    }
    draw_contents(a, buf, cx, cell, 0, |_| true, false);
    if cell.vt == vt::TELEPORTER {
        draw_teleporter(a, buf, cx, 0);
    }
}

#[cfg(test)]
mod tests;
