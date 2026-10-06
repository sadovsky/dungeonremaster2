//! The drawn-things hit table (SKULL.EXE 0x7F2EC, docs/04 "Hit table").
//!
//! While the view is drawn, clickable things record the screen rectangle
//! they were drawn into. A click in the viewport is tested against the
//! records in order (0x22A68), so the first matching record wins.

/// What a hit record stands for (record byte +11 in the original).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    /// Kind 1: the floor of a near cell (drop target).
    Floor,
    /// Kind 2: an item lying on the floor (take target).
    FloorItem,
    /// Kind 3: an item shown in the wall alcove ahead (take or place).
    AlcoveItem,
    /// Kind 4: a door button.
    DoorButton,
    /// Kind 6: a clickable wall ornament (forwarded to the wall-click routine).
    WallOrnament,
}

impl HitKind {
    /// The original's kind byte.
    pub fn code(self) -> u8 {
        match self {
            HitKind::Floor => 1,
            HitKind::FloorItem => 2,
            HitKind::AlcoveItem => 3,
            HitKind::DoorButton => 4,
            HitKind::WallOrnament => 6,
        }
    }
}

/// One record: viewport-relative rectangle, the thing drawn (None for
/// records that are not about one thing), the view cell and the kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub thing: Option<u16>,
    pub cell: u8,
    pub kind: HitKind,
}

impl Hit {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    /// Grow this record to cover another rectangle (0x51CC6 merges items
    /// that share a floor quadrant into one record).
    pub fn union(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let (x2, y2) = ((self.x + self.w).max(x + w), (self.y + self.h).max(y + h));
        self.x = self.x.min(x);
        self.y = self.y.min(y);
        self.w = x2 - self.x;
        self.h = y2 - self.y;
    }
}

/// The hit records of one rendered view, in drawing order.
#[derive(Clone, Debug, Default)]
pub struct HitTable {
    pub hits: Vec<Hit>,
    /// Record index per (cell, floor quadrant), so piles merge into one.
    quadrants: std::collections::HashMap<(u8, u8), usize>,
}

impl HitTable {
    /// First record containing the viewport point (x, y), as 0x22A68 scans.
    pub fn at(&self, x: i32, y: i32) -> Option<&Hit> {
        self.hits.iter().find(|h| h.contains(x, y))
    }

    pub(super) fn push(&mut self, h: Hit) {
        self.hits.push(h);
    }

    /// Record an item in a floor quadrant: the first item of a quadrant
    /// starts a record, later ones (and the rest of a pile) extend it.
    pub(super) fn item(&mut self, kind: HitKind, cell: u8, quadrant: u8, thing: u16, r: (i32, i32, i32, i32)) {
        if let Some(&i) = self.quadrants.get(&(cell, quadrant)) {
            // The record keeps the first (bottom) item; later ones only widen it.
            self.hits[i].union(r.0, r.1, r.2, r.3);
            return;
        }
        self.quadrants.insert((cell, quadrant), self.hits.len());
        self.push(Hit { x: r.0, y: r.1, w: r.2, h: r.3, thing: Some(thing), cell, kind });
    }
}
