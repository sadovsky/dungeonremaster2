//! Outdoor clock and weather (docs/04 "Outdoor weather and time of day",
//! docs/05 event 0x54).
//!
//! The hour clock feeds a light term into the darkness step; the rain
//! intensity follows one of four 32-step curves driven by timeline event
//! 0x54, and decides the cloud/storm backdrops and the rain overlay drawn
//! by the viewport. All tables are read from the user's SKULL.EXE.

use dm2_formats::gdat::Key;

use crate::state::GameState;
use crate::timeline::Event;

/// Timeline event that steps the rain curve (scheduled by 0x59F12).
pub const EV_WEATHER: u8 = 0x54;
/// Ticks per hour (0x555), 24 hours per day.
pub const HOUR_TICKS: u32 = 0x555;

/// Per-hour light adjustment, 24 signed bytes (0x80472 source).
const HOUR_LIGHT: u32 = 0x760EC;
/// Per-environment-state table, 4 bytes per state: +0 outdoor light flag
/// (0x8047B), +2 rain overlay allowed (checked by 0x4E79C).
const ENV_STATES: u32 = 0x75BE2;
/// Rain curves: 4 patterns of 32 signed steps (0x76104).
const RAIN_CURVES: u32 = 0x76104;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Weather {
    /// Tick of the next hour change (0x80430).
    pub next_hour: u32,
    /// Hour offset in ticks from the dungeon (0x80434): attribute (3,0,11,0) × 0x555.
    pub hour_offset: u32,
    /// Light adjustment for the current hour (0x80472).
    pub hour_light: i16,
    /// Environment state index of the map set (0x8046E, attribute 0x66).
    pub state: u16,
    /// Outdoor light model on (0x8047B).
    pub env: bool,
    /// Storm darkening flag (0x8047C), set by the heaviest cloud level.
    pub storm: u8,
    /// Rain intensity 0..255 (0x80470).
    pub rain: u16,
    /// Rain intensity before the last curve step (0x8046A).
    pub rain_prev: u16,
    /// Raining: 0 none, else the drawn level (0x8047E).
    pub rain_on: u8,
    /// Cloud build-up counter (0x80479).
    pub cloud: u8,
    /// Cloud backdrop level (0x8047A).
    pub cloud_level: u8,
    /// Wind direction (0x8047F).
    pub wind: u8,
    /// Curve pattern 0..3 (0x80474) and its multiplier (0x80480).
    pub pattern: u16,
    pub kind: u8,
    /// Curve step 0..32 (0x80477).
    pub step: u8,
    /// Thunder range (0x8046C), 4..7.
    pub thunder_range: u16,
    /// First weather cycle of a game still pending (0x76184).
    pub first: bool,
    /// Lightning flash: forces full light for the next darkness update (0x7F248).
    pub flash: bool,
    /// Map set features present (0x3AB31): lightning, storm and cloud
    /// backdrops, rain overlays.
    pub can_lightning: bool,
    pub can_storm: bool,
    pub can_cloud: bool,
    pub can_rain: bool,
    /// Map whose set was last read.
    pub map_seen: Option<usize>,
    /// The clock has been set up (new game, or restored from a save). A
    /// save written by the DOS game carries no weather block; the hour
    /// offset is then rederived from the dungeon, as the original does
    /// whenever it parses one (0x36909), without any random draws.
    pub ready: bool,
}

/// What the viewport draws for the weather.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WeatherView {
    /// Extra backdrop scripts (category 23 subs 0x67-0x6C).
    pub backdrops: Vec<u8>,
    /// Rain overlay sub (0x6D-0x74) and whether it is mirrored.
    pub rain: Option<(u8, bool)>,
}

fn table(g: &GameState, addr: u32, len: usize) -> Option<Vec<u8>> {
    g.data.as_ref()?.exe.slice(addr, len).map(<[u8]>::to_vec)
}

fn set_of(g: &GameState) -> u8 {
    g.dungeon.maps[g.party.map].tileset
}

fn hour_light_now(g: &GameState) -> i16 {
    let hour = ((g.tick.wrapping_add(g.weather.hour_offset) / HOUR_TICKS) % 24) as u32;
    table(g, HOUR_LIGHT + hour, 1).map_or(0, |b| b[0] as i8 as i16)
}

/// Read the map set's weather features (part of 0x3AB31, on map entry).
pub fn enter_map(g: &mut GameState) {
    let set = set_of(g);
    let Some(d) = g.data.clone() else { return };
    let has = |sub: u8| d.gdat.record(Key::new(23, set, 1, sub)).is_some();
    let w = &mut g.weather;
    w.state = d.gdat.lookup(Key::new(8, set, 11, 0x66)).unwrap_or(0);
    w.can_lightning = has(100);
    w.can_storm = has(0x6A);
    w.can_cloud = has(0x67);
    w.can_rain = has(0x71);
    w.map_seen = Some(g.party.map);
}

/// Recompute the derived weather state for the party's map without drawing
/// random numbers: the map set's features, the outdoor flag and the hour
/// light. The original does this when a game is loaded, before the first
/// frame; the remake also calls it after loading a save.
pub fn refresh(g: &mut GameState) {
    if g.data.is_none() {
        return;
    }
    // The hour offset isn't game state: the dungeon loader recomputes it
    // from attribute (3,0,11,0) on every load (0x36909 path, x 0x555).
    let hour = g.data.as_ref().and_then(|d| d.gdat.lookup(Key::new(3, 0, 11, 0))).unwrap_or(0).min(23) as u32;
    g.weather.hour_offset = hour * HOUR_TICKS;
    if !g.weather.ready {
        g.weather.next_hour = g.tick.wrapping_add(HOUR_TICKS);
        g.weather.ready = true;
    }
    enter_map(g);
    g.weather.env = table(g, ENV_STATES + g.weather.state as u32 * 4, 1).is_some_and(|b| b[0] != 0);
    g.weather.hour_light = hour_light_now(g);
}

/// Weather fields of the save's 60-byte globals record, as the original
/// writes them (0x3502B) and reads them back on load:
///
/// | Offset | Size | Global |
/// |--------|------|--------|
/// | 0x2A | u32 | 0x8047B outdoor-light flag |
/// | 0x2E | u8 | 0x8047C storm |
/// | 0x2F | u8 | 0x8047F wind |
/// | 0x30 | u8 | 0x8047E raining level |
/// | 0x31 | u8 | 0x8047A cloud backdrop level |
/// | 0x32 | u8 | 0x80479 cloud build-up |
/// | 0x33 | u8 | 0x80480 curve multiplier |
/// | 0x34 | u16 | 0x80470 rain intensity |
/// | 0x36 | u8 | 0x80477 curve step |
/// | 0x37 | u8 | 0x80474 curve pattern |
/// | 0x38 | u32 | 0x80430 tick of the next hour change |
pub fn read_globals(w: &mut Weather, r: &[u8]) {
    if r.len() < 0x3C {
        return;
    }
    w.env = u32::from_le_bytes([r[0x2A], r[0x2B], r[0x2C], r[0x2D]]) != 0;
    w.storm = r[0x2E];
    w.wind = r[0x2F];
    w.rain_on = r[0x30];
    w.cloud_level = r[0x31];
    w.cloud = r[0x32];
    w.kind = r[0x33];
    w.rain = u16::from_le_bytes([r[0x34], r[0x35]]);
    w.step = r[0x36];
    w.pattern = r[0x37] as u16;
    w.next_hour = u32::from_le_bytes([r[0x38], r[0x39], r[0x3A], r[0x3B]]);
    w.ready = true;
}

/// Write the weather fields into a 60-byte globals record (see `read_globals`).
pub fn write_globals(w: &Weather, r: &mut [u8]) {
    if r.len() < 0x3C {
        return;
    }
    r[0x2A..0x2E].copy_from_slice(&u32::from(w.env).to_le_bytes());
    r[0x2E] = w.storm;
    r[0x2F] = w.wind;
    r[0x30] = w.rain_on;
    r[0x31] = w.cloud_level;
    r[0x32] = w.cloud;
    r[0x33] = w.kind;
    r[0x34..0x36].copy_from_slice(&w.rain.to_le_bytes());
    r[0x36] = w.step;
    r[0x37] = w.pattern as u8;
    r[0x38..0x3C].copy_from_slice(&w.next_hour.to_le_bytes());
}

/// New-game setup from the dungeon (0x36909): the hour offset, then the
/// first weather cycle.
pub fn new_game(g: &mut GameState) {
    let hour = g
        .data
        .as_ref()
        .and_then(|d| d.gdat.lookup(Key::new(3, 0, 11, 0)))
        .unwrap_or(0)
        .min(23) as u32;
    g.weather.hour_offset = hour * HOUR_TICKS;
    g.weather.first = true;
    g.weather.ready = true;
    enter_map(g);
    init(g, true);
}

/// Start a weather cycle (0x59F38). `reset` is the full restart.
pub fn init(g: &mut GameState, reset: bool) {
    if reset {
        thunder(g);
        g.weather.next_hour = g.tick.wrapping_add(HOUR_TICKS);
        g.weather.storm = 0;
        g.weather.env = false;
        let delay;
        if !g.weather.first {
            delay = g.rng.random(8000) as u32 + 500;
            g.weather.pattern = g.rng.rand4();
            g.weather.kind = g.rng.random(3) as u8 + 1;
        } else {
            g.weather.cloud = 0;
            delay = g.rng.random(500) as u32;
            g.weather.pattern = 3;
            g.weather.kind = 1;
        }
        let w = &mut g.weather;
        w.cloud_level = 1;
        w.rain_on = 0;
        w.rain = 0;
        w.rain_prev = 0;
        w.step = 0;
        w.wind = g.rng.rand4() as u8;
        schedule(g, delay);
    } else {
        g.weather.rain_prev = 0;
        if g.weather.kind == 0 {
            g.weather.kind = 1;
        }
    }
    g.weather.thunder_range = g.rng.random(4) + 4;
    g.weather.hour_light = hour_light_now(g);
    g.weather.first = false;
}

/// Thunder (0x5A073): sound (0x17, map set, 0) at the party's square. The
/// original plays it through the delayed-sound queue, 1-15 ticks after the
/// flash depending on distance; the remake plays it at once.
fn thunder(g: &mut GameState) {
    let p = g.party;
    let set = g.dungeon.maps[p.map].tileset;
    g.effects.push(crate::effects::Effect::Sound { cat: 0x17, idx: set, sub: 0, map: p.map, x: p.x, y: p.y });
}

fn schedule(g: &mut GameState, delay: u32) {
    let ev = Event::new(EV_WEATHER, g.party.map as u8, g.tick.wrapping_add(delay));
    g.schedule(ev);
}

/// Event 0x54 (0x5A073 with argument 1): one step along the rain curve.
pub fn event(g: &mut GameState, _ev: Event) {
    g.weather.step += 1;
    if g.weather.step < 0x20 {
        let w = &g.weather;
        let idx = w.step as u32 + w.pattern as u32 * 0x20;
        let delta = table(g, RAIN_CURVES + idx, 1).map_or(0, |b| b[0] as i8 as i32);
        let w = &mut g.weather;
        w.rain_prev = w.rain;
        w.rain = (w.rain as i32 + w.kind as i32 * delta).clamp(0, 0xFF) as u16;
        let delay = g.rng.random(0x100) as u32 + 0x32;
        schedule(g, delay);
    } else {
        init(g, true);
    }
}

/// Per-tick update (0x5A073 with argument 0), run right after the timeline.
pub fn tick(g: &mut GameState) {
    if g.data.is_none() {
        return;
    }
    if !g.weather.ready {
        let hour = g.data.as_ref().and_then(|d| d.gdat.lookup(Key::new(3, 0, 11, 0))).unwrap_or(0).min(23) as u32;
        g.weather.hour_offset = hour * HOUR_TICKS;
        g.weather.hour_light = hour_light_now(g);
        g.weather.next_hour = g.tick.wrapping_add(HOUR_TICKS);
        g.weather.ready = true;
    }
    if g.weather.map_seen != Some(g.party.map) {
        enter_map(g);
    }
    g.weather.env = table(g, ENV_STATES + g.weather.state as u32 * 4, 1).is_some_and(|b| b[0] != 0);
    if g.weather.next_hour <= g.tick {
        g.weather.hour_light = hour_light_now(g);
        g.weather.next_hour = g.tick.wrapping_add(HOUR_TICKS);
    }
    let mut strike;
    if g.weather.rain == 0 {
        if g.weather.cloud != 0 && g.tick % 3 == 0 {
            g.weather.cloud -= 1;
        }
        strike = g.rng.random(0x40) == 0;
        g.weather.rain_on = 0;
        g.weather.cloud_level = 1;
    } else {
        let rain = g.weather.rain;
        let span = (0x100 - rain) + (g.rng.rnd() & 0x0F) as u16;
        let thr = if rain < 0xCD { 7 } else { 0x28 };
        g.weather.cloud_level = rain as u8;
        g.weather.rain_on = if g.weather.rain_on == 0 { u8::from(g.rng.random(span) < 8) } else { rain as u8 };
        // Cloud build-up while it rains (the decompiled condition is
        // tangled; this follows its visible branches).
        let (on, t) = (g.weather.rain_on, g.tick);
        if on != 0 && g.weather.cloud < 0xFF {
            let grow = on >= 0x80 || ((on < 0x40 || t & 1 != 0) && ((on > 0x0F && t % 3 == 0) || t & 3 == 0));
            if grow {
                g.weather.cloud += 1;
            }
        }
        strike = false;
        if g.weather.can_lightning {
            strike = g.rng.random(span) <= thr;
        }
    }
    if g.weather.env && g.weather.flash {
        g.weather.flash = false;
    }
    if strike {
        lightning(g);
    }
}

/// A storm strike (part of 0x5A073): thunder, and with enough rain a
/// lightning explosion on a random open square of the party's map.
/// Simplified: the original's square test (0x1E908 / 0x1D113), its
/// party-distance thunder rule and the fixed strike square of attribute
/// (8, set, 11, 0x6C) are only partly modelled.
fn lightning(g: &mut GameState) {
    let rain = g.weather.rain;
    if rain < 0xB6 {
        thunder(g);
    }
    if g.rng.random(rain + 1) <= 0x3B {
        return;
    }
    let attempts = (g.rng.rnd() & 7) + 1;
    let map = g.party.map;
    let (w, h) = (g.dungeon.maps[map].width as i32, g.dungeon.maps[map].height as i32);
    for _ in 0..attempts {
        let x = g.rng.random(0x20) as i32;
        let y = g.rng.random(0x20) as i32;
        if x >= w || y >= h {
            continue;
        }
        if g.dungeon.square(map, x, y).element() != dm2_formats::dungeon::Element::Floor {
            continue;
        }
        crate::missiles::explode(g, 0xFFB0, rain as u8, map, x, y, 0xFF);
        g.weather.flash = true;
        break;
    }
}

/// The darkness-step term of the outdoor light model (0x389C2): added to
/// the light sum when the environment flag is on. `thresholds` is the
/// step table at 0x7570E.
pub fn light_term(g: &GameState, thresholds: &[u8]) -> i32 {
    if !g.weather.env || thresholds.is_empty() {
        return 0;
    }
    let i = (g.weather.storm as i32 + g.weather.hour_light as i32).clamp(0, 5) as usize;
    thresholds.get(i).map_or(0, |&b| b as i8 as i32)
}

/// What the viewport should add for the current weather (0x5A073's
/// backdrop list and 0x4E733's rain overlay choice).
pub fn view(g: &GameState) -> WeatherView {
    let w = &g.weather;
    let mut v = WeatherView::default();
    if !w.env {
        return v;
    }
    if w.can_cloud && w.cloud_level > 0x0F {
        v.backdrops.push(if w.cloud_level >= 0x80 { 0x69 } else if w.cloud_level >= 0x40 { 0x68 } else { 0x67 });
    }
    if w.can_storm && w.cloud > 0x3F {
        v.backdrops.push(if w.cloud >= 0xC0 { 0x6C } else if w.cloud >= 0x80 { 0x6B } else { 0x6A });
    }
    let rain_allowed = table(g, ENV_STATES + 2 + w.state as u32 * 4, 1).is_some_and(|b| b[0] != 0);
    if w.rain_on != 0 && w.can_rain && rain_allowed {
        let rel = w.wind.wrapping_sub(g.party.dir) & 3;
        let base = if rel == 0 || rel == 2 { 0x71 } else { 0x6D };
        let level = match w.rain_on {
            0x80.. => 3,
            0x40.. => 2,
            0x10.. => 1,
            _ => 0,
        };
        v.rain = Some((base + level, rel == 1));
    }
    v
}

/// The heaviest cloud level also sets the storm darkening flag (0x8047C).
pub fn update_storm_flag(g: &mut GameState) {
    if g.weather.env && g.weather.can_cloud && g.weather.cloud_level >= 0x80 {
        g.weather.storm = 1;
    }
}

/// Bytes the weather takes in the remake's save trailer.
pub const SAVE_BYTES: usize = 28;

impl Weather {
    /// Serialise the persistent fields for the save trailer. The feature
    /// enables and the map seen are rebuilt on load.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(SAVE_BYTES);
        o.extend_from_slice(&self.next_hour.to_le_bytes());
        o.extend_from_slice(&self.hour_offset.to_le_bytes());
        o.extend_from_slice(&self.hour_light.to_le_bytes());
        o.extend_from_slice(&self.state.to_le_bytes());
        o.extend_from_slice(&self.rain.to_le_bytes());
        o.extend_from_slice(&self.rain_prev.to_le_bytes());
        o.extend_from_slice(&self.pattern.to_le_bytes());
        o.extend_from_slice(&self.thunder_range.to_le_bytes());
        o.extend_from_slice(&[self.storm, self.rain_on, self.cloud, self.cloud_level, self.wind, self.kind]);
        o.push(self.step);
        o.push(u8::from(self.env) | u8::from(self.first) << 1 | u8::from(self.flash) << 2);
        debug_assert_eq!(o.len(), SAVE_BYTES);
        o
    }

    pub fn from_bytes(b: &[u8]) -> Option<Weather> {
        if b.len() < SAVE_BYTES {
            return None;
        }
        let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        Some(Weather {
            next_hour: u32_at(0),
            hour_offset: u32_at(4),
            hour_light: u16_at(8) as i16,
            state: u16_at(10),
            rain: u16_at(12),
            rain_prev: u16_at(14),
            pattern: u16_at(16),
            thunder_range: u16_at(18),
            storm: b[20],
            rain_on: b[21],
            cloud: b[22],
            cloud_level: b[23],
            wind: b[24],
            kind: b[25],
            ready: true,
            step: b[26],
            env: b[27] & 1 != 0,
            first: b[27] & 2 != 0,
            flash: b[27] & 4 != 0,
            ..Weather::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::data::GameData;

    /// Night, day and rainy-day outdoor views at map 1 (2,9) facing north.
    // Backdrops are keyed with the map set's colour key and weather layers
    // draw behind the landmarks (checked against the original's outdoor
    // captures in DOSBox).
    const PINNED_OUTDOOR: (u64, u64, u64) = (0x6d709e3cb00e2ea7, 0xe25ac480f2385f8d, 0x3bffddab2fc9c966);

    fn game() -> Option<GameState> {
        let gd = Rc::new(GameData::load_default()?);
        let bytes = std::fs::read(crate::assets::default_data_dir().join("DUNGEON.DAT")).ok()?;
        let dg = dm2_formats::dungeon::Dungeon::parse(&bytes).ok()?;
        Some(GameState::new_game_with(&dg, gd))
    }

    #[test]
    fn thunder_plays_the_map_sets_sound_at_the_party() {
        let Some(mut g) = game() else { return };
        g.effects.clear();
        thunder(&mut g);
        let set = g.dungeon.maps[g.party.map].tileset;
        let p = g.party;
        assert!(g.effects.iter().any(|e| matches!(e,
            crate::effects::Effect::Sound { cat: 0x17, idx, sub: 0, x, y, .. } if *idx == set && *x == p.x && *y == p.y)));
    }

    #[test]
    fn hour_clock_follows_the_table() {
        let Some(mut g) = game() else { return };
        let table = table(&g, HOUR_LIGHT, 24).unwrap();
        for hours in [0u32, 5, 23] {
            g.tick = hours * HOUR_TICKS;
            g.weather.next_hour = g.tick;
            tick(&mut g);
            let hour = ((g.tick + g.weather.hour_offset) / HOUR_TICKS % 24) as usize;
            assert_eq!(g.weather.hour_light, table[hour] as i8 as i16, "hour {hour}");
            assert_eq!(g.weather.next_hour, g.tick + HOUR_TICKS);
        }
    }

    #[test]
    fn weather_cycle_is_deterministic_and_bounded() {
        let Some(base) = game() else { return };
        let run = || {
            let mut g = base.clone();
            g.rng.state = 0x1234_5678;
            g.weather.first = false;
            init(&mut g, true);
            let start = (g.weather.pattern, g.weather.kind, g.weather.wind);
            let mut rains = Vec::new();
            for _ in 0..40 {
                let ev = Event::new(EV_WEATHER, 0, g.tick);
                event(&mut g, ev);
                rains.push(g.weather.rain);
            }
            (start, rains, g.rng.state)
        };
        let (a, b) = (run(), run());
        assert_eq!(a, b);
        let ((pattern, kind, wind), rains, _) = a;
        assert!(pattern < 4 && (1..=3).contains(&kind) && wind < 4);
        assert!(rains.iter().all(|&r| r <= 0xFF));
    }

    #[test]
    fn save_bytes_round_trip() {
        let w = Weather {
            next_hour: 0x12345,
            hour_offset: 7 * HOUR_TICKS,
            hour_light: -2,
            state: 3,
            rain: 0x90,
            rain_prev: 0x80,
            pattern: 2,
            thunder_range: 6,
            storm: 1,
            rain_on: 0x90,
            cloud: 0x45,
            cloud_level: 0x90,
            wind: 3,
            kind: 2,
            step: 9,
            env: true,
            first: false,
            flash: true,
            ready: true,
            ..Weather::default()
        };
        let b = w.to_bytes();
        assert_eq!(b.len(), SAVE_BYTES);
        assert_eq!(Weather::from_bytes(&b).unwrap(), w);
    }

    /// Render the outdoor view at map 1 (2,9) facing north for a given hour,
    /// through the real darkness path; returns (darkness step, view hash).
    fn outdoor(hour: u32, rain: Option<u8>) -> Option<(u16, u64)> {
        let mut g = game()?;
        let exe = std::fs::read(crate::exe_tables::default_exe_path()).ok()?;
        let gdat = Rc::new(dm2_formats::gdat::Gdat::open(crate::assets::default_data_dir().join("GRAPHICS.DAT")).ok()?);
        let cd = crate::creatures::data::CreatureData::load(gdat, &exe).ok()?;
        let mut a = crate::assets::Assets::load(&crate::assets::default_data_dir()).ok()?;
        g.party = crate::world::PartyPos { map: 1, x: 2, y: 9, dir: 0 };
        g.weather.hour_offset = 0;
        g.tick = hour * HOUR_TICKS;
        g.weather.next_hour = g.tick;
        tick(&mut g);
        if let Some(level) = rain {
            g.weather.rain = level as u16;
            g.weather.rain_on = level;
        }
        let step = crate::creatures::fight::darkness_level(&g, &cd);
        let ex = crate::viewport::ViewExtras {
            tick: g.tick,
            darkness_step: step as i32,
            ambient: step as i32 * 10,
            weather: view(&g),
            ..Default::default()
        };
        let bm = crate::viewport::render_ex(&mut a, &g.dungeon, 1, 2, 9, 0, &ex);
        let lum: u64 = bm.px.iter().map(|&c| a.palette[c as usize].iter().map(|&v| v as u64).sum::<u64>()).sum();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in &bm.px {
            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
        eprintln!("hour {hour} rain {rain:?}: step {step} luminance {lum} hash {h:#x}");
        Some((step, h))
    }

    #[test]
    fn outdoor_view_follows_the_clock() {
        let Some((night, night_hash)) = outdoor(1, None) else { return };
        let (day, day_hash) = outdoor(10, None).unwrap();
        assert!(day < night, "day step {day} should be lighter than night step {night}");
        assert_ne!(day_hash, night_hash);
        let (_, rain_hash) = outdoor(10, Some(0x90)).unwrap();
        assert_ne!(rain_hash, day_hash, "rain overlay should change the view");
        assert_eq!((night_hash, day_hash, rain_hash), PINNED_OUTDOOR);
    }

    #[test]
    fn light_term_only_outdoors() {
        let Some(mut g) = game() else { return };
        let thr = [10u8, 20, 30, 40, 50, 60];
        g.weather.env = false;
        assert_eq!(light_term(&g, &thr), 0);
        g.weather.env = true;
        g.weather.storm = 1;
        g.weather.hour_light = 2;
        assert_eq!(light_term(&g, &thr), 40);
        g.weather.hour_light = 9;
        assert_eq!(light_term(&g, &thr), 60, "index clamps at 5");
    }
}
