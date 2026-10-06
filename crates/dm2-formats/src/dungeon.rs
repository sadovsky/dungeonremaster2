//! DUNGEON.DAT parser (DM2 PC). Format notes: `docs/03-dungeon-dat.md`.
//!
//! The file is a fixed header, per-map descriptors, a column table, the
//! square object list, packed text, sixteen arrays of fixed-size thing
//! records, raw map data and a 16-bit byte-sum checksum.

use std::fmt;

/// Record size of each thing type (table at 0x71808 in SKULL.EXE).
pub const THING_SIZES: [usize; 16] = [4, 6, 4, 8, 16, 4, 4, 4, 4, 8, 4, 0, 0, 0, 8, 4];
/// Spare records the game appends to each array when starting a new game (0x71818).
pub const THING_SPARES: [usize; 16] = [0, 0, 0, 0, 75, 100, 60, 0, 12, 5, 200, 0, 0, 0, 60, 50];

/// Header size in bytes.
const HEADER_LEN: usize = 44;
/// Size of one map descriptor.
const MAP_DESC_LEN: usize = 16;
/// First word of a compressed dungeon, which this parser does not handle.
const COMPRESSED_SIGNATURE: u16 = 0x8104;

/// Square byte returned for coordinates outside a map (element 7, rock).
pub const OUTSIDE_SQUARE: u8 = 0xE0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThingType {
    Door = 0,
    Teleporter = 1,
    Text = 2,
    Actuator = 3,
    Creature = 4,
    Weapon = 5,
    Clothing = 6,
    Scroll = 7,
    Potion = 8,
    Container = 9,
    Misc = 10,
    Unused11 = 11,
    Unused12 = 12,
    Unused13 = 13,
    Missile = 14,
    Cloud = 15,
}

impl ThingType {
    pub fn from_index(i: u16) -> ThingType {
        use ThingType::*;
        [
            Door, Teleporter, Text, Actuator, Creature, Weapon, Clothing, Scroll, Potion,
            Container, Misc, Unused11, Unused12, Unused13, Missile, Cloud,
        ][(i & 0x0F) as usize]
    }

    pub fn record_size(self) -> usize {
        THING_SIZES[self as usize]
    }
}

/// A 16-bit thing reference: cell (bits 14-15), type (10-13), index (0-9).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThingRef(pub u16);

impl ThingRef {
    /// Terminates a thing list.
    pub const END: ThingRef = ThingRef(0xFFFE);
    /// Unused slot.
    pub const NONE: ThingRef = ThingRef(0xFFFF);

    pub fn is_thing(self) -> bool {
        self.0 < 0xFFFE
    }
    pub fn kind(self) -> ThingType {
        ThingType::from_index(self.0 >> 10)
    }
    pub fn index(self) -> usize {
        (self.0 & 0x03FF) as usize
    }
    pub fn cell(self) -> u8 {
        (self.0 >> 14) as u8
    }
}

impl fmt::Debug for ThingRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ThingRef::END => write!(f, "END"),
            ThingRef::NONE => write!(f, "NONE"),
            r => write!(f, "{:?}#{}@{}", r.kind(), r.index(), r.cell()),
        }
    }
}

/// Square element type (bits 5-7 of a square byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element {
    Wall = 0,
    Floor = 1,
    Pit = 2,
    Stairs = 3,
    Door = 4,
    Teleporter = 5,
    TrickWall = 6,
    Rock = 7,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Square(pub u8);

impl Square {
    pub fn element(self) -> Element {
        use Element::*;
        [Wall, Floor, Pit, Stairs, Door, Teleporter, TrickWall, Rock][(self.0 >> 5) as usize]
    }
    /// Bit 4: the square owns a thing list.
    pub fn has_things(self) -> bool {
        self.0 & 0x10 != 0
    }
    /// Bits 0-3, meaning depends on the element.
    pub fn flags(self) -> u8 {
        self.0 & 0x0F
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapDesc {
    /// Offset of this map's squares within the map data block.
    pub data_offset: u16,
    /// Bytes 2-3: graphics/feature flags. Bit 7 enables `door_type0`, bit 8 `door_type1`.
    pub flags: u16,
    /// Bytes 4-5: more flags (bit 0 and bit 2 tested by map-transition code).
    pub flags2: u16,
    /// Position of this map within its depth layer.
    pub origin_x: u8,
    pub origin_y: u8,
    /// Bits 0-5 of word 8: maps sharing a depth are grouped by the game.
    pub depth: u8,
    pub width: u8,
    pub height: u8,
    pub wall_ornament_count: u8,
    pub floor_ornament_count: u8,
    pub door_ornament_count: u8,
    pub creature_type_count: u8,
    /// Bits 12-15 of word 12; meaning not confirmed (looks like a difficulty level).
    pub difficulty: u8,
    /// Category-8 graphics set used for walls/floor/ceiling.
    pub tileset: u8,
    pub door_type0: Option<u8>,
    pub door_type1: Option<u8>,
    /// Raw words 10, 12 and 14 for fields not yet named.
    pub raw_words: [u16; 3],
}

impl MapDesc {
    fn parse(b: &[u8]) -> MapDesc {
        let w = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let (w8, wa, wc, we) = (w(8), w(10), w(12), w(14));
        let flags = w(2);
        MapDesc {
            data_offset: w(0),
            flags,
            flags2: w(4),
            origin_x: b[6],
            origin_y: b[7],
            depth: (w8 & 0x3F) as u8,
            width: (((w8 >> 6) & 0x1F) + 1) as u8,
            height: ((w8 >> 11) + 1) as u8,
            wall_ornament_count: (wa & 0x0F) as u8,
            floor_ornament_count: ((wa >> 8) & 0x0F) as u8,
            door_ornament_count: (wc & 0x0F) as u8,
            creature_type_count: ((wc >> 4) & 0x0F) as u8,
            difficulty: (wc >> 12) as u8,
            tileset: ((we >> 4) & 0x0F) as u8,
            door_type0: (flags & 0x80 != 0).then_some(((we >> 8) & 0x0F) as u8),
            door_type1: (flags & 0x100 != 0).then_some((we >> 12) as u8),
            raw_words: [wa, wc, we],
        }
    }

    fn square_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    fn list_len(&self) -> usize {
        (self.creature_type_count
            + self.wall_ornament_count
            + self.floor_ornament_count
            + self.door_ornament_count) as usize
    }
}

/// The byte lists stored after a map's squares.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MapLists {
    pub creature_types: Vec<u8>,
    pub wall_ornaments: Vec<u8>,
    pub floor_ornaments: Vec<u8>,
    pub door_ornaments: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartPosition {
    pub x: u8,
    pub y: u8,
    pub facing: u8,
}

#[derive(Debug)]
pub enum Error {
    TooShort { needed: usize, have: usize },
    Compressed,
    BadChecksum { stored: u16, computed: u16 },
    BadLayout(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooShort { needed, have } => {
                write!(f, "dungeon data truncated: need {needed} bytes, have {have}")
            }
            Error::Compressed => write!(f, "compressed dungeon files are not supported"),
            Error::BadChecksum { stored, computed } => {
                write!(f, "checksum mismatch: stored {stored:#06x}, computed {computed:#06x}")
            }
            Error::BadLayout(why) => write!(f, "inconsistent dungeon layout: {why}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone)]
pub struct Dungeon {
    pub seed: u16,
    pub start: StartPosition,
    pub maps: Vec<MapDesc>,
    /// Per column (all maps, in order): index of its first entry in `object_list`.
    pub column_first: Vec<u16>,
    /// First thing of every square that has things, then spare `NONE` slots.
    pub object_list: Vec<ThingRef>,
    /// Packed 5-bit text (three codes per word).
    pub text: Vec<u16>,
    /// Raw records per thing type; each record is `THING_SIZES[type]` bytes.
    pub things: [Vec<u8>; 16],
    pub map_data: Vec<u8>,
    /// First column index of each map in `column_first`.
    map_first_column: Vec<usize>,
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.pos + n;
        if end > self.b.len() {
            return Err(Error::TooShort { needed: end, have: self.b.len() });
        }
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn words(&mut self, n: usize) -> Result<Vec<u16>, Error> {
        Ok(self
            .take(2 * n)?
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect())
    }
}

impl Dungeon {
    pub fn parse(b: &[u8]) -> Result<Dungeon, Error> {
        let mut r = Reader { b, pos: 0 };
        let hdr = r.words(HEADER_LEN / 2)?;
        if hdr[0] == COMPRESSED_SIGNATURE {
            return Err(Error::Compressed);
        }
        let map_data_size = hdr[1] as usize;
        let map_count = (hdr[2] & 0xFF) as usize;
        let text_words = hdr[3] as usize;
        let start = StartPosition {
            x: (hdr[4] & 0x1F) as u8,
            y: ((hdr[4] >> 5) & 0x1F) as u8,
            facing: ((hdr[4] >> 10) & 3) as u8,
        };
        let object_list_words = hdr[5] as usize;
        let counts: Vec<usize> = hdr[6..22].iter().map(|&c| c as usize).collect();

        let maps: Vec<MapDesc> = r
            .take(map_count * MAP_DESC_LEN)?
            .chunks_exact(MAP_DESC_LEN)
            .map(MapDesc::parse)
            .collect();
        let mut map_first_column = Vec::with_capacity(maps.len());
        let mut columns = 0;
        for m in &maps {
            map_first_column.push(columns);
            columns += m.width as usize;
        }
        let column_first = r.words(columns)?;
        let object_list = r.words(object_list_words)?.into_iter().map(ThingRef).collect();
        let text = r.words(text_words)?;
        let mut things: [Vec<u8>; 16] = Default::default();
        for (t, slot) in things.iter_mut().enumerate() {
            *slot = r.take(THING_SIZES[t] * counts[t])?.to_vec();
        }
        let map_data = r.take(map_data_size)?.to_vec();
        let body_end = r.pos;
        let stored = r.words(1)?[0];
        let computed = b[..body_end].iter().fold(0u16, |s, &x| s.wrapping_add(x as u16));
        if stored != computed {
            return Err(Error::BadChecksum { stored, computed });
        }
        for m in &maps {
            let end = m.data_offset as usize + m.square_count() + m.list_len();
            if end > map_data.len() {
                return Err(Error::BadLayout("map squares extend past the map data"));
            }
        }
        Ok(Dungeon {
            seed: hdr[0],
            start,
            maps,
            column_first,
            object_list,
            text,
            things,
            map_data,
            map_first_column,
        })
    }

    pub fn thing_count(&self, t: ThingType) -> usize {
        match t.record_size() {
            0 => 0,
            n => self.things[t as usize].len() / n,
        }
    }

    /// Raw record bytes for a thing.
    pub fn record(&self, r: ThingRef) -> Option<&[u8]> {
        let size = r.kind().record_size();
        let start = r.index() * size;
        self.things[r.kind() as usize].get(start..start + size)
    }

    /// Word `n` (byte offset 2n) of a thing record.
    pub fn record_word(&self, r: ThingRef, n: usize) -> Option<u16> {
        let rec = self.record(r)?;
        rec.get(2 * n..2 * n + 2).map(|w| u16::from_le_bytes([w[0], w[1]]))
    }

    /// Square byte at (x, y); maps are stored column-major.
    pub fn square(&self, map: usize, x: i32, y: i32) -> Square {
        let m = &self.maps[map];
        if x < 0 || y < 0 || x >= m.width as i32 || y >= m.height as i32 {
            return Square(OUTSIDE_SQUARE);
        }
        Square(self.map_data[m.data_offset as usize + x as usize * m.height as usize + y as usize])
    }

    pub fn map_lists(&self, map: usize) -> MapLists {
        let m = &self.maps[map];
        let mut o = m.data_offset as usize + m.square_count();
        let mut next = |n: u8| {
            let v = self.map_data[o..o + n as usize].to_vec();
            o += n as usize;
            v
        };
        MapLists {
            creature_types: next(m.creature_type_count),
            wall_ornaments: next(m.wall_ornament_count),
            floor_ornaments: next(m.floor_ornament_count),
            door_ornaments: next(m.door_ornament_count),
        }
    }

    /// First thing on a square, or `ThingRef::END` if it has none.
    pub fn first_thing(&self, map: usize, x: i32, y: i32) -> ThingRef {
        let m = &self.maps[map];
        if !self.square(map, x, y).has_things() || x < 0 || y < 0 {
            return ThingRef::END;
        }
        let col_start = m.data_offset as usize + x as usize * m.height as usize;
        let before = self.map_data[col_start..col_start + y as usize]
            .iter()
            .filter(|&&b| b & 0x10 != 0)
            .count();
        let idx = self.column_first[self.map_first_column[map] + x as usize] as usize + before;
        self.object_list.get(idx).copied().unwrap_or(ThingRef::END)
    }

    /// All things on a square, following each record's `next` link (word 0).
    pub fn things_at(&self, map: usize, x: i32, y: i32) -> Vec<ThingRef> {
        let mut out = Vec::new();
        let mut r = self.first_thing(map, x, y);
        while r.is_thing() && out.len() < 1024 {
            out.push(r);
            r = match self.record_word(r, 0) {
                Some(n) => ThingRef(n),
                None => break,
            };
        }
        out
    }

    /// Decode a packed text entry starting at word `offset`. Escape codes 29
    /// and 30 select entries from tables inside SKULL.EXE; they are returned
    /// as `{29:n}` / `{30:n}` placeholders.
    pub fn decode_text(&self, offset: usize) -> String {
        let mut s = String::new();
        let mut escape = 0u16;
        for &w in self.text.iter().skip(offset) {
            for code in [(w >> 10) & 0x1F, (w >> 5) & 0x1F, w & 0x1F] {
                if escape != 0 {
                    s.push_str(&format!("{{{escape}:{code}}}"));
                    escape = 0;
                    continue;
                }
                match code {
                    0..=25 => s.push((b'A' + code as u8) as char),
                    26 => s.push(' '),
                    27 => s.push('.'),
                    28 => s.push('\n'),
                    29 | 30 => escape = code,
                    _ => return s,
                }
            }
        }
        s
    }
}

/// Runtime mutation, used by the engine's game state.
impl Dungeon {
    fn square_index(&self, map: usize, x: i32, y: i32) -> Option<usize> {
        let m = &self.maps[map];
        if x < 0 || y < 0 || x >= m.width as i32 || y >= m.height as i32 {
            return None;
        }
        Some(m.data_offset as usize + x as usize * m.height as usize + y as usize)
    }

    /// Overwrite a square byte. The "has things" bit (4) is managed by
    /// `add_thing`/`remove_thing` and is preserved here.
    pub fn set_square(&mut self, map: usize, x: i32, y: i32, value: u8) {
        if let Some(i) = self.square_index(map, x, y) {
            self.map_data[i] = (value & !0x10) | (self.map_data[i] & 0x10);
        }
    }

    pub fn record_mut(&mut self, r: ThingRef) -> Option<&mut [u8]> {
        let size = r.kind().record_size();
        let start = r.index() * size;
        self.things[r.kind() as usize].get_mut(start..start + size)
    }

    pub fn set_record_word(&mut self, r: ThingRef, n: usize, v: u16) {
        if let Some(rec) = self.record_mut(r) {
            if let Some(w) = rec.get_mut(2 * n..2 * n + 2) {
                w.copy_from_slice(&v.to_le_bytes());
            }
        }
    }

    /// Position in `object_list` of a square's first-thing slot, whether or
    /// not the square currently has things.
    fn list_slot(&self, map: usize, x: i32, y: i32) -> usize {
        let m = &self.maps[map];
        let col_start = m.data_offset as usize + x as usize * m.height as usize;
        let before = self.map_data[col_start..col_start + y as usize].iter().filter(|&&b| b & 0x10 != 0).count();
        self.column_first[self.map_first_column[map] + x as usize] as usize + before
    }

    /// Append a thing to the end of a square's list (its `next` is set to END).
    pub fn add_thing(&mut self, map: usize, x: i32, y: i32, t: ThingRef) {
        let Some(sq) = self.square_index(map, x, y) else { return };
        // Strip the cell bits when comparing references to the list.
        self.set_record_word(t, 0, ThingRef::END.0);
        if self.map_data[sq] & 0x10 == 0 {
            let slot = self.list_slot(map, x, y);
            self.object_list.insert(slot, t);
            // Keep the list length fixed by consuming a spare NONE slot at the end.
            if let Some(p) = self.object_list.iter().rposition(|r| *r == ThingRef::NONE) {
                self.object_list.remove(p);
            }
            let col = self.map_first_column[map] + x as usize;
            for c in self.column_first.iter_mut().skip(col + 1) {
                *c += 1;
            }
            self.map_data[sq] |= 0x10;
            return;
        }
        let mut cur = self.first_thing(map, x, y);
        loop {
            let next = ThingRef(self.record_word(cur, 0).unwrap_or(ThingRef::END.0));
            if !next.is_thing() {
                self.set_record_word(cur, 0, t.0);
                return;
            }
            cur = next;
        }
    }

    /// Unlink a thing from a square's list. Returns false if it wasn't there.
    pub fn remove_thing(&mut self, map: usize, x: i32, y: i32, t: ThingRef) -> bool {
        let Some(sq) = self.square_index(map, x, y) else { return false };
        if self.map_data[sq] & 0x10 == 0 {
            return false;
        }
        let same = |a: ThingRef, b: ThingRef| a.0 & 0x3FFF == b.0 & 0x3FFF;
        let first = self.first_thing(map, x, y);
        let after = ThingRef(self.record_word(t, 0).unwrap_or(ThingRef::END.0));
        if same(first, t) {
            let slot = self.list_slot(map, x, y);
            if after.is_thing() {
                self.object_list[slot] = after;
            } else {
                self.object_list.remove(slot);
                self.object_list.push(ThingRef::NONE);
                let col = self.map_first_column[map] + x as usize;
                for c in self.column_first.iter_mut().skip(col + 1) {
                    *c -= 1;
                }
                self.map_data[sq] &= !0x10;
            }
            self.set_record_word(t, 0, ThingRef::END.0);
            return true;
        }
        let mut cur = first;
        while cur.is_thing() {
            let next = ThingRef(self.record_word(cur, 0).unwrap_or(ThingRef::END.0));
            if same(next, t) {
                self.set_record_word(cur, 0, after.0);
                self.set_record_word(t, 0, ThingRef::END.0);
                return true;
            }
            cur = next;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original() -> Option<Vec<u8>> {
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../original/dumast2/DATA/DUNGEON.DAT");
        std::fs::read(p).ok()
    }

    /// One 2x2 map with a door square that holds a door thing.
    fn synthetic() -> Vec<u8> {
        let mut b = Vec::new();
        let words = |b: &mut Vec<u8>, ws: &[u16]| ws.iter().for_each(|w| b.extend(w.to_le_bytes()));
        let mut counts = [0u16; 16];
        counts[0] = 1;
        // seed, map data size, map count, text words, start, object list words, counts
        words(&mut b, &[7, 4, 1, 0, 0x0401 | (1 << 5), 2]);
        words(&mut b, &counts);
        // descriptor: offset 0, width 2, height 2
        let mut desc = [0u8; 16];
        desc[8..10].copy_from_slice(&((1u16 << 6) | (1 << 11)).to_le_bytes());
        b.extend(desc);
        words(&mut b, &[0, 0]); // column table
        words(&mut b, &[0x0000, 0xFFFF]); // object list: door #0, one spare
        words(&mut b, &[0xFFFE, 0x0020]); // door record
        b.extend([0x00, 0x20, 0x90, 0x00]); // squares: wall, floor | door+things, wall
        let sum = b.iter().fold(0u16, |s, &x| s.wrapping_add(x as u16));
        b.extend(sum.to_le_bytes());
        b
    }

    #[test]
    fn parses_synthetic() {
        let d = Dungeon::parse(&synthetic()).unwrap();
        assert_eq!(d.start, StartPosition { x: 1, y: 1, facing: 1 });
        assert_eq!(d.maps[0].width, 2);
        assert_eq!(d.square(0, 1, 0).element(), Element::Door);
        assert_eq!(d.square(0, 0, 1).element(), Element::Floor);
        assert_eq!(d.square(0, 5, 5).element(), Element::Rock);
        assert_eq!(d.things_at(0, 1, 0), vec![ThingRef(0)]);
        assert_eq!(d.things_at(0, 0, 1), vec![]);
    }

    #[test]
    fn rejects_bad_checksum() {
        let mut b = synthetic();
        let n = b.len();
        b[n - 1] ^= 0xFF;
        assert!(matches!(Dungeon::parse(&b), Err(Error::BadChecksum { .. })));
    }

    #[test]
    fn decodes_packed_text() {
        let mut d = Dungeon::parse(&synthetic()).unwrap();
        // "HI" then end: H=7, I=8, 31
        d.text = vec![(7 << 10) | (8 << 5) | 31];
        assert_eq!(d.decode_text(0), "HI");
    }

    #[test]
    fn parses_original_file() {
        let Some(b) = original() else {
            eprintln!("original DUNGEON.DAT not present; skipping");
            return;
        };
        let d = Dungeon::parse(&b).unwrap();
        assert_eq!(d.maps.len(), 44);
        assert_eq!(d.thing_count(ThingType::Door), 53);
        assert_eq!(d.thing_count(ThingType::Creature), 299);
        // Every square flagged with things resolves to a valid list.
        let mut with_things = 0;
        for (m, desc) in d.maps.iter().enumerate() {
            for x in 0..desc.width as i32 {
                for y in 0..desc.height as i32 {
                    if d.square(m, x, y).has_things() {
                        with_things += 1;
                        let list = d.things_at(m, x, y);
                        assert!(!list.is_empty(), "map {m} ({x},{y}) has an empty list");
                        assert!(list.iter().all(|r| d.record(*r).is_some()));
                    }
                }
            }
        }
        let used = d.object_list.iter().filter(|r| r.is_thing()).count();
        assert_eq!(with_things, used);
        // Door squares hold door things.
        for (m, desc) in d.maps.iter().enumerate() {
            for x in 0..desc.width as i32 {
                for y in 0..desc.height as i32 {
                    if d.square(m, x, y).element() == Element::Door {
                        assert!(d.things_at(m, x, y).iter().any(|r| r.kind() == ThingType::Door));
                    }
                }
            }
        }
    }

    #[test]
    fn add_and_remove_things_round_trip() {
        let Some(b) = original() else { return };
        let orig = Dungeon::parse(&b).unwrap();
        let mut d = orig.clone();
        // Move the first thing of some populated square onto an empty floor
        // square and back again; every list in the dungeon must be unchanged.
        let m = 0usize;
        let (sx, sy) = (0..7)
            .flat_map(|x| (0..10).map(move |y| (x, y)))
            .find(|&(x, y)| d.square(m, x, y).has_things())
            .unwrap();
        let t = d.first_thing(m, sx, sy);
        let empty = (0..7).flat_map(|x| (0..10).map(move |y| (x, y)))
            .find(|&(x, y)| d.square(0, x, y).element() == Element::Floor && !d.square(0, x, y).has_things())
            .unwrap();
        let before_src = d.things_at(m, sx, sy);
        assert!(d.remove_thing(m, sx, sy, t));
        assert_eq!(d.things_at(m, sx, sy), before_src[1..].to_vec());
        d.add_thing(m, empty.0, empty.1, t);
        assert_eq!(d.things_at(m, empty.0, empty.1), vec![t]);
        assert!(d.remove_thing(m, empty.0, empty.1, t));
        assert!(!d.square(0, empty.0, empty.1).has_things());
        // Every other square's list is intact throughout.
        for (mi, md) in orig.maps.iter().enumerate() {
            for x in 0..md.width as i32 {
                for y in 0..md.height as i32 {
                    if (mi, x, y) != (m, sx, sy) {
                        assert_eq!(d.things_at(mi, x, y), orig.things_at(mi, x, y), "map {mi} ({x},{y})");
                    }
                }
            }
        }
    }
}
