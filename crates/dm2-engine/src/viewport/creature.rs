//! Creatures in the view (SKULL.EXE 0x52BF6, 0x52518, 0x51203, 0x50DEE).
//!
//! Each creature is assigned a point on a 21×17 grid of sub-squares that
//! covers the visible cells. The contents pass of the cell that owns a
//! point draws the creatures held there, once, so creatures are layered
//! correctly against the items and missiles of neighbouring sub-squares.

use std::collections::HashMap;

use dm2_formats::dungeon::ThingRef;
use dm2_formats::gdat::Key;

use super::{
    draw, key_attr, rotate_slot, scale_v, Cell, CreatureDraw, Ctx, Req, CELLS, DEPTH, DEPTH_SCALE, HAS_CONTENTS, LUNGE_SCALE, LUNGE_SLOT,
    NUDGE,
};
use crate::assets::Assets;
use crate::gfx::Bitmap;

/// Grid point of sub-square `slot` of view cell `c` (tables 0x75DDF and
/// 0x75DFF): lateral 8 + 4·side + column, depth 4 + 4·forward − row.
/// Neighbouring cells share their edge columns and rows.
pub(super) fn grid_point(c: usize, slot: u8) -> (i32, i32) {
    let (lat, fwd) = CELLS[c];
    (8 + 4 * lat + (slot % 5) as i32, 4 + 4 * fwd - (slot / 5) as i32)
}

/// Which cell's pass may draw at each grid point (0x52BF6): each cell owns
/// 5 lateral × 4 depth points from (8 + 4·side, 4·forward); later cells
/// overwrite shared columns.
fn owner_grid() -> HashMap<(i32, i32), usize> {
    let mut g = HashMap::new();
    for c in 0..16 {
        if !HAS_CONTENTS[c] {
            continue;
        }
        let (lat, fwd) = CELLS[c];
        for dl in 0..5 {
            for dd in 0..4 {
                g.insert((8 + 4 * lat + dl, 4 * fwd + dd), c);
            }
        }
    }
    g
}

/// One creature to draw: its own cell and thing.
#[derive(Clone, Copy, Debug)]
pub(super) struct Placed {
    pub cell: usize,
    pub thing: ThingRef,
}

/// The creature plan for one view: owner grid plus holders.
#[derive(Default)]
pub(super) struct Plan {
    owner: HashMap<(i32, i32), usize>,
    held: HashMap<(i32, i32), Vec<Placed>>,
}

impl Plan {
    /// Assign every visible creature to a grid point. A point already
    /// holding a creature pushes the next one a row nearer the party.
    pub(super) fn build(a: &Assets, cx: &Ctx, cells: &[Cell]) -> Plan {
        let mut p = Plan { owner: owner_grid(), held: HashMap::new() };
        for (c, cell) in cells.iter().enumerate().take(16) {
            if !HAS_CONTENTS[c] || (c == 0 && cx.ex.mid_step) {
                continue;
            }
            for &t in &cell.things {
                if t.kind() as u16 != 4 {
                    continue;
                }
                let Some(slot) = slot_of(a, cx, t, c) else { continue };
                let (lat, mut dep) = grid_point(c, slot);
                while dep >= 0 && p.held.contains_key(&(lat, dep)) {
                    dep -= 1;
                }
                if dep >= 0 {
                    p.held.insert((lat, dep), vec![Placed { cell: c, thing: t }]);
                }
            }
        }
        p
    }

    /// Creatures to draw while cell `c` is at sub-square `slot`; each is
    /// handed out once. Wall cells take any point.
    pub(super) fn take(&mut self, c: usize, slot: u8, wall: bool) -> Vec<Placed> {
        let pt = grid_point(c, slot);
        if !wall && self.owner.get(&pt) != Some(&c) {
            return Vec::new();
        }
        self.held.remove(&pt).unwrap_or_default()
    }
}

fn draw_info(cx: &Ctx, t: ThingRef) -> CreatureDraw {
    cx.ex.creatures.get(&(t.0 & 0x3FFF)).copied().unwrap_or_default()
}

fn frame_of(cx: &Ctx, t: ThingRef) -> usize {
    cx.ex.creature_frames.get(&(t.0 & 0x3FFF)).copied().unwrap_or(0) as usize
}

fn descriptor(a: &Assets, ctype: u8, frame: usize) -> Option<[u8; 8]> {
    let d = a.gdat.get(Key::new(15, ctype, 7, 253))?.get(frame * 8..frame * 8 + 8)?;
    let mut out = [0u8; 8];
    out.copy_from_slice(d);
    Some(out)
}

/// The view the creature is seen from: 2 (front) when its type always
/// faces the party, else party facing minus creature facing.
fn view_of(cx: &Ctx, rec: &[u8], info: &CreatureDraw) -> u8 {
    if info.faces_party {
        return 2;
    }
    let facing = ((u16::from_le_bytes([rec[14], rec[15]]) >> 8) & 3) as u8;
    cx.dir.wrapping_sub(facing) & 3
}

/// Sub-square the creature is drawn at in cell `c`.
fn slot_of(a: &Assets, cx: &Ctx, t: ThingRef, c: usize) -> Option<u8> {
    let info = draw_info(cx, t);
    if c == 3 {
        if let Some(step) = info.lunge {
            return LUNGE_SLOT.get(step as usize).copied();
        }
    }
    let rec = cx.dg.record(t)?;
    let view = view_of(cx, rec, &info);
    let alt = info.alt_frame.map(|f| f as usize).unwrap_or_else(|| frame_of(cx, t));
    let desc = descriptor(a, rec[4], alt)?;
    Some(info.slot.unwrap_or_else(|| rotate_slot(desc[4], view)))
}

/// Draw one creature group (0x50DEE) in its own cell `c`.
pub(super) fn draw_creature(a: &mut Assets, buf: &mut Bitmap, cx: &mut Ctx, t: ThingRef, c: usize) {
    let Some(rec) = cx.dg.record(t).map(|r| r.to_vec()) else { return };
    let ctype = rec[4];
    let info = draw_info(cx, t);
    let mut view = view_of(cx, &rec, &info);
    let Some(desc) = descriptor(a, ctype, frame_of(cx, t)) else { return };
    // The alternate descriptor supplies position, scale index and shift.
    let desc2 = info.alt_frame.and_then(|f| descriptor(a, ctype, f as usize)).unwrap_or(desc);
    let depth = DEPTH[c];
    let lunge = if c == 3 { info.lunge.map(|s| (s as usize).min(LUNGE_SLOT.len() - 1)) } else { None };

    // Image for the view, with the documented fallbacks and mirroring.
    let flags = desc[7];
    let mut flip = 0u8;
    let mut mirrored = false;
    let mut sub = desc[view as usize];
    if a.has_image(15, ctype, sub) {
        let f = flags >> ((3 - view) * 2);
        if f & 1 != 0 || (info.position & 0x40 != 0 && f & 2 != 0) {
            flip = 1;
        }
    } else {
        let opp = (view + 2) & 3;
        sub = desc[opp as usize];
        flip = opp & 1;
        mirrored = flip != 0;
        if !a.has_image(15, ctype, sub) {
            sub = desc[2];
            flip = 0;
        }
    }
    if !a.has_image(15, ctype, sub) {
        let before = flip;
        sub = view.wrapping_sub(6);
        flip = 0;
        if !a.has_image(15, ctype, sub) {
            let alt = ((view + 2) & 3).wrapping_sub(6);
            if view & 1 != 0 && a.has_image(15, ctype, alt) {
                sub = alt;
                flip = 1;
            } else {
                sub = 0xFC;
                flip = before;
            }
        }
        if flip != before {
            mirrored = true;
        }
    }

    let (slot, scale) = match lunge {
        // The lunge uses fixed positions and sizes, seen from the front.
        Some(step) => {
            view = 0;
            (LUNGE_SLOT[step], LUNGE_SCALE[step])
        }
        None => {
            let ds = DEPTH_SCALE[depth];
            let per_frame = a
                .gdat
                .get(Key::new(15, ctype, 7, 0xFE))
                .and_then(|tb| tb.get(desc2[5] as usize * 4 + view as usize).copied())
                .unwrap_or(64) as i32;
            (info.slot.unwrap_or_else(|| rotate_slot(desc2[4], view)), scale_v(per_frame, ds))
        }
    };

    // Offsets: the position byte's nudges plus the descriptor's shift
    // (signed byte 6): sideways for front/back views (direction set by
    // mirroring), up/down by 7/64 of it for side views.
    let shift = desc2[6] as i8 as i32;
    let (h, v) = match view {
        1 => (0, -7),
        3 => (0, 7),
        _ => (if mirrored { 64 } else { -64 }, 0),
    };
    let acc = |k: i32| if shift == 0 { 0 } else { ((shift >> 1) + k * shift) >> 6 };
    let xoff = NUDGE[(info.position & 7) as usize] + acc(h);
    let yoff = NUDGE[((info.position >> 3) & 7) as usize] + acc(v);

    let key = key_attr(a, 15, ctype, Some(4), true);
    let r = Req { flip, xs: scale, ys: scale, xoff, yoff, depth: Some(depth), key, ..Req::new(15, ctype, sub, 5000 + 25 * c as u16 + slot as u16) };
    draw(a, buf, cx, r);
}
