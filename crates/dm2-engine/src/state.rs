//! Mutable game state and the per-tick update (docs/05-timeline.md, "Main loop").
//!
//! `GameState` owns everything a save game would hold. Rendering reads it;
//! the frontend feeds it commands. Subsystems live in their own modules and
//! receive `&mut GameState`.

use dm2_formats::dungeon::Dungeon;

use crate::champions::{self, Champion, PartyStatus};
use crate::events;
use crate::rng::Rng;
use crate::timeline::Timeline;
use crate::world::PartyPos;

/// Event pool size. The original's capacity comes from a global; this is
/// generous and only matters if the pool runs out.
pub const TIMELINE_CAPACITY: usize = 1024;

/// A player command queued by the frontend (the original keeps up to 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Move(crate::world::Move),
    TurnLeft,
    TurnRight,
}

pub struct GameState {
    pub dungeon: Dungeon,
    pub party: PartyPos,
    pub rng: Rng,
    /// Game tick counter.
    pub tick: u32,
    pub timeline: Timeline,
    /// Tick before which the party may not move again (move cooldown).
    pub move_ready: u32,
    /// Map change requested this tick, applied at the start of the next.
    pub pending_map: Option<PartyPos>,
    /// The party's champions, in recruitment order (at most 4).
    pub champions: Vec<Champion>,
    /// Party-wide flags and counters used by the champion formulas.
    pub party_status: PartyStatus,
    commands: std::collections::VecDeque<Command>,
}

impl GameState {
    /// Fresh game from the original dungeon (new-game path of 0x370D2).
    pub fn new_game(dungeon: &Dungeon) -> GameState {
        let s = &dungeon.start;
        GameState {
            party: PartyPos { map: 0, x: s.x as i32, y: s.y as i32, dir: s.facing },
            dungeon: dungeon.clone(),
            // TODO(docs/05 open question): whether a new game keeps seed 0.
            rng: Rng::new(0),
            tick: 0,
            timeline: Timeline::with_capacity(TIMELINE_CAPACITY),
            move_ready: 0,
            pending_map: None,
            champions: Vec::new(),
            party_status: PartyStatus::default(),
            commands: Default::default(),
        }
    }

    /// Queue a player command; like the original, at most 3 are held.
    pub fn push_command(&mut self, c: Command) {
        if self.commands.len() < 3 {
            self.commands.push_back(c);
        }
    }

    /// Run one game tick (0x24691), minus rendering.
    pub fn advance(&mut self) {
        if let Some(p) = self.pending_map.take() {
            self.party = p;
        }
        while self.timeline.due(self.tick) {
            let Some(ev) = self.timeline.pop() else { break };
            events::dispatch(self, ev);
        }
        // TODO: creature updates (docs/08).
        champions::tick(self);
        while let Some(c) = self.commands.pop_front() {
            self.execute(c);
        }
        self.tick = self.tick.wrapping_add(1);
    }

    fn execute(&mut self, c: Command) {
        match c {
            Command::TurnLeft => self.party.turn_left(),
            Command::TurnRight => self.party.turn_right(),
            Command::Move(m) => {
                if self.tick < self.move_ready {
                    return;
                }
                let before = self.party;
                let mut p = self.party;
                if p.step(&self.dungeon, m) {
                    if p.map != before.map {
                        // Map changes take effect next tick.
                        self.pending_map = Some(p);
                    } else {
                        self.party = p;
                    }
                    let t = champions::party_move_time(&self.champions, &self.party_status, &mut self.rng);
                    self.move_ready = self.tick + t as u32;
                    self.party_status.last_moved = self.tick;
                    crate::events::party_moved(self, before);
                }
            }
        }
    }
}
