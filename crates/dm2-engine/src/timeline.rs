//! The timeline: a priority queue of 12-byte events (docs/05-timeline.md).
//!
//! Records live in a fixed pool chained through a free list (LIFO, like the
//! original), and a binary min-heap of record indices orders them. Because
//! the ordering below is total, pop order matches the original as long as
//! slots are allocated the same way.

/// One event. Field names follow docs/05; many types reuse the bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Event {
    /// Due tick (only the low 24 bits are significant).
    pub tick: u32,
    pub map: u8,
    /// Event type; 0 means a free record.
    pub kind: u8,
    /// Priority or small parameter (+5).
    pub prio: u8,
    pub x: u8,
    pub y: u8,
    /// +8: cell, or the low byte of a thing reference.
    pub b8: u8,
    /// +9: square action (0 set, 1 clear, 2 toggle), or the high byte.
    pub b9: u8,
    /// +10: extra parameter.
    pub w10: u16,
}

impl Event {
    pub fn new(kind: u8, map: u8, tick: u32) -> Event {
        Event { tick: tick & 0xFF_FFFF, map, kind, ..Default::default() }
    }

    /// Bytes 8-9 as a 16-bit value (thing references and packed coordinates).
    pub fn w8(&self) -> u16 {
        u16::from_le_bytes([self.b8, self.b9])
    }

    pub fn set_w8(&mut self, v: u16) {
        [self.b8, self.b9] = v.to_le_bytes();
    }

    /// Serialise in the original 12-byte layout (save games).
    pub fn to_bytes(&self) -> [u8; 12] {
        let t = (self.tick & 0xFF_FFFF) | (self.map as u32) << 24;
        let mut b = [0u8; 12];
        b[0..4].copy_from_slice(&t.to_le_bytes());
        b[4] = self.kind;
        b[5] = self.prio;
        b[6] = self.x;
        b[7] = self.y;
        b[8] = self.b8;
        b[9] = self.b9;
        b[10..12].copy_from_slice(&self.w10.to_le_bytes());
        b
    }

    pub fn from_bytes(b: &[u8; 12]) -> Event {
        let t = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        Event {
            tick: t & 0xFF_FFFF,
            map: (t >> 24) as u8,
            kind: b[4],
            prio: b[5],
            x: b[6],
            y: b[7],
            b8: b[8],
            b9: b[9],
            w10: u16::from_le_bytes([b[10], b[11]]),
        }
    }
}

#[derive(Clone)]
pub struct Timeline {
    slots: Vec<Event>,
    free: Vec<u16>,
    heap: Vec<u16>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TimelineError {
    /// Pool exhausted (the original stops with error 0x2D).
    Full,
}

impl Timeline {
    pub fn with_capacity(cap: usize) -> Timeline {
        // Free list hands out 0, 1, 2, ... first.
        let free = (0..cap as u16).rev().collect();
        Timeline { slots: vec![Event::default(); cap], free, heap: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    pub fn get(&self, slot: u16) -> Option<&Event> {
        self.slots.get(slot as usize).filter(|e| e.kind != 0)
    }

    /// "A runs before B" (0x560B8).
    fn before(&self, a: u16, b: u16) -> bool {
        let (ea, eb) = (&self.slots[a as usize], &self.slots[b as usize]);
        let (ta, tb) = (ea.tick & 0xFF_FFFF, eb.tick & 0xFF_FFFF);
        if ta != tb {
            return ta < tb;
        }
        if ea.kind != eb.kind {
            return ea.kind > eb.kind;
        }
        if ea.prio != eb.prio {
            return ea.prio > eb.prio;
        }
        a < b
    }

    fn sift_up(&mut self, mut i: usize) -> usize {
        while i > 0 {
            let p = (i - 1) / 2;
            if self.before(self.heap[i], self.heap[p]) {
                self.heap.swap(i, p);
                i = p;
            } else {
                break;
            }
        }
        i
    }

    fn sift_down(&mut self, mut i: usize) {
        loop {
            let (l, r) = (2 * i + 1, 2 * i + 2);
            let mut m = i;
            if l < self.heap.len() && self.before(self.heap[l], self.heap[m]) {
                m = l;
            }
            if r < self.heap.len() && self.before(self.heap[r], self.heap[m]) {
                m = r;
            }
            if m == i {
                break;
            }
            self.heap.swap(i, m);
            i = m;
        }
    }

    fn resift(&mut self, pos: usize) {
        let p = self.sift_up(pos);
        self.sift_down(p);
    }

    /// Schedule an event (0x56390). Returns its record index, or None for
    /// type 0 (which the original also refuses).
    pub fn schedule(&mut self, ev: Event) -> Result<Option<u16>, TimelineError> {
        if ev.kind == 0 {
            return Ok(None);
        }
        let slot = self.free.pop().ok_or(TimelineError::Full)?;
        self.slots[slot as usize] = Event { tick: ev.tick & 0xFF_FFFF, ..ev };
        self.heap.push(slot);
        let n = self.heap.len() - 1;
        self.sift_up(n);
        Ok(Some(slot))
    }

    /// Is the earliest event due at `now` (0x5646F)?
    pub fn due(&self, now: u32) -> bool {
        self.heap.first().is_some_and(|&s| self.slots[s as usize].tick <= now & 0xFF_FFFF)
    }

    /// Remove and return the earliest event (0x5643D).
    pub fn pop(&mut self) -> Option<Event> {
        let &slot = self.heap.first()?;
        self.remove_at(0);
        Some(self.release(slot))
    }

    fn release(&mut self, slot: u16) -> Event {
        let ev = std::mem::take(&mut self.slots[slot as usize]);
        self.free.push(slot);
        ev
    }

    fn remove_at(&mut self, pos: usize) {
        let last = self.heap.len() - 1;
        self.heap.swap(pos, last);
        self.heap.pop();
        if pos < self.heap.len() {
            self.resift(pos);
        }
    }

    /// Delete a scheduled record by index (0x562FF).
    pub fn delete(&mut self, slot: u16) -> Option<Event> {
        let pos = self.heap.iter().position(|&s| s == slot)?;
        self.remove_at(pos);
        Some(self.release(slot))
    }

    /// Change a scheduled record in place and re-sort it (0x562CE).
    pub fn modify(&mut self, slot: u16, f: impl FnOnce(&mut Event)) -> bool {
        let Some(pos) = self.heap.iter().position(|&s| s == slot) else { return false };
        f(&mut self.slots[slot as usize]);
        self.slots[slot as usize].tick &= 0xFF_FFFF;
        self.resift(pos);
        true
    }

    /// Pool capacity.
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Move every scheduled event down to slots 0..len, keeping their
    /// relative slot order, rebuild the heap and hand out free slots in
    /// ascending order again (the save preparation 0x55FDC / 0x55DBD /
    /// 0x56030). Returns (old slot, new slot) for every event, so callers
    /// can fix stored record indices.
    pub fn compact(&mut self) -> Vec<(u16, u16)> {
        let mut used: Vec<u16> = self.heap.clone();
        used.sort_unstable();
        let events: Vec<Event> = used.iter().map(|&s| self.slots[s as usize]).collect();
        let moves = used.iter().enumerate().map(|(n, &o)| (o, n as u16)).collect();
        *self = Timeline::from_slots(self.slots.len(), events);
        moves
    }

    /// Build a timeline whose events occupy slots 0..events.len() in order
    /// (a loaded save's timer array).
    pub fn from_slots(cap: usize, events: Vec<Event>) -> Timeline {
        let cap = cap.max(events.len());
        let n = events.len();
        let mut slots = vec![Event::default(); cap];
        for (i, e) in events.into_iter().enumerate() {
            slots[i] = Event { tick: e.tick & 0xFF_FFFF, ..e };
        }
        let mut t = Timeline { slots, free: (n as u16..cap as u16).rev().collect(), heap: (0..n as u16).collect() };
        for i in (0..n / 2).rev() {
            t.sift_down(i);
        }
        t
    }

    /// Scheduled events in slot order (the timer array a save writes after
    /// `compact`).
    pub fn slot_events(&self) -> Vec<(u16, Event)> {
        let mut v: Vec<(u16, Event)> = self.heap.iter().map(|&s| (s, self.slots[s as usize])).collect();
        v.sort_unstable_by_key(|e| e.0);
        v
    }

    /// Scheduled events in heap order (for saving and debugging).
    pub fn iter(&self) -> impl Iterator<Item = (u16, &Event)> {
        self.heap.iter().map(|&s| (s, &self.slots[s as usize]))
    }

    /// Structural self-check (tests and soak runs): heap order holds, every
    /// queued record is live, and each slot is either queued or free, never
    /// both and never neither.
    pub fn check(&self) -> Result<(), String> {
        let mut state = vec![0u8; self.slots.len()];
        for (pos, &s) in self.heap.iter().enumerate() {
            let i = s as usize;
            if i >= self.slots.len() {
                return Err(format!("heap entry {s} out of range"));
            }
            if state[i] != 0 {
                return Err(format!("slot {s} queued twice"));
            }
            state[i] = 1;
            if self.slots[i].kind == 0 {
                return Err(format!("queued slot {s} has type 0"));
            }
            if pos > 0 && self.before(s, self.heap[(pos - 1) / 2]) {
                return Err(format!("heap order broken at position {pos}"));
            }
        }
        for &s in &self.free {
            let i = s as usize;
            if i >= self.slots.len() || state[i] != 0 {
                return Err(format!("free slot {s} also queued or listed twice"));
            }
            state[i] = 2;
        }
        if let Some(i) = state.iter().position(|&v| v == 0) {
            return Err(format!("slot {i} is neither queued nor free (leaked)"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: u8, tick: u32, prio: u8) -> Event {
        Event { prio, ..Event::new(kind, 0, tick) }
    }

    #[test]
    fn ordering_rules() {
        let mut t = Timeline::with_capacity(16);
        t.schedule(ev(1, 10, 0)).unwrap(); // slot 0
        t.schedule(ev(4, 10, 1)).unwrap(); // slot 1: higher type wins
        t.schedule(ev(4, 10, 3)).unwrap(); // slot 2: higher priority wins
        t.schedule(ev(4, 10, 3)).unwrap(); // slot 3: tie -> lower slot first
        t.schedule(ev(9, 5, 0)).unwrap(); // slot 4: earliest tick
        let order: Vec<(u8, u8)> = std::iter::from_fn(|| t.pop()).map(|e| (e.kind, e.prio)).collect();
        assert_eq!(order, vec![(9, 0), (4, 3), (4, 3), (4, 1), (1, 0)]);
    }

    #[test]
    fn delete_modify_and_reuse() {
        let mut t = Timeline::with_capacity(4);
        let a = t.schedule(ev(1, 30, 0)).unwrap().unwrap();
        let b = t.schedule(ev(1, 20, 0)).unwrap().unwrap();
        assert!(t.modify(a, |e| e.tick = 5));
        assert!(t.due(5) && !t.due(4));
        assert_eq!(t.delete(b).unwrap().tick, 20);
        // the freed slot is reused first (LIFO free list)
        assert_eq!(t.schedule(ev(2, 1, 0)).unwrap(), Some(b));
        assert_eq!(t.pop().unwrap().kind, 2);
        assert_eq!(t.pop().unwrap().tick, 5);
        assert!(t.pop().is_none());
    }

    #[test]
    fn full_pool_and_bytes() {
        let mut t = Timeline::with_capacity(1);
        t.schedule(ev(1, 1, 0)).unwrap();
        assert_eq!(t.schedule(ev(1, 1, 0)), Err(TimelineError::Full));
        let e = Event { map: 7, x: 3, y: 4, b8: 2, b9: 1, w10: 0x1234, ..ev(4, 0x123456, 2) };
        assert_eq!(Event::from_bytes(&e.to_bytes()), e);
    }

    #[test]
    fn compact_preserves_pop_order() {
        let mut t = Timeline::with_capacity(16);
        let a = t.schedule(ev(4, 10, 1)).unwrap().unwrap();
        t.schedule(ev(4, 10, 1)).unwrap();
        let c = t.schedule(ev(1, 3, 0)).unwrap().unwrap();
        t.schedule(ev(2, 7, 0)).unwrap();
        t.delete(a);
        t.delete(c);
        let mut u = t.clone();
        let moves = u.compact();
        assert_eq!(moves, vec![(1, 0), (3, 1)]);
        assert_eq!(u.schedule(ev(9, 1, 0)).unwrap(), Some(2));
        u.pop();
        let a: Vec<Event> = std::iter::from_fn(|| t.pop()).collect();
        let b: Vec<Event> = std::iter::from_fn(|| u.pop()).collect();
        assert_eq!(a, b);
    }
}
