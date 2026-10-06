//! Audio output: a cpal stream fed by the engine's mixer.
//!
//! The mixer lives behind a mutex; the game loop hands it each tick's sound
//! requests and the current map, and the audio thread renders from it.
//! Set DM2_MUTE=1 to disable. Failure to open a device only prints a warning.

use std::path::Path;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use dm2_engine::audio::{Audio, sfx};
use dm2_engine::state::GameState;

pub struct Sound {
    audio: Arc<Mutex<Audio>>,
    _stream: cpal::Stream,
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
        Some(Sound { audio, _stream: stream })
    }

    /// After the game ticks: take the queued sound requests and follow the
    /// party's map for music.
    pub fn update(&mut self, game: &mut GameState) {
        let reqs = sfx::drain_sounds(&mut game.effects);
        if let Ok(mut a) = self.audio.lock() {
            a.set_map(game.party.map);
            if !reqs.is_empty() {
                a.play_sounds(&game.dungeon, &game.party, &reqs);
            }
        }
    }
}
