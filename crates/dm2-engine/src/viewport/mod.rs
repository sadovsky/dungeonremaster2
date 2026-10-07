//! The 3D dungeon view (docs/04-rendering.md, section 3).
//!
//! Every image is loaded at runtime from the user's own GRAPHICS.DAT; this
//! module only holds the traversal and placement rules.

pub mod backdrop;
mod creature;
pub mod hits;
pub mod light;
mod walltext;

use std::collections::HashMap;

use dm2_formats::dungeon::{Dungeon, Element, ThingRef, ThingType};
use dm2_formats::gdat::Key;

use crate::assets::Assets;
use crate::gfx::{Bitmap, Sprite};
use crate::layout::Placement;

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
/// Mid-step frame: ceiling and floor y shifts (0x7170A / 0x7170C) and the
/// inner rectangle placements through the viewport are clipped to (0x7170E).
const MID_STEP_CEILING_DY: i32 = -2;
const MID_STEP_FLOOR_DY: i32 = 3;
const MID_STEP_CLIP: (i32, i32, i32, i32) = (21, 8, 182, 110);
/// Depth darkening (64ths) in mid-step frames (0x75C07).
const MID_STEP_DARKEN: [i32; 5] = [0, 0, 5, 19, 36];
/// Mid-step walls (negated depth), 0x75C01 + depth as signed bytes.
const MID_STEP_WALL_BRIGHTEN: [i32; 5] = [0, 0, -7, -9, -10];
/// Attack lunge from the square ahead, by attack step: sub-square and
/// scale (0x75BB4 / 0x75BBB).
const LUNGE_SLOT: [u8; 7] = [2, 14, 22, 22, 22, 10, 12];
const LUNGE_SCALE: [i32; 7] = [52, 64, 78, 78, 78, 64, 64];
/// Position nudges selected by 3-bit fields (0x75BC2).
const NUDGE: [i32; 8] = [0, 1, 2, 3, 0, -3, -2, -1];
/// Door button position per view cell (0x75ECF); -1 = no button there.
const DOOR_BUTTON: [i8; 16] = [4, -1, -1, 3, -1, -1, 2, -1, -1, -1, -1, 1, -1, 0, -1, -1];
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
    /// Outdoor weather to draw: cloud/storm backdrops and the rain overlay.
    pub weather: crate::weather::WeatherView,
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
    /// Per-creature drawing state beyond the frame (jitter, alternate
    /// descriptor, attack lunge, facing rule), keyed like `creature_frames`.
    pub creatures: HashMap<u16, CreatureDraw>,
    /// The party is between squares (the step counter 0x7F258 is running):
    /// draw the mid-step frame (docs/04 "Mid-step frames").
    pub mid_step: bool,
    /// The party's darkness step, 0-5 (global 0x7F282); bounds how much a
    /// mid-step wall may be brightened (0x802CE = step × 10).
    pub darkness_step: i32,
    /// Floor-item stacking table (0x75B94): 16 pairs of 3-bit selectors
    /// into the nudge offsets, read from the user's SKULL.EXE by the
    /// frontend. None: piled items are not fanned out.
    pub stack_nudges: Option<[u8; 32]>,
    /// Door-frame tables read from the user's SKULL.EXE by the frontend.
    /// None: door frames (lintel and posts) are not drawn.
    pub door_frame: Option<DoorFrameTables>,
}

/// Door-frame drawing tables used by 0x531EC, read at runtime from the
/// user's own SKULL.EXE (never stored in this source).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DoorFrameTables {
    /// Lintel image sub per view cell (0x75EF9); 0xFF = none.
    pub lintel_sub: [u8; 16],
    /// Lintel layout id per view cell (word at 0x75F07 + 2·cell).
    pub lintel_rid: [u16; 16],
    /// Post image subs, two per index (0x75EDD); 0xFF = none.
    pub post_sub: [u8; 32],
    /// Left/right partner of each cell (0x75B28), used when the ambient
    /// level is non-zero.
    pub cell_index: [u8; 16],
}

impl DoorFrameTables {
    /// Build from the four executable slices, in the order above.
    pub fn from_slices(lintel_sub: &[u8], lintel_rid: &[u8], post_sub: &[u8], cell_index: &[u8]) -> Option<Self> {
        let mut rid = [0u16; 16];
        for (i, r) in rid.iter_mut().enumerate() {
            *r = u16::from_le_bytes([*lintel_rid.get(2 * i)?, *lintel_rid.get(2 * i + 1)?]);
        }
        Some(DoorFrameTables {
            lintel_sub: lintel_sub.get(..16)?.try_into().ok()?,
            lintel_rid: rid,
            post_sub: post_sub.get(..32)?.try_into().ok()?,
            cell_index: cell_index.get(..16)?.try_into().ok()?,
        })
    }
}

/// Drawing state of one creature group, filled by the frontend from
/// `creatures::view` and the creature type info.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CreatureDraw {
    /// Position byte of the active slot (+7): bits 0-2 x nudge, 3-5 y
    /// nudge, bit 6 allows the per-view "mirror if flagged" bit.
    pub position: u8,
    /// Type info flag 0x0004: always drawn with the front view.
    pub faces_party: bool,
    /// Alternate drawing descriptor (slot state 0x13 in the original):
    /// supplies the sub-square, scale index and shift instead of `frame`.
    pub alt_frame: Option<u16>,
    /// Attack step while lunging at the party from the square ahead
    /// (index into the lunge slot/scale tables), if attacking.
    pub lunge: Option<u8>,
    /// In-square position (5×5 sub-square, 12 = centre) once the creature
    /// module models positions inside a square; None uses the descriptor.
    pub slot: Option<u8>,
}

impl Default for ViewExtras {
    fn default() -> Self {
        ViewExtras {
            tick: 0,
            weather: Default::default(),
            lighting: true,
            ambient: 0,
            visual_seed: 0,
            layers: layers::ALL,
            creature_frames: HashMap::new(),
            missile_dirs: HashMap::new(),
            creatures: HashMap::new(),
            mid_step: false,
            darkness_step: 0,
            stack_nudges: None,
            door_frame: None,
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
    px: i32,
    py: i32,
    rng: u32,
    hits: hits::HitTable,
    plan: creature::Plan,
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
    /// A wall in a mid-step frame: the original passes its depth negated,
    /// which selects the brightening row and remap table 1 (0x4E3D5).
    wall_mid: bool,
    /// Darken by the ambient level only, with no depth row or set remap:
    /// the ceiling and floor (0x4E32A darkens them by 0x802CE directly).
    ambient_only: bool,
    /// Screen-pixel offset added after scaling (0x4E502's position adds).
    post: (i32, i32),
    /// Anchor override for the layout record (0x4E502's eighth argument,
    /// forwarded to the resolver); None keeps the record's own kind.
    anchor: Option<i16>,
}

impl Req {
    fn new(cat: u8, idx: u8, sub: u8, rid: u16) -> Req {
        Req { cat, idx, sub, rid, flip: 0, xs: 64, ys: 64, xoff: 0, yoff: 0, depth: None, key: None, wall_mid: false, ambient_only: false, post: (0, 0), anchor: None }
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
fn light_map(a: &mut Assets, cx: &Ctx, depth: Option<usize>, key: Option<u8>, wall_mid: bool, ambient_only: bool) -> Option<[u8; 256]> {
    if ambient_only {
        return if cx.ex.lighting { a.light.as_mut()?.for_depth_with(0, cx.ex.ambient, key, None) } else { None };
    }
    let depth = depth?;
    if !cx.ex.lighting {
        return None;
    }
    if wall_mid && depth >= 1 {
        // Negative-depth branch of 0x4E3D5: brightening row at 0x75C02
        // (0, -7, -9, -10 for depths 1-4), bounded below by -(step × 10),
        // with the set's remap table 1. Brightening past full light is
        // clamped (only reachable when the set has no remap table).
        let darken = MID_STEP_WALL_BRIGHTEN[depth.min(4)].max(-cx.ex.darkness_step * 10).max(0);
        let remap = a.gdat.get(Key::new(8, cx.set, 7, 1)).map(|t| t.to_vec());
        return a.light.as_mut()?.for_depth_with(darken, cx.ex.ambient, key, remap.as_deref());
    }
    // Mid-step frames use the in-between darkening row and the set's
    // remap tables 10-13 instead of 1-4 (0x4E3D5).
    let (sub, darken) = if cx.ex.mid_step { (depth as u8 + 9, MID_STEP_DARKEN[depth.min(4)]) } else { (depth as u8, light::DEPTH_DARKEN[depth.min(4)]) };
    let remap = if (1..=4).contains(&depth) {
        a.gdat.get(Key::new(8, cx.set, 7, sub)).map(|t| t.to_vec())
    } else {
        None
    };
    a.light.as_mut()?.for_depth_with(darken, cx.ex.ambient, key, remap.as_deref())
}

/// Place and draw one sprite with offsets, flip and light (0x1B54A/0x1B8E5).
fn draw_sprite(a: &mut Assets, buf: &mut Bitmap, cx: &Ctx, s: &Sprite, base_off: (i32, i32), r: &Req, xs: i32, ys: i32) -> Option<Placement> {
    let mut ox = scale_v(base_off.0 + r.xoff, xs);
    let oy = scale_v(base_off.1 + r.yoff, ys);
    if r.flip & 1 != 0 {
        ox = -ox;
    }
    let (ox, oy) = (ox + r.post.0, oy + r.post.1);
    let img = (s.w as i32, s.h as i32);
    let p = if (ox, oy) != (0, 0) {
        a.layout.resolve_anchored(r.rid | 0x8000, ox, oy, img, r.anchor)
    } else {
        a.layout.resolve_anchored(r.rid, img.0, img.1, img, r.anchor)
    };
    let mut p = p?;
    if cx.ex.mid_step {
        p = clip(p, MID_STEP_CLIP)?;
    }
    let lm = light_map(a, cx, r.depth, r.key, r.wall_mid, r.ambient_only);
    s.blit_mapped(buf, &p, r.flip, r.key, lm.as_ref());
    Some(p)
}

/// Intersect a placement with a rectangle (x, y, w, h), keeping the source
/// offsets in step (0x18F57).
fn clip(mut p: Placement, (cx, cy, cw, ch): (i32, i32, i32, i32)) -> Option<Placement> {
    let d = cx - p.x;
    if d > 0 {
        p.x += d;
        p.skip_x += d;
        p.w -= d;
    }
    let d = cy - p.y;
    if d > 0 {
        p.y += d;
        p.skip_y += d;
        p.h -= d;
    }
    p.w = p.w.min(cx + cw - p.x);
    p.h = p.h.min(cy + ch - p.y);
    (p.w > 0 && p.h > 0).then_some(p)
}

fn draw(a: &mut Assets, buf: &mut Bitmap, cx: &Ctx, r: Req) -> Option<Placement> {
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
    let orig = a.sprite(r.cat, r.idx, r.sub)?;
    let s = a.sprite_scaled(r.cat, r.idx, r.sub, xs, r.ys)?;
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
                // Mode 0, and mode 1 with bits 11-15 = 14, are wall writing
                // (shown when bit 0 is set); other mode-1 things name an
                // ornament (0x1E4EE).
                match ((w1 & 7) >> 1, w1 >> 11) {
                    (0, _) | (1, 14) => {
                        c.faces[rel] = 0;
                        if rel == 2 && w1 & 1 != 0 {
                            c.wall_text = Some(t);
                        }
                    }
                    (1, _) => c.faces[rel] = (w1 >> 3) as u8,
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
                    // Type 0x27 (map-edge link switch) shows its ornament only
                    // when bits 7+ of word 1, less one, name the current map
                    // (0x1E908); otherwise it contributes none.
                    let w1 = word(dg, t, 1);
                    if w1 & 0x7F == 0x27 && (w1 >> 7) as i64 - 1 != cx.map as i64 {
                        continue;
                    }
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
    render_full(a, dg, map, px, py, dir, ex).bitmap
}

/// A rendered view and the clickable things drawn into it.
pub struct Rendered {
    pub bitmap: Bitmap,
    pub hits: hits::HitTable,
}

/// Render the view and collect the drawn-things hit table (0x7F2EC).
pub fn render_full(a: &mut Assets, dg: &Dungeon, map: usize, px: i32, py: i32, dir: u8, ex: &ViewExtras) -> Rendered {
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
        px,
        py,
        rng: ex.visual_seed.wrapping_mul(0x9E37_79B9) ^ ex.tick.wrapping_add(1).wrapping_mul(0x85EB_CA6B) | 1,
        hits: hits::HitTable::default(),
        plan: creature::Plan::default(),
    };
    // Ceiling and floor (0x4E32A); only the parity-driven flips are modelled.
    let ceil_flip = if set_flags & 2 != 0 && set_flags & 4 == 0 { 1 - par } else { 0 };
    let floor_flip = if set_flags & 8 != 0 && set_flags & 0x10 == 0 { par } else { 0 };
    for (sub, rid, fl, dy) in [(1u8, LAYOUT_CEILING, ceil_flip, MID_STEP_CEILING_DY), (0, LAYOUT_FLOOR, floor_flip, MID_STEP_FLOOR_DY)] {
        // Darkened by the ambient level (0x802CE); shifted by a few pixels
        // for the in-between position (0x4E32A).
        let yoff = if ex.mid_step { dy } else { 0 };
        let r = Req { flip: fl, yoff, ambient_only: true, ..Req::new(8, set, sub, rid) };
        if let Some(s) = a.sprite(8, set, sub) {
            draw_sprite(a, &mut buf, &cx, &s, s.off, &r, 64, 64);
        }
    }
    // Map-set backdrops: horizon strips and distant landmarks (0x54699).
    let m = &dg.maps[map];
    let (gx, gy) = (m.origin_x as i32 + px, m.origin_y as i32 + py);
    let weather_backdrops: Vec<backdrop::Backdrop> =
        ex.weather.backdrops.iter().filter_map(|&n| backdrop::plan_one(&a.gdat, set, n, gx, gy, dir)).collect();
    // Backdrops use the map set's colour key (0x544BE passes 0x75BFA's high
    // word, attribute (8, set, 11, 100), to the drawer).
    let set_key = a.gdat.lookup(Key::new(8, set, 11, 100)).map(|k| k as u8);
    // Weather backdrops (clouds, storm) are the far layer: the map set's
    // landmarks are drawn over them, as in the original's captures.
    for b in weather_backdrops.into_iter().chain(backdrop::plan(&a.gdat, set, gx, gy, dir)) {
        let flip = u8::from(backdrop::mirrored(b.flip_kind, set_flags, par, ex.tick));
        let s = if b.scale == 64 { a.sprite(23, set, b.index) } else { a.sprite_scaled(23, set, b.index, b.scale, b.scale) };
        if let Some(s) = s {
            let r = Req { flip, ambient_only: true, key: set_key, post: (b.xoff, 0), xs: b.scale, ys: b.scale, ..Req::new(23, set, b.index, b.rid) };
            draw_sprite(a, &mut buf, &cx, &s, s.off, &r, b.scale, b.scale);
        }
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
    cx.plan = creature::Plan::build(a, &cx, &cells);
    for &c in DRAW_ORDER.iter() {
        draw_cell(a, &mut buf, &mut cx, &cells[c], c);
    }
    draw_party_cell(a, &mut buf, &mut cx, &cells[0]);
    draw_rain(a, &mut buf, &mut cx, set, ex);
    Rendered { bitmap: buf, hits: cx.hits }
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
    // Walls go through the lit drawer with depth 0, or their depth negated
    // in mid-step frames (0x53D47 area -> 0x4E502 -> 0x4E3D5).
    let depth = CELLS[c].1 as usize;
    let wall_mid = cx.ex.mid_step && depth >= 1;
    let r = Req { flip, key: Some(key), depth: Some(if wall_mid { depth } else { 0 }), wall_mid, ..Req::new(8, cx.set, sub, LAYOUT_WALL0 + c as u16) };
    draw(a, buf, cx, r);
    // Cells 16-20 show their front face's ornament too (0x53E9E runs the
    // wall faces for every wall cell; FACES gives 16-20 a front face).
    if !cx.on(layers::ORNAMENTS) {
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
    let ds = DEPTH_SCALE[depth];
    // Side faces seen obliquely at depths 2 and 3 are narrowed.
    let xs = match (side.abs() > 1, depth) {
        (true, 2) => 114,
        (true, 3) => 76,
        _ => ds,
    };
    if orn == 0 {
        draw_wall_writing(a, buf, cx, cell, c, side, xs, ds);
        return;
    }
    let key = key_attr(a, 9, orn, None, true);
    // Slot in the 5×5 face grid and the anchor kind (attribute 5: slot + 1
    // in the low byte, anchor in the high byte; default slot 12, anchor 0).
    // 0x4F3DF passes the anchor to the drawer, which hands it to the layout
    // resolver as an override, so 0 centres the image on the grid point.
    let (slot, anchor) = match attr(a, 9, orn, 5) {
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
    // Animated ornaments add 4 per frame to the sub (0x1E3DA).
    let step = (ornament_frame(a, 9, orn, cx.ex.tick, 0) << 2) as u8;
    let (sub, flip) = if side == 0 {
        (1u8, 0u8)
    } else if side > 0 {
        if a.has_image(9, orn, 2u8.wrapping_add(step)) { (2, 0) } else { (0, 1) }
    } else {
        (0, 0)
    };
    let mut r = Req::new(9, orn, sub.wrapping_add(step), rid);
    r.flip = flip;
    r.xs = xs;
    r.ys = ds;
    r.depth = Some(depth);
    r.key = key;
    r.anchor = Some(anchor as i16);
    let placed = draw(a, buf, cx, r);
    // Attribute 10 is the ornament's kind (0x1FCEC): 1 = an alcove showing
    // the items lying in it, 3 = a champion portrait mirror (0x4F3DF).
    let kind = attr(a, 9, orn, 10);
    if side == 0 && kind == 1 && !a.has_image(9, orn, 0x0F) {
        draw_alcove_items(a, buf, cx, cell, c, rid);
    }
    if side == 0 && kind == 3 {
        if let Some(portrait) = mirror_portrait(cx, cell) {
            // Image (22, champion, 1) at the ornament's placement and scale,
            // offset by attribute (9, orn, 12, 0xFD): x high byte, y low.
            let off = a.gdat.lookup(Key::new(9, orn, 12, 0xFD)).unwrap_or(0);
            let mut pr = Req::new(22, portrait, 1, rid);
            pr.xs = ds;
            pr.ys = ds;
            pr.xoff = (off >> 8) as u8 as i8 as i32;
            pr.yoff = (off & 0xFF) as u8 as i8 as i32;
            pr.depth = Some(depth);
            pr.anchor = Some(anchor as i16);
            // The original passes key −1 to the drawer: no colour key.
            pr.key = None;
            draw(a, buf, cx, pr);
        }
    }
    // Ornaments on the three nearest wall cells are clickable (kind 6).
    if let (Some(p), 1..=3) = (placed, c) {
        cx.hits.push(hits::Hit { x: p.x, y: p.y, w: p.w, h: p.h, thing: None, cell: c as u8, kind: hits::HitKind::WallOrnament });
    }
}

/// Animation frame of an ornament (0x1E3DA): the frame count attribute
/// (cat, orn, 11, 0x0D), bit 15 meaning frames start at 1, cycles with
/// the tick; otherwise an optional frame string (cat, orn, 5, 0x0D) is
/// indexed by the tick, digits giving frames 0-9 and letters
/// `char − 0x4B`. Frames keep 6 bits, as in the cell summary word.
fn ornament_frame(a: &Assets, cat: u8, orn: u8, tick: u32, phase: u32) -> u32 {
    ornament_frame_at(&a.gdat, cat, orn, tick, phase)
}

/// `ornament_frame` from the archive alone (also used by the light scan,
/// where a non-zero frame switches bit-15 light sources on).
pub fn ornament_frame_at(g: &dm2_formats::gdat::Gdat, cat: u8, orn: u8, tick: u32, phase: u32) -> u32 {
    let n = g.lookup(Key::new(cat, orn, 11, 0x0D)).unwrap_or(0) as u32;
    let t = tick.wrapping_add(phase);
    if n != 0 {
        let (count, base) = (n & 0x7FFF, (n >> 15) & 1);
        return if count == 0 { 0 } else { (t % count + base) & 0x3F };
    }
    let Some(seq) = crate::font::text(g, cat, orn, 0x0D, &crate::font::TextContext::default()) else { return 0 };
    if seq.is_empty() {
        return 0;
    }
    let ch = seq[(t % seq.len() as u32) as usize] as u32;
    let f = match ch {
        0x30..=0x39 => ch - 0x30,
        0x41..=0x5A => ch.wrapping_sub(0x4B),
        _ => ch,
    };
    f & 0x3F
}

/// Wall writing: the map set's writing panel, then the text composed at
/// 1:1 on a panel-sized bitmap and placed the same way (0x4F3DF).
#[allow(clippy::too_many_arguments)]
fn draw_wall_writing(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell, c: usize, side: i32, xs: i32, ys: i32) {
    let depth = DEPTH[c];
    let key = a.gdat.lookup(Key::new(8, cx.set, 11, 100)).unwrap_or(0) as u8;
    // Panel sub: 0xFC front; 0xFD on the left; on the right 0xFE, or 0xFD mirrored.
    let (sub, flip) = match side.signum() {
        0 => (0xFCu8, 0u8),
        -1 => (0xFD, 0),
        _ if a.has_image(8, cx.set, 0xFE) => (0xFE, 0),
        _ => (0xFD, 1),
    };
    let rid = if side == 0 {
        3100 + 25 * c as u16 + 12
    } else {
        match SIDE_ORN_BASE.get(c) {
            Some(&b) if b > 0 => b as u16 + 12,
            _ => return,
        }
    };
    let r = Req { flip, xs, ys, depth: Some(depth), key: Some(key), ..Req::new(8, cx.set, sub, rid) };
    let placed = draw(a, buf, cx, r);
    if side != 0 || placed.is_none() {
        return;
    }
    let Some(t) = cell.wall_text else { return };
    let Some(panel) = a.sprite(8, cx.set, sub) else { return };
    let Some(text) = walltext::text_of(a, cx.dg, t) else { return };
    let Some(img) = walltext::compose(a, cx.set, &text, panel.w, panel.h, key) else { return };
    let Some(scaled) = img.scaled(xs, ys) else { return };
    let r = Req { xs, ys, depth: Some(depth), key: Some(key & 15), ..Req::new(8, cx.set, sub, rid) };
    draw_sprite(a, buf, cx, &scaled, panel.off, &r, xs, ys);
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
    // 0x50081 takes the colour key from attribute 4, falling back to the
    // map set's key (attribute (8, set, 11, 100), via 0x75BFA) when it is 0.
    // Attribute 0x11 is a separate argument to the composing drawer
    // (0x4E620), not the key.
    let set_key = a.gdat.lookup(Key::new(8, cx.set, 11, 100)).and_then(|k| u8::try_from(k).ok());
    let key = key_attr(a, 10, orn, set_key, true);
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
    let key = Some(attr(a, 8, cx.set, 100) as u8);
    let r = Req { flip, key, depth: Some(DEPTH[c]), ..Req::new(8, cx.set, HOLE_SUB[c], HOLE_LAYOUT[c]) };
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
    // Keyed with the set's default colour and lit by depth, like stairs.
    let key = Some(attr(a, 8, cx.set, 100) as u8);
    let r = Req { flip, key, depth: Some(DEPTH[c]), ..Req::new(8, cx.set, sub as u8, PIT_LAYOUT[c] as u16) };
    draw(a, buf, cx, r);
}

/// Stairs, front-on (0x53B1B) or side-on (0x53BFB).
fn draw_stairs(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell, c: usize) {
    if !cx.on(layers::PITS_STAIRS) {
        return;
    }
    let k = c * 2 + ((cell.sq >> 2) & 1) as usize;
    // Stair art is keyed with the map set's default colour (attribute 100),
    // like the walls; without it the background shows as a solid box.
    let key = Some(attr(a, 8, cx.set, 100) as u8);
    if cell.vt == vt::STAIRS_FRONT {
        if k >= 32 || STAIR_FRONT_SUB[k] < 0 {
            return;
        }
        let (sub, flip) = if a.has_image(8, cx.set, STAIR_FRONT_SUB[k] as u8) {
            (STAIR_FRONT_SUB[k], 0)
        } else {
            (STAIR_FRONT_ALT[k], 1)
        };
        let r = Req { flip, key, depth: Some(DEPTH[c]), ..Req::new(8, cx.set, sub as u8, STAIR_FRONT_LAYOUT[k] as u16) };
        draw(a, buf, cx, r);
    } else if k < 18 && STAIR_SIDE_SUB[k] >= 0 && STAIR_SIDE_LAYOUT[k] >= 0 {
        draw(a, buf, cx, Req { key, depth: Some(DEPTH[c]), ..Req::new(8, cx.set, STAIR_SIDE_SUB[k] as u8, STAIR_SIDE_LAYOUT[k] as u16) });
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
    // Frame parts drawn before and after the panel, per view cell (0x539CB):
    // bit 0 lintel, bit 1 left post, bit 2 right post.
    let (before, after) = DOOR_FRAME_MASKS[c];
    draw_door_frame(a, buf, cx, t, c, before);
    draw_door_button(a, buf, cx, t, c);
    if state != 0 {
        draw_door_panel(a, buf, cx, t, c, state);
    }
    draw_door_frame(a, buf, cx, t, c, after);
}

/// Door-frame parts drawn before and after the panel for each view cell,
/// as 0x539CB passes them to 0x5346E (bit 0 lintel, 1 left post, 2 right
/// post). Cells without a door-across drawing are (0, 0).
const DOOR_FRAME_MASKS: [(u8, u8); 16] = [
    (6, 0), (0, 0), (0, 0), (7, 0), (1, 4), (1, 2), (7, 0), (1, 4),
    (1, 2), (0, 0), (0, 0), (6, 0), (2, 4), (4, 2), (0, 0), (0, 0),
];

/// Door frame (0x531EC): the lintel through the ambient-only drawer and
/// the two posts from the map set's images through the lit drawer at
/// depth 0, the right post mirrored. Door types with attribute 0x40 set
/// have no frame here. The post images swap with the ambient level.
fn draw_door_frame(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, t: ThingRef, c: usize, mask: u8) {
    if mask == 0 || c >= 16 {
        return;
    }
    let Some(tb) = cx.ex.door_frame else { return };
    let dt = door_type(cx, t);
    if attr(a, 14, dt, 0x40) != 0 {
        return;
    }
    let set = cx.set;
    let key = a.gdat.lookup(Key::new(8, set, 11, 100)).map(|k| k as u8);
    if mask & 1 != 0 && tb.lintel_sub[c] != 0xFF {
        let r = Req { key, ambient_only: true, ..Req::new(8, set, tb.lintel_sub[c], tb.lintel_rid[c]) };
        draw(a, buf, cx, r);
    }
    let lit = cx.ex.ambient != 0;
    let idx = if lit { tb.cell_index[c] as usize } else { c };
    let post = |k: bool| tb.post_sub.get(idx * 2 + k as usize).copied().unwrap_or(0xFF);
    let slot_rid = |slot: u16| 5000 + 25 * c as u16 + slot;
    if mask & 2 != 0 {
        let sub = post(lit);
        if sub != 0xFF {
            let r = Req { key, depth: Some(0), anchor: Some(4), ..Req::new(8, set, sub, slot_rid(10)) };
            draw(a, buf, cx, r);
        }
    }
    if mask & 4 != 0 {
        let sub = post(!lit);
        if sub != 0xFF {
            let r = Req { key, depth: Some(0), anchor: Some(3), flip: 1, ..Req::new(8, set, sub, slot_rid(14)) };
            draw(a, buf, cx, r);
        }
    }
}

/// The panel itself, with its ornament and damage overlay (0x5346E).
fn draw_door_panel(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, t: ThingRef, c: usize, state: u8) {
    let depth = DEPTH[c];
    let dt = door_type(cx, t);
    let w1 = word(cx.dg, t, 1);
    let key = key_attr(a, 14, dt, Some(10), true);
    let (sub, sc) = if depth > 0 && a.has_image(14, dt, depth as u8 - 1) {
        (depth as u8 - 1, 64)
    } else {
        (0, if depth == 0 { 113 } else { DEPTH_SCALE[depth] })
    };
    // A per-depth panel image is already drawn for its distance, so the
    // original lights it with depth 0 (ambient only); only the scaled
    // fallback image gets the depth's darkening (0x5346E -> 0x4E502).
    let light_depth = if sc == 64 && depth > 0 { 0 } else { depth };
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
    let r = |rid: u16| Req { depth: Some(light_depth), key: img_key, ..Req::new(14, dt, sub, rid) };
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

/// Door button (0x530D1, category-12 branch): doors whose record has
/// word 1 bit 6 set show button image (12, 0, 1, 5·bit 11) beside the
/// frame at layout 1950 + 5·attr(12, 0, 11, 8) + the cell's button
/// position. Buttons within reach (positions 3 and 4) are clickable.
/// TODO: buttons drawn from a door-button ornament (the other branch,
/// selected by the cell summary) are not modelled.
fn draw_door_button(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, t: ThingRef, c: usize) {
    let w1 = word(cx.dg, t, 1);
    let pos = DOOR_BUTTON[c];
    if w1 & 0x40 == 0 || pos < 0 {
        return;
    }
    let depth = DEPTH[c];
    let sub = 5 * ((w1 >> 11) & 1) as u8;
    let rid = 1950 + 5 * attr(a, 12, 0, 8) + pos as u16;
    let key = a.gdat.lookup(Key::new(8, cx.set, 11, 100)).map(|k| k as u8);
    let sc = DEPTH_SCALE[depth];
    let r = Req { xs: sc, ys: sc, depth: Some(depth), key, ..Req::new(12, 0, sub, rid) };
    if let (Some(p), 3 | 4) = (draw(a, buf, cx, r), pos) {
        cx.hits.push(hits::Hit { x: p.x, y: p.y, w: p.w, h: p.h, thing: Some(t.0), cell: c as u8, kind: hits::HitKind::DoorButton });
    }
}

/// Teleporter shimmer (0x509E6): the noise texture shown through a per-depth
/// mask at a random offset each frame. Visual randomness only.
fn draw_teleporter(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, c: usize) {
    if !cx.on(layers::TELEPORTERS) || c >= 16 {
        return;
    }
    let (phase, mask, w, h) = TELEPORTER[c];
    let Some(noise) = a.sprite(24, 0, 20) else { return };
    // One random byte for the column, one random bit plus the cell's phase
    // for the row (0x509E6; the original takes both from the game RNG).
    let rx = (cx.rand() & 0xFF) as usize;
    let ry = (((cx.rand() & 1) + phase as u32) * 16) as usize;
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
                    // A mirrored mask of odd width is shifted by a pixel.
                    let mw = if mirror { ms.w - (ms.w & 1) } else { ms.w };
                    let mx = if mirror { mw.wrapping_sub(1 + x) } else { x };
                    mx < mw && y < ms.h && ms.px[y * ms.w + mx] != key
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
    let wall = cell.vt == vt::WALL;
    for (i, &s) in order.iter().enumerate() {
        if !filter(s) {
            continue;
        }
        if !creatures_only && c < 16 && cx.on(layers::ITEMS) {
            // Items piled in one quadrant fan out by a per-quadrant
            // counter, 0-15 (0x522A7 -> 0x51EB7).
            let mut stack = 0usize;
            for &t in &cell.things {
                if (5..=10).contains(&(t.kind() as u16)) && QUAD_SLOT[(t.cell().wrapping_sub(cx.dir) & 3) as usize] == s {
                    draw_item(a, buf, cx, t, c, s, depth, stack);
                    stack = (stack + 1) & 15;
                }
            }
        }
        // Creatures held at this sub-square's grid point (0x52518).
        if cx.on(layers::CREATURES) && c < 16 {
            let held = cx.plan.take(c, s, wall);
            if !held.is_empty() {
                for p in &held {
                    creature::draw_creature(a, buf, cx, p.thing, p.cell);
                }
                // Missiles in the seven sub-squares before this one go on top.
                if !creatures_only {
                    for &ps in &order[i.saturating_sub(7)..i] {
                        draw_missiles_at(a, buf, cx, cell, c, ps, depth);
                    }
                }
            }
        }
        if !creatures_only && c < 16 {
            draw_missiles_at(a, buf, cx, cell, c, s, depth);
        }
    }
}

fn draw_missiles_at(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell, c: usize, s: u8, depth: usize) {
    if !cx.on(layers::MISSILES) {
        return;
    }
    for &t in &cell.things {
        if t.kind() as u16 == 14 && QUAD_SLOT[(t.cell().wrapping_sub(cx.dir) & 3) as usize] == s {
            draw_missile(a, buf, cx, t, c, s, depth);
        }
    }
}

fn draw_item(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, t: ThingRef, c: usize, slot: u8, depth: usize, stack: usize) {
    let row = slot as usize / 5;
    if c == 0 && 4 - row < 2 {
        return; // behind the camera
    }
    let (cat, idx) = item_key(cx.dg, t);
    let sc = ITEM_SCALE[depth * 4 + 4 - row];
    let key = key_attr(a, cat, idx, Some(10), false);
    let post = cx.ex.stack_nudges.map_or((0, 0), |t| (NUDGE[(t[2 * stack] & 7) as usize], NUDGE[(t[2 * stack + 1] & 7) as usize]));
    let r = Req { xs: sc, ys: sc, depth: Some(depth), key, post, ..Req::new(cat, idx, 0, 5000 + 25 * c as u16 + slot as u16) };
    let placed = draw(a, buf, cx, r);
    // Items within reach (the party's square and the one ahead) are
    // clickable; a pile in one quadrant shares a record (0x522A7, 0x51CC6).
    if let (Some(p), 0 | 3) = (placed, c) {
        let quadrant = (t.cell().wrapping_sub(cx.dir) & 3) as u8;
        cx.hits.item(hits::HitKind::FloorItem, c as u8, quadrant, t.0, (p.x, p.y, p.w, p.h));
    }
}

/// Champion number of the portrait actuator (type 0x7E: word 1 bits 0-6 the
/// type, bits 7 and up the champion) on a wall square shown as a mirror.
fn mirror_portrait(cx: &Ctx, cell: &Cell) -> Option<u8> {
    cx.dg.things_at(cx.map, cell.x, cell.y).into_iter().find_map(|t| {
        (t.kind() == ThingType::Actuator)
            .then(|| cx.dg.record_word(t, 1))
            .flatten()
            .filter(|w| w & 0x7F == 0x7E)
            .map(|w| (w >> 7) as u8)
    })
}

/// Items lying in a wall alcove ahead (0x528C5): only on front faces at
/// depth 1, for items in the wall's quadrant that faces the party. The
/// square ahead records one merged hit (kind 3) for taking or placing.
fn draw_alcove_items(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, cell: &Cell, c: usize, rid: u16) {
    if !cx.on(layers::ITEMS) || DEPTH[c] != 1 {
        return;
    }
    let facing_cell = (cx.dir + 2) & 3;
    let items: Vec<ThingRef> = cell.things.iter().copied().filter(|t| (5..=10).contains(&(t.kind() as u16)) && t.cell() == facing_cell).collect();
    for t in items {
        let (cat, idx) = item_key(cx.dg, t);
        let key = key_attr(a, cat, idx, Some(10), false);
        let sc = DEPTH_SCALE[1];
        let r = Req { xs: sc, ys: sc, depth: Some(1), key, ..Req::new(cat, idx, 0, rid) };
        if let (Some(p), 3) = (draw(a, buf, cx, r), c) {
            cx.hits.item(hits::HitKind::AlcoveItem, 3, 4, t.0, (p.x, p.y, p.w, p.h));
        }
    }
}

/// Quadrant index of a 5×5 sub-square (0x159C3): 0-3 for the floor
/// quadrants, 4 for the centre.
fn quadrant_of(slot: u8) -> Option<u8> {
    match slot {
        6 => Some(0),
        8 => Some(1),
        18 => Some(2),
        16 => Some(3),
        12 => Some(4),
        _ => None,
    }
}

/// Missile or spell effect in flight (0x518B0).
fn draw_missile(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, t: ThingRef, c: usize, slot: u8, depth: usize) {
    let Some(quad) = quadrant_of(slot) else { return };
    // In the party's own square only the front half is visible.
    if depth == 0 && quad >= 2 {
        return;
    }
    let carried = ThingRef(word(cx.dg, t, 1));
    let (cat, idx) = if carried.0 >= 0xFF80 {
        (13u8, (carried.0 - 0xFF80) as u8)
    } else if (5..=10).contains(&(carried.kind() as u16)) {
        item_key(cx.dg, carried)
    } else {
        return;
    };
    // Which images the carried thing has decides how it flies (0x151E6).
    let kind: i8 = if !a.has_image(cat, idx, 8) {
        -1
    } else if !a.has_image(cat, idx, 12) {
        3
    } else if a.has_image(cat, idx, 10) {
        1
    } else if a.has_image(cat, idx, 9) {
        0
    } else {
        2
    };
    let rid = 5000 + 25 * c as u16 + slot as u16;
    if kind < 0 {
        // No flight images: the item itself, at chest height.
        let sc = ITEM_SCALE[(depth * 4 + 4 - slot as usize / 5).min(ITEM_SCALE.len() - 1)];
        let key = key_attr(a, cat, idx, Some(10), false);
        let r = Req { xs: sc, ys: sc, yoff: -92, depth: Some(depth), key, ..Req::new(cat, idx, 0, rid) };
        draw(a, buf, cx, r);
        return;
    }
    // Scale by depth and quadrant row; spells also by their power.
    let power = cx.dg.record(t).map(|r| r[4]).unwrap_or(0xFF);
    let spell = cat == 13 && power != 0xFF;
    let sc = if spell || depth != 0 {
        let si = depth as i32 * 2 - (quad as i32 >> 1);
        if si < 0 {
            return;
        }
        let base = MISSILE_SCALE[(si as usize).min(MISSILE_SCALE.len() - 1)];
        if cat == 13 {
            let p = if carried.0 == 0xFF82 { (power as i32 >> 1) + 0x80 } else { power as i32 };
            scale_v(((p << 7) / 255 + 1) >> 1, base).max(8)
        } else {
            base
        }
    } else {
        64
    };
    // Sub and flips from the flight direction against the view.
    let mdir = cx.ex.missile_dirs.get(&(t.0 & 0x3FFF)).copied().unwrap_or((cx.dir + 2) & 3);
    let side = CELLS[c].0;
    let odd = (cx_cell_xy(cx, c).0 + cx_cell_xy(cx, c).1) & 1 != 0;
    let mut flip = 0u8;
    let sub;
    if kind == 3 {
        sub = 8;
    } else if mdir & 1 == cx.dir & 1 {
        // Flying along the line of sight.
        if kind == 0 {
            if odd {
                flip = 2;
                sub = if quad > 1 { 9 } else { 8 };
            } else {
                sub = if quad < 2 { 9 } else { 8 };
            }
        } else if kind == 2 || (kind == 1 && mdir != cx.dir) {
            sub = 8;
        } else {
            sub = 10;
        }
        if side < 0 || (side == 0 && quad != 1 && quad != 2) {
            flip |= 1;
        }
        if quad & 1 != 0 && cat == 13 {
            flip |= 2;
        }
    } else {
        // Flying across the view.
        sub = 12;
        if kind == 0 {
            if quad == 0 || quad == 3 {
                flip = 1;
            }
            if odd {
                flip |= 2;
            } else {
                flip ^= 1;
            }
        } else if (cx.dir + 1) & 3 == mdir {
            flip = 1;
        }
    }
    let fmask = if cat == 13 { attr(a, 13, idx, 1) as u8 } else { 3 };
    let key = key_attr(a, cat, idx, Some(10), true);
    let r = Req { flip: flip & fmask, xs: sc, ys: sc, yoff: -92, depth: Some(depth), key, ..Req::new(cat, idx, sub, rid) };
    draw(a, buf, cx, r);
}

/// Map position of view cell `c`.
fn cx_cell_xy(cx: &Ctx, c: usize) -> (i32, i32) {
    let (lat, fwd) = CELLS[c];
    let d = cx.dir as usize;
    (cx.px + DX[d] * fwd + DX[(d + 1) & 3] * lat, cx.py + DY[d] * fwd + DY[(d + 1) & 3] * lat)
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

/// Rain overlay (0x4E79C): the set's rain image tiled over the viewport with
/// a fresh random offset each frame, mirrored when the wind blows from the
/// left. Tentative: the original's blit (0x13EE3) arguments are only partly
/// decoded, and it draws its offsets from the game RNG, while the remake
/// uses the visual random source so rendering never changes the game.
fn draw_rain(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, set: u8, ex: &ViewExtras) {
    let Some((sub, mirror)) = ex.weather.rain else { return };
    let Some(s) = a.sprite(23, set, sub) else { return };
    if s.w == 0 || s.h == 0 {
        return;
    }
    let ox = (cx.rand() & 0xFF) as usize % s.w;
    let oy = (cx.rand() & 0x1F) as usize % s.h;
    for y in 0..buf.h {
        let sy = (y + oy) % s.h;
        for x in 0..buf.w {
            let mut sx = (x + ox) % s.w;
            if mirror {
                sx = s.w - 1 - sx;
            }
            let v = s.px[sy * s.w + sx];
            if v == 0 {
                continue;
            }
            buf.px[y * buf.w + x] = match &s.cmap {
                Some(m) => m[(v & 15) as usize],
                None => v,
            };
        }
    }
}
