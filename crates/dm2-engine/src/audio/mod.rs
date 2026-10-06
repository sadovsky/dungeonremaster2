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
pub mod sfx;

use std::collections::HashMap;
use std::path::Path;

use dm2_formats::dungeon::Dungeon;
use dm2_formats::gdat::{Gdat, Key};

use crate::world::PartyPos;
use bnk::Bank;
use hmp::Song;
use midi::Driver;
use music::{Sequencer, SongList, FADE_SECS};
use sfx::{Sfx, SoundRequest};

/// Music is mixed below the effects; 18 FM voices can sum well above 1.
const MUSIC_GAIN: f32 = 0.18;

pub struct Audio {
    sample_rate: u32,
    gdat: Gdat,
    songlist: SongList,
    songs: HashMap<u8, Option<Song>>,
    driver: Driver,
    /// Playing song number and its sequencer.
    current: Option<(u8, Sequencer)>,
    /// Song to start once the fade-out ends (Some(0) = silence).
    pending: Option<u8>,
    /// Seconds of fade-out left.
    fade_left: f64,
    sfx: Sfx,
    pub music_volume: f32,
    pub sfx_volume: f32,
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
        Ok(Audio::from_parts(gdat, songlist, melodic, drums, has_header, sample_rate))
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
            fade_left: 0.0,
            sfx: Sfx::new(has_header),
            music_volume: 1.0,
            sfx_volume: 1.0,
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

    /// Play song `n` now, or after fading out the current one.
    pub fn play_song(&mut self, n: u8) {
        let playing = self.current.as_ref().map(|c| c.0);
        if playing == Some(n) || (playing.is_none() && n == 0) || self.pending == Some(n) {
            return;
        }
        if playing.is_some() {
            self.pending = Some(n);
            self.fade_left = FADE_SECS;
        } else {
            self.start(n);
        }
    }

    /// The party is on `map`: switch to its song from SONGLIST.DAT.
    pub fn set_map(&mut self, map: usize) {
        if let Some(n) = self.songlist.song_for_map(map) {
            self.play_song(n);
        }
    }

    pub fn current_song(&self) -> Option<u8> {
        self.current.as_ref().map(|c| c.0)
    }

    /// Queue the sound requests of one game tick.
    pub fn play_sounds(&mut self, dg: &Dungeon, party: &PartyPos, reqs: &[SoundRequest]) {
        let Audio { sfx, gdat, .. } = self;
        sfx.play(gdat, dg, party, reqs);
    }

    /// Render interleaved stereo frames into `out` (length must be even).
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let sr = self.sample_rate as f64;
        if self.current.is_some() {
            let vol = MUSIC_GAIN * self.music_volume;
            for frame in out.chunks_exact_mut(2) {
                let mut gain = vol;
                if self.pending.is_some() {
                    self.fade_left -= 1.0 / sr;
                    if self.fade_left <= 0.0 {
                        let n = self.pending.take().unwrap_or(0);
                        self.start(n);
                        if self.current.is_none() {
                            break;
                        }
                    } else {
                        gain *= (self.fade_left / FADE_SECS) as f32;
                    }
                }
                let Some((_, seq)) = self.current.as_mut() else { break };
                seq.step(&mut self.driver, sr);
                let (l, r) = self.driver.frame(sr);
                frame[0] = l as f32 * gain;
                frame[1] = r as f32 * gain;
            }
        } else if let Some(n) = self.pending.take() {
            self.start(n);
        }
        self.sfx.volume = 0.8 * self.sfx_volume;
        self.sfx.mix(out, self.sample_rate);
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        a.set_map(m1);
        assert_eq!(a.current_song(), Some(s1));
        a.set_map(m2);
        assert_eq!(a.current_song(), Some(s1), "old song keeps playing during the fade");
        let mut buf = vec![0f32; (8000.0 * (FADE_SECS + 0.1)) as usize * 2];
        a.render(&mut buf);
        assert_eq!(a.current_song(), Some(s2));
    }

    #[test]
    fn sound_requests_start_voices() {
        let Some(mut a) = real(22050) else { return };
        let Ok(dg) = Dungeon::parse(&std::fs::read(crate::assets::default_data_dir().join("DUNGEON.DAT")).unwrap()) else {
            return;
        };
        let rec = a.gdat.records.iter().find(|r| r.key.kind == 2).unwrap().key;
        let party = PartyPos { map: 0, x: 2, y: 2, dir: 0 };
        let req = SoundRequest { cat: rec.cat, idx: rec.idx, sub: rec.sub, map: 0, x: 2, y: 1 };
        a.play_sounds(&dg, &party, &[req, req]);
        assert_eq!(a.sfx.active(), 1, "duplicate request from the same spot is dropped");
        let mut buf = vec![0f32; 1024];
        a.render(&mut buf);
        assert!(buf.iter().any(|&s| s != 0.0));
    }
}
