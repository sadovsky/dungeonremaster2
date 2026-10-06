//! Minimal reader for the user's SKULL.EXE (LE format, docs/01-executable.md).
//!
//! Only used to read plain data tables (mouse zones, key bindings) from the
//! data object at runtime, so nothing from the executable is embedded in
//! this source. Fixups are not applied: the tables read here hold no
//! pointers.

use std::path::Path;

pub struct Exe {
    /// Data object contents, loaded at `data_base`.
    data: Vec<u8>,
    data_base: u32,
}

fn u32le(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

impl Exe {
    pub fn open(path: &Path) -> Option<Exe> {
        Self::from_bytes(&std::fs::read(path).ok()?)
    }

    pub fn from_bytes(exe: &[u8]) -> Option<Exe> {
        let le = u32le(exe, 0x3C)? as usize;
        if exe.get(le..le + 2)? != b"LE" {
            return None;
        }
        let h = |o: usize| u32le(exe, le + o);
        let (n_pages, page_size, last_page) = (h(0x14)?, h(0x28)? as usize, h(0x2C)? as usize);
        let (objtab, nobj, data_pages) = (le + h(0x40)? as usize, h(0x44)?, h(0x80)? as usize);
        // The data object is the last writable, non-executable object (object 2).
        for i in (0..nobj as usize).rev() {
            let o = objtab + 24 * i;
            let (vsize, base, flags) = (u32le(exe, o)?, u32le(exe, o + 4)?, u32le(exe, o + 8)?);
            let (first, count) = (u32le(exe, o + 12)? as usize, u32le(exe, o + 16)? as usize);
            if flags & 4 != 0 {
                continue;
            }
            let mut data = vec![0u8; vsize as usize];
            for k in 0..count {
                let page_no = first + k;
                let size = if page_no as u32 == n_pages { last_page } else { page_size };
                let src = data_pages + (page_no - 1) * page_size;
                let dst = k * page_size;
                let n = size.min(data.len().saturating_sub(dst)).min(exe.len().saturating_sub(src));
                data[dst..dst + n].copy_from_slice(&exe[src..src + n]);
            }
            return Some(Exe { data, data_base: base });
        }
        None
    }

    pub fn u8_at(&self, addr: u32) -> Option<u8> {
        self.data.get(addr.checked_sub(self.data_base)? as usize).copied()
    }

    pub fn i16_at(&self, addr: u32) -> Option<i16> {
        self.u16_at(addr).map(|v| v as i16)
    }

    pub fn u16_at(&self, addr: u32) -> Option<u16> {
        let o = addr.checked_sub(self.data_base)? as usize;
        self.data.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
}
