//! Song sequencer and SONGLIST.DAT selection (docs/11-audio.md "Music").
//!
//! The song for a map comes from SONGLIST.DAT (byte per map; 0 = silence,
//! 0xFF = no entry). On a change the current song fades out and the new one
//! starts (0x10AF6 / 0x1095D; the fade length is not documented, so it is a
//! constant here). Songs loop: HMI loop controllers 110 (start) and 111
//! (end) repeat a section per track, and a finished song starts again.

use super::hmp::{Msg, Song};
use super::midi::Driver;

/// HMI loop-start / loop-end controllers.
const CC_LOOP_START: u8 = 110;
const CC_LOOP_END: u8 = 111;
/// Fade-out length in seconds (tentative; the original's isn't documented).
pub const FADE_SECS: f64 = 1.0;

struct Track {
    /// Next event index.
    pos: usize,
    /// Song ticks added to this track's event times by loop jumps.
    offset: u32,
    /// Loop start: event index and the track-local tick it began at.
    loop_start: Option<(usize, u32)>,
    /// Remaining repeats for the active loop; None = forever.
    loop_left: Option<u8>,
}

pub struct Sequencer {
    song: Song,
    tracks: Vec<Track>,
    tick: u32,
    /// Fractional ticks accumulated from rendered samples.
    acc: f64,
}

impl Sequencer {
    pub fn new(song: Song) -> Sequencer {
        let tracks = (0..song.tracks).map(|_| Track { pos: 0, offset: 0, loop_start: None, loop_left: None }).collect();
        Sequencer { song, tracks, tick: 0, acc: 0.0 }
    }

    fn restart(&mut self, drv: &mut Driver) {
        drv.all_notes_off();
        *self = Sequencer::new(std::mem::replace(&mut self.song, Song { tick_rate: 1, seconds: 0, tracks: 0, track_events: vec![], end_tick: 0 }));
    }

    /// Send every event due at or before the current tick.
    fn dispatch(&mut self, drv: &mut Driver) {
        let mut all_done = true;
        for (ti, t) in self.tracks.iter_mut().enumerate() {
            let evs = &self.song.track_events[ti];
            // Bounded: a loop jump re-reads at most the loop body once per tick.
            let mut guard = evs.len() * 2 + 16;
            while t.pos < evs.len() && guard > 0 {
                guard -= 1;
                let e = evs[t.pos];
                if e.tick + t.offset > self.tick {
                    break;
                }
                t.pos += 1;
                match e.msg {
                    Msg::Control { cc: CC_LOOP_START, value, .. } => {
                        t.loop_start = Some((t.pos, e.tick));
                        t.loop_left = if value == 0 || value >= 127 { None } else { Some(value) };
                    }
                    Msg::Control { cc: CC_LOOP_END, .. } => {
                        if let Some((start_pos, start_tick)) = t.loop_start {
                            let again = match &mut t.loop_left {
                                None => true,
                                Some(0) => false,
                                Some(n) => {
                                    *n -= 1;
                                    true
                                }
                            };
                            if again && e.tick > start_tick {
                                t.offset += e.tick - start_tick;
                                t.pos = start_pos;
                            }
                        }
                    }
                    m => drv.handle(m),
                }
            }
            if t.pos < evs.len() {
                all_done = false;
            }
        }
        if all_done {
            self.restart(drv);
        }
    }

    /// Advance by one output sample.
    pub fn step(&mut self, drv: &mut Driver, sr: f64) {
        self.acc += self.song.tick_rate as f64 / sr;
        while self.acc >= 1.0 {
            self.acc -= 1.0;
            self.dispatch(drv);
            self.tick += 1;
        }
    }

    pub fn tick(&self) -> u32 {
        self.tick
    }
}

/// Map number -> song number from SONGLIST.DAT (0 = silence).
#[derive(Clone, Debug, Default)]
pub struct SongList(pub Vec<u8>);

impl SongList {
    /// Song for a map, or None when the table has no entry for it.
    pub fn song_for_map(&self, map: usize) -> Option<u8> {
        match self.0.get(map) {
            Some(&0xFF) | None => None,
            Some(&s) => Some(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::bnk::Bank;
    use crate::audio::hmp::Event;

    fn tiny_song(events: Vec<(u32, Msg)>) -> Song {
        let end = events.iter().map(|e| e.0).max().unwrap_or(0);
        Song {
            tick_rate: 120,
            seconds: 1,
            tracks: 1,
            track_events: vec![events.into_iter().map(|(tick, msg)| Event { tick, track: 0, msg }).collect()],
            end_tick: end,
        }
    }

    #[test]
    fn songlist_lookup() {
        let s = SongList(vec![3, 0, 0xFF]);
        assert_eq!(s.song_for_map(0), Some(3));
        assert_eq!(s.song_for_map(1), Some(0));
        assert_eq!(s.song_for_map(2), None);
        assert_eq!(s.song_for_map(9), None);
    }

    #[test]
    fn finite_loop_repeats_then_continues() {
        // A loop from tick 10 to tick 20 that repeats twice delays the
        // event after it by two loop lengths.
        let on = Msg::NoteOn { ch: 0, note: 60, vel: 100 };
        let song = tiny_song(vec![
            (10, Msg::Control { ch: 0, cc: CC_LOOP_START, value: 2 }),
            (15, on),
            (20, Msg::Control { ch: 0, cc: CC_LOOP_END, value: 0 }),
            (25, Msg::Program { ch: 0, program: 5 }),
            // Keeps the song running so it doesn't restart mid-test.
            (1000, Msg::Program { ch: 0, program: 0 }),
        ]);
        let empty = Bank { patches: vec![Default::default(); 128] };
        let mut drv = Driver::new(empty.clone(), empty);
        let mut seq = Sequencer::new(song);
        let sr = 120.0; // one sample per tick
        let mut program_at = None;
        for _ in 0..200 {
            seq.step(&mut drv, sr);
            if program_at.is_none() && seq.tracks[0].pos == 4 {
                program_at = Some(seq.tick());
            }
        }
        assert_eq!(program_at, Some(25 + 2 * 10 + 1));
    }
}
