//! Creature animation sequences (docs/08 "Animation"; SKULL.EXE 0x14E42,
//! 0x14F1B, 0x1501A).
//!
//! A sequence is addressed by its start frame (from the action map) and an
//! offset within it; offset 0xFFFF means "before the first frame".

use crate::rng::Rng;

pub const NO_FRAME: u16 = 0xFFFF;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Frame(pub [u8; 4]);

impl Frame {
    /// Sound id; 0x7F means silent.
    pub fn sound(self) -> u8 {
        self.0[0] & 0x7F
    }
    /// Non-zero while the sequence continues; also the advance distance.
    pub fn cont(self) -> u16 {
        (self.0[1] >> 4) as u16
    }
    pub fn branch_chance(self) -> u8 {
        self.0[1] & 15
    }
    pub fn jump(self) -> u16 {
        (self.0[2] & 0x3F) as u16
    }
    /// Frame events stay chained (zero-time) while armed.
    pub fn chain(self) -> bool {
        self.0[2] & 0x40 != 0
    }
    /// Run the gameplay event for this frame.
    pub fn event(self) -> bool {
        self.0[2] & 0x80 != 0
    }
    pub fn jitter(self) -> bool {
        self.0[3] & 1 != 0
    }
    pub fn random_flip(self) -> bool {
        self.0[3] & 2 != 0
    }
    pub fn extra_ticks(self) -> u16 {
        ((self.0[3] >> 2) & 3) as u16
    }
    pub fn base_ticks(self) -> u16 {
        (self.0[3] >> 4) as u16
    }
}

#[derive(Clone, Debug, Default)]
pub struct Anim {
    /// (action, first frame) pairs.
    map: Vec<(i16, u16)>,
    /// Start used when an action has no entry (the value paired with the
    /// terminator).
    default: u16,
    frames: Vec<Frame>,
}

/// Result of stepping (0x1501A): sequence over, a frame to play, or the
/// "stop" jump.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Ended,
    Playing,
    Stopped,
}

impl Anim {
    pub fn parse(map: &[u8], frames: &[u8]) -> Anim {
        let mut pairs = Vec::new();
        let mut default = 0;
        for c in map.chunks_exact(4) {
            let a = i16::from_le_bytes([c[0], c[1]]);
            let f = u16::from_le_bytes([c[2], c[3]]);
            if a == -1 {
                default = f;
                break;
            }
            pairs.push((a, f));
        }
        let frames = frames.chunks_exact(4).map(|c| Frame([c[0], c[1], c[2], c[3]])).collect();
        Anim { map: pairs, default, frames }
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// First frame of the sequence for an action.
    pub fn seq_start(&self, action: u8) -> u16 {
        self.map.iter().find(|&&(a, _)| a == action as i16).map(|&(_, f)| f).unwrap_or(self.default)
    }

    pub fn frame(&self, start: u16, off: u16) -> Frame {
        let o = if off == NO_FRAME { 0 } else { off };
        self.frames.get(start as usize + o as usize).copied().unwrap_or_default()
    }

    /// Advance to the next playable frame (0x14F1B). Returns true if a frame
    /// with a non-zero duration was reached.
    pub fn advance(&self, start: u16, off: &mut u16, rng: &mut Rng) -> bool {
        if *off == NO_FRAME {
            *off = 0;
        } else {
            let hop = self.frame(start, *off).cont();
            if hop == 0 {
                return false;
            }
            *off += hop;
        }
        for _ in 0..self.frames.len().max(1) {
            let f = self.frame(start, *off);
            if f.cont() == 0 {
                return false;
            }
            let c = f.branch_chance();
            if c == 0xF || (rng.rnd() & 15) as u8 <= c {
                return f.extra_ticks() + f.base_ticks() != 0;
            }
            *off += 1;
        }
        false
    }

    /// Follow the frame's jump (0x1501A).
    pub fn next(&self, start: u16, off: &mut u16) -> Step {
        if *off == NO_FRAME {
            *off = 0;
        } else {
            let j = self.frame(start, *off).jump();
            if j == 0 {
                return Step::Stopped;
            }
            *off += j;
        }
        if self.frame(start, *off).cont() != 0 {
            Step::Playing
        } else {
            Step::Ended
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim() -> Anim {
        // action 5 -> frame 1; default 0.
        let map = [5, 0, 1, 0, 0xFF, 0xFF, 0, 0];
        let frames = [
            0x7F, 0x00, 0x00, 0x00, // 0: end
            0x01, 0x1F, 0x81, 0x20, // 1: continue, always, jump 1, event, 2 ticks
            0x7F, 0x1F, 0x00, 0x10, // 2: continue, always, stop, 1 tick
            0x7F, 0x00, 0x00, 0x00, // 3: end
        ];
        Anim::parse(&map, &frames)
    }

    #[test]
    fn sequence_lookup_and_stepping() {
        let a = anim();
        assert_eq!(a.seq_start(5), 1);
        assert_eq!(a.seq_start(9), 0);
        let mut rng = Rng::new(1);
        let mut off = NO_FRAME;
        assert!(a.advance(1, &mut off, &mut rng));
        assert_eq!(off, 0);
        assert!(a.frame(1, off).event());
        assert_eq!(a.next(1, &mut off), Step::Playing);
        assert_eq!(off, 1);
        assert_eq!(a.next(1, &mut off), Step::Stopped);
        // advancing past the last continuing frame ends the sequence
        assert!(!a.advance(1, &mut off, &mut rng));
    }
}
