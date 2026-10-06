//! The game's random number generator (SKULL.EXE 0x1C6A1, docs/05-timeline.md).
//!
//! A 32-bit linear congruential generator; callers use bits 8 and up.

#[derive(Clone, Debug, Default)]
pub struct Rng {
    pub state: u32,
}

impl Rng {
    pub fn new(state: u32) -> Self {
        Rng { state }
    }

    fn step(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(0xBB40_E62D).wrapping_add(11);
        self.state >> 8
    }

    /// `random(n)`: uniform-ish value in 0..n. Like the original, n == 0
    /// returns 0 without advancing the state.
    pub fn random(&mut self, n: u16) -> u16 {
        if n == 0 {
            return 0;
        }
        ((self.step() & 0xFFFF) as u16) % n
    }

    /// One random bit.
    pub fn bit(&mut self) -> u16 {
        (self.step() & 1) as u16
    }

    /// Two random bits (0..4).
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
