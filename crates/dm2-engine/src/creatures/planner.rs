//! The square-search planner (docs/08 "The planner"; 0x3188A).
//!
//! Breadth-first and distance-limited, so the first match is the nearest;
//! ties go to the earlier goal in the list. Simplifications: the search
//! stays on the creature's map (the original also crosses stairs and pits),
//! and only the goal kinds the docs describe are matched.

use std::collections::VecDeque;

use dm2_formats::dungeon::{ThingRef, ThingType};

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    pub goal: usize,
    pub x: i32,
    pub y: i32,
    pub distance: u8,
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

/// Does square (x, y) satisfy goal `goal`?
pub fn satisfies(g: &GameState, s: &Searcher, goal: &Goal, x: i32, y: i32, distance: u8) -> bool {
    match goal.kind {
        0 | 1 => distance == 0,
        2 | 3 => party_at(g, s.map, x, y),
        8 | 9 => g.dungeon.things_at(s.map, x, y).iter().any(|t| {
            matches!(
                t.kind(),
                ThingType::Weapon | ThingType::Clothing | ThingType::Scroll | ThingType::Potion | ThingType::Container | ThingType::Misc
            )
        }),
        0x0F | 0x11 => g.dungeon.things_at(s.map, x, y).iter().any(|t| t.kind() == ThingType::Actuator),
        0x12 => creature_at(g, s.map, x, y, s.group).is_some_and(|c| {
            goal.arg < 0 || g.dungeon.record(c).is_some_and(|r| r[4] as i8 == goal.arg)
        }),
        _ => false,
    }
}

/// Search outward for the nearest square that satisfies any goal.
pub fn search(g: &GameState, s: &Searcher, goals: &[Goal]) -> Option<Found> {
    if goals.is_empty() {
        return None;
    }
    let m = &g.dungeon.maps[s.map];
    let (w, h) = (m.width as i32, m.height as i32);
    let mut seen = vec![false; (w * h).max(1) as usize];
    let max_limit = goals.iter().map(|gl| gl.limit).max().unwrap_or(0);
    let mut q = VecDeque::new();
    q.push_back((s.x, s.y, 0u8));
    if s.x >= 0 && s.y >= 0 && s.x < w && s.y < h {
        seen[(s.x * h + s.y) as usize] = true;
    }
    while let Some((x, y, d)) = q.pop_front() {
        for (i, gl) in goals.iter().enumerate() {
            if d <= gl.limit && satisfies(g, s, gl, x, y, d) {
                return Some(Found { goal: i, x, y, distance: d });
            }
        }
        if d >= max_limit {
            continue;
        }
        for dir in 0..4 {
            let (nx, ny) = (x + DX[dir], y + DY[dir]);
            if nx < 0 || ny < 0 || nx >= w || ny >= h || seen[(nx * h + ny) as usize] {
                continue;
            }
            seen[(nx * h + ny) as usize] = true;
            // The party's square and other groups are goals, not paths.
            let target_only = party_at(g, s.map, nx, ny) || creature_at(g, s.map, nx, ny, s.group).is_some();
            if target_only {
                for (i, gl) in goals.iter().enumerate() {
                    if d < gl.limit && satisfies(g, s, gl, nx, ny, d + 1) {
                        return Some(Found { goal: i, x: nx, y: ny, distance: d + 1 });
                    }
                }
                continue;
            }
            if terrain::can_enter(g, s.map, nx, ny, s.mask, s.size) {
                q.push_back((nx, ny, d + 1));
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
