//! Positional sound effects (docs/11-audio.md "Sound effects", play
//! function 0x15CA9).
//!
//! The source square is made relative to the party and rotated into the
//! party's facing; that gives the left/right pan and the distance used for
//! attenuation. Requests for the same sample from the same relative spot in
//! one batch are dropped, and at most 20 positional requests are accepted
//! per batch, as in the original's pending queue.
//!
//! Tentative (not documented in detail): the attenuation curve, the audible
//! range and how a different map level adds to the distance.

use std::collections::HashMap;
use std::sync::Arc;

use dm2_formats::dungeon::Dungeon;
use dm2_formats::gdat::{Gdat, Key};

use crate::effects::Effect;
use crate::world::PartyPos;

/// Positional requests accepted per batch (the original's pending queue).
pub const MAX_PENDING: usize = 20;
/// Simultaneously playing effect voices.
pub const MAX_VOICES: usize = 16;
/// Distance (in squares, |dx| + |dy|) beyond which a sound is inaudible.
pub const AUDIBLE_RANGE: i32 = 8;

/// A decoded sample: mono, -1..1.
#[derive(Clone, Debug)]
pub struct Sample {
    pub rate: u32,
    pub data: Vec<f32>,
}

/// Decode a type-2 entry. With archive flag 0x20 the entry has a 6-byte
/// header holding the rate; without it the game skips 2 bytes and plays at
/// 5500 Hz. Samples are unsigned 8-bit.
pub fn decode(entry: &[u8], has_header: bool) -> Option<Sample> {
    let (rate, skip) = if has_header {
        (u16::from_le_bytes([*entry.first()?, *entry.get(1)?]) as u32, 6)
    } else {
        (5500, 2)
    };
    let data = entry.get(skip..)?.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect();
    Some(Sample { rate: rate.max(1000), data })
}

/// A sound the simulation asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundRequest {
    pub cat: u8,
    pub idx: u8,
    pub sub: u8,
    pub map: usize,
    pub x: i32,
    pub y: i32,
    /// Volume as passed to the original's play function (200 is usual).
    pub vol: u8,
}

/// The original's usual sound volume argument.
pub const DEFAULT_VOL: u8 = 200;

/// Remove the sound requests from the effect queue (presentation-only
/// effects otherwise stay queued for the frontend).
pub fn drain_sounds(effects: &mut Vec<Effect>) -> Vec<SoundRequest> {
    let mut out = Vec::new();
    effects.retain(|e| match *e {
        Effect::Sound { cat, idx, sub, map, x, y } => {
            out.push(SoundRequest { cat, idx, sub, map, x, y, vol: DEFAULT_VOL });
            false
        }
        Effect::SoundAt { cat, idx, sub, map, x, y, vol } => {
            out.push(SoundRequest { cat, idx, sub, map, x, y, vol });
            false
        }
        _ => true,
    });
    out
}

/// Rotate a map offset (dx east, dy south) into the party's view:
/// (right, forward). Facing 0 north, 1 east, 2 south, 3 west.
pub fn rotate(dx: i32, dy: i32, dir: u8) -> (i32, i32) {
    match dir & 3 {
        0 => (dx, -dy),
        1 => (dy, dx),
        2 => (-dx, dy),
        _ => (-dy, -dx),
    }
}

/// Relative position of a source: (right, forward) in squares from the
/// party, plus the number of map levels between them.
pub fn relative(dg: &Dungeon, party: &PartyPos, map: usize, x: i32, y: i32) -> (i32, i32, i32) {
    let (mut dx, mut dy, mut levels) = (x - party.x, y - party.y, 0);
    if map != party.map && map < dg.maps.len() && party.map < dg.maps.len() {
        let (a, b) = (&dg.maps[map], &dg.maps[party.map]);
        dx = a.origin_x as i32 + x - (b.origin_x as i32 + party.x);
        dy = a.origin_y as i32 + y - (b.origin_y as i32 + party.y);
        levels = (a.depth as i32 - b.depth as i32).abs();
    }
    let (right, forward) = rotate(dx, dy, party.dir);
    (right, forward, levels)
}

/// Gain and pan (-1 left .. 1 right) for a relative position, or None if
/// out of range.
pub fn place(right: i32, forward: i32, levels: i32) -> Option<(f32, f32)> {
    let dist = right.abs() + forward.abs() + 2 * levels;
    if dist > AUDIBLE_RANGE {
        return None;
    }
    let gain = 1.0 - dist as f32 / (AUDIBLE_RANGE + 1) as f32;
    let pan = (right as f32 * 0.35).clamp(-1.0, 1.0);
    Some((gain, pan))
}

struct Playing {
    sample: Arc<Sample>,
    pos: f64,
    gain_l: f32,
    gain_r: f32,
}

pub struct Sfx {
    cache: HashMap<(u8, u8, u8), Option<Arc<Sample>>>,
    playing: Vec<Playing>,
    has_header: bool,
    pub volume: f32,
}

impl Sfx {
    pub fn new(has_header: bool) -> Sfx {
        Sfx { cache: HashMap::new(), playing: Vec::new(), has_header, volume: 0.8 }
    }

    fn sample(&mut self, g: &Gdat, cat: u8, idx: u8, sub: u8) -> Option<Arc<Sample>> {
        let hdr = self.has_header;
        self.cache
            .entry((cat, idx, sub))
            .or_insert_with(|| g.get(Key::new(cat, idx, 2, sub)).and_then(|e| decode(e, hdr)).map(Arc::new))
            .clone()
    }

    /// Start the sounds of one batch (one game tick).
    pub fn play(&mut self, g: &Gdat, dg: &Dungeon, party: &PartyPos, reqs: &[SoundRequest]) {
        let mut seen = Vec::new();
        for r in reqs.iter().take(MAX_PENDING) {
            let (right, forward, levels) = relative(dg, party, r.map, r.x, r.y);
            let key = (r.cat, r.idx, r.sub, right, forward, levels);
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            let Some((gain, pan)) = place(right, forward, levels) else { continue };
            let Some(sample) = self.sample(g, r.cat, r.idx, r.sub) else { continue };
            if self.playing.len() >= MAX_VOICES {
                self.playing.remove(0);
            }
            let g = gain * self.volume * r.vol as f32 / DEFAULT_VOL as f32;
            self.playing.push(Playing {
                sample,
                pos: 0.0,
                gain_l: g * (1.0 - pan.max(0.0)),
                gain_r: g * (1.0 + pan.min(0.0)),
            });
        }
    }

    /// Mix into an interleaved stereo buffer at `sr` Hz (adds to `out`).
    pub fn mix(&mut self, out: &mut [f32], sr: u32) {
        for p in &mut self.playing {
            let step = p.sample.rate as f64 / sr as f64;
            for frame in out.chunks_exact_mut(2) {
                let i = p.pos as usize;
                let Some(&a) = p.sample.data.get(i) else { break };
                let b = p.sample.data.get(i + 1).copied().unwrap_or(a);
                let t = (p.pos - i as f64) as f32;
                let s = a + (b - a) * t;
                frame[0] += s * p.gain_l;
                frame[1] += s * p.gain_r;
                p.pos += step;
            }
        }
        self.playing.retain(|p| (p.pos as usize) < p.sample.data.len());
    }

    pub fn active(&self) -> usize {
        self.playing.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_unsigned_pcm() {
        let e = [0x77, 0x2B, 8, 1, 0, 0, 128, 255, 0];
        let s = decode(&e, true).unwrap();
        assert_eq!(s.rate, 11127);
        assert_eq!(s.data.len(), 3);
        assert_eq!(s.data[0], 0.0);
        assert!(s.data[1] > 0.99 && s.data[2] == -1.0);
    }

    #[test]
    fn rotation_follows_facing() {
        // A source one square north of the party (dx 0, dy -1).
        assert_eq!(rotate(0, -1, 0), (0, 1)); // facing north: ahead
        assert_eq!(rotate(0, -1, 1), (-1, 0)); // facing east: to the left
        assert_eq!(rotate(0, -1, 2), (0, -1)); // facing south: behind
        assert_eq!(rotate(0, -1, 3), (1, 0)); // facing west: to the right
    }

    #[test]
    fn attenuation_and_range() {
        let near = place(0, 0, 0).unwrap().0;
        let far = place(0, 4, 0).unwrap().0;
        assert!(near > far && far > 0.0);
        assert!(place(5, 4, 0).is_none());
        assert!(place(1, 0, 0).unwrap().1 > 0.0);
    }

    #[test]
    fn drains_only_sounds() {
        let mut q = vec![Effect::Sound { cat: 3, idx: 0, sub: 1, map: 0, x: 1, y: 1 }, Effect::EndGame];
        let s = drain_sounds(&mut q);
        assert_eq!(s.len(), 1);
        assert_eq!(q.len(), 1);
    }
}
