//! Audio output: a cpal stream fed by the engine's mixer.
//!
//! The mixer lives behind a mutex; the game loop hands it each tick's sound
//! requests and the current map, and the audio thread renders from it.
//! Set DM2_MUTE=1 to disable. Failure to open a device only prints a warning.
//! DM2_AUDIO_DUMP=path.wav also writes everything played to a WAV file, so
//! live audio can be checked without listening.

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use dm2_engine::audio::{Audio, sfx};
use dm2_engine::state::GameState;

pub struct Sound {
    audio: Arc<Mutex<Audio>>,
    _stream: cpal::Stream,
    /// Last game tick the music was stepped for.
    last_tick: Option<u32>,
}

/// 16-bit stereo WAV written as it plays. The header's sizes are rewritten
/// about twice a second, so the file stays valid if the game is killed.
struct WavDump {
    file: File,
    bytes: u32,
    since_header: u32,
    rate: u32,
}

impl WavDump {
    fn create(path: &Path, rate: u32) -> std::io::Result<WavDump> {
        let mut d = WavDump { file: File::create(path)?, bytes: 0, since_header: 0, rate };
        d.header()?;
        Ok(d)
    }

    fn header(&mut self) -> std::io::Result<()> {
        let mut h = Vec::with_capacity(44);
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&(36 + self.bytes).to_le_bytes());
        h.extend_from_slice(b"WAVEfmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&1u16.to_le_bytes());
        h.extend_from_slice(&2u16.to_le_bytes());
        h.extend_from_slice(&self.rate.to_le_bytes());
        h.extend_from_slice(&(self.rate * 4).to_le_bytes());
        h.extend_from_slice(&4u16.to_le_bytes());
        h.extend_from_slice(&16u16.to_le_bytes());
        h.extend_from_slice(b"data");
        h.extend_from_slice(&self.bytes.to_le_bytes());
        let end = self.file.stream_position()?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&h)?;
        self.file.seek(SeekFrom::Start(end.max(44)))?;
        Ok(())
    }

    fn write(&mut self, stereo: &[f32]) {
        let pcm: Vec<u8> =
            stereo.iter().flat_map(|&v| ((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
        if self.file.write_all(&pcm).is_err() {
            return;
        }
        self.bytes += pcm.len() as u32;
        self.since_header += pcm.len() as u32;
        if self.since_header >= self.rate * 2 {
            self.since_header = 0;
            let _ = self.header();
        }
    }
}

impl Sound {
    /// Open the default output device. None when muted or unavailable.
    pub fn start(data_dir: &Path) -> Option<Sound> {
        if std::env::var_os("DM2_MUTE").is_some_and(|v| v != "0") {
            return None;
        }
        let warn = |what: &str| eprintln!("warning: audio disabled ({what})");
        let host = cpal::default_host();
        let Some(device) = host.default_output_device() else {
            warn("no output device");
            return None;
        };
        let config = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => {
                warn(&e.to_string());
                return None;
            }
        };
        let rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let audio = match Audio::load(data_dir, rate) {
            Ok(a) => Arc::new(Mutex::new(a)),
            Err(e) => {
                warn(&e.to_string());
                return None;
            }
        };
        let mixer = audio.clone();
        let mut stereo = Vec::new();
        let mut dump = std::env::var_os("DM2_AUDIO_DUMP").and_then(|p| match WavDump::create(Path::new(&p), rate) {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!("warning: DM2_AUDIO_DUMP: {e}");
                None
            }
        });
        let stream = device.build_output_stream(
            &config.into(),
            move |out: &mut [f32], _| {
                let frames = out.len() / channels.max(1);
                stereo.resize(frames * 2, 0.0);
                if let Ok(mut a) = mixer.lock() {
                    a.render(&mut stereo);
                } else {
                    stereo.fill(0.0);
                }
                if let Some(d) = dump.as_mut() {
                    d.write(&stereo);
                }
                // Map the stereo mix onto the device's channel layout.
                for (f, frame) in out.chunks_exact_mut(channels.max(1)).enumerate() {
                    let (l, r) = (stereo[2 * f], stereo[2 * f + 1]);
                    for (c, s) in frame.iter_mut().enumerate() {
                        *s = match (channels, c) {
                            (1, _) => (l + r) * 0.5,
                            (_, 0) => l,
                            (_, 1) => r,
                            _ => 0.0,
                        };
                    }
                }
            },
            |e| eprintln!("audio stream error: {e}"),
            None,
        );
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                warn(&e.to_string());
                return None;
            }
        };
        if let Err(e) = stream.play() {
            warn(&e.to_string());
            return None;
        }
        Some(Sound { audio, _stream: stream, last_tick: None })
    }

    /// After the game ticks: take the queued sound requests, and step the
    /// music once for every game tick since the last call, as the original's
    /// main loop does (0x10AF6 once per tick).
    pub fn update(&mut self, game: &mut GameState) {
        let reqs = sfx::drain_sounds(&mut game.effects);
        let ticks = match self.last_tick {
            Some(t) => game.tick.wrapping_sub(t).min(256),
            None => 1,
        };
        self.last_tick = Some(game.tick);
        if let Ok(mut a) = self.audio.lock() {
            if game.game_over {
                // The end of the game shuts both sound drivers down.
                a.stop_all();
                return;
            }
            for _ in 0..ticks {
                a.music_tick(game.party.map);
            }
            let portraits: Vec<u8> = game.champions.iter().map(|c| c.portrait()).collect();
            a.play_tick(&game.dungeon, &game.party, game.party_status.asleep, &portraits, &reqs);
        }
    }

    /// A new or loaded game: the music follows its map from its first tick.
    pub fn reset(&mut self) {
        self.last_tick = None;
    }
}
