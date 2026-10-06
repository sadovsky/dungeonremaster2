//! Runes and spells (docs/07-combat-magic.md, "Magic").
//!
//! The rune costs, power multipliers and spell table come from the user's
//! SKULL.EXE through `ExeTables`; nothing is embedded here.

use crate::champions::{self, stat, Champion, PartyStatus};
use crate::combat::Effect;
use crate::exe::{ExeTables, Spell};
use crate::rng::Rng;

/// Rune symbols are 0x60 + row * 6 + column.
pub const RUNE_BASE: u8 = 0x60;
pub const MAX_RUNES: usize = 4;

/// Result codes passed to the message routine (0x4384B), before the class
/// is ORed in.
pub const FAIL_PRACTICE: u8 = 0x10;
pub const FAIL_MEANINGLESS: u8 = 0x20;
pub const FAIL_NEED_FLASK: u8 = 0x30;

fn rune_count(c: &Champion) -> usize {
    c.raw[0x1E] as usize
}

/// The runes entered so far.
pub fn runes(c: &Champion) -> Vec<u8> {
    c.raw[0x22..0x22 + rune_count(c).min(MAX_RUNES)].to_vec()
}

/// Mana cost of entering rune `column` as the next rune (0x42924).
pub fn rune_cost(c: &Champion, column: usize, t: &ExeTables) -> i16 {
    let row = rune_count(c).min(3);
    let base = t.rune_costs[row][column] as i32;
    if row == 0 {
        base as i16
    } else {
        let first = c.raw[0x22].wrapping_sub(RUNE_BASE) as usize;
        ((base * t.power_mult[first.min(5)] as i32) >> 3) as i16
    }
}

/// Enter a rune if the champion has the mana (0x42924). Returns true when
/// the rune was added.
pub fn enter_rune(c: &mut Champion, column: usize, t: &ExeTables) -> bool {
    let n = rune_count(c);
    if n >= MAX_RUNES || column >= 6 {
        return false;
    }
    let cost = rune_cost(c, column, t);
    if cost > c.mana() {
        return false;
    }
    c.set_mana(c.mana() - cost);
    c.flag_redraw(0x0800);
    c.raw[0x22 + n] = RUNE_BASE + (n * 6 + column) as u8;
    c.raw[0x1E] = (n + 1) as u8;
    if n + 1 < MAX_RUNES {
        c.raw[0x22 + n + 1] = 0;
    }
    true
}

/// Remove the last rune without refunding it (0x429F5).
pub fn remove_rune(c: &mut Champion) {
    let n = rune_count(c);
    if n > 0 {
        c.raw[0x22 + n - 1] = 0;
        c.raw[0x1E] = (n - 1) as u8;
    }
}

pub fn clear_runes(c: &mut Champion) {
    c.raw[0x22..0x26].fill(0);
    c.raw[0x1E] = 0;
}

/// Find the spell for a rune sequence (0x421FD). Needs at least two runes;
/// a spell whose required power byte is 0 matches any power.
pub fn find_spell<'a>(runes: &[u8], t: &'a ExeTables) -> Option<&'a Spell> {
    if runes.len() < 2 || runes[1] == 0 {
        return None;
    }
    let mut key = 0u32;
    for (i, &r) in runes.iter().take(4).enumerate() {
        if r == 0 {
            break;
        }
        key |= (r as u32) << (24 - 8 * i);
    }
    t.spells.iter().find(|s| if s.key >> 24 == 0 { key & 0xFF_FFFF == s.key } else { key == s.key })
}

/// What a successful cast asks of the world besides the champion's own
/// changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CastEffect {
    /// Fill the empty flask in a hand with potion `kind` of `power`.
    MakePotion { kind: u8, power: u8 },
    /// Create the item named by attribute key (13, 15, 11, 0x42); into the
    /// leader's hand if empty, otherwise dropped on the party square.
    CreateItem,
    /// Summon a minion of creature type `kind` in front of the party with
    /// this power, or recall an existing one (type 0x35).
    Summon { kind: u8, power: u16 },
    Other(Effect),
}

/// Outcome of `cast`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CastResult {
    /// Code for the message routine: FAIL_* | class.
    Failed(u8),
    Success { effects: Vec<CastEffect>, cooldown: u16 },
}

fn class_of(skill: u8) -> u8 {
    if skill > 3 { (skill - 4) >> 2 } else { skill }
}

/// Cast the champion's runes (0x428A2 then 0x422F5). `has_flask` tells
/// whether an empty flask is in a hand (0x4225E).
#[allow(clippy::too_many_arguments)]
pub fn cast(
    champions: &mut [Champion],
    party: &mut PartyStatus,
    idx: usize,
    t: &ExeTables,
    has_flask: bool,
    tick: u32,
    map_multiplier: u32,
    rng: &mut Rng,
) -> CastResult {
    let rs = runes(&champions[idx]);
    let Some(spell) = find_spell(&rs, t).copied() else {
        clear_runes(&mut champions[idx]);
        return CastResult::Failed(FAIL_MEANINGLESS);
    };
    let r = cast_spell(champions, party, idx, &spell, rs[0].wrapping_sub(0x5F), has_flask, tick, map_multiplier, rng);
    if !matches!(r, CastResult::Failed(code) if code & 0xF0 == FAIL_NEED_FLASK) {
        clear_runes(&mut champions[idx]);
    }
    r
}

/// The spell itself (0x422F5); `power` is 1-6.
#[allow(clippy::too_many_arguments)]
pub fn cast_spell(
    champions: &mut [Champion],
    party: &mut PartyStatus,
    idx: usize,
    spell: &Spell,
    power: u8,
    has_flask: bool,
    tick: u32,
    map_multiplier: u32,
    rng: &mut Rng,
) -> CastResult {
    let p = power as i32;
    let base = spell.base_level as i32;
    let cooldown = (spell.duration as i32 * (p + 18) / 24) as u16;
    let required = base + p;
    let xp = (rng.rnd() & 7) as i32 + 16 * required + 8 * base * (p - 1) + required * required;
    let sk = spell.skill as usize;
    let lv = champions::level(&champions[idx], party, sk, true) as i32;
    let fail = |code: u8| CastResult::Failed(code | class_of(spell.skill));
    let mut deficit = required - lv;
    while deficit > 0 {
        let r = (rng.rnd() & 0x7F) as i32;
        let w = (champions::stat_current(&champions[idx], stat::WISDOM, rng) as i32 + 15).min(115);
        if w < r {
            let shift = (required - lv).clamp(0, 31) as u32;
            champions::add_experience(champions, party, idx, sk, (xp >> shift).max(0) as u32, tick, map_multiplier, rng);
            return fail(FAIL_PRACTICE);
        }
        deficit -= 1;
    }
    let mut effects = Vec::new();
    let mut level = lv;
    match spell.kind {
        1 => {
            if !has_flask {
                return fail(FAIL_NEED_FLASK);
            }
            let r = (rng.rnd() & 15) as i32;
            effects.push(CastEffect::MakePotion { kind: spell.kind_type, power: (p * 40 + r) as u8 });
        }
        2 => {
            if spell.kind_type == 4 {
                level *= 2;
            }
            let energy = ((level * 2 + 4) * (p + 2)).clamp(21, 255) as u8;
            effects.push(CastEffect::Other(crate::combat::projectile(
                champions,
                idx,
                0xFF80u16.wrapping_add(spell.kind_type as u16),
                energy,
            )));
        }
        3 => {
            let q = p + 1;
            let s = q * 4;
            let shield = |kind: u8, v: i32| CastEffect::Other(Effect::PartyEffect { kind, strength: v as u16 });
            match spell.kind_type {
                0 => effects.push(CastEffect::Other(Effect::Light { kind: 0x27, strength: (36 * q) as u16 })),
                1 => effects.push(CastEffect::Other(Effect::Light { kind: 6, strength: (36 * q) as u16 })),
                5 => effects.push(CastEffect::Other(Effect::Light { kind: 0x26, strength: (36 * q) as u16 })),
                2 => effects.push(shield(1, s * s + 100)),
                8 => effects.push(shield(0, s * s + 100)),
                4 => effects.push(shield(2, s * s)),
                6 => effects.push(shield(5, (s + 3) * (s + 3))),
                7 => effects.push(shield(4, (s + 3) * (s + 3))),
                9 => effects.push(shield(6, (s + 3) * (s + 3))),
                10 => effects.push(shield(3, (s + 3) * (s + 3))),
                3 => effects.push(CastEffect::Other(Effect::Invisibility { ticks: (32 * q) as u16 })),
                0x0B => party.haste = (party.haste as i32 + 32 * q).min(255) as u8,
                0x0E => {
                    let e = ((2 * level + 4) * (p + 2)).clamp(21, 255);
                    effects.push(CastEffect::Other(Effect::Explosion { kind: 0xFF8E, strength: e as u16 }));
                }
                0x0F => effects.push(CastEffect::CreateItem),
                _ => {}
            }
        }
        4 => {
            if spell.kind_type == 0x35 {
                effects.push(CastEffect::Summon { kind: 0x35, power: 0 });
            } else {
                let r = rng.rand4() as i32;
                let pw = ((2 * level + r) * p / 6) as u16;
                effects.push(CastEffect::Summon { kind: spell.kind_type, power: pw });
            }
        }
        _ => {}
    }
    if cooldown != 0 {
        champions::add_experience(champions, party, idx, sk, xp as u32, tick, map_multiplier, rng);
    }
    CastResult::Success { effects, cooldown }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exe::default_exe_path;

    fn tables() -> Option<ExeTables> {
        ExeTables::load(&default_exe_path()).ok()
    }

    fn mage() -> Champion {
        let mut c = Champion::default();
        c.set_health(50);
        c.set_max_health(50);
        c.set_stamina(500);
        c.set_max_stamina(500);
        c.set_mana(900);
        c.set_max_mana(900);
        for s in 0..7 {
            c.set_stat_raw(s, 0, 60);
            c.set_stat_raw(s, 1, 60);
        }
        c
    }

    #[test]
    fn rune_entry_costs_and_buffer() {
        let Some(t) = tables() else { return };
        let mut c = mage();
        let first_cost = t.rune_costs[0][2] as i16;
        assert!(enter_rune(&mut c, 2, &t));
        assert_eq!(c.mana(), 900 - first_cost);
        let second = (t.rune_costs[1][1] as i32 * t.power_mult[2] as i32 >> 3) as i16;
        assert_eq!(rune_cost(&c, 1, &t), second);
        assert!(enter_rune(&mut c, 1, &t));
        assert_eq!(runes(&c), vec![0x62, 0x60 + 6 + 1]);
        remove_rune(&mut c);
        assert_eq!(runes(&c), vec![0x62]);
        c.set_mana(0);
        assert!(!enter_rune(&mut c, 0, &t));
    }

    #[test]
    fn every_spell_is_findable_from_its_runes() {
        let Some(t) = tables() else { return };
        for s in &t.spells {
            let mut rs = vec![if s.key >> 24 == 0 { RUNE_BASE } else { (s.key >> 24) as u8 }];
            for sh in [16, 8, 0] {
                let r = (s.key >> sh) as u8;
                if r != 0 {
                    rs.push(r);
                }
            }
            let found = find_spell(&rs, &t).expect("spell");
            assert_eq!(found.key & 0xFF_FFFF, s.key & 0xFF_FFFF);
        }
        assert!(find_spell(&[0x60], &t).is_none());
    }

    #[test]
    fn cast_checks_level_with_fixed_rng() {
        let Some(t) = tables() else { return };
        // A spell far above the champion's level must fail or succeed with
        // the exact random-number usage of the original loop.
        let spell = Spell { key: 0, base_level: 6, skill: 16, kind: 3, kind_type: 0x0B, duration: 20 };
        let mut champs = vec![mage()];
        let mut party = PartyStatus { last_attacked: 0, ..Default::default() };
        let mut rng = Rng::new(42);
        let mut e = Rng::new(42);
        let r = cast_spell(&mut champs, &mut party, 0, &spell, 6, false, 5000, 0, &mut rng);
        let xp_roll = e.rnd() & 7;
        let required = 12i32;
        let xp = xp_roll as i32 + 16 * required + 8 * 6 * 5 + required * required;
        let mut deficit = required - 1;
        let mut failed = false;
        while deficit > 0 {
            if 75 < (e.rnd() & 0x7F) as i32 {
                failed = true;
                break;
            }
            deficit -= 1;
        }
        match r {
            CastResult::Failed(code) => {
                assert!(failed);
                assert_eq!(code, FAIL_PRACTICE | 3); // skill 16 is a wizard sub-skill
                // sub-skill 16 is not a fighter/ninja skill: no halving;
                // xp >> deficit with deficit 11
                assert_eq!(champs[0].experience(16), (xp >> 11) as u32);
            }
            CastResult::Success { cooldown, .. } => {
                assert!(!failed);
                assert_eq!(cooldown, (20 * 24 / 24) as u16);
                assert_eq!(party.haste, (32 * 7) as u8);
            }
        }
        let _ = t;
    }
}
