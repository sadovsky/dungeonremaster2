//! The game's random number generator (SKULL.EXE 0x1C6A1, docs/05-timeline.md).
//!
//! A 32-bit linear congruential generator; callers use bits 8 and up.
//!
//! For comparing the random stream with the original, `trace_start` turns on
//! a per-call-site count of draws (off by default; it costs one branch).

use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::Location;

thread_local! {
    static TRACE: RefCell<Option<HashMap<(&'static str, u32), u32>>> = const { RefCell::new(None) };
}

/// Start counting draws per call site (file, line) on this thread.
pub fn trace_start() {
    TRACE.with(|t| *t.borrow_mut() = Some(HashMap::new()));
}

/// While tracing, count an arbitrary event under (`key`, `n`) alongside the
/// draws, e.g. ("think", creature index).
pub fn trace_tag(key: &'static str, n: u32) {
    TRACE.with(|t| {
        if let Some(m) = t.borrow_mut().as_mut() {
            *m.entry((key, n)).or_default() += 1;
        }
    });
}

/// Stop counting and return the counts.
pub fn trace_take() -> HashMap<(&'static str, u32), u32> {
    TRACE.with(|t| t.borrow_mut().take()).unwrap_or_default()
}

#[derive(Clone, Debug, Default)]
pub struct Rng {
    pub state: u32,
}

impl Rng {
    pub fn new(state: u32) -> Self {
        Rng { state }
    }

    #[track_caller]
    fn step(&mut self) -> u32 {
        // Location::caller() must be read here: closures don't inherit
        // #[track_caller].
        let l = Location::caller();
        TRACE.with(|t| {
            if let Some(m) = t.borrow_mut().as_mut() {
                *m.entry((l.file(), l.line())).or_default() += 1;
            }
        });
        self.state = self.state.wrapping_mul(0xBB40_E62D).wrapping_add(11);
        self.state >> 8
    }

    /// Raw output (`rnd()`, 0x1C6A1); callers mask the bits they need.
    #[track_caller]
    pub fn rnd(&mut self) -> u32 {
        self.step()
    }

    /// `random(n)`: uniform-ish value in 0..n. Like the original, n == 0
    /// returns 0 without advancing the state.
    #[track_caller]
    pub fn random(&mut self, n: u16) -> u16 {
        if n == 0 {
            return 0;
        }
        ((self.step() & 0xFFFF) as u16) % n
    }

    /// One random bit.
    #[track_caller]
    pub fn bit(&mut self) -> u16 {
        (self.step() & 1) as u16
    }

    /// Two random bits (0..4).
    #[track_caller]
    pub fn rand4(&mut self) -> u16 {
        (self.step() & 3) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_does_not_advance() {
        let mut r = Rng::new(5);
        assert_eq!(r.random(0), 0);
        assert_eq!(r.state, 5);
    }

    #[test]
    fn sequence_from_zero() {
        let mut r = Rng::new(0);
        r.random(100);
        assert_eq!(r.state, 11);
        r.random(100);
        assert_eq!(r.state, 11u32.wrapping_mul(0xBB40_E62D).wrapping_add(11));
    }
}
