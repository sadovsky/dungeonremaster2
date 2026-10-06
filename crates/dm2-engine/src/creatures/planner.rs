//! The square-search planner (docs/08 "The planner"; 0x3188A).
//!
//! Breadth-first and distance-limited, so the first match is the nearest;
//! ties go to the earlier goal in the list. Goal types follow the final
//! switch of 0x3188A (docs/08 "Planner goal types").

use std::collections::{HashSet, VecDeque};

use dm2_formats::dungeon::{Element, ThingRef, ThingType};

use crate::state::GameState;
use crate::viewport::{DX, DY};

use super::terrain;

/// Default search radius when a goal gives none (tentative).
pub const DEFAULT_LIMIT: u8 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Goal {
    pub kind: u8,
    pub arg: i8,
    /// Program to run when this goal is chosen.
    pub program: u8,
    /// Distance limit in squares.
    pub limit: u8,
    /// Address of the behaviour's goal data (0 when none).
    pub data: u32,
    /// Spec words +4 and +6: goal mode and value (meaning per goal type).
    pub mode: u16,
    pub value: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    pub goal: usize,
    /// Where the goal was met (possibly on another layer).
    pub map: usize,
    pub x: i32,
    pub y: i32,
    pub distance: u8,
    /// For a goal on another map: the stairs square on the searcher's map
    /// that leads toward it.
    pub via: Option<(i32, i32)>,
}

/// Who is searching: position, terrain mask, door size and own group.
#[derive(Clone, Copy, Debug)]
pub struct Searcher {
    pub map: usize,
    pub x: i32,
    pub y: i32,
    pub mask: u16,
    pub size: u16,
    pub group: ThingRef,
}

fn party_at(g: &GameState, map: usize, x: i32, y: i32) -> bool {
    g.party.map == map && g.party.x == x && g.party.y == y && !g.champions.is_empty()
}

fn creature_at(g: &GameState, map: usize, x: i32, y: i32, not: ThingRef) -> Option<ThingRef> {
    g.dungeon
        .things_at(map, x, y)
        .into_iter()
        .find(|t| t.kind() == ThingType::Creature && t.0 & 0x3FFF != not.0 & 0x3FFF)
}

fn manhattan(ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    (ax - bx).abs() + (ay - by).abs()
}

/// Does square (x, y) on `map` satisfy goal `goal` (final switch of
/// 0x3188A)? `start_dist` is the start square's distance from the party,
/// for the flee goal.
pub fn satisfies_on(g: &GameState, s: &Searcher, goal: &Goal, map: usize, x: i32, y: i32, distance: u8, start_dist: i32) -> bool {
    let party_map = g.party.map == map && !g.champions.is_empty();
    let to_party = manhattan(x, y, g.party.x, g.party.y);
    match goal.kind {
        // The start square itself.
        0 => distance == 0,
        // The creature's post (thing record word +0x0C).
        1 => {
            let p = super::goals::post(g, s.group);
            p.map() == map && (p.x(), p.y()) == (x, y)
        }
        // The party, by mode: 0 its square; 1 its square while it faces one
        // of the directions in the value mask; 2 straight ahead of it within
        // `value` squares; 4 exactly `value` squares away in line with it.
        2 => party_map && match goal.mode {
            0 | 3 => to_party == 0,
            1 => to_party == 0 && goal.value & (1 << g.party.dir) != 0,
            2 => {
                to_party > 0
                    && to_party <= goal.value as i32
                    && (x == g.party.x || y == g.party.y)
                    && super::ai::direction_toward(g.party.x, g.party.y, x, y) == g.party.dir
            }
            4 => to_party == goal.value as i32 && (x == g.party.x || y == g.party.y),
            _ => false,
        },
        // The creature's home square (slot +0x0C).
        3 => super::home_of(g, s.group).is_some_and(|h| h.map() == map && (h.x(), h.y()) == (x, y)),
        // Two squares from the party.
        4 => party_map && to_party == 2,
        // Flee: a square farther from the party than where we stand (the
        // original keeps the farthest square found; tentative).
        5 => party_map && to_party > start_dist + 1,
        // Next to the party, to close in (0x2C404 path test, simplified).
        6 | 7 => party_map && to_party == 1,
        8 | 9 => g.dungeon.things_at(map, x, y).iter().any(|t| {
            matches!(
                t.kind(),
                ThingType::Weapon | ThingType::Clothing | ThingType::Scroll | ThingType::Potion | ThingType::Container | ThingType::Misc
            )
        }),
        0x0F | 0x11 => g.dungeon.things_at(map, x, y).iter().any(|t| t.kind() == ThingType::Actuator),
        // A creature of type `mode` (0xFFFF: any).
        0x12 => creature_at(g, map, x, y, s.group).is_some_and(|c| {
            goal.mode == 0xFFFF || g.dungeon.record(c).is_some_and(|r| r[4] as u16 == goal.mode)
        }),
        _ => false,
    }
}

/// Does square (x, y) on the searcher's map satisfy goal `goal`?
pub fn satisfies(g: &GameState, s: &Searcher, goal: &Goal, x: i32, y: i32, distance: u8) -> bool {
    let start = manhattan(s.x, s.y, g.party.x, g.party.y);
    satisfies_on(g, s, goal, s.map, x, y, distance, start)
}

pub fn search(g: &GameState, s: &Searcher, goals: &[Goal]) -> Option<Found> {
    if goals.is_empty() {
        return None;
    }
    let max_limit = goals.iter().map(|gl| gl.limit).max().unwrap_or(0);
    let start_dist = (s.x - g.party.x).abs() + (s.y - g.party.y).abs();
    let inside = |map: usize, x: i32, y: i32| {
        let m = &g.dungeon.maps[map];
        x >= 0 && y >= 0 && x < m.width as i32 && y < m.height as i32
    };
    let mut seen: HashSet<(usize, i32, i32)> = HashSet::new();
    let mut q: VecDeque<(usize, i32, i32, u8, Option<(i32, i32)>)> = VecDeque::new();
    q.push_back((s.map, s.x, s.y, 0, None));
    seen.insert((s.map, s.x, s.y));
    let found = |i: usize, map: usize, x: i32, y: i32, d: u8, via: Option<(i32, i32)>| Found { goal: i, map, x, y, distance: d, via };
    while let Some((map, x, y, d, via)) = q.pop_front() {
        for (i, gl) in goals.iter().enumerate() {
            if d <= gl.limit && satisfies_on(g, s, gl, map, x, y, d, start_dist) {
                return Some(found(i, map, x, y, d, via));
            }
        }
        if d >= max_limit {
            continue;
        }
        // Stairs lead to the square at the same world position on the
        // adjacent layer (bit 2 of the stairs square: clear = down).
        if d > 0 && g.dungeon.square(map, x, y).element() == Element::Stairs {
            let delta = if g.dungeon.square(map, x, y).0 & 4 == 0 { 1 } else { -1 };
            if let Some((nm, nx, ny)) = crate::world::layer_map(&g.dungeon, map, delta, x, y) {
                if seen.insert((nm, nx, ny)) {
                    let v = if map == s.map { Some((x, y)) } else { via };
                    q.push_back((nm, nx, ny, d + 1, v));
                }
            }
        }
        for dir in 0..4 {
            let (nx, ny) = (x + DX[dir], y + DY[dir]);
            if !inside(map, nx, ny) || !seen.insert((map, nx, ny)) {
                continue;
            }
            // The party's square and other groups are goals, not paths.
            let target_only = party_at(g, map, nx, ny) || creature_at(g, map, nx, ny, s.group).is_some();
            if target_only {
                for (i, gl) in goals.iter().enumerate() {
                    if d < gl.limit && satisfies_on(g, s, gl, map, nx, ny, d + 1, start_dist) {
                        return Some(found(i, map, nx, ny, d + 1, via));
                    }
                }
                continue;
            }
            if terrain::can_enter(g, map, nx, ny, s.mask, s.size) {
                q.push_back((map, nx, ny, d + 1, via));
            }
        }
    }
    None
}

/// First step from (x, y) toward (tx, ty) along a shortest open path, or
/// None if unreachable within `limit`.
pub fn first_step(g: &GameState, s: &Searcher, tx: i32, ty: i32, limit: u8) -> Option<u8> {
    let m = &g.dungeon.maps[s.map];
    let (w, h) = (m.width as i32, m.height as i32);
    let mut prev: Vec<Option<(i32, i32, u8)>> = vec![None; (w * h).max(1) as usize];
    let mut q = VecDeque::new();
    q.push_back((s.x, s.y, 0u8));
    let idx = |x: i32, y: i32| (x * h + y) as usize;
    if s.x < 0 || s.y < 0 || s.x >= w || s.y >= h {
        return None;
    }
    prev[idx(s.x, s.y)] = Some((s.x, s.y, 0));
    while let Some((x, y, d)) = q.pop_front() {
        if (x, y) == (tx, ty) {
            // Walk back to the first step.
            let (mut cx, mut cy) = (x, y);
            loop {
                let (px, py, dir) = prev[idx(cx, cy)]?;
                if (px, py) == (s.x, s.y) {
                    return Some(dir);
                }
                (cx, cy) = (px, py);
            }
        }
        if d >= limit {
            continue;
        }
        for dir in 0..4u8 {
            let (nx, ny) = (x + DX[dir as usize], y + DY[dir as usize]);
            if nx < 0 || ny < 0 || nx >= w || ny >= h || prev[idx(nx, ny)].is_some() {
                continue;
            }
            let goal = (nx, ny) == (tx, ty);
            if goal || terrain::can_enter(g, s.map, nx, ny, s.mask, s.size) && creature_at(g, s.map, nx, ny, s.group).is_none() {
                prev[idx(nx, ny)] = Some((x, y, dir));
                q.push_back((nx, ny, d + 1));
            }
        }
    }
    None
}
