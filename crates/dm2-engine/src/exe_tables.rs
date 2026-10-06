//! Game-mechanics tables read from the user's own SKULL.EXE at runtime.
//!
//! Nothing here is copied into the source: the rune costs, spell table,
//! action-code names and armour weights are read from the data object of
//! the executable, at the addresses documented in docs/06 and docs/07.

use std::path::Path;

/// Base address of the LE data object (object 2).
const DATA_BASE: u32 = 0x70000;
const RUNE_COSTS: u32 = 0x757DC; // 4 rows × 6
const POWER_MULT: u32 = 0x757F4; // 6, in eighths
const SPELLS: u32 = 0x757FE; // 33 × 8
const SPELL_COUNT: usize = 33;
const ACTION_CODES: u32 = 0x7590E; // NUL-separated two-letter names
const SLOT_DEFENCE: u32 = 0x759A6; // per body-part weight used by armour_value
const THING_CATEGORY: u32 = 0x72294; // GRAPHICS.DAT category per thing type

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spell {
    /// Rune symbols: byte 3 required power (0 = any), bytes 2/1/0 the
    /// element, form and alignment runes (0 = unused).
    pub key: u32,
    pub base_level: u8,
    pub skill: u8,
    /// 1 potion, 2 projectile, 3 other, 4 summon.
    pub kind: u8,
    pub kind_type: u8,
    pub duration: u8,
}

#[derive(Clone, Debug)]
pub struct ExeTables {
    /// Mana cost per rune, `[row][column]`.
    pub rune_costs: [[u8; 6]; 4],
    /// Power-rune multipliers in eighths.
    pub power_mult: [u8; 6],
    pub spells: Vec<Spell>,
    /// Two-letter action-code names, by slot.
    pub action_codes: Vec<String>,
    /// Per-slot weighting of hand-held shields in the armour value.
    pub slot_defence: [u8; 6],
    /// GRAPHICS.DAT category for each thing type (docs/09).
    pub thing_category: [u8; 16],
}

#[derive(Debug)]
pub enum ExeError {
    Io(std::io::Error),
    NotLe,
    Short,
}

impl std::fmt::Display for ExeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ExeError {}

fn u32le(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Contents of an LE object (no fixups applied; the tables read here hold
/// no pointers).
pub fn le_object(exe: &[u8], object: usize) -> Result<Vec<u8>, ExeError> {
    let le = u32le(exe, 0x3C).ok_or(ExeError::Short)? as usize;
    if exe.get(le..le + 2) != Some(b"LE") {
        return Err(ExeError::NotLe);
    }
    let h = |o: usize| u32le(exe, le + o).ok_or(ExeError::Short);
    let (n_pages, page_size, last_page) = (h(0x14)?, h(0x28)? as usize, h(0x2C)? as usize);
    let (objtab, nobj, data_pages) = (le + h(0x40)? as usize, h(0x44)? as usize, h(0x80)? as usize);
    if object == 0 || object > nobj {
        return Err(ExeError::Short);
    }
    let o = objtab + 24 * (object - 1);
    let vsize = u32le(exe, o).ok_or(ExeError::Short)? as usize;
    let first = u32le(exe, o + 12).ok_or(ExeError::Short)?;
    let count = u32le(exe, o + 16).ok_or(ExeError::Short)?;
    let mut buf = vec![0u8; vsize];
    for k in 0..count {
        let page_no = first + k;
        let size = if page_no == n_pages { last_page } else { page_size };
        let src = data_pages + (page_no as usize - 1) * page_size;
        let dst = k as usize * page_size;
        let n = size.min(vsize.saturating_sub(dst)).min(exe.len().saturating_sub(src));
        buf[dst..dst + n].copy_from_slice(&exe[src..src + n]);
    }
    Ok(buf)
}

impl ExeTables {
    pub fn load(path: &Path) -> Result<ExeTables, ExeError> {
        let exe = std::fs::read(path).map_err(ExeError::Io)?;
        Self::from_exe(&exe)
    }

    pub fn from_exe(exe: &[u8]) -> Result<ExeTables, ExeError> {
        let d = le_object(exe, 2)?;
        let at = |a: u32, n: usize| -> Result<&[u8], ExeError> {
            let o = (a - DATA_BASE) as usize;
            d.get(o..o + n).ok_or(ExeError::Short)
        };
        let mut rune_costs = [[0u8; 6]; 4];
        let rc = at(RUNE_COSTS, 24)?;
        for (r, row) in rune_costs.iter_mut().enumerate() {
            row.copy_from_slice(&rc[r * 6..r * 6 + 6]);
        }
        let mut power_mult = [0u8; 6];
        power_mult.copy_from_slice(at(POWER_MULT, 6)?);
        let mut spells = Vec::with_capacity(SPELL_COUNT);
        for b in at(SPELLS, 8 * SPELL_COUNT)?.chunks_exact(8) {
            let w = u16::from_le_bytes([b[6], b[7]]);
            spells.push(Spell {
                key: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                base_level: b[4],
                skill: b[5],
                kind: (w & 15) as u8,
                kind_type: ((w >> 4) & 0x3F) as u8,
                duration: (w >> 10) as u8,
            });
        }
        let action_codes = at(ACTION_CODES, 64)?
            .split(|&c| c == 0)
            .take_while(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        let mut slot_defence = [0u8; 6];
        slot_defence.copy_from_slice(at(SLOT_DEFENCE, 6)?);
        let mut thing_category = [0u8; 16];
        thing_category.copy_from_slice(at(THING_CATEGORY, 16)?);
        Ok(ExeTables { rune_costs, power_mult, spells, action_codes, slot_defence, thing_category })
    }

    /// Slot number of a two-letter action code.
    pub fn code_slot(&self, code: &str) -> Option<usize> {
        self.action_codes.iter().position(|c| c == code)
    }
}

/// The repo's default location for the user's SKULL.EXE.
pub fn default_exe_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../original/dumast2/SKULL.EXE")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tables_from_users_exe() {
        let Ok(t) = ExeTables::load(&default_exe_path()) else { return };
        assert_eq!(t.spells.len(), SPELL_COUNT);
        assert!(t.spells.iter().all(|s| (1..=4).contains(&s.kind)));
        assert!(t.power_mult.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(t.action_codes.len(), 18);
        for c in ["SK", "LV", "CM", "BZ", "TR", "ST", "PA", "TA", "NC", "EX", "PB", "DM", "AT"] {
            assert!(t.code_slot(c).is_some(), "{c}");
        }
    }
}
