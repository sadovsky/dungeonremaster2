//! Audio: positional sound effects and FM music (docs/11-audio.md).
//!
//! Presentation only: nothing here changes the game state. Everything is
//! loaded at runtime from the user's own install (GRAPHICS.DAT for samples
//! and songs, SONGLIST.DAT, MELODIC.BNK and DRUM.BNK). `Audio::render`
//! produces interleaved stereo f32 frames for whatever output the frontend
//! uses.

pub mod bnk;
pub mod hmp;
pub mod midi;
pub mod music;
pub mod opl;
pub mod registry;
pub mod sfx;

use std::collections::HashMap;
use std::path::Path;

use dm2_formats::dungeon::Dungeon;
use dm2_formats::gdat::{Gdat, Key};

use crate::world::PartyPos;
use bnk::Bank;
use hmp::Song;
use midi::Driver;
use music::{Sequencer, SongList, FADE_START};
use registry::Registry;
use sfx::{Sfx, SoundRequest};

/// FM output scale at the default music level, calibrated so map 0's song
/// matches the original's level in DOSBox (docs/11, "Mixer levels").
const MUSIC_GAIN: f32 = 0.18;
/// Sample playback gain relative to the FM music scale. Calibrated on the
/// wall-bump cry recorded from the original in DOSBox (docs/11): at full
/// digital master the remake's cry peaked 17.9 dB above the original's.
const SFX_GAIN: f32 = 0.127;
/// Loudness exponent of the song volume during a fade (amplitude goes as
/// (volume / 127)^FADE_EXP); fitted to the original's stairs fade.
pub const FADE_EXP: f32 = 2.5;
/// Default options levels (0-7): music 5 and digital 7 at startup (0x103D7, 0x1169A).
pub const DEFAULT_MUSIC_LEVEL: u8 = 5;
pub const DEFAULT_DIGITAL_LEVEL: u8 = 7;

/// MIDI master volume byte for an options music level (0x10736): l*2 | l<<4.
pub fn midi_master(level: u8) -> u8 {
    let l = level.min(7);
    l * 2 | l << 4
}

/// Digital master volume (0-0x7FFF) for an options level (0x10736).
pub fn digital_master(level: u8) -> u16 {
    let l = level.min(7) as u16;
    l << 12 | l << 9 | l << 6 | l << 3 | l
}

pub struct Audio {
    sample_rate: u32,
    gdat: Gdat,
    songlist: SongList,
    songs: HashMap<u8, Option<Song>>,
    driver: Driver,
    /// Playing song number and its sequencer.
    current: Option<(u8, Sequencer)>,
    /// Song to start when the fade-out ends (0x7EF86; 0 = silence).
    pending: Option<u8>,
    /// Fade counter (0x704C6): 0 = none, else the music volume out of 127,
    /// stepped down once per game tick by `music_tick`.
    fade: u8,
    /// Song most recently chosen for the party's map (0x7050E).
    map_song: Option<u8>,
    sfx: Sfx,
    /// Sound keys registered for the party's map, and the portraits it
    /// was built with.
    registry: Option<(Registry, Vec<u8>)>,
    pub music_volume: f32,
    pub sfx_volume: f32,
    /// Options volume levels 0-7 (music 0 stops the music).
    pub music_level: u8,
    pub digital_level: u8,
}

#[derive(Debug)]
pub enum AudioError {
    Missing(std::path::PathBuf),
    Bad(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioError::Missing(p) => write!(f, "missing {}", p.display()),
            AudioError::Bad(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for AudioError {}

impl Audio {
    /// Load from the game's DATA directory; the instrument banks are read
    /// from its parent (the install directory).
    pub fn load(data_dir: &Path, sample_rate: u32) -> Result<Audio, AudioError> {
        let read = |p: std::path::PathBuf| std::fs::read(&p).map_err(|_| AudioError::Missing(p));
        let gdat = Gdat::from_bytes(read(data_dir.join("GRAPHICS.DAT"))?).map_err(|e| AudioError::Bad(format!("GRAPHICS.DAT: {e:?}")))?;
        let songlist = SongList(read(data_dir.join("SONGLIST.DAT"))?);
        let bank = |name: &str| -> Result<Bank, AudioError> {
            Bank::parse(&read(data_dir.join("..").join(name))?).map_err(|e| AudioError::Bad(format!("{name}: {e:?}")))
        };
        let (melodic, drums) = (bank("MELODIC.BNK")?, bank("DRUM.BNK")?);
        // Archive flag 0x20: samples carry the 6-byte header (docs/11).
        let has_header = gdat.lookup(Key::new(0, 0, 11, 0)).unwrap_or(0) & 0x20 != 0;
        let mut audio = Audio::from_parts(gdat, songlist, melodic, drums, has_header, sample_rate);
        // The pan table lives in the executable; without it a proportional pan is used.
        audio.sfx.pan_table =
            crate::exe::Exe::open(&data_dir.join("../SKULL.EXE")).and_then(|e| sfx::PanTable::from_exe(&e));
        Ok(audio)
    }

    pub fn from_parts(gdat: Gdat, songlist: SongList, melodic: Bank, drums: Bank, has_header: bool, sample_rate: u32) -> Audio {
        Audio {
            sample_rate: sample_rate.max(8000),
            gdat,
            songlist,
            songs: HashMap::new(),
            driver: Driver::new(melodic, drums),
            current: None,
            pending: None,
            fade: 0,
            map_song: None,
            sfx: Sfx::new(has_header),
            registry: None,
            music_volume: 1.0,
            sfx_volume: 1.0,
            music_level: DEFAULT_MUSIC_LEVEL,
            digital_level: DEFAULT_DIGITAL_LEVEL,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Parsed song `n`, key (4, n, 3, 0). Song numbers in SONGLIST.DAT are
    /// used directly as the index (tentative: 0 is treated as silence).
    fn song(&mut self, n: u8) -> Option<Song> {
        let g = &self.gdat;
        self.songs.entry(n).or_insert_with(|| g.get(Key::new(4, n, 3, 0)).and_then(|d| Song::parse(d).ok())).clone()
    }

    fn start(&mut self, n: u8) {
        self.driver.reset();
        self.current = if n == 0 { None } else { self.song(n).map(|s| (n, Sequencer::new(s))) };
    }

    /// Start song `n` at once, with no fade (0 = silence). Used for
    /// listening checks; gameplay goes through `music_tick`.
    pub fn play_song(&mut self, n: u8) {
        self.pending = None;
        self.fade = 0;
        self.map_song = Some(n);
        self.start(n);
    }

    /// One game tick of the original's music update (0x10AF6), run by the
    /// main loop once per tick with the party's map. A different song for
    /// the map starts at once if nothing is playing or a fade is already
    /// under way; otherwise the current song fades out over 126 ticks (the
    /// counter starts at 127 and is the music volume) and the new song then
    /// starts at full volume.
    pub fn music_tick(&mut self, map: usize) {
        if self.fade == 1 {
            let n = self.pending.take().unwrap_or(0);
            self.fade = 0;
            self.start(n);
            return;
        }
        if self.fade >= 2 {
            self.fade -= 1;
        }
        let Some(n) = self.songlist.song_for_map(map) else { return };
        if self.map_song == Some(n) {
            return;
        }
        self.map_song = Some(n);
        if self.current.is_none() || self.fade != 0 {
            self.pending = None;
            self.fade = 0;
            self.start(n);
        } else {
            self.pending = Some(n);
            self.fade = FADE_START;
        }
    }

    /// Follow the party's map for one tick (kept for older callers).
    pub fn set_map(&mut self, map: usize) {
        self.music_tick(map);
    }

    pub fn current_song(&self) -> Option<u8> {
        self.current.as_ref().map(|c| c.0)
    }

    /// Queue the sound requests of one game tick (awake, no champions).
    pub fn play_sounds(&mut self, dg: &Dungeon, party: &PartyPos, reqs: &[SoundRequest]) {
        self.play_tick(dg, party, false, &[], reqs);
    }

    /// Queue the sound requests of one game tick through the original's
    /// play rules: the party map's registered keys, the sound-distance
    /// grid, distance attenuation and halved volume while `asleep`.
    /// `portraits` are the recruited champions' portrait numbers.
    pub fn play_tick(&mut self, dg: &Dungeon, party: &PartyPos, asleep: bool, portraits: &[u8], reqs: &[SoundRequest]) {
        if reqs.is_empty() {
            return;
        }
        let stale = !matches!(&self.registry, Some((r, p)) if r.map == party.map && p == portraits);
        if stale {
            self.registry = Some((Registry::for_map(&self.gdat, dg, party.map, portraits), portraits.to_vec()));
        }
        let Audio { sfx, gdat, registry, .. } = self;
        sfx.play(gdat, dg, party, asleep, registry.as_ref().map(|(r, _)| r), reqs);
    }

    /// Stop the music and every effect: the end of the game shuts both
    /// sound drivers down (0x2005B -> 0x10D29 -> 0x1043C).
    pub fn stop_all(&mut self) {
        self.current = None;
        self.pending = None;
        self.fade = 0;
        self.sfx.stop_all();
    }

    /// Render interleaved stereo frames into `out` (length must be even).
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let sr = self.sample_rate as f64;
        if self.current.is_some() {
            // The options level scales like the song volume, relative to the
            // calibrated default.
            let master = midi_master(self.music_level) as f32 / midi_master(DEFAULT_MUSIC_LEVEL) as f32;
            let mut gain = MUSIC_GAIN * master.powf(FADE_EXP) * self.music_volume;
            if self.fade >= 2 {
                gain *= (self.fade as f32 / 127.0).powf(FADE_EXP);
            }
            for frame in out.chunks_exact_mut(2) {
                let Some((_, seq)) = self.current.as_mut() else { break };
                seq.step(&mut self.driver, sr);
                let (l, r) = self.driver.frame(sr);
                frame[0] = l as f32 * gain;
                frame[1] = r as f32 * gain;
            }
        }
        // Samples play at the digital master volume (0x7FFF = full scale).
        self.sfx.volume = SFX_GAIN * digital_master(self.digital_level) as f32 / 32767.0 * self.sfx_volume;
        self.sfx.mix(out, self.sample_rate);
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_levels_map_to_driver_volumes() {
        assert_eq!(midi_master(0), 0);
        assert_eq!(midi_master(DEFAULT_MUSIC_LEVEL), 90);
        assert_eq!(midi_master(7), 126);
        assert_eq!(midi_master(9), 126, "levels clamp to 7");
        assert_eq!(digital_master(DEFAULT_DIGITAL_LEVEL), 0x7FFF);
        assert_eq!(digital_master(0), 0);
        assert_eq!(digital_master(4), 4 << 12 | 4 << 9 | 4 << 6 | 4 << 3 | 4);
    }

    fn real(sr: u32) -> Option<Audio> {
        Audio::load(&crate::assets::default_data_dir(), sr).ok()
    }

    #[test]
    fn mixer_is_deterministic() {
        let (Some(mut a), Some(mut b)) = (real(22050), real(22050)) else { return };
        let n = a.songlist.0.iter().copied().find(|&s| s != 0 && s != 0xFF).unwrap();
        a.play_song(n);
        b.play_song(n);
        // Some songs open with a few seconds of rests: render 4 s.
        let (mut x, mut y) = (vec![0f32; 22050 * 2], vec![0f32; 22050 * 2]);
        let mut heard = false;
        for _ in 0..4 {
            a.render(&mut x);
            b.render(&mut y);
            assert_eq!(x, y);
            heard |= x.iter().any(|&s| s != 0.0);
        }
        assert!(heard, "song {n} rendered silence");
    }

    #[test]
    fn map_change_fades_to_the_new_song() {
        let Some(mut a) = real(8000) else { return };
        let songs: Vec<(usize, u8)> =
            a.songlist.0.iter().copied().enumerate().filter(|&(_, s)| s != 0 && s != 0xFF).collect();
        let (m1, s1) = songs[0];
        let Some(&(m2, s2)) = songs.iter().find(|&&(_, s)| s != s1) else { return };
        a.music_tick(m1);
        assert_eq!(a.current_song(), Some(s1), "first song starts at once");
        a.music_tick(m2);
        assert_eq!(a.current_song(), Some(s1), "old song keeps playing during the fade");
        // 126 more ticks step the counter from 127 down to 1; the next tick
        // starts the new song.
        for _ in 0..126 {
            a.music_tick(m2);
            assert_eq!(a.current_song(), Some(s1));
        }
        a.music_tick(m2);
        assert_eq!(a.current_song(), Some(s2));
        assert_eq!(a.fade, 0);
    }

    #[test]
    fn second_change_during_a_fade_starts_at_once() {
        let Some(mut a) = real(8000) else { return };
        let mut seen: Vec<(usize, u8)> = Vec::new();
        for (m, s) in a.songlist.0.iter().copied().enumerate() {
            if s != 0 && s != 0xFF && !seen.iter().any(|&(_, t)| t == s) {
                seen.push((m, s));
            }
        }
        let [(m1, _), (m2, _), (m3, s3), ..] = seen[..] else { return };
        a.music_tick(m1);
        a.music_tick(m2);
        a.music_tick(m2);
        a.music_tick(m3);
        assert_eq!(a.current_song(), Some(s3));
    }

    #[test]
    fn sound_requests_start_voices() {
        let Some(mut a) = real(22050) else { return };
        let Ok(dg) = Dungeon::parse(&std::fs::read(crate::assets::default_data_dir().join("DUNGEON.DAT")).unwrap()) else {
            return;
        };
        let party = PartyPos { map: 0, x: 2, y: 2, dir: 0 };
        let reg = Registry::for_map(&a.gdat, &dg, 0, &[]);
        let rec = a.gdat.records.iter().find(|r| r.key.kind == 2 && reg.contains(r.key.cat, r.key.idx, r.key.sub)).unwrap().key;
        let req = SoundRequest { vol: sfx::DEFAULT_VOL, mode: 1, cat: rec.cat, idx: rec.idx, sub: rec.sub, map: 0, x: 2, y: 1 };
        a.play_sounds(&dg, &party, &[req, req]);
        assert_eq!(a.sfx.active(), 1, "duplicate request from the same spot is dropped");
        let mut buf = vec![0f32; 1024];
        a.render(&mut buf);
        assert!(buf.iter().any(|&s| s != 0.0));
    }
}
