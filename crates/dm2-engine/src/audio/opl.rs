//! A compact two-operator FM voice modelled on the YM3812/YMF262 (OPL2/3).
//!
//! Written from the chip's documented behaviour rather than ported from an
//! emulator: phase generator with the frequency multiplier table, the four
//! OPL2 waveforms, an envelope generator in the chip's 0.1875 dB units with
//! rate scaling (KSR), key-scale level, total level, feedback, FM/additive
//! connection, and the tremolo/vibrato LFOs. Pitch is quantised to OPL
//! F-number/block pairs, so tuning matches the chip.

use super::bnk::{OpParams, Patch};

/// Native OPL sample clock.
pub const OPL_RATE: f64 = 49716.0;

/// Frequency multiplier per MULT value (0 means ×½).
const MULT: [f64; 16] = [0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 10.0, 12.0, 12.0, 15.0, 15.0];
/// Key-scale level base per F-number top four bits (chip ROM values).
const KSL_ROM: [i32; 16] = [0, 32, 40, 45, 48, 51, 53, 55, 56, 58, 59, 60, 61, 62, 63, 64];
/// Right shift applied to the KSL value for KSL settings 0..3
/// (0 = off, 1 = 3 dB/oct, 2 = 1.5 dB/oct, 3 = 6 dB/oct).
const KSL_SHIFT: [u32; 4] = [31, 1, 2, 0];
/// Envelope silence level (9-bit attenuation, 0.1875 dB per step).
const ENV_MAX: f64 = 511.0;

/// Convert a frequency in Hz to the nearest OPL (F-number, block) pair.
pub fn fnum_block(hz: f64) -> (u16, u8) {
    for block in 0..8u8 {
        let f = hz * (1u32 << (20 - block)) as f64 / OPL_RATE;
        if f < 1024.0 {
            return (f.round().clamp(0.0, 1023.0) as u16, block);
        }
    }
    (1023, 7)
}

/// The frequency an (F-number, block) pair actually produces.
pub fn fnum_hz(fnum: u16, block: u8) -> f64 {
    fnum as f64 * OPL_RATE / (1u32 << (20 - block)) as f64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Off,
}

#[derive(Clone, Debug)]
struct Operator {
    p: OpParams,
    /// Total level override (patch TL plus volume scaling), 0..63.
    tl: u8,
    phase: f64,
    env: f64,
    stage: Stage,
    out: f64,
    prev_out: f64,
}

impl Operator {
    fn new(p: OpParams) -> Operator {
        Operator { p, tl: p.total_level, phase: 0.0, env: ENV_MAX, stage: Stage::Off, out: 0.0, prev_out: 0.0 }
    }

    /// Rate-scaling offset from block and F-number (KSR).
    fn ksr_offset(&self, fnum: u16, block: u8) -> u32 {
        let k = (block as u32) << 1 | (fnum as u32 >> 9) & 1;
        if self.p.ksr { k } else { k >> 2 }
    }

    /// Envelope change per sample for a 4-bit rate in the current stage.
    /// Decay/release: the datasheet's 0→96 dB time is 39.28 s at rate 1,
    /// halving every 4 steps of the effective rate.
    fn decay_step(&self, rate: u8, fnum: u16, block: u8, sr: f64) -> f64 {
        if rate == 0 {
            return 0.0;
        }
        let re = (4 * rate as u32 + self.ksr_offset(fnum, block)).min(63) as f64;
        let secs = 39.28 / 2f64.powf((re - 4.0) / 4.0);
        (ENV_MAX + 1.0) / (secs * sr)
    }

    /// Attack coefficient: the datasheet's 0→100% time is 2.826 s at rate 1,
    /// halving every 4 steps; rates of 60 and up are instant. The attack is
    /// exponential in the attenuation domain, as on the chip.
    fn attack_coef(&self, fnum: u16, block: u8, sr: f64) -> Option<f64> {
        if self.p.attack == 0 {
            return Some(0.0);
        }
        let re = (4 * self.p.attack as u32 + self.ksr_offset(fnum, block)).min(63);
        if re >= 60 {
            return None;
        }
        let secs = 2.826 / 2f64.powf((re as f64 - 4.0) / 4.0);
        Some(((ENV_MAX + 1.0).ln() / (secs * sr)).min(1.0))
    }

    fn sustain_level(&self) -> f64 {
        // 3 dB per step; 15 means -93 dB.
        let sl = if self.p.sustain_level == 15 { 31 } else { self.p.sustain_level as u32 };
        (sl * 16) as f64
    }

    fn key_on(&mut self) {
        self.stage = Stage::Attack;
        self.phase = 0.0;
    }

    fn key_off(&mut self) {
        if self.stage != Stage::Off {
            self.stage = Stage::Release;
        }
    }

    fn step_env(&mut self, fnum: u16, block: u8, sr: f64) {
        match self.stage {
            Stage::Attack => match self.attack_coef(fnum, block, sr) {
                None => {
                    self.env = 0.0;
                    self.stage = Stage::Decay;
                }
                Some(k) => {
                    self.env -= k * (self.env + 1.0);
                    if self.env <= 0.0 {
                        self.env = 0.0;
                        self.stage = Stage::Decay;
                    }
                }
            },
            Stage::Decay => {
                self.env += self.decay_step(self.p.decay, fnum, block, sr);
                let sl = self.sustain_level();
                if self.env >= sl {
                    self.env = sl;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                // Non-sustaining (EG-TYP 0) envelopes keep falling at the release rate.
                if !self.p.sustaining {
                    self.env += self.decay_step(self.p.release, fnum, block, sr);
                }
            }
            Stage::Release => {
                self.env += self.decay_step(self.p.release, fnum, block, sr);
            }
            Stage::Off => {}
        }
        if self.env >= ENV_MAX {
            self.env = ENV_MAX;
            if matches!(self.stage, Stage::Release | Stage::Sustain) {
                self.stage = Stage::Off;
            }
        }
    }

    /// Total attenuation in 0.1875 dB units.
    fn attenuation(&self, fnum: u16, block: u8, tremolo: f64) -> f64 {
        let mut ksl = (KSL_ROM[(fnum >> 6) as usize & 15] << 2) - ((8 - block as i32) << 5);
        if ksl < 0 {
            ksl = 0;
        }
        let ksl = if self.p.ksl == 0 { 0 } else { ksl >> KSL_SHIFT[self.p.ksl as usize] };
        let trem = if self.p.tremolo { tremolo } else { 0.0 };
        self.env + (self.tl as f64) * 4.0 + ksl as f64 + trem
    }

    fn wave(&self, phase: f64) -> f64 {
        let ph = phase.rem_euclid(1.0);
        let s = (ph * std::f64::consts::TAU).sin();
        match self.p.waveform {
            0 => s,
            1 => s.max(0.0),
            2 => s.abs(),
            _ => {
                if ph.rem_euclid(0.5) < 0.25 { s.abs() } else { 0.0 }
            }
        }
    }

    /// One sample of output (±1 scale), given a phase modulation in cycles.
    fn output(&mut self, fnum: u16, block: u8, inc: f64, modulation: f64, tremolo: f64) -> f64 {
        let att = self.attenuation(fnum, block, tremolo);
        let out = if att >= ENV_MAX {
            0.0
        } else {
            self.wave(self.phase + modulation) * 10f64.powf(-(att * 0.1875) / 20.0)
        };
        self.phase = (self.phase + inc).rem_euclid(1.0);
        self.prev_out = self.out;
        self.out = out;
        out
    }
}

/// A two-operator FM voice.
#[derive(Clone, Debug)]
pub struct Voice {
    patch: Patch,
    m: Operator,
    c: Operator,
    fnum: u16,
    block: u8,
    hz: f64,
}

impl Voice {
    pub fn new(patch: Patch) -> Voice {
        Voice { patch, m: Operator::new(patch.modulator), c: Operator::new(patch.carrier), fnum: 0, block: 0, hz: 0.0 }
    }

    pub fn set_patch(&mut self, patch: Patch) {
        *self = Voice { fnum: self.fnum, block: self.block, hz: self.hz, ..Voice::new(patch) };
    }

    /// Set pitch in Hz; quantised to the chip's F-number/block grid.
    pub fn set_frequency(&mut self, hz: f64) {
        let (f, b) = fnum_block(hz);
        self.fnum = f;
        self.block = b;
        self.hz = fnum_hz(f, b);
    }

    /// Scale output level: `level` 0..=1 raises the total level of the
    /// audible operators the way AdLib drivers do (TL' = 63 − (63 − TL)·level).
    pub fn set_level(&mut self, level: f64) {
        // Amplitude-proportional: a level of L adds -20*log10(L) dB of
        // attenuation, in total-level steps of 0.75 dB.
        let extra = if level <= 0.0 { 63.0 } else { (-20.0 * level.min(1.0).log10() / 0.75).round() };
        let scale = |tl: u8| (tl as f64 + extra).min(63.0) as u8;
        self.c.tl = scale(self.patch.carrier.total_level);
        self.m.tl = if self.patch.additive() { scale(self.patch.modulator.total_level) } else { self.patch.modulator.total_level };
    }

    pub fn key_on(&mut self) {
        self.m.key_on();
        self.c.key_on();
    }

    pub fn key_off(&mut self) {
        self.m.key_off();
        self.c.key_off();
    }

    pub fn is_silent(&self) -> bool {
        self.c.stage == Stage::Off && (!self.patch.additive() || self.m.stage == Stage::Off)
    }

    /// Render one sample at output rate `sr`. `lfo` carries the shared
    /// tremolo depth (attenuation units) and vibrato factor.
    pub fn sample(&mut self, sr: f64, lfo: Lfo) -> f64 {
        let (fnum, block) = (self.fnum, self.block);
        self.m.step_env(fnum, block, sr);
        self.c.step_env(fnum, block, sr);
        let vib = |op: &Operator| if op.p.vibrato { lfo.vibrato } else { 1.0 };
        let inc_m = self.hz * MULT[self.m.p.multiple as usize] * vib(&self.m) / sr;
        let inc_c = self.hz * MULT[self.c.p.multiple as usize] * vib(&self.c) / sr;
        // Feedback: average of the modulator's last two outputs; the chip
        // adds (sum >> (9 - fb)) of a 13-bit output to a 10-bit phase.
        let fb = self.patch.modulator.feedback;
        let fbmod = if fb == 0 { 0.0 } else { (self.m.out + self.m.prev_out) * 4.0 / (1u32 << (9 - fb)) as f64 };
        let mo = self.m.output(fnum, block, inc_m, fbmod, lfo.tremolo);
        if self.patch.additive() {
            mo + self.c.output(fnum, block, inc_c, 0.0, lfo.tremolo)
        } else {
            // A full-scale modulator shifts the carrier phase by up to 4 cycles.
            self.c.output(fnum, block, inc_c, mo * 4.0, lfo.tremolo)
        }
    }
}

/// Shared low-frequency oscillators (tremolo 3.7 Hz / 1 dB, vibrato 6.1 Hz / 7 cents).
#[derive(Clone, Copy, Debug, Default)]
pub struct Lfo {
    pub tremolo: f64,
    pub vibrato: f64,
}

#[derive(Clone, Debug, Default)]
pub struct LfoGen {
    t: f64,
}

impl LfoGen {
    pub fn step(&mut self, sr: f64) -> Lfo {
        self.t += 1.0 / sr;
        let tri = |f: f64| {
            let x = (self.t * f).rem_euclid(1.0);
            if x < 0.5 { 4.0 * x - 1.0 } else { 3.0 - 4.0 * x }
        };
        // 1 dB ≈ 5.33 attenuation units, applied as 0..depth.
        let tremolo = (tri(3.7) + 1.0) * 0.5 * (1.0 / 0.1875);
        let vibrato = 2f64.powf(tri(6.1) * 7.0 / 1200.0);
        Lfo { tremolo, vibrato }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_patch() -> Patch {
        let op = OpParams { multiple: 1, attack: 15, decay: 4, sustain_level: 2, sustaining: true, release: 6, ..Default::default() };
        Patch { modulator: OpParams { total_level: 63, ..op }, carrier: op, ..Default::default() }
    }

    #[test]
    fn fnum_round_trip_is_close() {
        for hz in [32.7, 261.63, 440.0, 3520.0] {
            let (f, b) = fnum_block(hz);
            assert!((fnum_hz(f, b) - hz).abs() / hz < 0.002, "{hz}");
        }
    }

    #[test]
    fn voice_sounds_then_releases_to_silence() {
        let sr = 44100.0;
        let mut v = Voice::new(simple_patch());
        v.set_frequency(440.0);
        v.set_level(1.0);
        v.key_on();
        let lfo = Lfo { tremolo: 0.0, vibrato: 1.0 };
        let peak = (0..4410).map(|_| v.sample(sr, lfo).abs()).fold(0.0, f64::max);
        assert!(peak > 0.5, "peak {peak}");
        v.key_off();
        for _ in 0..(sr as usize * 3) {
            v.sample(sr, lfo);
        }
        assert!(v.is_silent());
    }

    #[test]
    fn modulator_adds_harmonics() {
        // With the modulator audible in FM, the carrier's zero crossings move.
        let sr = 44100.0;
        let mut plain = Voice::new(simple_patch());
        let mut fm = Voice::new(Patch { modulator: OpParams { total_level: 0, ..simple_patch().modulator }, ..simple_patch() });
        for v in [&mut plain, &mut fm] {
            v.set_frequency(220.0);
            v.key_on();
        }
        let lfo = Lfo { tremolo: 0.0, vibrato: 1.0 };
        let diff: f64 = (0..2000).map(|_| (plain.sample(sr, lfo) - fm.sample(sr, lfo)).abs()).sum();
        assert!(diff > 10.0);
    }
}
