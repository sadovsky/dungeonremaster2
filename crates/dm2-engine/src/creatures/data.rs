//! Creature tables, read at runtime from the user's own SKULL.EXE and
//! GRAPHICS.DAT (docs/08-creatures-ai.md, "Where creature data comes from").
//! Nothing here is copied into the source.

use std::collections::HashMap;
use std::rc::Rc;

use dm2_formats::gdat::{Gdat, Key};

use crate::exe_tables::{le_object, ExeError, ExeTables};

use super::anim::Anim;

/// Base address of the LE data object; raw pointer values in it are offsets
/// from here.
const DATA_BASE: u32 = 0x70000;
/// 36-byte info records.
const INFO: u32 = 0x71968;
pub const INFO_SIZE: usize = 36;
const INFO_COUNT: usize = 63;
/// AI class flags, 4 bytes per class (two 16-bit words).
const CLASS_FLAGS: u32 = 0x7507A;
/// Per-class pointer to its behaviour sets (6-byte entries).
const BEHAVIOUR_SETS: u32 = 0x7518C;
/// Pointers to the 63 behaviour programs.
const PROGRAMS: u32 = 0x74F7E;
pub const PROGRAM_COUNT: u8 = 63;
/// Per-action flag bytes.
const ACTION_FLAGS: u32 = 0x75136;
/// Two behaviour lists that take the cheap random-wander path in think.
pub const WANDER_LISTS: [u32; 2] = [0x73399, 0x73392];

/// The 36-byte creature info record (docs/08, "Info record").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub raw: [u8; INFO_SIZE],
}

impl Default for Info {
    fn default() -> Info {
        Info { raw: [0; INFO_SIZE] }
    }
}

impl Info {
    fn w(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.raw[o], self.raw[o + 1]])
    }
    pub fn flags(&self) -> u16 {
        self.w(0)
    }
    /// Info flag bit 0: inanimate or scripted.
    pub fn inanimate(&self) -> bool {
        self.raw[0] & 1 != 0
    }
    /// Size class compared with how far a door is open (bits 6-7).
    pub fn door_size(&self) -> u16 {
        (self.raw[0] >> 6) as u16 & 3
    }
    pub fn flags1(&self) -> u8 {
        self.raw[1]
    }
    pub fn defence(&self) -> u8 {
        self.raw[2]
    }
    pub fn regen(&self) -> i8 {
        self.raw[3] as i8
    }
    pub fn base_hp(&self) -> u16 {
        self.w(4)
    }
    pub fn attack(&self) -> u8 {
        self.raw[6]
    }
    pub fn poison(&self) -> u8 {
        self.raw[7]
    }
    pub fn dexterity(&self) -> u8 {
        self.raw[8]
    }
    pub fn jitter(&self) -> u8 {
        self.raw[9]
    }
    pub fn terrain(&self) -> u16 {
        self.w(0x0A)
    }
    pub fn item_flags(&self) -> u16 {
        self.w(0x0C)
    }
    pub fn alertness_word(&self) -> u16 {
        self.w(0x16)
    }
    pub fn word18(&self) -> u16 {
        self.w(0x18)
    }
    pub fn flags19(&self) -> u8 {
        self.raw[0x19]
    }
    /// Body-part selector nibbles for attacks on champions.
    pub fn hit_parts(&self) -> u16 {
        self.w(0x1A)
    }
    pub fn attack_type(&self) -> u8 {
        self.raw[0x1C]
    }
    pub fn size(&self) -> u8 {
        self.raw[0x23]
    }
}

/// One behaviour-list entry (7 bytes; docs/08 "Behaviour lists").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BehaviourEntry {
    pub program: u8,
    /// n > 0: 1 in n; n < 0: (1 − 1/|n|); 0: always.
    pub probability: i8,
    /// Address of the goal data passed to the goal builder.
    pub goal_data: u32,
}

/// One 7-byte program row (docs/08 "Programs and the interpreter").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub op: i8,
    pub on_done: i8,
    pub on_other: i8,
    pub arg3: i8,
    pub arg4: i8,
    pub goal: u8,
    pub goal_arg: i8,
}

impl Row {
    fn from(b: &[u8]) -> Row {
        Row {
            op: b[0] as i8,
            on_done: b[1] as i8,
            on_other: b[2] as i8,
            arg3: b[3] as i8,
            arg4: b[4] as i8,
            goal: b[5],
            goal_arg: b[6] as i8,
        }
    }
    pub fn goal_kind(&self) -> u8 {
        self.goal & 0x1F
    }
}

/// Everything the creature code reads from the original files.
pub struct CreatureData {
    pub gdat: Rc<Gdat>,
    pub tables: ExeTables,
    obj: Vec<u8>,
    anims: std::cell::RefCell<HashMap<u8, Option<Rc<Anim>>>>,
    /// Parsed item-kind sets by (creature type, set, classifying creatures).
    pub kind_sets: std::cell::RefCell<HashMap<(u8, u8, bool), Option<super::kinds::KindSet>>>,
}

impl CreatureData {
    pub fn load(gdat: Rc<Gdat>, exe: &[u8]) -> Result<CreatureData, ExeError> {
        let obj = le_object(exe, 2)?;
        let tables = ExeTables::from_exe(exe)?;
        Ok(CreatureData { gdat, tables, obj, anims: Default::default(), kind_sets: Default::default() })
    }

    fn bytes(&self, addr: u32, n: usize) -> Option<&[u8]> {
        let o = addr.checked_sub(DATA_BASE)? as usize;
        self.obj.get(o..o + n)
    }
    fn u16_at(&self, addr: u32) -> u16 {
        self.bytes(addr, 2).map(|b| u16::from_le_bytes([b[0], b[1]])).unwrap_or(0)
    }
    /// Little-endian word at a data-object address.
    pub fn word_at(&self, addr: u32) -> Option<u16> {
        self.bytes(addr, 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    fn ptr_at(&self, addr: u32) -> Option<u32> {
        let b = self.bytes(addr, 4)?;
        let v = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        (v != 0).then_some(v + DATA_BASE)
    }

    pub fn info(&self, index: u16) -> Option<Info> {
        if index as usize >= INFO_COUNT {
            return None;
        }
        let b = self.bytes(INFO + INFO_SIZE as u32 * index as u32, INFO_SIZE)?;
        let mut raw = [0u8; INFO_SIZE];
        raw.copy_from_slice(b);
        Some(Info { raw })
    }

    /// AI class flags: first word | second word << 16.
    pub fn class_flags(&self, class: u16) -> u32 {
        let a = CLASS_FLAGS + 4 * class as u32;
        self.u16_at(a) as u32 | (self.u16_at(a + 2) as u32) << 16
    }

    /// The class's behaviour sets: (condition mask, list address), ending
    /// with the zero-mask entry (included).
    pub fn behaviour_sets(&self, class: u16) -> Vec<(u16, u32)> {
        let mut out = Vec::new();
        let Some(mut a) = self.ptr_at(BEHAVIOUR_SETS + 4 * class as u32) else { return out };
        for _ in 0..32 {
            let mask = self.u16_at(a);
            let list = self.ptr_at(a + 2).unwrap_or(0);
            out.push((mask, list));
            if mask == 0 {
                break;
            }
            a += 6;
        }
        out
    }

    pub fn behaviour_list(&self, addr: u32) -> Vec<BehaviourEntry> {
        let mut out = Vec::new();
        let mut a = addr;
        for _ in 0..32 {
            let Some(b) = self.bytes(a, 7) else { break };
            out.push(BehaviourEntry {
                program: b[0],
                probability: b[1] as i8,
                goal_data: u32::from_le_bytes([b[2], b[3], b[4], b[5]]).wrapping_add(DATA_BASE),
            });
            if b[6] == 0 {
                break;
            }
            a += 7;
        }
        out
    }

    pub fn row(&self, program: u8, step: i8) -> Option<Row> {
        if program >= PROGRAM_COUNT || step < 0 {
            return None;
        }
        let base = self.ptr_at(PROGRAMS + 4 * program as u32)?;
        self.bytes(base + 7 * step as u32, 7).map(Row::from)
    }

    pub fn action_flags(&self, action: u8) -> u8 {
        self.bytes(ACTION_FLAGS + action as u32, 1).map(|b| b[0]).unwrap_or(0)
    }

    /// Animation tables for a creature type, cached.
    pub fn anim(&self, ty: u8) -> Option<Rc<Anim>> {
        self.anims
            .borrow_mut()
            .entry(ty)
            .or_insert_with(|| {
                let map = self.gdat.get(Key::new(15, ty, 8, 251))?;
                let frames = self.gdat.get(Key::new(15, ty, 7, 252))?;
                Some(Rc::new(Anim::parse(map, frames)))
            })
            .clone()
    }
}
