//! Mutable game state and the per-tick update (docs/05-timeline.md, "Main loop").
//!
//! `GameState` owns everything a save game would hold. Rendering reads it;
//! the frontend feeds it commands. Subsystems live in their own modules and
//! receive `&mut GameState`.

use std::rc::Rc;

use dm2_formats::dungeon::Dungeon;

use crate::champions::{self, Champion, PartyStatus};
use crate::attrs::Attributes;
use crate::data::GameData;
use crate::effects::Effect;
use crate::events;
use crate::movement;
use crate::rng::Rng;
use crate::timeline::{Event, Timeline};
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
    /// An interface command number from the zone or key tables (docs/10),
    /// handled by `hand::dispatch`.
    Ui(u16),
    /// Command 0x50: a click in the 3D view.
    Viewport(crate::hand::ViewRegion),
}

#[derive(Clone)]
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
    /// GRAPHICS.DAT numeric attributes (door strength, ...). Empty unless
    /// loaded with `set_attributes`.
    pub attrs: Attributes,
    /// Requests for the presentation layer (sounds, text) and outcomes for
    /// other systems. Drained by the frontend.
    pub effects: Vec<Effect>,
    /// Shared read-only data (GRAPHICS.DAT, SKULL.EXE tables). Systems that
    /// need item attributes or formula tables do nothing without it.
    pub data: Option<Rc<GameData>>,
    /// Leader champion (0x7F222), None when the party is empty.
    pub leader: Option<usize>,
    /// Party light level adjustments from spells and items (0x412E1).
    pub light: i16,
    /// Duration counter decremented by event 0x47 (0x7FFEE).
    pub magic_counter: u16,
    /// Set when the last champion dies (0x7F24C).
    pub game_over: bool,
    /// Presentation only: the square the party just left and the ticks
    /// left of its in-between walking frame (the original's step counter
    /// 0x7F258, set to half the move time when it exceeds 1). Not saved.
    pub walk: Option<(PartyPos, u16)>,
    /// Outdoor clock and weather (docs/04, event 0x54).
    pub weather: crate::weather::Weather,
    /// Active creature slots (docs/08); None = free.
    pub creature_slots: Vec<Option<crate::creatures::slot::Slot>>,
    /// Creature tables from the user's files; creatures stay inert without them.
    pub creature_data: Option<std::rc::Rc<crate::creatures::data::CreatureData>>,
    /// Map whose creatures were last activated for the party.
    pub creature_map_seen: Option<usize>,
    /// Save-game fields the engine does not model yet (script variables,
    /// unknown globals); kept so a loaded save writes them back unchanged.
    pub legacy: crate::save::Legacy,
    /// The leader's hand, open inventory and action menu (docs/10).
    pub hand: crate::hand::HandState,
    commands: std::collections::VecDeque<Command>,
}

impl GameState {
    /// Fresh game from the original dungeon (new-game path of 0x370D2).
    pub fn new_game(dungeon: &Dungeon) -> GameState {
        let s = &dungeon.start;
        let mut dungeon = dungeon.clone();
        // A new game appends spare thing records and list slots (docs/03).
        dungeon.add_spares();
        GameState {
            party: PartyPos { map: 0, x: s.x as i32, y: s.y as i32, dir: s.facing },
            dungeon,
            // TODO(docs/05 open question): whether a new game keeps seed 0.
            rng: Rng::new(0),
            tick: 0,
            timeline: Timeline::with_capacity(TIMELINE_CAPACITY),
            move_ready: 0,
            pending_map: None,
            champions: Vec::new(),
            party_status: PartyStatus::default(),
            attrs: Attributes::default(),
            effects: Vec::new(),
            data: None,
            leader: None,
            light: 0,
            magic_counter: 0,
            game_over: false,
            walk: None,
            weather: Default::default(),
            creature_slots: Vec::new(),
            creature_data: None,
            creature_map_seen: None,
            legacy: Default::default(),
            hand: Default::default(),
            commands: Default::default(),
        }
    }

    /// Fresh game with the shared data attached, including the starting
    /// champion the original recruits automatically (0x49D46, docs/06
    /// "Starting party").
    pub fn new_game_with(dungeon: &Dungeon, data: Rc<GameData>) -> GameState {
        let mut g = GameState::new_game(dungeon);
        g.attrs = Attributes::from_gdat(&data.gdat);
        g.data = Some(data);
        crate::party::recruit_starting_champion(&mut g);
        crate::weather::new_game(&mut g);
        g
    }

    pub fn set_attributes(&mut self, attrs: Attributes) {
        self.attrs = attrs;
    }

    /// Schedule an event. A full pool drops the event (the original stops
    /// with error 0x2D).
    pub fn schedule(&mut self, ev: Event) -> Option<u16> {
        self.timeline.schedule(ev).ok().flatten()
    }

    /// Queue a player command; like the original, at most 3 are held.
    pub fn push_command(&mut self, c: Command) {
        if self.commands.len() < 3 {
            self.commands.push_back(c);
        }
    }

    /// Run one game tick (0x24691), minus rendering.
    pub fn advance(&mut self) {
        self.walk = self.walk.and_then(|(p, n)| (n > 1).then_some((p, n - 1)));
        if let Some(p) = self.pending_map.take() {
            movement::arrive(self, p);
        }
        while self.timeline.due(self.tick) {
            let Some(ev) = self.timeline.pop() else { break };
            events::dispatch(self, ev);
        }
        crate::weather::tick(self);
        crate::weather::update_storm_flag(self);
        crate::creatures::update(self);
        champions::tick(self);
        while let Some(c) = self.commands.pop_front() {
            self.execute(c);
        }
        crate::apply::apply_effects(self);
        self.tick = self.tick.wrapping_add(1);
    }

    fn execute(&mut self, c: Command) {
        match c {
            Command::TurnLeft => self.party.turn_left(),
            Command::TurnRight => self.party.turn_right(),
            Command::Ui(n) => {
                crate::hand::dispatch(self, n);
            }
            Command::Viewport(r) => {
                crate::hand::viewport_click(self, r);
            }
            Command::Move(m) => {
                if self.tick < self.move_ready {
                    return;
                }
                let from = self.party;
                if movement::party_command(self, m) {
                    let t = champions::party_move_time(&self.champions, &self.party_status, &mut self.rng);
                    self.walk = (t > 1).then_some((from, (t >> 1) as u16));
                    self.move_ready = self.tick + t as u32;
                    self.party_status.last_moved = self.tick;
                }
            }
        }
    }
}
