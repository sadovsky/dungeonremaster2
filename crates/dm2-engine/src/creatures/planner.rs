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
    /// Info word +0x0E: the attack and capability mask (0x7F574).
    pub attack_mask: u16,
    /// Info word +0x14 bits 12-15: reach in squares.
    pub range: u16,
    /// Info byte 0 (flag 0x20: non-material).
    pub info0: u8,
    /// AI class flags (word at 0x7507A).
    pub cflags: u32,
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
pub fn satisfies_on(g: &mut GameState, s: &Searcher, goal: &Goal, map: usize, x: i32, y: i32, distance: u8, start_dist: i32) -> bool {
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
        // A way to reach the party from here (0x2C404 with move flags 1 for
        // kind 6 and 0 for kind 7; both evaluate alike when not committing).
        6 | 7 => party_map && path_to_party(g, s, goal.value, map, x, y),
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
pub fn satisfies(g: &mut GameState, s: &Searcher, goal: &Goal, x: i32, y: i32, distance: u8) -> bool {
    let start = manhattan(s.x, s.y, g.party.x, g.party.y);
    satisfies_on(g, s, goal, s.map, x, y, distance, start)
}

/// Per-kind goal flags (0x752EA, read from the user's SKULL.EXE): bit 0 the
/// goal is tested on each square, bit 1 adds a square to the search radius,
/// bit 2 keeps a matched goal active, 0x10 leaves the goal's target alone,
/// 0x20 targets the party's square, 0x40 keeps the other limits on a match.
const GOAL_FLAGS: u32 = 0x752EA;

/// Search state for the planner's priority rules (0x3188A).
struct Priority {
    flags: Vec<u8>,
    limit: Vec<i32>,
    met: Vec<bool>,
    active: usize,
    max_limit: i32,
    best: Option<Found>,
}

impl Priority {
    fn new(g: &GameState, s: &Searcher, goals: &[Goal]) -> Priority {
        let table: Vec<u8> = g
            .creature_data
            .as_ref()
            .and_then(|d| d.bytes_at(GOAL_FLAGS, 0x1C).map(|b| b.to_vec()))
            .unwrap_or_default();
        let party = !g.champions.is_empty();
        let flags = goals
            .iter()
            .map(|gl| {
                let f = table.get(gl.kind as usize).copied().unwrap_or(1);
                // A home goal whose home is the party's square swaps bits 0-1.
                let home_at_party = gl.kind == 3
                    && party
                    && super::home_of(g, s.group)
                        .is_some_and(|h| h.map() == g.party.map && (h.x(), h.y()) == (g.party.x, g.party.y));
                if home_at_party { f ^ 3 } else { f }
            })
            .collect::<Vec<u8>>();
        let limit: Vec<i32> = goals.iter().map(|gl| gl.limit as i32).collect();
        let mut p = Priority { flags, limit, met: vec![false; goals.len()], active: goals.len(), max_limit: 0, best: None };
        p.max_limit = p.radius();
        p
    }

    /// Search radius over the active goals not yet met (goal 0 always counts).
    fn radius(&self) -> i32 {
        (0..self.active)
            .filter(|&k| k == 0 || !self.met[k])
            .map(|k| self.limit[k] + i32::from(self.flags[k] & 2 != 0))
            .max()
            .unwrap_or(0)
    }

    fn testable(&self, i: usize, d: i32) -> bool {
        i < self.active && self.flags[i] & 1 != 0 && d <= self.limit[i]
    }

    /// Goal `i` met at distance `d`: record it, narrow the active goals and
    /// tighten their limits. Returns true when the search is over.
    fn met(&mut self, goals: &[Goal], i: usize, d: i32, at: Found) -> bool {
        self.met[i] = true;
        self.best = Some(at);
        if i == 0 && (self.flags[0] & 4 == 0 || self.limit[0] <= d) {
            return true;
        }
        // Goals just before the match with a negative argument drop out.
        let mut j = i;
        while j > 0 && goals[j - 1].arg < 0 {
            j -= 1;
            if j == 0 {
                return true;
            }
        }
        self.active = if self.flags[i] & 4 != 0 { i + 1 } else { j };
        if self.flags[i] & 0x40 == 0 {
            let mut sum = 0i32;
            for k in (0..j).rev() {
                if goals[k].arg > 0 {
                    sum += goals[k].arg as i32;
                }
                if sum + d < self.limit[k] {
                    self.limit[k] = d + sum;
                }
            }
        }
        if self.flags[..self.active].iter().fold(0u8, |a, &f| a | f) & 1 == 0 {
            return true;
        }
        self.max_limit = self.radius();
        false
    }
}

/// Test the active goals on one square; true when the search is over.
#[allow(clippy::too_many_arguments)]
fn test_square(
    g: &mut GameState,
    s: &Searcher,
    goals: &[Goal],
    pr: &mut Priority,
    start_dist: i32,
    map: usize,
    x: i32,
    y: i32,
    d: u8,
    via: Option<(i32, i32)>,
) -> bool {
    for i in 0..goals.len() {
        if !pr.testable(i, d as i32) {
            continue;
        }
        if satisfies_on(g, s, &goals[i], map, x, y, d, start_dist) {
            // The goal's target: the party's square for kinds flagged 0x20.
            let at = if pr.flags[i] & 0x30 == 0x20 && g.party.map == map {
                Found { goal: i, map, x: g.party.x, y: g.party.y, distance: d, via }
            } else {
                Found { goal: i, map, x, y, distance: d, via }
            };
            return pr.met(goals, i, d as i32, at);
        }
    }
    false
}

/// The planner's search (0x3188A): breadth-first from the creature's square.
/// Goals are ranked by list order, not distance: goal 0 ends the search, while
/// a later goal is kept as the best so far and the search goes on for the
/// goals before it, within limits tightened by the goals' arguments.
pub fn search(g: &mut GameState, s: &Searcher, goals: &[Goal]) -> Option<Found> {
    if goals.is_empty() {
        return None;
    }
    let mut pr = Priority::new(g, s, goals);
    let start_dist = (s.x - g.party.x).abs() + (s.y - g.party.y).abs();
    let inside = |g: &GameState, map: usize, x: i32, y: i32| {
        let m = &g.dungeon.maps[map];
        x >= 0 && y >= 0 && x < m.width as i32 && y < m.height as i32
    };
    let mut seen: HashSet<(usize, i32, i32)> = HashSet::new();
    let mut q: VecDeque<(usize, i32, i32, u8, Option<(i32, i32)>)> = VecDeque::new();
    q.push_back((s.map, s.x, s.y, 0, None));
    seen.insert((s.map, s.x, s.y));
    while let Some((map, x, y, d, via)) = q.pop_front() {
        if d as i32 > pr.max_limit {
            continue;
        }
        if test_square(g, s, goals, &mut pr, start_dist, map, x, y, d, via) {
            return pr.best;
        }
        if d as i32 >= pr.max_limit {
            continue;
        }
        // Stairs lead to the square at the same world position on the
        // adjacent layer (bit 2 of the stairs square: clear = down).
        // An open pit (bit 3 set, not imaginary) leads one layer down.
        let sq = g.dungeon.square(map, x, y);
        let pit = sq.element() == Element::Pit && sq.0 & 8 != 0 && sq.0 & 1 == 0;
        if d > 0 && (sq.element() == Element::Stairs || pit) {
            let delta = if pit || sq.0 & 4 == 0 { 1 } else { -1 };
            if let Some((nm, nx, ny)) = crate::world::layer_map(&g.dungeon, map, delta, x, y) {
                if seen.insert((nm, nx, ny)) {
                    let v = if map == s.map { Some((x, y)) } else { via };
                    q.push_back((nm, nx, ny, d + 1, v));
                }
            }
        }
        for dir in 0..4 {
            let (nx, ny) = (x + DX[dir], y + DY[dir]);
            if !inside(g, map, nx, ny) || !seen.insert((map, nx, ny)) {
                continue;
            }
            // The party's square and other groups are goals, not paths.
            let target_only = party_at(g, map, nx, ny) || creature_at(g, map, nx, ny, s.group).is_some();
            if target_only {
                if test_square(g, s, goals, &mut pr, start_dist, map, nx, ny, d + 1, via) {
                    return pr.best;
                }
                continue;
            }
            if terrain::can_enter(g, map, nx, ny, s.mask, s.size) {
                q.push_back((map, nx, ny, d + 1, via));
            }
        }
    }
    pr.best
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

/// Straight-line sight from (x, y) to (tx, ty) for squares in one row or
/// column (0x2BBAD with the blocking test 0x2B9FC): walking from the target
/// toward (x, y), each square stepped into is tested until the walk is one
/// square from (x, y); neither end square is tested.
fn clear_line(g: &mut GameState, map: usize, x: i32, y: i32, tx: i32, ty: i32) -> bool {
    let (sx, sy) = ((x - tx).signum(), (y - ty).signum());
    let (mut cx, mut cy) = (tx, ty);
    loop {
        cx += sx;
        cy += sy;
        if (cx - x).abs() + (cy - y).abs() < 1 {
            return true;
        }
        if super::ai::blocks_scan(g, map, cx, cy) {
            return false;
        }
        if (cx - x).abs() + (cy - y).abs() < 2 {
            return true;
        }
    }
}

/// The path test (0x2C404) as the planner uses it for goal kinds 6 and 7,
/// without committing an action: can the creature act on the party from
/// (x, y)? `value` is the goal's value word, ANDed into the creature's attack
/// mask for the call. The party must be in line; a reach of 0 needs bits 0-2
/// of the mask, 2 or more needs bits 3-11 and a clear line, and the reach must
/// not exceed the creature's. Bit 2 needs a champion within one square
/// holding an item of the creature's kind set 0x0B; class flag 0x200 drops
/// the ranged bits when a kind-0xE cloud is on the party's square; class flag
/// 0x4000 refuses, three times in four, a door square next to the party
/// other than the creature's own.
fn path_to_party(g: &mut GameState, s: &Searcher, value: u16, map: usize, x: i32, y: i32) -> bool {
    let (tx, ty) = (g.party.x, g.party.y);
    let mut mask = s.attack_mask & value;
    if mask == 0 || (x != tx && y != ty) {
        return false;
    }
    let d = manhattan(x, y, tx, ty);
    if d > 1 {
        mask &= 0xFF8;
        if mask == 0 {
            return false;
        }
    } else if d == 0 {
        mask &= 7;
        if mask == 0 {
            return false;
        }
    }
    if (s.range as i32) < d {
        return false;
    }
    if d == 0 {
        // Standing on the party's square: refused if any neighbour can be
        // entered (0x2D792 in mode 0; approximated by the terrain test).
        for dir in 0..4 {
            if terrain::can_enter(g, map, x + DX[dir], y + DY[dir], s.mask, s.size) {
                return false;
            }
        }
    }
    if d > 1 && !clear_line(g, map, x, y, tx, ty) {
        return false;
    }
    if mask & 4 != 0 {
        let near = d < 2;
        let dir = super::ai::direction_toward(tx, ty, x, y);
        let mut found = false;
        for start in 0..4u8 {
            let Some(c) = (if near { crate::movement::champion_toward(g, dir, start) } else { None }) else { continue };
            let holds = [1usize, 0].iter().any(|&slot| {
                let item = g.champions[c].inventory(slot);
                item != crate::champions::EMPTY && super::kinds::matches(g, s.group, ThingRef(item), 0x0B)
            });
            if holds && (!found || g.rng.bit() != 0) {
                found = true;
            }
        }
        if !found {
            mask &= !4;
            if mask == 0 {
                return false;
            }
        }
    }
    if s.cflags & 0x200 != 0 {
        let cloud = g.dungeon.things_at(map, tx, ty).into_iter().any(|t| {
            t.kind() == ThingType::Cloud && g.dungeon.record_word(t, 1).unwrap_or(0) & 0x7F == 0x0E
        });
        if cloud {
            mask &= 7;
            if mask == 0 {
                return false;
            }
        }
    }
    if s.cflags & 0x4000 != 0 && s.info0 & 0x20 == 0 && (map, x, y) != (s.map, s.x, s.y)
        && g.dungeon.square(map, x, y).element() == Element::Door && d < 2 && g.rng.rand4() != 0
    {
        return false;
    }
    true
}
