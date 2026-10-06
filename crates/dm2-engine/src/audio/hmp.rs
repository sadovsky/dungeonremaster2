//! HMI HMP songs (GRAPHICS.DAT type 3; docs/11-audio.md "HMP format").
//!
//! Parsed into one time-ordered event list. Delta times use HMI's reversed
//! variable-length encoding; timing comes only from the header tick rate.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    NoteOff { ch: u8, note: u8 },
    NoteOn { ch: u8, note: u8, vel: u8 },
    Control { ch: u8, cc: u8, value: u8 },
    Program { ch: u8, program: u8 },
    PitchBend { ch: u8, value: u16 },
    Aftertouch { ch: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    /// Absolute time in song ticks.
    pub tick: u32,
    /// Track the event came from (loop controllers act per track).
    pub track: u16,
    pub msg: Msg,
}

#[derive(Clone, Debug)]
pub struct Song {
    /// Ticks per second (120 in every DM2 song).
    pub tick_rate: u32,
    /// Length declared in the header, in seconds.
    pub seconds: u32,
    pub tracks: usize,
    /// Per track, in order. Each track's events are sorted by tick.
    pub track_events: Vec<Vec<Event>>,
    /// Tick of the last event in any track.
    pub end_tick: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum HmpError {
    NotHmp,
    Truncated,
}

/// HMP delta: 7-bit groups, least significant first; a set top bit marks
/// the last byte.
fn rev_vlq(b: &[u8], p: &mut usize) -> Option<u32> {
    let (mut v, mut shift) = (0u32, 0);
    loop {
        let c = *b.get(*p)?;
        *p += 1;
        v |= ((c & 0x7F) as u32) << shift;
        shift += 7;
        if c & 0x80 != 0 {
            return Some(v);
        }
        if shift > 28 {
            return None;
        }
    }
}

/// Standard MIDI variable-length value (used only for sysex lengths).
fn midi_vlq(b: &[u8], p: &mut usize) -> Option<u32> {
    let mut v = 0u32;
    for _ in 0..4 {
        let c = *b.get(*p)?;
        *p += 1;
        v = (v << 7) | (c & 0x7F) as u32;
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

fn u32_at(d: &[u8], o: usize) -> Result<u32, HmpError> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or(HmpError::Truncated)
}

impl Song {
    pub fn parse(d: &[u8]) -> Result<Song, HmpError> {
        if d.len() < 0x40 || &d[..8] != b"HMIMIDIP" {
            return Err(HmpError::NotHmp);
        }
        let first = if &d[8..14] == b"013195" { 0x388 } else { 0x308 };
        let tracks = u32_at(d, 0x30)? as usize;
        let tick_rate = u32_at(d, 0x38)?.max(1);
        let seconds = u32_at(d, 0x3C)?;
        let mut p = first;
        let mut track_events = Vec::with_capacity(tracks);
        let mut end_tick = 0;
        for t in 0..tracks {
            let len = u32_at(d, p + 4)? as usize;
            let body = d.get(p + 12..p + len).ok_or(HmpError::Truncated)?;
            p += len;
            let mut evs = Vec::new();
            let (mut q, mut tick, mut status) = (0usize, 0u32, 0u8);
            while q < body.len() {
                tick += rev_vlq(body, &mut q).ok_or(HmpError::Truncated)?;
                let c = *body.get(q).ok_or(HmpError::Truncated)?;
                if c == 0xFF {
                    let kind = *body.get(q + 1).ok_or(HmpError::Truncated)?;
                    let n = *body.get(q + 2).ok_or(HmpError::Truncated)? as usize;
                    q += 3 + n;
                    if kind == 0x2F {
                        break;
                    }
                    continue;
                }
                if c == 0xF0 || c == 0xF7 {
                    q += 1;
                    let n = midi_vlq(body, &mut q).ok_or(HmpError::Truncated)? as usize;
                    q += n;
                    continue;
                }
                if c & 0x80 != 0 {
                    status = c;
                    q += 1;
                }
                let ch = status & 15;
                let a = *body.get(q).ok_or(HmpError::Truncated)?;
                let two = |q: usize| body.get(q + 1).copied().ok_or(HmpError::Truncated);
                let msg = match status & 0xF0 {
                    0x80 => {
                        two(q)?;
                        q += 2;
                        Msg::NoteOff { ch, note: a }
                    }
                    0x90 => {
                        let v = two(q)?;
                        q += 2;
                        if v == 0 { Msg::NoteOff { ch, note: a } } else { Msg::NoteOn { ch, note: a, vel: v } }
                    }
                    0xA0 => {
                        two(q)?;
                        q += 2;
                        Msg::Aftertouch { ch }
                    }
                    0xB0 => {
                        let v = two(q)?;
                        q += 2;
                        Msg::Control { ch, cc: a, value: v }
                    }
                    0xC0 => {
                        q += 1;
                        Msg::Program { ch, program: a }
                    }
                    0xD0 => {
                        q += 1;
                        Msg::Aftertouch { ch }
                    }
                    0xE0 => {
                        let v = two(q)?;
                        q += 2;
                        Msg::PitchBend { ch, value: (a as u16 & 0x7F) | (v as u16 & 0x7F) << 7 }
                    }
                    _ => return Err(HmpError::Truncated),
                };
                evs.push(Event { tick, track: t as u16, msg });
            }
            end_tick = end_tick.max(tick);
            track_events.push(evs);
        }
        Ok(Song { tick_rate, seconds, tracks, track_events, end_tick })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm2_formats::gdat::{Gdat, Key};

    #[test]
    fn reversed_vlq() {
        let b = [0x05 | 0x80, 0x10, 0x81];
        let mut p = 0;
        assert_eq!(rev_vlq(&b, &mut p), Some(5));
        assert_eq!(rev_vlq(&b, &mut p), Some(0x10 | 1 << 7));
        assert_eq!(p, 3);
    }

    #[test]
    fn real_songs_match_their_declared_length() {
        let Ok(g) = Gdat::open(crate::assets::default_data_dir().join("GRAPHICS.DAT")) else { return };
        let mut n = 0;
        for i in 0..29u8 {
            let Some(d) = g.get(Key::new(4, i, 3, 0)) else { continue };
            let s = Song::parse(d).unwrap();
            assert_eq!(s.tick_rate, 120);
            let secs = s.end_tick as f64 / s.tick_rate as f64;
            // The header length is whole seconds; events end within 2 s of
            // it. Song 0 (SONGLIST's "silence" number) runs ~6% long.
            let slack = if i == 0 { s.seconds as f64 * 0.1 } else { 2.0 };
            assert!((secs - s.seconds as f64).abs() <= slack, "song {i}: {secs} vs {}", s.seconds);
            assert!(s.track_events.iter().map(Vec::len).sum::<usize>() > 0);
            n += 1;
        }
        assert_eq!(n, 29);
    }
}
