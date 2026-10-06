//! GRAPHICS.DAT archive reader (DM2 PC, archive version 3+).
//!
//! Layout and index format: docs/02-graphics-dat.md.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

/// Types whose index value is a plain number, not an entry number.
pub const VALUE_TYPES: [u8; 2] = [0x0B, 0x0C];

/// Data type (the `D` field of an index key).
pub mod kind {
    pub const IMAGE: u8 = 1;
    pub const PALETTE_ETC: u8 = 7;
    pub const TABLE_1024: u8 = 9;
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Format(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io: {e}"),
            Error::Format(s) => write!(f, "format: {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

fn bad(msg: impl Into<String>) -> Error {
    Error::Format(msg.into())
}

/// Four-part lookup key: category, index, data type, sub-index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    pub cat: u8,
    pub idx: u8,
    pub kind: u8,
    pub sub: u8,
}

impl Key {
    pub const fn new(cat: u8, idx: u8, kind: u8, sub: u8) -> Self {
        Key { cat, idx, kind, sub }
    }
}

/// One row of the metadata index.
#[derive(Clone, Copy, Debug)]
pub struct Record {
    pub key: Key,
    /// `F` field (language / variant; 0x10 en, 0x30 de, 0x40 fr observed).
    pub f: u8,
    /// `G` field (0xFF = resident at startup, 1 = sets bit 15 of value).
    pub g: u8,
    /// Raw 16-bit value (`P` field).
    pub value: u16,
}

impl Record {
    /// Entry number referenced by this record, if it is not a numeric value.
    pub fn entry(&self) -> Option<u16> {
        if VALUE_TYPES.contains(&self.key.kind) {
            None
        } else {
            Some(self.value & 0x7FFF)
        }
    }
}

pub struct Gdat {
    data: Vec<u8>,
    pub version: u16,
    offsets: Vec<usize>,
    sizes: Vec<usize>,
    pub records: Vec<Record>,
    index: HashMap<Key, usize>,
}

impl Gdat {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        Self::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self, Error> {
        let u16le = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]);
        if data.len() < 8 {
            return Err(bad("file too short"));
        }
        let sig = u16le(0);
        if sig & 0x8000 == 0 {
            return Err(bad("missing signature bit"));
        }
        let version = sig & 0x7FFF;
        if version < 3 {
            return Err(bad(format!("unsupported archive version {version}")));
        }
        let count = u16le(2) as usize;
        let meta_len = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        let mut sizes = Vec::with_capacity(count);
        sizes.push(meta_len);
        for i in 1..count {
            sizes.push(u16le(8 + 2 * (i - 1)) as usize);
        }
        let mut offsets = Vec::with_capacity(count);
        let mut off = 8 + 2 * (count - 1);
        for s in &sizes {
            offsets.push(off);
            off += s;
        }
        if off != data.len() {
            return Err(bad(format!("entry sizes end at {off}, file is {}", data.len())));
        }
        let mut g = Gdat { data, version, offsets, sizes, records: Vec::new(), index: HashMap::new() };
        g.parse_index()?;
        Ok(g)
    }

    pub fn entry_count(&self) -> usize {
        self.sizes.len()
    }

    /// Raw bytes of archive entry `i`.
    pub fn entry(&self, i: u16) -> Option<&[u8]> {
        let i = i as usize;
        let o = *self.offsets.get(i)?;
        Some(&self.data[o..o + self.sizes[i]])
    }

    fn parse_index(&mut self) -> Result<(), Error> {
        let m = self.entry(0).ok_or_else(|| bad("no entry 0"))?.to_vec();
        let be = |o: usize| u16::from_be_bytes([m[o], m[o + 1]]);
        if be(0) != 0x8001 {
            return Err(bad("index marker is not 0x8001"));
        }
        let nrec = be(2) as usize;
        let nfld = be(4) as usize;
        let mut fields: HashMap<u8, (usize, usize)> = HashMap::new();
        let mut width = 0;
        for k in 0..nfld {
            let (letter, w) = (m[6 + 2 * k], m[7 + 2 * k] as usize);
            fields.insert(letter, (width, w));
            width += w;
        }
        let base = 6 + 2 * nfld;
        if base + nrec * width > m.len() {
            return Err(bad("index records overrun entry 0"));
        }
        let field = |row: &[u8], letter: u8| -> u32 {
            fields.get(&letter).map_or(0, |&(o, w)| {
                row[o..o + w].iter().fold(0u32, |acc, &b| (acc << 8) | b as u32)
            })
        };
        for r in 0..nrec {
            let row = &m[base + r * width..base + (r + 1) * width];
            let rec = Record {
                key: Key::new(
                    field(row, b'T') as u8,
                    field(row, b'I') as u8,
                    field(row, b'D') as u8,
                    field(row, b'S') as u8,
                ),
                f: field(row, b'F') as u8,
                g: field(row, b'G') as u8,
                value: field(row, b'P') as u16,
            };
            // First record wins, like the game's sorted binary search on unique keys.
            self.index.entry(rec.key).or_insert(self.records.len());
            self.records.push(rec);
        }
        Ok(())
    }

    pub fn record(&self, key: Key) -> Option<&Record> {
        self.index.get(&key).map(|&i| &self.records[i])
    }

    /// Value lookup mirroring the game (0x3C5D3): entry number or raw number.
    pub fn lookup(&self, key: Key) -> Option<u16> {
        self.record(key).map(|r| r.entry().unwrap_or(r.value))
    }

    /// Bytes of the entry a key refers to.
    pub fn get(&self, key: Key) -> Option<&[u8]> {
        self.record(key)?.entry().and_then(|e| self.entry(e))
    }
}

/// Location of the user's GRAPHICS.DAT in this repo, for tests and tools.
pub fn default_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../original/dumast2/DATA/GRAPHICS.DAT")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load() -> Option<Gdat> {
        let p = default_path();
        if !p.exists() {
            eprintln!("skipping: {} not found", p.display());
            return None;
        }
        Some(Gdat::open(p).expect("parse GRAPHICS.DAT"))
    }

    #[test]
    fn header_and_index() {
        let Some(g) = load() else { return };
        assert_eq!(g.version, 5);
        assert_eq!(g.entry_count(), 5624);
        assert_eq!(g.records.len(), 11854);
        // The master palette is entry 206 under (1,0,9,254).
        assert_eq!(g.lookup(Key::new(1, 0, kind::TABLE_1024, 254)), Some(206));
        assert_eq!(g.get(Key::new(1, 0, kind::TABLE_1024, 254)).unwrap().len(), 1024);
        assert_eq!(g.lookup(Key::new(0, 0, 8, 0)), None);
    }
}
