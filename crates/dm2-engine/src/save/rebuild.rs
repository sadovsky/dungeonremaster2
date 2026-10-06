//! Rebuilding the dynamic objects when a save is loaded (0x35B97).
//!
//! The original doesn't trust the snapshot's dynamic records. It cuts every
//! square's list at its first dynamic thing (type 4 and up), frees every
//! record of those types, and then reallocates them, lowest free index
//! first, in the order the bit stream lists them: inventories, the leader's
//! hand, things held by timers, then each square's things, with nested
//! possessions, contents and payloads. Timer references to missiles and
//! clouds are repaired as the things are read, and the cross-reference
//! block restores links between creatures, containers and missiles.
//!
//! The effect is that loading renumbers dynamic things into stream order;
//! saving again from that state writes the same bytes as the original does.

use super::*;

const END: u16 = 0xFFFE;
const FREE: u16 = 0xFFFF;

#[derive(Clone, Copy)]
enum Dest {
    Inventory(usize, usize),
    Hand,
    Timer(u16),
    Square(usize, i32, i32),
    /// Append to the chain held in word 1 of a thing (possessions,
    /// contents, a missile's payload).
    List(ThingRef),
}

struct Rebuild<'a, 'b> {
    g: &'a mut GameState,
    t: &'a SaveTables,
    data: Rc<GameData>,
    r: &'a mut BitReader<'b>,
    misc_mask: Vec<u8>,
    in_creature: bool,
    in_missile: bool,
    registered: Vec<ThingRef>,
    hand: u16,
}

/// Read the dynamic part of a save into `g` (whose dungeon came from the
/// snapshot). `header_marker` is the header's first word: the things held
/// by timers are only present when it is non-zero (0x35B0E).
pub(super) fn run(g: &mut GameState, r: &mut BitReader, t: &SaveTables, header_marker: u16) -> Result<(), SaveError> {
    let data = g.data.clone().ok_or(SaveError::NoTables)?;
    for c in g.champions.iter_mut() {
        for s in 0..INVENTORY_SLOTS {
            c.set_inventory(s, END);
        }
    }
    cut_dynamic_tails(g);
    free_dynamic_records(g);
    let misc_mask = t.types[ThingType::Misc as usize].clone().unwrap_or_default();
    let mut b = Rebuild { g, t, data, r, misc_mask, in_creature: false, in_missile: false, registered: Vec::new(), hand: END };
    let n = b.g.champions.len();
    for c in 0..n {
        for s in 0..INVENTORY_SLOTS {
            b.chain(Dest::Inventory(c, s), false, false)?;
        }
    }
    b.chain(Dest::Hand, false, false)?;
    for c in b.g.champions.iter_mut() {
        for s in 0..INVENTORY_SLOTS {
            if c.inventory(s) == END {
                c.set_inventory(s, FREE);
            }
        }
    }
    b.g.hand.held = if b.hand == END { FREE } else { b.hand };
    if header_marker != 0 {
        let held: Vec<u16> = b
            .g
            .timeline
            .slot_events()
            .into_iter()
            .filter(|(_, e)| TIMERS_HOLDING_THINGS.contains(&e.kind))
            .map(|(s, _)| s)
            .collect();
        for s in held {
            b.g.timeline.modify(s, |e| e.set_w8(END));
            b.chain(Dest::Timer(s), false, false)?;
        }
    }
    b.squares()?;
    b.cross_references()?;
    deactivate_all_creatures(b.g);
    Ok(())
}

/// Game-start initialisation after a load (0x551D4 → 0x342A3): no creature
/// holds an active slot, so every creature record's slot byte is 0xFF.
fn deactivate_all_creatures(g: &mut GameState) {
    g.creature_slots.iter_mut().for_each(|s| *s = None);
    let size = THING_SIZES[ThingType::Creature as usize];
    for rec in g.dungeon.things[ThingType::Creature as usize].chunks_exact_mut(size) {
        rec[5] = 0xFF;
    }
}

/// Cut each square's list at its first dynamic thing (types 4 and up).
fn cut_dynamic_tails(g: &mut GameState) {
    for mi in 0..g.dungeon.maps.len() {
        let m = &g.dungeon.maps[mi];
        let (w, h) = (m.width as i32, m.height as i32);
        for x in 0..w {
            for y in 0..h {
                let list = g.dungeon.things_at(mi, x, y);
                if let Some(k) = list.iter().position(|t| t.kind() as usize > 3) {
                    for &t in &list[k..] {
                        g.dungeon.remove_thing(mi, x, y, t);
                    }
                }
            }
        }
    }
}

/// Mark every record of types 4-15 free.
fn free_dynamic_records(g: &mut GameState) {
    for kind in 4..16 {
        let size = THING_SIZES[kind];
        if size == 0 {
            continue;
        }
        for rec in g.dungeon.things[kind].chunks_exact_mut(size) {
            rec[0..2].copy_from_slice(&FREE.to_le_bytes());
        }
    }
}

impl Rebuild<'_, '_> {
    fn bits(&mut self, mask: u16) -> Result<u16, SaveError> {
        let mut v = [0u8; 2];
        self.r.get(&mut v, &mask.to_le_bytes())?;
        Ok(u16::from_le_bytes(v))
    }

    fn word(&self, t: ThingRef, n: usize) -> u16 {
        self.g.dungeon.record_word(t, n).unwrap_or(END)
    }

    /// Allocate the lowest free record of a type (0x1DDD7): zero-filled, with
    /// `next` (and a container's contents) set to the end marker. Misc items
    /// keep their last three records in reserve.
    fn alloc(&mut self, kind: usize, cell: u16) -> Result<ThingRef, SaveError> {
        let size = THING_SIZES[kind];
        let recs = &mut self.g.dungeon.things[kind];
        let n = if size == 0 { 0 } else { recs.len() / size };
        let limit = if kind == ThingType::Misc as usize { n.saturating_sub(3) } else { n };
        let i = (0..limit)
            .find(|&i| u16::from_le_bytes([recs[i * size], recs[i * size + 1]]) == FREE)
            .ok_or(SaveError::NoFreeRecord)?;
        let rec = &mut recs[i * size..(i + 1) * size];
        rec.fill(0);
        rec[0..2].copy_from_slice(&END.to_le_bytes());
        if kind == ThingType::Container as usize {
            rec[2..4].copy_from_slice(&END.to_le_bytes());
        }
        Ok(ThingRef(i as u16 | (kind as u16) << 10 | cell << 14))
    }

    /// Append `t` at a destination (0x1D3DB).
    fn append(&mut self, dest: Dest, t: ThingRef) {
        match dest {
            Dest::Inventory(c, s) => self.g.champions[c].set_inventory(s, t.0),
            Dest::Hand => self.hand = t.0,
            Dest::Timer(s) => {
                self.g.timeline.modify(s, |e| e.set_w8(t.0));
            }
            Dest::Square(m, x, y) => self.g.dungeon.add_thing(m, x, y, t),
            Dest::List(p) => {
                let head = self.word(p, 1);
                if !ThingRef(head).is_thing() {
                    self.g.dungeon.set_record_word(p, 1, t.0);
                    return;
                }
                let mut cur = ThingRef(head);
                loop {
                    let next = ThingRef(self.word(cur, 0));
                    if !next.is_thing() {
                        self.g.dungeon.set_record_word(cur, 0, t.0);
                        return;
                    }
                    cur = next;
                }
            }
        }
    }

    /// Read a thing chain (0x3573A). `cells`: each thing carries its cell;
    /// `whole`: read until a 0 bit, else a single thing.
    fn chain(&mut self, dest: Dest, cells: bool, whole: bool) -> Result<(), SaveError> {
        loop {
            if !self.r.bit()? {
                return Ok(());
            }
            let kind = self.bits(0x0F)? as usize;
            let cell = if cells && kind != ThingType::Creature as usize { self.bits(0x03)? } else { 0 };
            if kind == ThingType::Cloud as usize && self.in_missile {
                // A missile's payload that is a spell code, not a thing.
                let v = self.bits(0x7F)?;
                if let Dest::List(p) = dest {
                    self.g.dungeon.set_record_word(p, 1, v | 0xFF80);
                }
                return Ok(());
            }
            let t = self.alloc(kind, cell)?;
            self.append(dest, t);
            if let Some(base) = self.t.types[kind].clone() {
                self.thing(t, kind, base)?;
            }
            if !whole {
                return Ok(());
            }
        }
    }

    fn thing(&mut self, t: ThingRef, kind: usize, base: Vec<u8>) -> Result<(), SaveError> {
        let mut mask = base;
        let mut alt = false;
        match kind {
            k if k == ThingType::Creature as usize => {
                let ty = self.bits(0x7F)? as u8;
                if let Some(rec) = self.g.dungeon.record_mut(t) {
                    rec[4] = ty;
                }
                if creature_alt(self.g, self.g.creature_data.as_deref(), t) {
                    mask = self.t.creature_alt.clone();
                    alt = true;
                }
            }
            k if k == ThingType::Container as usize => {
                let v = self.bits(0x03)? as u8;
                if let Some(rec) = self.g.dungeon.record_mut(t) {
                    rec[4] = rec[4] & 0xF9 | (v & 3) << 1;
                }
                if self.word(t, 2) & 6 == 2 {
                    mask = self.t.container_alt.clone();
                    alt = true;
                }
            }
            k if k == ThingType::Missile as usize && self.in_creature => {
                mask = self.t.missile_alt.clone();
                alt = true;
            }
            k if k == ThingType::Misc as usize => mask = self.misc_mask.clone(),
            _ => {}
        }
        let mut rec = self.g.dungeon.record(t).map(<[u8]>::to_vec).unwrap_or_default();
        self.r.get(&mut rec, &mask)?;
        if let Some(dst) = self.g.dungeon.record_mut(t) {
            dst.copy_from_slice(&rec);
        }
        match kind {
            k if k == ThingType::Creature as usize => {
                self.in_creature = true;
                self.g.dungeon.set_record_word(t, 1, END);
                self.chain(Dest::List(t), alt, true)?;
                self.in_creature = false;
            }
            k if k == ThingType::Container as usize && alt => {
                if self.r.bit()? {
                    self.registered.push(t);
                } else {
                    self.g.dungeon.set_record_word(t, 1, END);
                }
            }
            k if k == ThingType::Container as usize => {
                if is_money_container(self.g, &self.data, t) {
                    self.misc_mask = self.t.misc_in_money.clone();
                }
                self.g.dungeon.set_record_word(t, 1, END);
                self.chain(Dest::List(t), false, true)?;
                self.misc_mask = self.t.types[ThingType::Misc as usize].clone().unwrap_or_default();
            }
            k if k == ThingType::Missile as usize && alt => self.registered.push(t),
            k if k == ThingType::Missile as usize => {
                // The missile's flight timer points back at it.
                let slot = self.word(t, 3);
                let [lo, hi] = t.0.to_le_bytes();
                self.g.timeline.modify(slot, |e| {
                    e.x = lo;
                    e.y = hi;
                });
                self.g.dungeon.set_record_word(t, 1, END);
                self.in_missile = true;
                self.chain(Dest::List(t), false, false)?;
                self.in_missile = false;
            }
            k if k == ThingType::Cloud as usize => {
                if self.r.bit()? {
                    let slot = self.bits(0x03FF)?;
                    self.g.timeline.modify(slot, |e| e.set_w8(t.0));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Square bits, the static things' fields, then the dynamic list.
    fn squares(&mut self) -> Result<(), SaveError> {
        for mi in 0..self.g.dungeon.maps.len() {
            let (w, h) = (self.g.dungeon.maps[mi].width as i32, self.g.dungeon.maps[mi].height as i32);
            for x in 0..w {
                for y in 0..h {
                    let (mask, skip) = square_mask(self.g, mi, x, y);
                    if mask != 0 {
                        let mut v = [self.g.dungeon.square(mi, x, y).0];
                        self.r.get(&mut v, &[mask])?;
                        self.g.dungeon.set_square(mi, x, y, v[0]);
                    }
                    if skip {
                        continue;
                    }
                    for t in self.g.dungeon.things_at(mi, x, y) {
                        let kind = t.kind() as usize;
                        let Some(mask) = self.t.types[kind].clone() else { continue };
                        if t.kind() == ThingType::Actuator {
                            let w1 = self.word(t, 1);
                            if ACTUATORS_WITH_VALUE.contains(&(w1 & 0x7F)) {
                                let v = self.bits(0x01FF)?;
                                self.g.dungeon.set_record_word(t, 1, w1 & 0x7F | v << 7);
                            }
                        }
                        let mut rec = self.g.dungeon.record(t).map(<[u8]>::to_vec).unwrap_or_default();
                        self.r.get(&mut rec, &mask)?;
                        if let Some(dst) = self.g.dungeon.record_mut(t) {
                            dst.copy_from_slice(&rec);
                        }
                    }
                    self.chain(Dest::Square(mi, x, y), true, true)?;
                }
            }
        }
        Ok(())
    }

    /// Links of registered things (0x3484F): a container's word 1 gets a
    /// creature index, a missile's a container index.
    fn cross_references(&mut self) -> Result<(), SaveError> {
        for t in std::mem::take(&mut self.registered) {
            let v = self.bits(0x03FF)?;
            let link = match t.kind() {
                ThingType::Container => v | (ThingType::Creature as u16) << 10,
                ThingType::Missile => v | (ThingType::Container as u16) << 10,
                _ => continue,
            };
            self.g.dungeon.set_record_word(t, 1, link);
        }
        Ok(())
    }
}
