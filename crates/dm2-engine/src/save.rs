//! Save games in the original SKSAVEn.DAT format (docs/12-savegame.md).
//!
//! A save is: a 42-byte header, the dungeon snapshot (DUNGEON.DAT layout
//! without the checksum), then one bit-packed stream holding the globals,
//! champions, timers and the dynamic object lists. Every bit-packed block
//! goes through a per-byte mask table read from the user's SKULL.EXE at
//! runtime; nothing from the executable is embedded here.
//!
//! The original masks drop some engine state (the top of the RNG state, a
//! timer's last word, parts of the champion records) and the engine keeps
//! state the original doesn't save at all (the move cooldown, pending
//! damage, ...). So after the original stream this writer appends a
//! trailer with the engine's exact state. The DOS game reads a fixed
//! amount and ignores the trailer; without a trailer (a save written by
//! the DOS game) the reader falls back to the original fields.

use std::path::Path;
use std::rc::Rc;

use dm2_formats::dungeon::{Dungeon, Element, ThingRef, ThingType, THING_SIZES};
use dm2_formats::gdat::Key;

use crate::champions::{Champion, MAX_CHAMPIONS, RECORD_SIZE};
use crate::creatures::{self, data::CreatureData};
use crate::data::GameData;
use crate::exe::Exe;
use crate::state::GameState;
use crate::timeline::{Event, Timeline};
use crate::world::PartyPos;

const HEADER_LEN: usize = 42;
const HEADER_MARKER: u16 = 1;
const NAME_LEN: usize = 40;
const GLOBALS_LEN: usize = 60;
const INVENTORY_SLOTS: usize = 30;
const TRAILER_MAGIC: &[u8; 4] = b"DM2R";
const TRAILER_VERSION: u16 = 2;

/// Mask table addresses in the data object (docs/12, "Layout").
mod addr {
    pub const GLOBALS: u32 = 0x75316;
    pub const ALL_BITS: u32 = 0x75312;
    pub const CHAMPION: u32 = 0x75352;
    pub const MISC: u32 = 0x75459;
    pub const TIMER: u32 = 0x7545F;
    pub const TYPE_TABLE: u32 = 0x754D7;
    pub const CREATURE_ALT: u32 = 0x7548B;
    pub const CONTAINER_ALT: u32 = 0x754B3;
    pub const MISSILE_ALT: u32 = 0x754CF;
    pub const MISC_IN_MONEY: u32 = 0x754BF;
}

/// Actuator types whose word 1 >> 7 is saved as an extra 9-bit value.
const ACTUATORS_WITH_VALUE: [u16; 8] = [0x1B, 0x1D, 0x27, 0x2C, 0x2D, 0x30, 0x32, 0x41];
/// Timer types that hold a thing off the map (word at +8).
const TIMERS_HOLDING_THINGS: [u8; 2] = [0x3C, 0x3D];
const EV_CLOUD: u8 = 0x19;
const EV_MISSILES: [u8; 2] = [0x1D, 0x1E];
const EV_CHAMPION_ACTION: u8 = 0x0C;
/// Champion record offset holding the record index of its 0x0C event.
const CHAMPION_EVENT_FIELD: usize = 0x2E;

#[derive(Debug)]
pub enum SaveError {
    Io(std::io::Error),
    /// No SKULL.EXE tables available (GameState::data is None).
    NoTables,
    BadHeader,
    Dungeon(dm2_formats::dungeon::Error),
    Truncated,
    /// Rebuilding the objects ran out of free records of a type.
    NoFreeRecord,
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for SaveError {}

impl From<std::io::Error> for SaveError {
    fn from(e: std::io::Error) -> Self {
        SaveError::Io(e)
    }
}

/// Mask tables from the user's SKULL.EXE.
#[derive(Clone, Debug)]
pub struct SaveTables {
    pub globals: Vec<u8>,
    /// Two 0xFF bytes: the flag, byte and word variable blocks.
    pub all: Vec<u8>,
    pub champion: Vec<u8>,
    pub misc: Vec<u8>,
    pub timer: Vec<u8>,
    /// Per thing type; None where the original saves no record.
    pub types: [Option<Vec<u8>>; 16],
    pub creature_alt: Vec<u8>,
    pub container_alt: Vec<u8>,
    pub missile_alt: Vec<u8>,
    pub misc_in_money: Vec<u8>,
}

impl SaveTables {
    pub fn from_exe(exe: &Exe) -> Option<SaveTables> {
        let get = |a: u32, n: usize| exe.slice(a, n).map(<[u8]>::to_vec);
        let mut types: [Option<Vec<u8>>; 16] = Default::default();
        for (t, slot) in types.iter_mut().enumerate() {
            let p = exe.slice(addr::TYPE_TABLE + 4 * t as u32, 4)?;
            let p = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            // Unrelocated pointers are offsets into the data object.
            if p != 0 && THING_SIZES[t] != 0 {
                *slot = Some(get(exe.data_base() + p, THING_SIZES[t])?);
            }
        }
        Some(SaveTables {
            globals: get(addr::GLOBALS, GLOBALS_LEN)?,
            all: get(addr::ALL_BITS, 2)?,
            champion: get(addr::CHAMPION, RECORD_SIZE)?,
            misc: get(addr::MISC, 6)?,
            timer: get(addr::TIMER, 12)?,
            types,
            creature_alt: get(addr::CREATURE_ALT, THING_SIZES[ThingType::Creature as usize])?,
            container_alt: get(addr::CONTAINER_ALT, THING_SIZES[ThingType::Container as usize])?,
            missile_alt: get(addr::MISSILE_ALT, THING_SIZES[ThingType::Missile as usize])?,
            misc_in_money: get(addr::MISC_IN_MONEY, THING_SIZES[ThingType::Misc as usize])?,
        })
    }
}

/// Save fields the engine doesn't model, carried from a loaded save so they
/// are written back unchanged.
#[derive(Clone, Debug)]
pub struct Legacy {
    /// The 60-byte globals record as last read (known fields are
    /// overwritten from the live state when saving).
    pub globals: [u8; GLOBALS_LEN],
    /// 0x7F100: global flag bytes.
    pub flags: [u8; 8],
    /// 0x7F0C0 / 0x7F108: dungeon script byte and word variables.
    pub byte_vars: [u8; 64],
    pub word_vars: [u16; 64],
    /// 0x7FFEC: six bytes, of which the masks keep bytes 3-4.
    pub misc: [u8; 6],
    /// 0x7FBB4: the leader's hand item (EMPTY when none).
    pub leader_hand: u16,
    /// Save name from the header.
    pub name: String,
}

impl Default for Legacy {
    fn default() -> Self {
        Legacy {
            globals: [0; GLOBALS_LEN],
            flags: [0; 8],
            byte_vars: [0; 64],
            word_vars: [0; 64],
            misc: [0; 6],
            leader_hand: 0xFFFF,
            name: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Bit packer (0x343BE / 0x34536)

/// Writes the bits selected by a mask, highest bit first, into one stream.
#[derive(Default)]
pub struct BitWriter {
    pub out: Vec<u8>,
    acc: u8,
    n: u8,
    /// Optional field labels at bit positions, for comparing streams.
    pub trace: Option<Vec<(usize, String)>>,
}

impl BitWriter {
    /// Label the field that starts here (only when tracing).
    pub fn mark(&mut self, label: impl FnOnce() -> String) {
        if let Some(t) = self.trace.as_mut() {
            let p = self.out.len() * 8 + self.n as usize;
            t.push((p, label()));
        }
    }

    pub fn bit(&mut self, b: bool) {
        self.acc = self.acc << 1 | b as u8;
        self.n += 1;
        if self.n == 8 {
            self.out.push(self.acc);
            self.acc = 0;
            self.n = 0;
        }
    }

    /// One record: every data byte through its mask byte.
    pub fn put(&mut self, data: &[u8], mask: &[u8]) {
        for (&d, &m) in data.iter().zip(mask) {
            for k in (0..8).rev() {
                if m >> k & 1 != 0 {
                    self.bit(d >> k & 1 != 0);
                }
            }
        }
    }

    /// A value of up to 16 bits, masked like a 2-byte little-endian field.
    pub fn put_u16(&mut self, v: u16, mask: u16) {
        self.put(&v.to_le_bytes(), &mask.to_le_bytes());
    }

    /// Flush the last partial byte (0x344C0). The original rotates its
    /// accumulator so the pending bits move to the top; the low bits are 0.
    pub fn flush(&mut self) {
        if self.n != 0 {
            self.out.push(self.acc.rotate_left(8 - self.n as u32));
            self.acc = 0;
            self.n = 0;
        }
    }
}

pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u8,
    n: u8,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0, acc: 0, n: 0 }
    }

    pub fn bit(&mut self) -> Result<bool, SaveError> {
        if self.n == 0 {
            self.acc = *self.data.get(self.pos).ok_or(SaveError::Truncated)?;
            self.pos += 1;
            self.n = 8;
        }
        self.n -= 1;
        Ok(self.acc >> self.n & 1 != 0)
    }

    /// Read one record into `data`; bits outside the mask keep their value.
    pub fn get(&mut self, data: &mut [u8], mask: &[u8]) -> Result<(), SaveError> {
        for (d, &m) in data.iter_mut().zip(mask) {
            for k in (0..8).rev() {
                if m >> k & 1 != 0 {
                    if self.bit()? {
                        *d |= 1 << k;
                    } else {
                        *d &= !(1 << k);
                    }
                }
            }
        }
        Ok(())
    }

    /// Bytes consumed so far, counting a partly read byte.
    pub fn consumed(&self) -> usize {
        self.pos
    }
}

// ---------------------------------------------------------------------------
// Preparing the live state (0x3426D, 0x55FDC)

/// What the original does to the running game before writing a save:
/// deactivate every creature slot (their state goes back into the creature
/// records) and compact the timeline into slots 0..n, fixing the record
/// indices stored by missiles and champions. Creatures on the party's map
/// are activated again by the next tick, exactly as after loading.
pub fn prepare(g: &mut GameState) {
    for si in 0..g.creature_slots.len() {
        if g.creature_slots[si].is_some() {
            creatures::deactivate(g, si);
        }
    }
    g.creature_map_seen = None;
    let moves = g.timeline.compact();
    for c in g.champions.iter_mut() {
        c.set_u16(CHAMPION_EVENT_FIELD, 0xFFFF);
    }
    for (_, new) in moves {
        let Some(ev) = g.timeline.get(new).copied() else { continue };
        if EV_MISSILES.contains(&ev.kind) {
            let m = ThingRef(u16::from_le_bytes([ev.x, ev.y]));
            if m.is_thing() && m.kind() == ThingType::Missile {
                g.dungeon.set_record_word(m, 3, new);
            }
        } else if ev.kind == EV_CHAMPION_ACTION {
            if let Some(c) = g.champions.get_mut(ev.prio as usize) {
                c.set_u16(CHAMPION_EVENT_FIELD, new);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Writing

/// Would the original stop with SYSTEM ERROR 71 when loading this state?
///
/// Model of the original's load path (0x55310): the pool of active creature
/// slots is sized (0x342F9), then game start (0x551D4) runs the map-change
/// routine (0x24629) for the party's map *before* the slots are cleared
/// (0x342A3). That routine's map-entry pass (0x59785) creates the creature
/// of every first-entry spawn text not yet done; placing it activates it,
/// no slot is usable yet, and activation (0x306A8) raises error 0x47.
/// Normal play can't reach such a state (arriving on the map runs the
/// spawns and marks them done), but moving the party by other means can.
/// Returns the offending squares of the party's map.
pub fn original_load_hazard(g: &GameState) -> Vec<(i32, i32)> {
    crate::map_entry::pending_spawns(g, g.party.map)
}

/// Serialise a game. Works on a prepared copy, so `g` is left as it is; use
/// `save` to also prepare the live game the way the original does.
pub fn to_bytes(g: &GameState, name: &str) -> Result<Vec<u8>, SaveError> {
    to_bytes_traced(g, name, false).map(|(b, _, _)| b)
}

/// Like `to_bytes`, also returning where the bit stream starts in the file
/// and (when `trace`) the field labels by bit position within the stream.
pub fn to_bytes_traced(g: &GameState, name: &str, trace: bool) -> Result<(Vec<u8>, usize, Vec<(usize, String)>), SaveError> {
    let mut g = g.clone();
    prepare(&mut g);
    let data = g.data.clone().ok_or(SaveError::NoTables)?;
    let t = SaveTables::from_exe(&data.exe).ok_or(SaveError::NoTables)?;
    let mut out = Vec::new();
    // 1. Header.
    out.extend_from_slice(&HEADER_MARKER.to_le_bytes());
    let mut nm = [0u8; NAME_LEN];
    for (d, s) in nm.iter_mut().zip(name.bytes().take(NAME_LEN - 1)) {
        *d = s;
    }
    out.extend_from_slice(&nm);
    debug_assert_eq!(out.len(), HEADER_LEN);
    // 2. Dungeon snapshot.
    out.extend_from_slice(&g.dungeon.to_snapshot());
    // 3. Bit-packed globals, champions and timers.
    let stream_start = out.len();
    let mut w = BitWriter { trace: trace.then(Vec::new), ..Default::default() };
    let events = g.timeline.slot_events();
    w.mark(|| "globals".into());
    w.put(&globals_record(&g, events.len()), &t.globals);
    w.mark(|| "flags".into());
    for b in g.legacy.flags {
        w.put(&[b], &t.all[..1]);
    }
    w.mark(|| "byte vars".into());
    for b in g.legacy.byte_vars {
        w.put(&[b], &t.all[..1]);
    }
    w.mark(|| "word vars".into());
    for v in g.legacy.word_vars {
        w.put(&v.to_le_bytes(), &t.all[..2]);
    }
    for (i, c) in g.champions.iter().enumerate() {
        w.mark(|| format!("champion {i}"));
        w.put(&c.raw, &t.champion);
    }
    w.mark(|| "misc".into());
    let mut misc = g.legacy.misc;
    misc[3] = g.party_status.counter_0b;
    misc[4] = g.party_status.haste;
    w.put(&misc, &t.misc);
    for (i, (_, ev)) in events.iter().enumerate() {
        w.mark(|| format!("timer {i} type {:#x}", ev.kind));
        w.put(&ev.to_bytes(), &t.timer);
    }
    // 4. Dynamic objects.
    let mut d = Dynamic::new(&g, &t, &data);
    for (i, c) in g.champions.iter().enumerate() {
        for slot in 0..INVENTORY_SLOTS {
            w.mark(|| format!("champion {i} slot {slot}"));
            d.chain(&mut w, c.inventory(slot), false, false);
        }
    }
    w.mark(|| "leader hand".into());
    d.chain(&mut w, g.hand.held, false, false);
    for (i, (_, ev)) in events.iter().enumerate() {
        if TIMERS_HOLDING_THINGS.contains(&ev.kind) {
            w.mark(|| format!("timer {i} held thing"));
            d.chain(&mut w, ev.w8(), false, false);
        }
    }
    d.squares(&mut w);
    w.mark(|| "cross references".into());
    d.cross_references(&mut w);
    w.mark(|| "end".into());
    w.flush();
    out.extend_from_slice(&w.out);
    let labels = w.trace.take().unwrap_or_default();
    // Engine trailer.
    let trailer = engine_trailer(&g, &events);
    out.extend_from_slice(&trailer);
    out.extend_from_slice(&(trailer.len() as u32).to_le_bytes());
    out.extend_from_slice(TRAILER_MAGIC);
    Ok((out, stream_start, labels))
}

/// Save to `path` (SKSAVEn.DAT). The live game is prepared first, as the
/// original does, so continuing it matches loading the file.
pub fn save(g: &mut GameState, path: &Path, name: &str) -> Result<(), SaveError> {
    prepare(g);
    let bytes = to_bytes(g, name)?;
    // Like the original, keep the previous file as .BAK.
    if path.exists() {
        let _ = std::fs::rename(path, path.with_extension("BAK"));
    }
    std::fs::write(path, &bytes)?;
    adopt_loaded(g, &bytes)
}

/// Loading renumbers dynamic things into stream order. So that continuing
/// after a save matches loading it, take the dungeon, champions, hand and
/// timers from the bytes just written.
fn adopt_loaded(g: &mut GameState, bytes: &[u8]) -> Result<(), SaveError> {
    let data = g.data.clone().ok_or(SaveError::NoTables)?;
    let l = from_bytes(bytes, data, g.creature_data.clone())?;
    g.dungeon = l.dungeon;
    g.champions = l.champions;
    g.hand.held = l.hand.held;
    g.timeline = l.timeline;
    Ok(())
}

/// Write a save without touching the live game.
pub fn write(g: &GameState, path: &Path) -> Result<(), SaveError> {
    let name = g.legacy.name.clone();
    std::fs::write(path, to_bytes(g, &name)?)?;
    Ok(())
}

/// The 60-byte globals record (layout from 0x3502B, docs/12).
fn globals_record(g: &GameState, timers: usize) -> [u8; GLOBALS_LEN] {
    let mut r = g.legacy.globals;
    let mut put16 = |o: usize, v: u16| r[o..o + 2].copy_from_slice(&v.to_le_bytes());
    put16(0x08, g.champions.len() as u16);
    put16(0x0A, g.party.x as u16);
    put16(0x0C, g.party.y as u16);
    put16(0x0E, g.party.dir as u16);
    put16(0x10, g.party.map as u16);
    put16(0x12, g.leader.map_or(0xFFFF, |l| l as u16));
    put16(0x14, timers as u16);
    r[0x00..0x04].copy_from_slice(&g.tick.to_le_bytes());
    r[0x04..0x08].copy_from_slice(&g.rng.state.to_le_bytes());
    r[0x16..0x1A].copy_from_slice(&g.party_status.last_attacked.to_le_bytes());
    r[0x1A..0x1E].copy_from_slice(&g.party_status.last_moved.to_le_bytes());
    crate::weather::write_globals(&g.weather, &mut r);
    r
}

/// Creature type flag bit 0 selects the alternative mask (0x1F9A3).
fn creature_alt(g: &GameState, d: Option<&CreatureData>, t: ThingRef) -> bool {
    let Some(d) = d else { return false };
    creatures::type_info(g, d, creatures::creature_type(g, t)).is_some_and(|(i, _)| i.raw[0] & 1 != 0)
}

/// A money container (0x1F2AB): state bits clear and a (20, idx, 5, 0x40)
/// text entry for its item index.
fn is_money_container(g: &GameState, data: &GameData, t: ThingRef) -> bool {
    if g.dungeon.record_word(t, 2).unwrap_or(0) & 6 != 0 {
        return false;
    }
    let db = data.item_db(&g.dungeon);
    db.key(t).is_some_and(|(_, idx)| data.gdat.lookup(Key::new(0x14, idx, 5, 0x40)).is_some())
}

/// A teleporter square that is a map-edge link saves no bits, and its
/// list is written only from the side whose partner map has the higher
/// index (0x34E73 / 0x35B97). Returns (mask, skip_list).
fn square_mask(g: &GameState, map: usize, x: i32, y: i32) -> (u8, bool) {
    let sq = g.dungeon.square(map, x, y);
    match sq.element() {
        Element::Pit => (0x08, false),
        Element::Door => (0x07, false),
        Element::TrickWall => (0x04, false),
        Element::Teleporter => match crate::movement::edge_link(g, map, x, y) {
            Some(link) => (0, link.map < map),
            None => (0x08, false),
        },
        _ => (0, false),
    }
}

/// The thing-chain and square writer (0x3491A, 0x34E73, 0x34797).
struct Dynamic<'a> {
    g: &'a GameState,
    t: &'a SaveTables,
    data: &'a GameData,
    creatures: Option<Rc<CreatureData>>,
    /// The misc-item mask, switched while inside a money container.
    misc_mask: Vec<u8>,
    in_creature: bool,
    in_missile: bool,
    /// Order in which creatures and containers were written, by record index.
    creature_order: Vec<u16>,
    container_order: Vec<u16>,
    creature_count: u16,
    container_count: u16,
    /// Things whose link is written as a cross-reference at the end.
    registered: Vec<ThingRef>,
}

impl<'a> Dynamic<'a> {
    fn new(g: &'a GameState, t: &'a SaveTables, data: &'a GameData) -> Self {
        let n = |k: ThingType| g.dungeon.thing_count(k).max(1);
        Dynamic {
            g,
            t,
            data,
            creatures: g.creature_data.clone(),
            misc_mask: t.types[ThingType::Misc as usize].clone().unwrap_or_default(),
            in_creature: false,
            in_missile: false,
            creature_order: vec![0; n(ThingType::Creature)],
            container_order: vec![0; n(ThingType::Container)],
            creature_count: 0,
            container_count: 0,
            registered: Vec::new(),
        }
    }

    fn word(&self, t: ThingRef, n: usize) -> u16 {
        self.g.dungeon.record_word(t, n).unwrap_or(0xFFFE)
    }

    fn creature_alt(&self, t: ThingRef) -> bool {
        creature_alt(self.g, self.creatures.as_deref(), t)
    }

    fn is_money_container(&self, t: ThingRef) -> bool {
        is_money_container(self.g, self.data, t)
    }

    /// Write a thing chain (0x3491A). `cells`: write each thing's cell;
    /// `whole`: follow `next` links to the end of the list.
    fn chain(&mut self, w: &mut BitWriter, first: u16, cells: bool, whole: bool) {
        let mut r = first;
        loop {
            if r == 0xFFFE || r == 0xFFFF {
                break;
            }
            let t = ThingRef(r);
            let kind = t.kind() as usize;
            w.mark(|| format!("  thing {:?} #{}", t.kind(), t.index()));
            if kind > 3 {
                w.bit(true);
                w.put(&[kind as u8], &[0x0F]);
                if cells && kind != ThingType::Creature as usize {
                    w.put(&[(r >> 14) as u8], &[3]);
                }
            }
            if kind == ThingType::Cloud as usize && self.in_missile {
                // A cloud carried by a missile: only its record index.
                w.put_u16(r, 0x007F);
                return;
            }
            if let Some(base_mask) = self.t.types[kind].clone() {
                self.thing(w, t, base_mask);
            }
            if !whole {
                return;
            }
            r = self.word(t, 0);
        }
        // Terminator: always for a whole list, and for an empty single slot.
        if whole || r == 0xFFFF {
            w.mark(|| "  end of chain".into());
            w.bit(false);
        }
    }

    fn thing(&mut self, w: &mut BitWriter, t: ThingRef, base_mask: Vec<u8>) {
        let Some(rec) = self.g.dungeon.record(t).map(<[u8]>::to_vec) else { return };
        let idx = t.index();
        let mut mask = base_mask;
        let mut alt = false;
        match t.kind() {
            ThingType::Actuator => {
                let w1 = self.word(t, 1);
                if ACTUATORS_WITH_VALUE.contains(&(w1 & 0x7F)) {
                    w.put_u16(w1 >> 7, 0x01FF);
                }
            }
            ThingType::Creature => {
                w.put(&[rec[4]], &[0x7F]);
                if self.creature_alt(t) {
                    mask = self.t.creature_alt.clone();
                    alt = true;
                }
                if let Some(o) = self.creature_order.get_mut(idx) {
                    *o = self.creature_count;
                }
                self.creature_count += 1;
            }
            ThingType::Container => {
                let w2 = self.word(t, 2);
                w.put(&[(w2 >> 1 & 3) as u8], &[3]);
                if w2 & 6 == 2 {
                    mask = self.t.container_alt.clone();
                    alt = true;
                }
                if let Some(o) = self.container_order.get_mut(idx) {
                    *o = self.container_count;
                }
                self.container_count += 1;
            }
            ThingType::Missile if self.in_creature => {
                mask = self.t.missile_alt.clone();
                alt = true;
            }
            ThingType::Misc => mask = self.misc_mask.clone(),
            _ => {}
        }
        w.put(&rec, &mask);
        match t.kind() {
            ThingType::Creature => {
                let outer = self.in_creature;
                self.in_creature = true;
                self.chain(w, self.word(t, 1), alt, true);
                self.in_creature = outer;
            }
            ThingType::Container if alt => {
                let w1 = self.word(t, 1);
                let has = w1 != 0xFFFF && w1 != 0xFFFE;
                w.bit(has);
                if has {
                    self.registered.push(t);
                }
            }
            ThingType::Container => {
                let money = self.is_money_container(t);
                let saved = self.misc_mask.clone();
                if money {
                    self.misc_mask = self.t.misc_in_money.clone();
                }
                self.chain(w, self.word(t, 1), false, true);
                self.misc_mask = saved;
            }
            ThingType::Missile if alt => self.registered.push(t),
            ThingType::Missile => {
                let outer = self.in_missile;
                self.in_missile = true;
                self.chain(w, self.word(t, 1), false, false);
                self.in_missile = outer;
            }
            ThingType::Cloud => {
                // A cloud kept alive by a type-0x19 timer writes that timer's
                // index; timers are in slot order after `prepare`.
                let me = t.0;
                let timer = self.g.timeline.slot_events().into_iter().position(|(_, e)| e.kind == EV_CLOUD && e.w8() == me);
                match timer {
                    Some(i) => {
                        w.bit(true);
                        w.put_u16(i as u16, 0x03FF);
                    }
                    None => w.bit(false),
                }
            }
            _ => {}
        }
    }

    /// Per-square state and thing lists, every map in order (0x34E73).
    fn squares(&mut self, w: &mut BitWriter) {
        let g = self.g;
        for (mi, m) in g.dungeon.maps.iter().enumerate() {
            for x in 0..m.width as i32 {
                for y in 0..m.height as i32 {
                    let sq = g.dungeon.square(mi, x, y);
                    let (mask, skip_list) = square_mask(g, mi, x, y);
                    w.mark(|| format!("square map {mi} ({x},{y}) {:?}", sq.element()));
                    if mask != 0 {
                        w.put(&[sq.0], &[mask]);
                    }
                    if !skip_list {
                        let first = if sq.has_things() { g.dungeon.first_thing(mi, x, y).0 } else { 0xFFFE };
                        self.chain(w, first, true, true);
                    }
                }
            }
        }
    }

    /// Links of registered things as 10-bit indices (0x34797).
    fn cross_references(&mut self, w: &mut BitWriter) {
        for t in std::mem::take(&mut self.registered) {
            let target = (self.word(t, 1) & 0x3FF) as usize;
            let v = match t.kind() {
                ThingType::Container => self.creature_order.get(target),
                ThingType::Missile => self.container_order.get(target),
                _ => continue,
            };
            w.put_u16(v.copied().unwrap_or(0), 0x03FF);
        }
    }
}

// ---------------------------------------------------------------------------
// Engine trailer

struct Out(Vec<u8>);

impl Out {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn pos(&mut self, p: Option<PartyPos>) {
        match p {
            None => self.u8(0),
            Some(p) => {
                self.u8(1);
                self.u16(p.map as u16);
                self.i32(p.x);
                self.i32(p.y);
                self.u8(p.dir);
            }
        }
    }
}

struct In<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> In<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], SaveError> {
        let s = self.b.get(self.pos..self.pos + n).ok_or(SaveError::Truncated)?;
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, SaveError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, SaveError> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, SaveError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&mut self) -> Result<i32, SaveError> {
        Ok(self.u32()? as i32)
    }
    fn pos(&mut self) -> Result<Option<PartyPos>, SaveError> {
        Ok(match self.u8()? {
            0 => None,
            _ => Some(PartyPos { map: self.u16()? as usize, x: self.i32()?, y: self.i32()?, dir: self.u8()? }),
        })
    }
}

/// Engine state the original format can't hold exactly.
fn engine_trailer(g: &GameState, events: &[(u16, Event)]) -> Vec<u8> {
    let mut o = Out(TRAILER_MAGIC.to_vec());
    o.u16(TRAILER_VERSION);
    o.u32(g.tick);
    o.u32(g.rng.state);
    o.pos(Some(g.party));
    o.pos(g.pending_map);
    o.u32(g.move_ready);
    o.u16(g.leader.map_or(0xFFFF, |l| l as u16));
    o.u16(g.light as u16);
    o.u16(g.magic_counter);
    o.u8(g.game_over as u8);
    o.u8(g.champions.len() as u8);
    for c in &g.champions {
        o.0.extend_from_slice(&c.raw);
    }
    let p = &g.party_status;
    o.u8(p.asleep as u8);
    o.u8(p.invulnerable as u8);
    o.u16(p.recruiting.map_or(0xFFFF, |r| r as u16));
    o.u32(p.last_attacked);
    o.u32(p.last_moved);
    o.u16(p.regen_counter);
    for i in 0..MAX_CHAMPIONS {
        o.u16(p.pending_damage[i] as u16);
        o.u16(p.pending_wounds[i]);
        o.0.extend_from_slice(&p.level_ups[i]);
    }
    o.u8(p.haste);
    o.u8(p.counter_0b);
    o.u16(g.timeline.capacity() as u16);
    o.u16(events.len() as u16);
    for (_, e) in events {
        o.0.extend_from_slice(&e.to_bytes());
    }
    o.u16(g.hand.held);
    // Optional tail: outdoor weather and clock (absent in older trailers).
    o.0.extend_from_slice(&g.weather.to_bytes());
    o.0
}

fn apply_trailer(g: &mut GameState, b: &[u8]) -> Result<(), SaveError> {
    let mut i = In { b, pos: 0 };
    if i.take(4)? != TRAILER_MAGIC || i.u16()? != TRAILER_VERSION {
        return Err(SaveError::BadHeader);
    }
    g.tick = i.u32()?;
    g.rng.state = i.u32()?;
    g.party = i.pos()?.ok_or(SaveError::BadHeader)?;
    g.pending_map = i.pos()?;
    g.move_ready = i.u32()?;
    g.leader = Some(i.u16()?).filter(|&l| l != 0xFFFF).map(usize::from);
    g.light = i.u16()? as i16;
    g.magic_counter = i.u16()?;
    g.game_over = i.u8()? != 0;
    let n = i.u8()? as usize;
    g.champions.clear();
    for _ in 0..n {
        let mut c = Champion::default();
        c.raw.copy_from_slice(i.take(RECORD_SIZE)?);
        g.champions.push(c);
    }
    let p = &mut g.party_status;
    p.asleep = i.u8()? != 0;
    p.invulnerable = i.u8()? != 0;
    p.recruiting = Some(i.u16()?).filter(|&r| r != 0xFFFF).map(usize::from);
    p.last_attacked = i.u32()?;
    p.last_moved = i.u32()?;
    p.regen_counter = i.u16()?;
    for k in 0..MAX_CHAMPIONS {
        p.pending_damage[k] = i.u16()? as i16;
        p.pending_wounds[k] = i.u16()?;
        p.level_ups[k].copy_from_slice(i.take(4)?);
    }
    p.haste = i.u8()?;
    p.counter_0b = i.u8()?;
    let cap = i.u16()? as usize;
    let n = i.u16()? as usize;
    let mut events = Vec::with_capacity(n);
    for _ in 0..n {
        let mut e = [0u8; 12];
        e.copy_from_slice(i.take(12)?);
        events.push(Event::from_bytes(&e));
    }
    g.timeline = Timeline::from_slots(cap, events);
    g.hand.held = i.u16()?;
    if let Some(w) = i.take(crate::weather::SAVE_BYTES).ok().and_then(crate::weather::Weather::from_bytes) {
        g.weather = w;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Reading

/// Rebuild a game from save bytes. `creatures` enables creature AI, as
/// for a new game.
pub fn from_bytes(b: &[u8], data: Rc<GameData>, creatures: Option<Rc<CreatureData>>) -> Result<GameState, SaveError> {
    let t = SaveTables::from_exe(&data.exe).ok_or(SaveError::NoTables)?;
    if b.len() < HEADER_LEN || u16::from_le_bytes([b[0], b[1]]) != HEADER_MARKER {
        return Err(SaveError::BadHeader);
    }
    let name_bytes = &b[2..HEADER_LEN];
    let name_end = name_bytes.iter().position(|&c| c == 0).unwrap_or(NAME_LEN);
    let (dungeon, used) = Dungeon::parse_snapshot(&b[HEADER_LEN..]).map_err(SaveError::Dungeon)?;
    let stream_start = HEADER_LEN + used;
    // A trailer sits at the very end: payload, u32 length, magic.
    let trailer = trailer_slice(&b[stream_start..]);

    // A blank state around the snapshot (no spares added, no recruiting).
    let mut g = GameState::new_game(&dungeon);
    g.dungeon = dungeon;
    g.attrs = crate::attrs::Attributes::from_gdat(&data.gdat);
    g.data = Some(data);
    if let Some(c) = creatures {
        creatures::set_data(&mut g, c);
    }
    g.legacy.name = String::from_utf8_lossy(&name_bytes[..name_end]).into_owned();

    let header_marker = u16::from_le_bytes([b[0], b[1]]);
    let mut r = BitReader::new(&b[stream_start..]);
    let mut globals = [0u8; GLOBALS_LEN];
    r.get(&mut globals, &t.globals)?;
    g.legacy.globals = globals;
    // Weather and clock state (bytes 0x2A-0x3B); a remake trailer, if
    // present, later replaces it with the exact state.
    crate::weather::read_globals(&mut g.weather, &globals);
    for f in g.legacy.flags.iter_mut() {
        let mut v = [0u8];
        r.get(&mut v, &t.all[..1])?;
        *f = v[0];
    }
    for f in g.legacy.byte_vars.iter_mut() {
        let mut v = [0u8];
        r.get(&mut v, &t.all[..1])?;
        *f = v[0];
    }
    for f in g.legacy.word_vars.iter_mut() {
        let mut v = [0u8; 2];
        r.get(&mut v, &t.all[..2])?;
        *f = u16::from_le_bytes(v);
    }
    let g16 = |o: usize| u16::from_le_bytes([globals[o], globals[o + 1]]);
    let g32 = |o: usize| u32::from_le_bytes([globals[o], globals[o + 1], globals[o + 2], globals[o + 3]]);
    let champions = g16(0x08) as usize;
    g.champions.clear();
    for _ in 0..champions {
        let mut c = Champion::default();
        // Masked-out bytes stay at the blank record's values.
        r.get(&mut c.raw, &t.champion)?;
        g.champions.push(c);
    }
    let mut misc = [0u8; 6];
    r.get(&mut misc, &t.misc)?;
    g.legacy.misc = misc;
    g.party_status.counter_0b = misc[3];
    g.party_status.haste = misc[4];
    let timers = g16(0x14) as usize;
    let mut events = Vec::with_capacity(timers);
    for _ in 0..timers {
        let mut e = [0u8; 12];
        r.get(&mut e, &t.timer)?;
        events.push(Event::from_bytes(&e));
    }
    g.timeline = Timeline::from_slots(crate::state::TIMELINE_CAPACITY, events);
    g.tick = g32(0x00);
    g.rng.state = g32(0x04);
    g.party = PartyPos { map: g16(0x10) as usize, x: g16(0x0A) as i32, y: g16(0x0C) as i32, dir: (g16(0x0E) & 3) as u8 };
    let leader = g16(0x12);
    g.leader = (leader != 0xFFFF && (leader as usize) < g.champions.len()).then_some(leader as usize);
    g.party_status.last_attacked = g32(0x16);
    g.party_status.last_moved = g32(0x1A);
    // Rebuild the dynamic objects from the stream, as the original does.
    rebuild::run(&mut g, &mut r, &t, header_marker)?;

    if let Some(tr) = trailer {
        // The trailer's exact champion records and timers carry the thing
        // numbers from before the rebuild; keep the rebuilt references.
        let inv: Vec<Vec<u16>> = g.champions.iter().map(|c| (0..INVENTORY_SLOTS).map(|s| c.inventory(s)).collect()).collect();
        let held = g.hand.held;
        let rebuilt = g.timeline.slot_events();
        apply_trailer(&mut g, tr)?;
        for (c, slots) in g.champions.iter_mut().zip(&inv) {
            for (s, &v) in slots.iter().enumerate() {
                c.set_inventory(s, v);
            }
        }
        g.hand.held = held;
        let now = g.timeline.slot_events();
        if now.len() == rebuilt.len() {
            for ((slot, e), (_, before)) in now.into_iter().zip(rebuilt) {
                if EV_MISSILES.contains(&e.kind) {
                    g.timeline.modify(slot, |e| {
                        e.x = before.x;
                        e.y = before.y;
                    });
                } else if e.kind == EV_CLOUD || TIMERS_HOLDING_THINGS.contains(&e.kind) {
                    g.timeline.modify(slot, |e| e.set_w8(before.w8()));
                }
            }
        }
    }
    g.creature_map_seen = None;
    // The original recomputes the outdoor flag and hour light on load.
    crate::weather::refresh(&mut g);
    Ok(g)
}

pub fn read(path: &Path, data: Rc<GameData>, creatures: Option<Rc<CreatureData>>) -> Result<GameState, SaveError> {
    from_bytes(&std::fs::read(path)?, data, creatures)
}

/// Load with the original fallback order (0x370D2): SKSAVEn.DAT, then
/// SKSAVEn.BAK.
pub fn load_slot(dir: &Path, slot: u8, data: Rc<GameData>, creatures: Option<Rc<CreatureData>>) -> Result<GameState, SaveError> {
    let dat = slot_path(dir, slot);
    match read(&dat, data.clone(), creatures.clone()) {
        Ok(g) => Ok(g),
        Err(_) => read(&dat.with_extension("BAK"), data, creatures),
    }
}

/// `SKSAVEn.DAT` in `dir`.
pub fn slot_path(dir: &Path, slot: u8) -> std::path::PathBuf {
    dir.join(format!("SKSAVE{}.DAT", slot % 10))
}

fn trailer_slice(stream: &[u8]) -> Option<&[u8]> {
    let n = stream.len();
    if n < 8 || &stream[n - 4..] != TRAILER_MAGIC {
        return None;
    }
    let len = u32::from_le_bytes([stream[n - 8], stream[n - 7], stream[n - 6], stream[n - 5]]) as usize;
    let start = (n - 8).checked_sub(len)?;
    Some(&stream[start..n - 8])
}

mod rebuild;

#[cfg(test)]
mod tests;
