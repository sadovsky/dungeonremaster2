//! Light from things around the party: the "light scan" that sets the word
//! at 0x7F970, which the darkness step (0x389C2) adds to the light sum.
//! See docs/04-rendering.md, "Darkness step and light sources".
//!
//! The original runs the planner (0x3188A) in mode 7 with goal type 0x17: a
//! breadth-first search from the party out to the map set's radius
//! (attribute (8, set, 11, 0x6D), at most 8 squares). Every open square
//! reached is checked for floor ornaments and creatures that give light
//! (0x38C46 with flags 3); every wall the search runs into is checked for an
//! ornament on the face it was reached through (flags 4). Each source adds
//! max(2, light - attenuation[distance]) to the total.

use std::collections::VecDeque;

use dm2_formats::dungeon::{Element, ThingRef};
use dm2_formats::gdat::Key;

use crate::creatures::data::CreatureData;
use crate::state::GameState;
use crate::viewport::{DX, DY};

/// Attenuation by distance for ordinary sources (words, index min(d, 5)).
const ATTEN: u32 = 0x75739;
/// Attenuation by distance for daylight-scaled sources (words, index d < 9).
const ATTEN_DAYLIGHT: u32 = 0x75727;
/// Daylight percentage table, indexed by the clamped weather/hour value.
const DAYLIGHT_PERCENT: u32 = 0x7570E;
/// Light attribute of ornaments and creature types.
const ATTR_LIGHT: u8 = 0xF8;
/// Non-zero marks a wall ornament whose light follows the daylight (windows).
const ATTR_DAYLIGHT: u8 = 99;
/// Map set attribute: radius of the light scan.
const ATTR_RADIUS: u8 = 0x6D;

fn word(data: &CreatureData, base: u32, i: usize) -> i32 {
    data.bytes_at(base + 2 * i as u32, 2).map_or(0, |b| i16::from_le_bytes([b[0], b[1]]) as i32)
}

fn thing_word(g: &GameState, t: ThingRef, n: usize) -> u16 {
    g.dungeon.record_word(t, n).unwrap_or(0)
}

/// Wall ornament on the face of (x, y) that looks back towards `from_dir`
/// (the direction the scan was travelling), as the cell summary's slot 2.
fn wall_ornament(g: &GameState, map: usize, x: i32, y: i32, from_dir: u8) -> Option<u8> {
    let lists = g.dungeon.map_lists(map);
    let face = (from_dir + 2) & 3;
    for t in g.dungeon.things_at(map, x, y) {
        if t.cell() != face {
            continue;
        }
        match t.kind() as u16 {
            2 => {
                let w1 = thing_word(g, t, 1);
                if (w1 & 7) >> 1 == 1 && w1 >> 11 != 14 {
                    return Some((w1 >> 3) as u8);
                }
            }
            3 => {
                let nib = (thing_word(g, t, 2) >> 12) as usize;
                if nib != 0 {
                    return lists.wall_ornaments.get(nib - 1).copied();
                }
            }
            _ => {}
        }
    }
    None
}

/// Floor ornament on (x, y) from text or actuator things.
fn floor_ornament(g: &GameState, map: usize, x: i32, y: i32) -> Option<u8> {
    let lists = g.dungeon.map_lists(map);
    let mut orn = None;
    for t in g.dungeon.things_at(map, x, y) {
        match t.kind() as u16 {
            2 => {
                let w1 = thing_word(g, t, 1);
                if w1 & 6 == 2 {
                    orn = Some((w1 >> 3) as u8);
                }
            }
            3 => {
                let nib = (thing_word(g, t, 2) >> 12) as usize;
                if nib != 0 {
                    if let Some(&o) = lists.floor_ornaments.get(nib - 1) {
                        orn = Some(o);
                    }
                }
            }
            _ => {}
        }
    }
    orn
}

/// Bit 15 of the light attribute: the source shines only while its
/// ornament's animation frame is non-zero (the cell summary keeps frame × 10
/// in the slot's high byte, 0x1E908 / 0x1E3DA), so animated torches go dark
/// on frame 0.
fn lit(g: &GameState, data: &CreatureData, cat: u8, orn: u8) -> bool {
    crate::viewport::ornament_frame_at(&data.gdat, cat, orn, g.tick, 0) != 0
}

/// Daylight percentage (0..99) for daylight-scaled sources.
fn daylight_percent(g: &GameState, data: &CreatureData) -> i32 {
    let i = (g.weather.storm as i32 + g.weather.hour_light as i32).clamp(0, 5) as u32;
    data.bytes_at(DAYLIGHT_PERCENT + i, 1).map_or(0, |b| b[0] as i8 as i32)
}

/// One square's contribution (0x38C46). `wall` selects the wall-ornament
/// branch (flags 4); otherwise floor ornaments and creatures (flags 3).
fn square_light(g: &GameState, data: &CreatureData, dist: i32, map: usize, x: i32, y: i32, from_dir: u8, wall: bool) -> i32 {
    let gl = |cat: u8, idx: u8, n: u8| data.gdat.lookup(Key::new(cat, idx, 11, n)).unwrap_or(0);
    let mut plain = 0i32;
    let mut daylight = 0i32;
    if wall {
        if let Some(o) = wall_ornament(g, map, x, y, from_dir) {
            let v = gl(9, o, ATTR_LIGHT);
            if v != 0 {
                let light = (v & 0x7FFF) as i32;
                if gl(9, o, ATTR_DAYLIGHT) != 0 {
                    daylight += light * daylight_percent(g, data) / 100;
                } else if v & 0x8000 == 0 || lit(g, data, 9, o) {
                    plain = light;
                }
            }
        }
    } else {
        if let Some(o) = floor_ornament(g, map, x, y) {
            let v = gl(10, o, ATTR_LIGHT);
            if v != 0 && (v & 0x8000 == 0 || lit(g, data, 10, o)) {
                plain = (v & 0x7FFF) as i32;
            }
        }
        if let Some(c) = crate::creatures::group_at(g, map, x, y) {
            let ty = crate::creatures::creature_type(g, c);
            plain += (gl(15, ty, ATTR_LIGHT) & 0x7FFF) as i32;
        }
    }
    let mut total = 0;
    if daylight != 0 && dist < 9 {
        total += (daylight - word(data, ATTEN_DAYLIGHT, dist as usize)).max(3);
    }
    if plain != 0 {
        total += (plain - word(data, ATTEN, dist.min(5) as usize)).max(2);
    }
    total
}

/// The light-scan total for the party's position (the word at 0x7F970).
pub fn party_light(g: &GameState, data: &CreatureData) -> i32 {
    let map = g.party.map;
    let m = &g.dungeon.maps[map];
    let radius = (data.gdat.lookup(Key::new(8, m.tileset, 11, ATTR_RADIUS)).unwrap_or(0) as i32).min(8);
    if radius <= 0 {
        return 0;
    }
    let (w, h) = (m.width as i32, m.height as i32);
    let idx = |x: i32, y: i32| (x * h + y) as usize;
    let mut seen = vec![false; (w * h).max(1) as usize];
    let mut q = VecDeque::new();
    let (px, py) = (g.party.x, g.party.y);
    if px < 0 || py < 0 || px >= w || py >= h {
        return 0;
    }
    seen[idx(px, py)] = true;
    q.push_back((px, py, 0i32));
    let mut total = 0;
    while let Some((x, y, d)) = q.pop_front() {
        total += square_light(g, data, d, map, x, y, g.party.dir, false);
        if d >= radius {
            continue;
        }
        for dir in 0..4u8 {
            let (nx, ny) = (x + DX[dir as usize], y + DY[dir as usize]);
            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            let i = idx(nx, ny);
            let el = g.dungeon.square(map, nx, ny).element();
            if crate::world::blocks(&g.dungeon, map, nx, ny) {
                // Every open square checks its wall neighbours, so a wall
                // seen from several squares counts once per sighting.
                if matches!(el, Element::Wall | Element::TrickWall) {
                    total += square_light(g, data, d + 1, map, nx, ny, dir, true);
                }
                continue;
            }
            if !seen[i] {
                seen[i] = true;
                q.push_back((nx, ny, d + 1));
            }
        }
    }
    total
}
