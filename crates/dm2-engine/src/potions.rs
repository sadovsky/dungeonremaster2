//! Drinking potions (the potion branch of 0x39C3F, docs/09 "Potions").
//!
//! A potion's word 1 holds the kind (bits 8-14) and the power (bits 0-7).
//! Three amounts derived from the power drive the effects:
//! `s = power / 25 + 8`, the divisor `d = ((511 − power) / ((power + 1) / 8
//! + 32)) / 2`, and the power itself.

use crate::champions::{self, stat};
use crate::state::GameState;

/// Party effect kind used by kind 12 (+0x102 value 2: armour bonus).
const EFFECT_ARMOUR: u8 = 2;

/// Raise a stat's current value (0x459C8). Gains that push the stat past
/// its maximum shrink by a quarter for every 20 points of overshoot; the
/// result is clamped to 10-220.
pub fn raise_stat(c: &mut champions::Champion, s: usize, amount: i16) {
    let (cur, max) = (c.stat_raw(s, 0) as i16, c.stat_raw(s, 1) as i16);
    let mut amount = amount;
    let over = cur + amount - max;
    if (over < 0) == (amount < 0) {
        let mut over = over.abs();
        while over > 20 {
            amount -= amount >> 2;
            over -= 20;
        }
    }
    c.set_stat_raw(s, 0, (cur + amount).clamp(10, 220) as u8);
}

/// Cure poison (0x475D3): reduce the champion's poison by `amount`. The
/// original trims the remaining dose stored on each pending poison event;
/// this engine keeps the dose in the pool, so emptying the pool cancels
/// the champion's pending poison events.
pub fn cure_poison(g: &mut GameState, idx: usize, amount: i16) {
    let Some(c) = g.champions.get_mut(idx) else { return };
    champions::cure_poison(c, amount);
    if c.poison_pool() > 0 {
        return;
    }
    let pending: Vec<u16> = g
        .timeline
        .iter()
        .filter(|(_, e)| e.kind == champions::EVENT_POISON && e.prio as usize == idx)
        .map(|(s, _)| s)
        .collect();
    for s in pending {
        g.timeline.delete(s);
    }
}

/// Apply potion `kind` with `power` to champion `idx`. Returns false for
/// kinds that have no drink effect (the potion is then not consumed).
pub fn drink(g: &mut GameState, idx: usize, kind: u16, power: u16) -> bool {
    if idx >= g.champions.len() {
        return false;
    }
    let p = power & 0xFF;
    let s = (p / 25 + 8) as i16;
    let d = ((((0x1FF - p as i32) / (((p as i32 + 1) >> 3) + 0x20)) >> 1).max(1)) as i32;
    match kind {
        6 => raise_stat(&mut g.champions[idx], stat::DEXTERITY, s),
        7 => raise_stat(&mut g.champions[idx], stat::STRENGTH, (p / 35 + 5) as i16),
        8 => raise_stat(&mut g.champions[idx], stat::WISDOM, s),
        9 => raise_stat(&mut g.champions[idx], stat::VITALITY, s),
        10 => {
            let q = (p / 7) as i16;
            cure_poison(g, idx, s + p as i16 + q * q);
        }
        11 => {
            let c = &mut g.champions[idx];
            let gain = ((c.max_stamina() as i32 - c.stamina() as i32)).min(c.max_stamina() as i32 / d);
            c.set_stamina((c.stamina() as i32 + gain) as i16);
        }
        12 => {
            let a = s + (s >> 1);
            party_armour(g, idx, a);
        }
        13 => {
            let c = &mut g.champions[idx];
            let mut m = (c.mana() as i32 + 2 * s as i32 - 8).min(900);
            let max = c.max_mana() as i32;
            if m > max {
                let floor = (c.mana() as i32).max(max);
                m -= (m - floor) >> 1;
            }
            c.set_mana(m as i16);
        }
        14 => {
            let tries = (p / 42).max(1);
            let rng = &mut g.rng;
            let c = &mut g.champions[idx];
            c.set_health((c.health() as i32 + c.max_health() as i32 / d) as i16);
            let old = c.wounds();
            if old != 0 {
                let mut n = tries;
                for _ in 0..10 {
                    for _ in 0..n {
                        let w = c.wounds() & rng.rnd() as u16;
                        c.set_wounds(w);
                    }
                    n = 1;
                    if c.wounds() != old {
                        break;
                    }
                }
            }
        }
        15 => {
            let c = &mut g.champions[idx];
            c.set_water((c.water() as i32 + 0x640).min(0x800) as i16);
        }
        _ => return false,
    }
    // Afterwards stamina and health are clamped to their maxima.
    let c = &mut g.champions[idx];
    if c.stamina() > c.max_stamina() {
        c.set_stamina(c.max_stamina());
    }
    if c.health() > c.max_health() {
        c.set_health(c.max_health());
    }
    true
}

/// Kind 12: an armour bonus for the drinker (0x4565A with mask = the
/// drinker's bit, effect 2, strength `a`, lasting `a²` ticks).
fn party_armour(g: &mut GameState, idx: usize, a: i16) {
    let ticks = (a as i32 * a as i32) as u16;
    crate::party::party_effect(g, 1 << idx, EFFECT_ARMOUR, a, ticks);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::champions::Champion;
    use dm2_formats::dungeon::Dungeon;

    fn state_with_champion() -> Option<GameState> {
        let dir = crate::assets::default_data_dir();
        let dg = Dungeon::parse(&std::fs::read(dir.join("DUNGEON.DAT")).ok()?).ok()?;
        let mut g = GameState::new_game(&dg);
        let mut c = Champion::default();
        c.set_health(40);
        c.set_max_health(100);
        c.set_stamina(10);
        c.set_max_stamina(200);
        c.set_mana(5);
        c.set_max_mana(50);
        for s in 0..7 {
            c.set_stat_raw(s, 0, 40);
            c.set_stat_raw(s, 1, 50);
        }
        g.champions.push(c);
        Some(g)
    }

    #[test]
    fn stat_gain_shrinks_past_maximum() {
        let mut c = Champion::default();
        c.set_stat_raw(stat::STRENGTH, 0, 50);
        c.set_stat_raw(stat::STRENGTH, 1, 50);
        // 40 over the maximum: one step while the overshoot exceeds 20
        // (40 -> 30), then 20 is no longer above 20.
        raise_stat(&mut c, stat::STRENGTH, 40);
        assert_eq!(c.stat_raw(stat::STRENGTH, 0), 80);
        // 70 over: steps at 70, 50 and 30 (70 -> 53 -> 40 -> 30).
        c.set_stat_raw(stat::STRENGTH, 0, 50);
        raise_stat(&mut c, stat::STRENGTH, 70);
        assert_eq!(c.stat_raw(stat::STRENGTH, 0), 80);
        // Below the maximum the gain is not reduced.
        c.set_stat_raw(stat::STRENGTH, 0, 20);
        raise_stat(&mut c, stat::STRENGTH, 10);
        assert_eq!(c.stat_raw(stat::STRENGTH, 0), 30);
        // Clamped to 220.
        c.set_stat_raw(stat::STRENGTH, 0, 215);
        c.set_stat_raw(stat::STRENGTH, 1, 250);
        raise_stat(&mut c, stat::STRENGTH, 30);
        assert_eq!(c.stat_raw(stat::STRENGTH, 0), 220);
    }

    #[test]
    fn potion_formulas() {
        let Some(mut g) = state_with_champion() else { return };
        // Power 100: s = 12, d = (411 / (12 + 32)) / 2 = 4.
        assert!(drink(&mut g, 0, 11, 100));
        assert_eq!(g.champions[0].stamina(), 10 + 200 / 4);
        assert!(drink(&mut g, 0, 13, 100));
        // 5 + 24 - 8 = 21, under the maximum.
        assert_eq!(g.champions[0].mana(), 21);
        assert!(drink(&mut g, 0, 6, 100));
        assert_eq!(g.champions[0].stat_raw(stat::DEXTERITY, 0), 52);
        assert!(drink(&mut g, 0, 14, 100));
        assert_eq!(g.champions[0].health(), 40 + 100 / 4);
        assert!(drink(&mut g, 0, 15, 100));
        assert_eq!(g.champions[0].water(), 0x640);
        assert!(!drink(&mut g, 0, 3, 100));
    }

    #[test]
    fn mana_overflow_halves_the_excess() {
        let Some(mut g) = state_with_champion() else { return };
        g.champions[0].set_mana(45);
        // 45 + 2*18 - 8 = 73 > 50: 73 - (73 - 50) / 2 = 62.
        assert!(drink(&mut g, 0, 13, 250));
        assert_eq!(g.champions[0].mana(), 62);
    }

    #[test]
    fn cure_potion_cancels_poison_events() {
        let Some(mut g) = state_with_champion() else { return };
        champions::poison(&mut g, 0, 40);
        assert!(g.timeline.iter().any(|(_, e)| e.kind == champions::EVENT_POISON));
        assert!(drink(&mut g, 0, 10, 200));
        assert_eq!(g.champions[0].poison_pool(), 0);
        assert!(!g.timeline.iter().any(|(_, e)| e.kind == champions::EVENT_POISON));
    }

    #[test]
    fn armour_potion_schedules_expiry() {
        let Some(mut g) = state_with_champion() else { return };
        assert!(drink(&mut g, 0, 12, 100));
        // s = 12, a = 18.
        assert_eq!(g.champions[0].raw[0x102], EFFECT_ARMOUR);
        assert_eq!(g.champions[0].shield_value(), 18);
        assert!(g.timeline.iter().any(|(_, e)| e.kind == 0x48 && e.tick == 18 * 18));
    }
}
