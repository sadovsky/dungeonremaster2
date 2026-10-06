//! Read-only game data shared by the simulation: the user's GRAPHICS.DAT
//! plus the tables read from their SKULL.EXE at runtime. Nothing here is
//! copied into the source.

use std::path::Path;

use dm2_formats::dungeon::Dungeon;
use dm2_formats::gdat::Gdat;

use crate::exe::Exe;
use crate::exe_tables::ExeTables;
use crate::items::ItemDb;

pub struct GameData {
    pub gdat: Gdat,
    pub tables: ExeTables,
    pub exe: Exe,
}

/// Addresses of small tables in the data object (docs/07, docs/09).
mod addr {
    /// Per-slot allowed-item masks for slots 0-12 (u16 each).
    pub const SLOT_MASKS: u32 = 0x7166C;
    /// Missile reflection directions (docs/05 "Missile flight").
    pub const REFLECT: u32 = 0x716A4;
    /// Cloud damage flags per cloud kind 0-7.
    pub const CLOUD_FLAGS: u32 = 0x716C4;
    /// Five (first slot, last slot, required thing type) rows used when a
    /// recruited champion picks up their belongings (0x4916B).
    pub const STARTING_SLOTS: u32 = 0x75A2D;
}

impl GameData {
    pub fn load(data_dir: &Path, exe_path: &Path) -> Option<GameData> {
        let gdat = Gdat::open(data_dir.join("GRAPHICS.DAT")).ok()?;
        let bytes = std::fs::read(exe_path).ok()?;
        Self::from_parts(gdat, &bytes)
    }

    pub fn from_parts(gdat: Gdat, exe_bytes: &[u8]) -> Option<GameData> {
        Some(GameData { gdat, tables: ExeTables::from_exe(exe_bytes).ok()?, exe: Exe::from_bytes(exe_bytes)? })
    }

    /// The user's files in the repo's default locations (tests, tools).
    pub fn load_default() -> Option<GameData> {
        Self::load(&crate::assets::default_data_dir(), &crate::exe_tables::default_exe_path())
    }

    pub fn item_db<'a>(&'a self, dg: &'a Dungeon) -> ItemDb<'a> {
        ItemDb { gdat: &self.gdat, dungeon: dg, categories: &self.tables.thing_category }
    }

    /// Allowed-item mask of inventory slot 0-12.
    pub fn slot_mask(&self, slot: usize) -> u16 {
        self.exe.u16_at(addr::SLOT_MASKS + 2 * slot as u32).unwrap_or(0)
    }

    /// Reflected direction for a spell hitting a reflecting creature,
    /// indexed by direction, cell·4 and facing parity·16 (4 = unchanged).
    pub fn reflect_dir(&self, dir: u8, cell: u8, facing_parity: u8) -> u8 {
        let i = dir as u32 + cell as u32 * 4 + facing_parity as u32 * 16;
        self.exe.u8_at(addr::REFLECT + i).unwrap_or(4)
    }

    /// Damage flags of cloud kind 0-7: bit 0 random roll, bit 1 champions,
    /// bit 2 the party square, bit 3 creatures (0x181F0).
    pub fn cloud_flags(&self, kind: u8) -> u8 {
        if kind > 7 {
            return 0;
        }
        self.exe.u8_at(addr::CLOUD_FLAGS + kind as u32).unwrap_or(0)
    }

    /// The five (first, last, thing type or 0xFFFF) slot ranges.
    pub fn starting_slot_ranges(&self) -> [(u16, u16, u16); 5] {
        let mut out = [(0, 0, 0xFFFF); 5];
        for (i, r) in out.iter_mut().enumerate() {
            let a = addr::STARTING_SLOTS + 6 * i as u32;
            *r = (
                self.exe.u16_at(a).unwrap_or(0),
                self.exe.u16_at(a + 2).unwrap_or(0),
                self.exe.u16_at(a + 4).unwrap_or(0xFFFF),
            );
        }
        out
    }
}
