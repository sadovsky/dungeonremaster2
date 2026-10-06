//! MIDI-to-FM driver: maps channel messages onto 18 two-operator voices
//! using the MELODIC and DRUM banks (docs/11-audio.md).
//!
//! HMI's own voice allocator lives in HMIMDRV.386 and has not been
//! reversed, so this follows the usual AdLib-driver conventions: free voice
//! first, then the oldest released voice, then the oldest sounding voice;
//! velocity, channel volume and expression scale the audible operators'
//! total level; MIDI channel 9 plays DRUM.BNK patch `note`.

use super::bnk::Bank;
use super::hmp::Msg;
use super::opl::{Lfo, LfoGen, Voice};

pub const VOICES: usize = 18;
pub const DRUM_CHANNEL: u8 = 9;

#[derive(Clone, Copy, Debug)]
struct Channel {
    program: u8,
    volume: u8,
    expression: u8,
    pan: u8,
    bend: u16,
    sustain: bool,
}

impl Default for Channel {
    fn default() -> Self {
        Channel { program: 0, volume: 100, expression: 127, pan: 64, bend: 8192, sustain: false }
    }
}

#[derive(Clone, Debug)]
struct Slot {
    voice: Voice,
    ch: u8,
    note: u8,
    vel: u8,
    on: bool,
    /// Key released while the sustain pedal was down.
    held: bool,
    age: u64,
    active: bool,
}

pub struct Driver {
    melodic: Bank,
    drums: Bank,
    channels: [Channel; 16],
    slots: Vec<Slot>,
    clock: u64,
    lfo: LfoGen,
}

fn note_hz(note: f64) -> f64 {
    440.0 * 2f64.powf((note - 69.0) / 12.0)
}

impl Driver {
    pub fn new(melodic: Bank, drums: Bank) -> Driver {
        let slots = (0..VOICES)
            .map(|_| Slot { voice: Voice::new(Default::default()), ch: 0, note: 0, vel: 0, on: false, held: false, age: 0, active: false })
            .collect();
        Driver { melodic, drums, channels: [Channel::default(); 16], slots, clock: 0, lfo: LfoGen::default() }
    }

    pub fn reset(&mut self) {
        self.channels = [Channel::default(); 16];
        for s in &mut self.slots {
            s.voice.key_off();
            s.on = false;
            s.held = false;
            s.active = false;
        }
    }

    pub fn all_notes_off(&mut self) {
        for s in &mut self.slots {
            if s.on || s.held {
                s.voice.key_off();
                s.on = false;
                s.held = false;
            }
        }
    }

    fn level(&self, ch: u8, vel: u8) -> f64 {
        let c = &self.channels[ch as usize];
        (vel as f64 / 127.0) * (c.volume as f64 / 127.0) * (c.expression as f64 / 127.0)
    }

    fn pitch(&self, ch: u8, note: u8) -> f64 {
        let bend = (self.channels[ch as usize].bend as f64 - 8192.0) / 8192.0 * 2.0;
        note_hz(note as f64 + bend)
    }

    fn pick_slot(&self) -> usize {
        let free = self.slots.iter().position(|s| !s.active || s.voice.is_silent());
        if let Some(i) = free {
            return i;
        }
        let released = (0..VOICES).filter(|&i| !self.slots[i].on && !self.slots[i].held).min_by_key(|&i| self.slots[i].age);
        released.unwrap_or_else(|| (0..VOICES).min_by_key(|&i| self.slots[i].age).unwrap())
    }

    fn note_on(&mut self, ch: u8, note: u8, vel: u8) {
        let patch = if ch == DRUM_CHANNEL { self.drums.get(note) } else { self.melodic.get(self.channels[ch as usize].program) };
        let i = self.pick_slot();
        self.clock += 1;
        let hz = self.pitch(ch, note);
        let level = self.level(ch, vel);
        let s = &mut self.slots[i];
        s.voice.set_patch(patch);
        s.voice.set_frequency(hz);
        s.voice.set_level(level);
        s.voice.key_on();
        *s = Slot { ch, note, vel, on: true, held: false, age: self.clock, active: true, voice: s.voice.clone() };
    }

    fn note_off(&mut self, ch: u8, note: u8) {
        let sustain = self.channels[ch as usize].sustain;
        for s in &mut self.slots {
            if s.active && s.on && s.ch == ch && s.note == note {
                s.on = false;
                if sustain {
                    s.held = true;
                } else {
                    s.voice.key_off();
                }
            }
        }
    }

    fn refresh_channel(&mut self, ch: u8) {
        for i in 0..VOICES {
            let s = &self.slots[i];
            if s.active && s.ch == ch && (s.on || s.held) {
                let (hz, level) = (self.pitch(ch, s.note), self.level(ch, s.vel));
                let v = &mut self.slots[i].voice;
                v.set_frequency(hz);
                v.set_level(level);
            }
        }
    }

    pub fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::NoteOn { ch, note, vel } => self.note_on(ch, note, vel),
            Msg::NoteOff { ch, note } => self.note_off(ch, note),
            Msg::Program { ch, program } => self.channels[ch as usize].program = program,
            Msg::PitchBend { ch, value } => {
                self.channels[ch as usize].bend = value;
                self.refresh_channel(ch);
            }
            Msg::Control { ch, cc, value } => {
                let c = &mut self.channels[ch as usize];
                match cc {
                    7 => c.volume = value,
                    10 => c.pan = value,
                    11 => c.expression = value,
                    64 => {
                        c.sustain = value >= 64;
                        if !c.sustain {
                            for s in &mut self.slots {
                                if s.ch == ch && s.held {
                                    s.held = false;
                                    s.voice.key_off();
                                }
                            }
                        }
                    }
                    121 => *c = Channel::default(),
                    123 => {
                        for s in &mut self.slots {
                            if s.ch == ch && (s.on || s.held) {
                                s.on = false;
                                s.held = false;
                                s.voice.key_off();
                            }
                        }
                    }
                    _ => {}
                }
                if matches!(cc, 7 | 11) {
                    self.refresh_channel(ch);
                }
            }
            Msg::Aftertouch { .. } => {}
        }
    }

    /// Mix one stereo frame. OPL3 output routing is left/right/both, so pan
    /// selects one side or both rather than a smooth position.
    pub fn frame(&mut self, sr: f64) -> (f64, f64) {
        let lfo: Lfo = self.lfo.step(sr);
        let (mut l, mut r) = (0.0, 0.0);
        for s in &mut self.slots {
            if !s.active {
                continue;
            }
            let v = s.voice.sample(sr, lfo);
            let pan = self.channels[s.ch as usize].pan;
            if pan <= 42 {
                l += v;
            } else if pan >= 86 {
                r += v;
            } else {
                l += v;
                r += v;
            }
            if !s.on && !s.held && s.voice.is_silent() {
                s.active = false;
            }
        }
        (l, r)
    }
}
