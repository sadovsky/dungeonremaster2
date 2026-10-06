//! Render one song through the FM synth to a WAV file for listening checks.
//!
//!   cargo run --release -p dm2-engine --example songwav -- SONG [SECONDS] [OUT.wav]
//!
//! Reads the user's own game files; the default output goes under re/audio/
//! (gitignored).
use dm2_engine::assets::default_data_dir;
use dm2_engine::audio::Audio;

const RATE: u32 = 44100;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let song: u8 = args.first().and_then(|s| s.parse().ok()).expect("usage: songwav SONG [SECONDS] [OUT.wav]");
    let secs: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let out = args.get(2).cloned().unwrap_or_else(|| format!("re/audio/song_{song:02}.wav"));
    let mut a = Audio::load(&default_data_dir(), RATE).expect("load game audio data");
    a.play_song(song);
    let frames = (secs * RATE as f64) as usize;
    let mut pcm = Vec::with_capacity(frames * 4);
    let mut buf = vec![0f32; 4096 * 2];
    let mut done = 0;
    while done < frames {
        let n = (frames - done).min(4096);
        a.render(&mut buf[..n * 2]);
        for s in &buf[..n * 2] {
            pcm.extend_from_slice(&((s * 32767.0) as i16).to_le_bytes());
        }
        done += n;
    }
    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&2u16.to_le_bytes()); // stereo
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm);
    if let Some(dir) = std::path::Path::new(&out).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&out, wav).expect("write wav");
    println!("{out}: song {song}, {secs} s");
}
