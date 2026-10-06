//! Creature combat (docs/08 "Damage and death").

use crate::champions::{self, skill};
use crate::combat;
use crate::items::ItemDb;
use crate::rng::Rng;
use crate::state::GameState;

use super::data::{CreatureData, Info};

/// Creature-vs-creature damage (0x31113). `None` is a miss.
pub fn creature_vs_creature(att: &Info, def: &Info, rng: &mut Rng) -> Option<u16> {
    if def.dexterity() == 0xFF {
        return None;
    }
    let a = rng.random(32) as i32 + att.dexterity() as i32;
    let d = rng.random(32) as i32 + def.dexterity() as i32;
    if a < d && rng.random(4) != 0 {
        return None;
    }
    let s = att.attack() as i32;
    let mut dmg = s + s.min((rng.rnd() & 15) as i32) - ((def.defence() as i32 + rng.random(32) as i32) >> 3);
    if dmg < 2 {
        if rng.bit() == 0 {
            return None;
        }
        dmg = rng.random(4) as i32 + 2;
    }
    dmg += rng.random(dmg as u16) as i32 + rng.random(4) as i32;
    dmg += rng.random(dmg as u16) as i32;
    dmg = dmg / 4 + rng.random(4) as i32 + 1;
    if rng.bit() != 0 {
        dmg -= rng.random((dmg / 4 + 1) as u16) as i32;
    }
    Some(dmg.max(0) as u16)
}

/// Body part hit, from the info +0x1A nibbles (tentative mapping of the
/// selector to parts: torso, head, legs, feet).
fn hit_parts(info: &Info, rng: &mut Rng) -> u16 {
    const PARTS: [u16; 4] =
        [champions::part::TORSO, champions::part::HEAD, champions::part::LEGS, champions::part::FEET];
    let r = rng.rnd();
    let mut sel = 0usize;
    let mut nib = info.hit_parts();
    if r & 0x70 != 0 && nib != 0 {
        let t = ((r & 15) as u16).max(1);
        while sel < 3 && nib & 15 < t {
            sel += 1;
            nib >>= 4;
        }
    }
    PARTS[sel]
}

/// One creature blow against champion `idx` (0x18758). Returns damage dealt.
pub fn attack_champion(g: &mut GameState, data: &CreatureData, info: &Info, idx: usize) -> i16 {
    if !g.champions.get(idx).is_some_and(|c| c.is_alive()) {
        return 0;
    }
    let atype = info.attack_type();
    // Light-dependent bonus terms (0x7F3BC / 0x7F282) are not modelled.
    let boost = 0i32;
    let dex = match atype {
        9 => (info.dexterity() as u16 * 2).min(0xFF),
        8 => 0xFF,
        _ => info.dexterity() as u16,
    };
    if !g.party_status.asleep && dex != 0xFF {
        let r = (g.rng.rnd() & 0x1F) as i32;
        let target = dex as i32 + r + boost;
        let cdex = champions::dexterity(&g.champions[idx], &g.party_status, &mut g.rng) as i32;
        let dodged = (cdex > target - 16 && g.rng.bit() != 0) || champions::lucky(&mut g.champions[idx], 60, &mut g.rng);
        if dodged {
            return 0;
        }
    }
    let parts = hit_parts(info, &mut g.rng);
    let s = info.attack() as i32;
    let mut s = s + s.min((g.rng.rnd() & 15) as i32 + boost);
    if atype != 8 {
        s -= 2 * champions::level(&g.champions[idx], &g.party_status, skill::PARRY, true) as i32;
        if s < 2 {
            if g.rng.bit() != 0 {
                return 0;
            }
            s = g.rng.rand4() as i32 + 2;
        }
    }
    let half = (s >> 1) as u16;
    let u = half as i32 + g.rng.random(half) as i32 + g.rng.rand4() as i32;
    let mut dmg = ((u + g.rng.random(u as u16) as i32) >> 2) + g.rng.rand4() as i32 + 1;
    if g.rng.bit() != 0 {
        dmg -= g.rng.random(((dmg >> 1) + 1) as u16) as i32 - 1;
    }
    let db = ItemDb { gdat: &data.gdat, dungeon: &g.dungeon, categories: &data.tables.thing_category };
    let dealt = combat::damage_champion(
        &mut g.champions,
        &mut g.party_status,
        idx,
        dmg as i16,
        parts,
        atype as u16,
        &db,
        &data.tables,
        &mut g.rng,
    );
    if dealt != 0 && info.poison() != 0 && g.rng.bit() != 0 {
        let p = champions::stat_adjusted(&g.champions[idx], champions::stat::VITALITY, info.poison() as i16, &mut g.rng);
        if p > 0 {
            champions::poison(g, idx, p);
        }
    }
    dealt
}

/// Outcome of a hit on a creature (0x24E62).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HitResult {
    pub became_afraid: bool,
    pub turned_to_party: bool,
    /// Pending damage now meets or exceeds HP: interrupt the current action.
    pub interrupt: bool,
}

/// Register a hit: add to the owed damage and update fear (0x24E62).
/// `status` is the creature record's +0x0A word, `hp` its +0x06 word.
pub fn take_hit(
    info: &Info,
    class_flags: u32,
    status: &mut u16,
    pending: &mut u16,
    hp: u16,
    amount: u16,
    rng: &mut Rng,
) -> HitResult {
    let mut r = HitResult::default();
    *pending = pending.saturating_add(amount);
    if *status & 4 == 0 {
        let max = info.base_hp().max(1) as u32;
        let scared =
            amount > 30 || (amount > 4 && rng.random(4) == 0) || (amount as u32 * 100 / max) > 15;
        if scared {
            *status |= 4;
            r.became_afraid = true;
        }
    }
    if class_flags & 0x80 == 0 && rng.bit() != 0 {
        r.turned_to_party = true;
    }
    r.interrupt = *pending >= hp;
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(attack: u8, dex: u8, def: u8, hp: u16) -> Info {
        let mut raw = [0u8; 36];
        raw[6] = attack;
        raw[8] = dex;
        raw[2] = def;
        raw[4..6].copy_from_slice(&hp.to_le_bytes());
        Info { raw }
    }

    #[test]
    fn untouchable_defender_is_never_hit() {
        let mut rng = Rng::new(7);
        assert_eq!(creature_vs_creature(&info(50, 50, 10, 100), &info(1, 0xFF, 0, 10), &mut rng), None);
        assert_eq!(rng.state, 7, "no random calls before the 0xFF check");
    }

    #[test]
    fn fixed_seed_damage_is_stable() {
        let a = info(60, 40, 20, 100);
        let d = info(10, 10, 20, 100);
        let mut rng = Rng::new(12345);
        let hits: Vec<Option<u16>> = (0..8).map(|_| creature_vs_creature(&a, &d, &mut rng)).collect();
        // Every outcome is either a miss or a positive, bounded amount.
        assert!(hits.iter().flatten().all(|&h| h <= 200));
        // And the sequence is reproducible from the seed.
        let mut rng2 = Rng::new(12345);
        let again: Vec<Option<u16>> = (0..8).map(|_| creature_vs_creature(&a, &d, &mut rng2)).collect();
        assert_eq!(hits, again);
        assert_eq!(rng.state, rng2.state);
    }

    #[test]
    fn heavy_hits_cause_fear() {
        let i = info(10, 10, 10, 100);
        let mut rng = Rng::new(3);
        let (mut status, mut pending) = (0u16, 0u16);
        let r = take_hit(&i, 0x80, &mut status, &mut pending, 100, 40, &mut rng);
        assert!(r.became_afraid && status & 4 != 0);
        assert_eq!(pending, 40);
        assert!(!r.turned_to_party, "class flag 0x80 never turns");
        let r = take_hit(&i, 0x80, &mut status, &mut pending, 100, 70, &mut rng);
        assert!(r.interrupt, "owed damage reached HP");
    }
}
