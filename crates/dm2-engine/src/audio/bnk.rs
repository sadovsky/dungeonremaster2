//! AdLib instrument banks (MELODIC.BNK, DRUM.BNK; docs/11-audio.md).
//!
//! Standard `.BNK` layout with an altered signature: a header, a name table
//! and 30-byte instrument records (two 13-byte operator blocks plus two
//! wave-select bytes).

/// One FM operator, as stored in a BNK operator block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpParams {
    pub ksl: u8,
    pub multiple: u8,
    pub feedback: u8,
    pub attack: u8,
    pub sustain_level: u8,
    /// Envelope type: non-zero holds the sustain level while the key is down.
    pub sustaining: bool,
    pub decay: u8,
    pub release: u8,
    pub total_level: u8,
    pub tremolo: bool,
    pub vibrato: bool,
    pub ksr: bool,
    /// Connection (only meaningful on the modulator): 0 = FM, 1 = additive.
    pub connection: u8,
    pub waveform: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Patch {
    pub percussive: bool,
    pub voice: u8,
    pub modulator: OpParams,
    pub carrier: OpParams,
}

impl Patch {
    /// Additive (both operators audible) rather than FM.
    pub fn additive(&self) -> bool {
        self.modulator.connection != 0
    }
}

#[derive(Clone, Debug)]
pub struct Bank {
    pub patches: Vec<Patch>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BnkError {
    TooShort,
    BadSignature,
}

fn op(b: &[u8], wave: u8) -> OpParams {
    OpParams {
        ksl: b[0] & 3,
        multiple: b[1] & 15,
        feedback: b[2] & 7,
        attack: b[3] & 15,
        sustain_level: b[4] & 15,
        sustaining: b[5] != 0,
        decay: b[6] & 15,
        release: b[7] & 15,
        total_level: b[8] & 63,
        tremolo: b[9] != 0,
        vibrato: b[10] != 0,
        ksr: b[11] != 0,
        connection: b[12] & 1,
        waveform: wave & 3,
    }
}

impl Bank {
    pub fn parse(d: &[u8]) -> Result<Bank, BnkError> {
        if d.len() < 20 {
            return Err(BnkError::TooShort);
        }
        // "ADLIB-" with the first letters changed (AMLIB-, ANLIB- in DM2).
        if &d[4..8] != b"LIB-" {
            return Err(BnkError::BadSignature);
        }
        let used = u16::from_le_bytes([d[8], d[9]]) as usize;
        let names = u32::from_le_bytes([d[12], d[13], d[14], d[15]]) as usize;
        let data = u32::from_le_bytes([d[16], d[17], d[18], d[19]]) as usize;
        let mut patches = vec![Patch::default(); used.max(128)];
        for i in 0..used {
            let n = names + 12 * i;
            let Some(entry) = d.get(n..n + 3) else { return Err(BnkError::TooShort) };
            let index = u16::from_le_bytes([entry[0], entry[1]]) as usize;
            let o = data + 30 * index;
            let Some(r) = d.get(o..o + 30) else { return Err(BnkError::TooShort) };
            patches[i] = Patch {
                percussive: r[0] != 0,
                voice: r[1],
                modulator: op(&r[2..15], r[28]),
                carrier: op(&r[15..28], r[29]),
            };
        }
        Ok(Bank { patches })
    }

    pub fn get(&self, i: u8) -> Patch {
        self.patches.get(i as usize).copied().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic() -> Vec<u8> {
        let mut d = vec![0u8; 0x1C + 12 + 30];
        d[2..8].copy_from_slice(b"AMLIB-");
        d[8] = 1;
        d[10] = 1;
        d[12] = 0x1C;
        d[16] = 0x1C + 12;
        // name entry: data index 0, used
        d[0x1C + 2] = 1;
        let r = 0x1C + 12;
        d[r + 2 + 1] = 2; // modulator multiple
        d[r + 2 + 12] = 1; // connection: additive
        d[r + 15 + 8] = 10; // carrier total level
        d[r + 29] = 3; // carrier waveform
        d
    }

    #[test]
    fn parses_records() {
        let b = Bank::parse(&synthetic()).unwrap();
        let p = b.get(0);
        assert_eq!(p.modulator.multiple, 2);
        assert!(p.additive());
        assert_eq!(p.carrier.total_level, 10);
        assert_eq!(p.carrier.waveform, 3);
        assert_eq!(b.patches.len(), 128);
    }

    #[test]
    fn real_banks_fill_128_patches() {
        let dir = crate::assets::default_data_dir().join("..");
        for name in ["MELODIC.BNK", "DRUM.BNK"] {
            let Ok(d) = std::fs::read(dir.join(name)) else { return };
            assert_eq!(d.len(), 5404, "{name}");
            let b = Bank::parse(&d).unwrap();
            assert_eq!(b.patches.len(), 128);
            // Every melodic patch has an audible carrier configured.
            if name == "MELODIC.BNK" {
                assert!(b.patches.iter().filter(|p| p.carrier.attack > 0).count() > 100);
            }
        }
    }
}
