//! Depth lighting for the viewport (docs/04 section 6; SKULL.EXE 0x4E3D5,
//! 0x1AFC2). Brightness changes are palette-index remaps along colour ramps.

use std::collections::HashMap;

use dm2_formats::gdat::{Gdat, Key};

/// Darkening (in 64ths) per view depth, normal frames (0x75C0C).
pub const DEPTH_DARKEN: [i32; 5] = [0, 0, 12, 28, 46];

struct Ramp {
    bright: Vec<u8>,
    index: Vec<u8>,
}

pub struct Light {
    ramps: Vec<Ramp>,
    /// Palette index -> (ramp, position).
    back: Vec<(u8, u8)>,
    cache: HashMap<(i32, Option<u8>), [u8; 256]>,
}

impl Light {
    /// Parse the ramp table (1,0,7,2): count, lengths, brightness lists,
    /// index lists, then a 256 × (ramp, position) back map.
    pub fn load(g: &Gdat) -> Option<Light> {
        let d = g.get(Key::new(1, 0, 7, 2))?;
        let n = *d.first()? as usize;
        let lens: Vec<usize> = d.get(1..1 + n)?.iter().map(|&l| l as usize).collect();
        let mut o = 1 + n;
        let mut ramps: Vec<Ramp> = Vec::with_capacity(n);
        for &l in &lens {
            ramps.push(Ramp { bright: d.get(o..o + l)?.to_vec(), index: Vec::new() });
            o += l;
        }
        for (r, &l) in ramps.iter_mut().zip(&lens) {
            r.index = d.get(o..o + l)?.to_vec();
            o += l;
        }
        let back = d.get(o..o + 512)?.chunks_exact(2).map(|c| (c[0], c[1])).collect();
        Some(Light { ramps, back, cache: HashMap::new() })
    }

    /// Darken one palette index by `factor` (64 = unchanged), stepping away
    /// from `key` if the nearest shade would equal it (0x1AFC2).
    fn darken(&self, c: u8, factor: i32, key: Option<u8>) -> u8 {
        let (ri, pos) = self.back[c as usize];
        let Some(r) = self.ramps.get(ri as usize) else { return c };
        let Some(&b) = r.bright.get(pos as usize) else { return c };
        let v = ((b as i32 * factor) >> 6).max(0);
        let last = r.bright.len() as i32 - 1;
        let mut s = 0;
        while s < last {
            let below = v - r.bright[s as usize] as i32;
            let above = r.bright[s as usize + 1] as i32 - v;
            if below >= 0 && above >= 0 {
                if above < below {
                    s += 1;
                }
                break;
            }
            s += 1;
        }
        let pick = |i: i32| r.index[i as usize];
        if Some(pick(s)) != key {
            return pick(s);
        }
        // Nearest neighbour that isn't the key.
        let (mut lo, mut hi) = (s - 1, s + 1);
        loop {
            let i = if lo < 0
                || (hi <= last && (r.bright[hi as usize] as i32 - v) < (v - r.bright[lo as usize] as i32))
            {
                hi += 1;
                hi - 1
            } else {
                lo -= 1;
                lo + 1
            };
            if i < 0 || i > last {
                return pick(s);
            }
            if Some(pick(i)) != key {
                return pick(i);
            }
        }
    }

    /// 256-entry darkening map for a light parameter (0 = full light).
    fn darkening(&mut self, level: i32, key: Option<u8>) -> &[u8; 256] {
        if !self.cache.contains_key(&(level, key)) {
            let factor = (64 - level).max(0);
            let mut m = [0u8; 256];
            for (i, e) in m.iter_mut().enumerate() {
                *e = self.darken(i as u8, factor, key);
            }
            if let Some(k) = key {
                m[k as usize] = k;
            }
            self.cache.insert((level, key), m);
        }
        &self.cache[&(level, key)]
    }

    /// Colour map for drawing at view depth `depth` (0x4E3D5, normal
    /// frames). `set_remap` is the map set's own table for that depth, if
    /// any; `ambient` is the ambient darkness (0 = full light). Returns None
    /// when nothing changes.
    pub fn for_depth(&mut self, depth: usize, ambient: i32, key: Option<u8>, set_remap: Option<&[u8]>) -> Option<[u8; 256]> {
        self.for_depth_with(DEPTH_DARKEN[depth.min(4)], ambient, key, set_remap)
    }

    /// As `for_depth`, with an explicit depth darkening (64ths); the
    /// mid-step frame uses its own row (0x75C07).
    pub fn for_depth_with(&mut self, darken: i32, ambient: i32, key: Option<u8>, set_remap: Option<&[u8]>) -> Option<[u8; 256]> {
        if let Some(t) = set_remap.filter(|t| t.len() >= 256) {
            let mut m = [0u8; 256];
            m.copy_from_slice(&t[..256]);
            if ambient > 0 {
                let d = *self.darkening(ambient, key);
                for e in m.iter_mut() {
                    *e = d[*e as usize];
                }
            }
            return Some(m);
        }
        let level = 64 - (((64 - darken) * (64 - ambient)) >> 6);
        if level == 0 {
            return None;
        }
        Some(*self.darkening(level, key))
    }
}
