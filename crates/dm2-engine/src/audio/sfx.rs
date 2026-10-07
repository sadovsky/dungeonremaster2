//! Positional sound effects (docs/11-audio.md "Sound effects", play
//! function 0x15CA9).
//!
//! As in the original, a request plays only if:
//! - it is an interface or party sound (mode < 1), or its source is on the
//!   party's map;
//! - its key is registered for the party's map (`registry`);
//! - when it is more than one square away, the party's sound-distance grid
//!   (a breadth-first search over passable squares, up to 8 steps, built by
//!   0x2F8F5 and read by 0x2FACC) reaches its square. When the path is
//!   longer than the straight distance, the source is pushed out along the
//!   same direction to the path length.
//!
//! The source square is made relative to the party and rotated into the
//! party's facing. The driver's volume falls off with the square of the
//! distance: out = volume * 8 / (dx^2 + dy^2 + 8), out of 255 (0x10877).
//! Requests for the same sample from the same relative spot in one batch
//! are dropped; at most 20 positional and 6 interface requests are accepted
//! per batch. While the party sleeps, volumes are halved.
//!
//! Tentative: the pan curve (the original uses a 16-step table indexed by
//! the angle, not ported).

use std::collections::HashMap;
use std::sync::Arc;

use dm2_formats::dungeon::Dungeon;
use dm2_formats::gdat::{Gdat, Key};

use crate::effects::Effect;
use crate::world::{self, PartyPos};

use super::registry::Registry;

/// Positional requests accepted per batch (the original's pending queue).
pub const MAX_PENDING: usize = 20;
/// Interface requests (mode < 0) accepted per batch.
pub const MAX_INTERFACE: usize = 6;
/// Steps the sound-distance search covers from the party.
pub const SOUND_RANGE: u8 = 8;
/// Simultaneously playing effect voices.
pub const MAX_VOICES: usize = 16;

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
    /// The play function's mode: 1 queued at (x, y), 0 played at once,
    /// negative through the interface queue, 2 or more delayed by that many
    /// ticks less one (the simulation schedules those; see `delayed`).
    pub mode: i8,
}

/// The original's usual sound volume argument.
pub const DEFAULT_VOL: u8 = 200;

/// Remove the sound requests from the effect queue (presentation-only
/// effects otherwise stay queued for the frontend).
pub fn drain_sounds(effects: &mut Vec<Effect>) -> Vec<SoundRequest> {
    let mut out = Vec::new();
    effects.retain(|e| match *e {
        Effect::Sound { cat, idx, sub, map, x, y } => {
            out.push(SoundRequest { cat, idx, sub, map, x, y, vol: DEFAULT_VOL, mode: 1 });
            false
        }
        Effect::SoundAt { cat, idx, sub, map, x, y, vol, mode } => {
            out.push(SoundRequest { cat, idx, sub, map, x, y, vol, mode });
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

/// Relative position of a source on the party's map: (right, forward).
pub fn relative(party: &PartyPos, x: i32, y: i32) -> (i32, i32) {
    rotate(x - party.x, y - party.y, party.dir)
}

/// The party's sound-distance grid (0x2F8F5 via the planner 0x3188A):
/// for each square of the party's map, the number of steps from the party
/// plus one (0 = not reached within `SOUND_RANGE`). Squares that block
/// movement are marked with bit 7.
pub struct SoundGrid {
    w: i32,
    h: i32,
    cells: Vec<u8>,
}

impl SoundGrid {
    pub fn build(dg: &Dungeon, party: &PartyPos) -> SoundGrid {
        let Some(m) = dg.maps.get(party.map) else { return SoundGrid { w: 0, h: 0, cells: Vec::new() } };
        let (w, h) = (m.width as i32, m.height as i32);
        let mut cells = vec![0u8; (w * h).max(0) as usize];
        let idx = |x: i32, y: i32| (x * h + y) as usize;
        for x in 0..w {
            for y in 0..h {
                if world::blocks(dg, party.map, x, y) {
                    cells[idx(x, y)] = 0x80;
                }
            }
        }
        let (px, py) = (party.x, party.y);
        if px < 0 || py < 0 || px >= w || py >= h {
            return SoundGrid { w, h, cells };
        }
        cells[idx(px, py)] = 1;
        let mut q = std::collections::VecDeque::from([(px, py, 0u8)]);
        while let Some((x, y, d)) = q.pop_front() {
            if d >= SOUND_RANGE {
                continue;
            }
            for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h || cells[idx(nx, ny)] != 0 {
                    continue;
                }
                cells[idx(nx, ny)] = d + 2;
                q.push_back((nx, ny, d + 1));
            }
        }
        SoundGrid { w, h, cells }
    }

    fn cell(&self, x: i32, y: i32) -> Option<u8> {
        (x >= 0 && y >= 0 && x < self.w && y < self.h).then(|| self.cells[(x * self.h + y) as usize])
    }

    /// Steps from the party to (x, y), or None if the sound can't reach
    /// it (0x2FACC). A blocking square (a wall actuator, a closed door)
    /// takes its nearest reached neighbour's distance.
    pub fn distance(&self, x: i32, y: i32) -> Option<i32> {
        let mut v = self.cell(x, y)?;
        if v & 0x80 != 0 {
            v = [(0, -1), (1, 0), (0, 1), (-1, 0)]
                .iter()
                .filter_map(|(dx, dy)| self.cell(x + dx, y + dy))
                .filter(|&n| n != 0 && n & 0x80 == 0)
                .min()
                .unwrap_or(0);
        }
        (v != 0).then(|| v as i32 - 1)
    }
}

/// Push a source out to `path` squares along its direction when the path
/// is longer than the straight distance (rounded as 0x15CA9 does).
pub fn stretch(right: i32, forward: i32, dist: i32, path: i32) -> (i32, i32) {
    if dist <= 0 || path <= dist {
        return (right, forward);
    }
    let k = (path << 10) / dist;
    let s = |v: i32| if v < 0 { -((-v * k + 0x200) >> 10) } else { (v * k + 0x200) >> 10 };
    (s(right), s(forward))
}

/// Driver volume 0..1 for a request volume and relative position (0x10877).
pub fn attenuate(vol: u8, right: i32, forward: i32) -> f32 {
    let d2 = (right * right + forward * forward) as u32;
    let out = (((vol as u32) << 8) / (d2 + 8)) >> 5;
    out.min(255) as f32 / 255.0
}

/// The original's pan tables (0x10877), read from the user's SKULL.EXE: eight
/// descending thresholds for (|right| << 11) / |forward| (0x704DE) and
/// sixteen pan words from left to right (0x704EE), 0x8000 being centre.
#[derive(Clone, Debug, PartialEq)]
pub struct PanTable {
    pub thresholds: [u16; 8],
    pub pans: [u16; 16],
}

impl PanTable {
    pub fn from_exe(exe: &crate::exe::Exe) -> Option<PanTable> {
        let mut t = PanTable { thresholds: [0; 8], pans: [0; 16] };
        for (i, v) in t.thresholds.iter_mut().enumerate() {
            *v = exe.u16_at(0x704DE + 2 * i as u32)?;
        }
        for (i, v) in t.pans.iter_mut().enumerate() {
            *v = exe.u16_at(0x704EE + 2 * i as u32)?;
        }
        Some(t)
    }

    /// Pan word for a sound `right` squares to the side and `forward` ahead.
    pub fn word(&self, right: i32, forward: i32) -> u16 {
        if right == 0 {
            return 0x8000;
        }
        if forward == 0 {
            return if right < 0 { self.pans[0] } else { self.pans[15] };
        }
        let ratio = ((right.unsigned_abs() << 11) / forward.unsigned_abs()) as u16;
        let i = self.thresholds.iter().position(|&t| t <= ratio).unwrap_or(7);
        if right < 0 { self.pans[i] } else { self.pans[15 - i] }
    }
}

/// Pan (-1 left .. 1 right) for a relative position: the original's table
/// when available, otherwise a proportional curve.
pub fn pan(table: Option<&PanTable>, right: i32, forward: i32) -> f32 {
    if let Some(t) = table {
        return (t.word(right, forward) as f32 - 32768.0) / 32768.0;
    }
    if right == 0 {
        return 0.0;
    }
    (right as f32 / (right.abs() + forward.abs()) as f32 * 0.8).clamp(-1.0, 1.0)
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
    /// The original's pan table, if SKULL.EXE was available.
    pub pan_table: Option<PanTable>,
}

/// The key a request actually plays: the play wrapper 0x15BAA takes a
/// fallback index (0xFE in every traced call) and substitutes it when the
/// requested index has no sample, before the play function's checks. So a
/// champion's wall-bump cry (0x16, portrait, 0x8A) plays the shared
/// (0x16, 0xFE, 0x8A) sample (docs/11).
pub fn resolve_idx(g: &Gdat, cat: u8, idx: u8, sub: u8) -> u8 {
    if g.record(Key::new(cat, idx, 2, sub)).is_none() && g.record(Key::new(cat, 0xFE, 2, sub)).is_some() {
        0xFE
    } else {
        idx
    }
}

impl Sfx {
    pub fn new(has_header: bool) -> Sfx {
        Sfx { cache: HashMap::new(), playing: Vec::new(), has_header, volume: 1.0, pan_table: None }
    }

    fn sample(&mut self, g: &Gdat, cat: u8, idx: u8, sub: u8) -> Option<Arc<Sample>> {
        let hdr = self.has_header;
        self.cache
            .entry((cat, idx, sub))
            .or_insert_with(|| g.get(Key::new(cat, idx, 2, sub)).and_then(|e| decode(e, hdr)).map(Arc::new))
            .clone()
    }

    /// Start the sounds of one batch (one game tick), applying the original
    /// play function's acceptance rules. `registry` is the party map's set
    /// of playable keys (None accepts every key).
    pub fn play(&mut self, g: &Gdat, dg: &Dungeon, party: &PartyPos, asleep: bool, registry: Option<&Registry>, reqs: &[SoundRequest]) {
        let mut grid: Option<SoundGrid> = None;
        let (mut queued, mut interface) = (Vec::new(), Vec::new());
        for r0 in reqs {
            let r = &SoundRequest { idx: resolve_idx(g, r0.cat, r0.idx, r0.sub), ..*r0 };
            if r.mode >= 1 && r.map != party.map {
                continue;
            }
            if registry.is_some_and(|reg| !reg.contains(r.cat, r.idx, r.sub)) {
                continue;
            }
            let vol = if asleep { r.vol >> 1 } else { r.vol };
            let (mut right, mut forward) = relative(party, r.x, r.y);
            let key = (r.cat, r.idx, r.sub, right, forward);
            let seen = if r.mode < 0 { &mut interface } else { &mut queued };
            let full = if r.mode < 0 { seen.len() >= MAX_INTERFACE } else { r.mode >= 1 && seen.len() >= MAX_PENDING };
            if seen.contains(&key) || full {
                continue;
            }
            seen.push(key);
            let dist = right.abs() + forward.abs();
            if dist > 1 {
                let grid = grid.get_or_insert_with(|| SoundGrid::build(dg, party));
                let Some(path) = grid.distance(r.x, r.y) else { continue };
                (right, forward) = stretch(right, forward, dist, path);
            }
            let Some(sample) = self.sample(g, r.cat, r.idx, r.sub) else { continue };
            if self.playing.len() >= MAX_VOICES {
                self.playing.remove(0);
            }
            let gain = attenuate(vol, right, forward) * self.volume;
            let p = pan(self.pan_table.as_ref(), right, forward);
            self.playing.push(Playing {
                sample,
                pos: 0.0,
                gain_l: gain * (1.0 - p.max(0.0)),
                gain_r: gain * (1.0 + p.min(0.0)),
            });
        }
    }

    /// Stop every playing effect (game over shuts the driver down).
    pub fn stop_all(&mut self) {
        self.playing.clear();
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

    fn table() -> PanTable {
        // Synthetic values in the original's shape: descending thresholds
        // ending at 0, pan words from left to right.
        PanTable { thresholds: [800, 400, 200, 100, 50, 20, 10, 0], pans: std::array::from_fn(|i| 0x0400 + 0x1000 * i as u16) }
    }

    #[test]
    fn pan_table_follows_the_original_rules() {
        let t = table();
        assert_eq!(t.word(0, 3), 0x8000, "straight ahead is centred");
        assert_eq!(t.word(-2, 0), t.pans[0], "directly left");
        assert_eq!(t.word(2, 0), t.pans[15], "directly right");
        // (1 << 11) / 4 = 512 >= 400 -> index 1; left uses pans[1], right pans[14].
        assert_eq!(t.word(-1, 4), t.pans[1]);
        assert_eq!(t.word(1, 4), t.pans[14]);
        // A far, nearly centred sound only passes the last threshold.
        assert_eq!(t.word(1, 255), t.pans[8]);
        assert!(pan(Some(&t), -1, 4) < 0.0 && pan(Some(&t), 1, 4) > 0.0);
        assert_eq!(pan(Some(&t), 0, 2), 0.0);
    }

    #[test]
    fn pan_table_loads_from_the_executable() {
        let Some(exe) = crate::exe::Exe::open(&crate::exe_tables::default_exe_path()) else { return };
        let t = PanTable::from_exe(&exe).expect("pan tables");
        assert!(t.thresholds.windows(2).all(|w| w[0] > w[1]) && t.thresholds[7] == 0);
        assert!(t.pans.windows(2).all(|w| w[0] < w[1]));
        assert!(t.pans[7] < 0x8000 && t.pans[8] > 0x8000, "centre lies between the two middle entries");
    }

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
    fn attenuation_follows_the_driver_curve() {
        // At the party the volume byte passes through unchanged.
        assert!((attenuate(200, 0, 0) - 200.0 / 255.0).abs() < 1e-6);
        // Four squares ahead: 8 / (16 + 8) of the volume.
        assert_eq!((attenuate(200, 0, 4) * 255.0).round() as u32, ((200u32 << 8) / 24) >> 5);
        assert!(attenuate(200, 0, 4) > attenuate(200, 0, 5));
        assert!(attenuate(0x80, 0, 0) < attenuate(200, 0, 0));
    }

    #[test]
    fn stretch_pushes_sources_out_to_the_path_length() {
        assert_eq!(stretch(0, 2, 2, 2), (0, 2));
        assert_eq!(stretch(0, 2, 2, 6), (0, 6));
        assert_eq!(stretch(-1, 1, 2, 4), (-2, 2));
    }

    #[test]
    fn sound_grid_stops_at_walls_and_range() {
        let Ok(bytes) = std::fs::read(crate::assets::default_data_dir().join("DUNGEON.DAT")) else { return };
        let dg = Dungeon::parse(&bytes).unwrap();
        // Map 1: the party at (2,9) outside the walled courtyard; (10,9)
        // inside it is not reachable, (2,5) straight up the open column is
        // 4 steps away.
        let party = PartyPos { map: 1, x: 2, y: 9, dir: 0 };
        let grid = SoundGrid::build(&dg, &party);
        assert_eq!(grid.distance(2, 9), Some(0));
        assert_eq!(grid.distance(2, 5), Some(4));
        assert_eq!(grid.distance(10, 9), None, "courtyard behind the trick wall");
        assert_eq!(grid.distance(2, 18), None, "beyond 8 steps");
        // A wall square takes its nearest reached neighbour.
        assert!(grid.distance(6, 9).is_some());
    }

    #[test]
    fn drains_only_sounds() {
        let mut q = vec![Effect::Sound { cat: 3, idx: 0, sub: 1, map: 0, x: 1, y: 1 }, Effect::EndGame];
        let s = drain_sounds(&mut q);
        assert_eq!(s.len(), 1);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn missing_sound_keys_fall_back_to_the_shared_index() {
        let Ok(g) = Gdat::open(dm2_formats::gdat::default_path()) else { return };
        // The wall-bump cry is requested per portrait but only the shared
        // 0xFE entry has a sample (0x15BAA's fallback, docs/11).
        assert!(g.record(Key::new(0x16, 14, 2, 0x8A)).is_none());
        assert_eq!(resolve_idx(&g, 0x16, 14, 0x8A), 0xFE);
        // A key with its own sample keeps its index.
        let own = g
            .records
            .iter()
            .find(|r| r.key.cat == 0x16 && r.key.kind == 2 && r.key.idx != 0xFE)
            .expect("a per-portrait champion sound");
        assert_eq!(resolve_idx(&g, 0x16, own.key.idx, own.key.sub), own.key.idx);
        // No shared entry either: the index is left alone (nothing plays).
        assert_eq!(resolve_idx(&g, 0x16, 14, 0x7F), 14);
    }
}
