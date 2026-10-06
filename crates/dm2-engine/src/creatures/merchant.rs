//! Merchant pricing and the `I`, `J`, `K` opcodes (docs/08 "Merchants").
//!
//! Pure functions over pile values so the rules can be tested without a
//! dungeon. The caller sums the counter quadrants: goods value, coin value
//! and gem value per side.

use crate::rng::Rng;

/// Merchant actions (tentative meanings, docs/08).
pub mod action {
    pub const REFUSE: u8 = 0x1B;
    pub const ACCEPT: u8 = 0x1C;
    pub const WAIT: u8 = 0x1D;
    pub const PROMPT: u8 = 0x1E;
    pub const REJECT_ITEM: u8 = 0x1F;
    pub const GRUDGING: u8 = 0x20;
}

/// Most coins a price may need (0x15958).
pub const MAX_COINS: u32 = 18;

/// Round `value` down to what at most 18 coins can pay, choosing greedily
/// from `denominations` (largest first) (0x15958).
pub fn price(value: u32, denominations: &[u32]) -> u32 {
    let mut d: Vec<u32> = denominations.iter().copied().filter(|&v| v > 0).collect();
    d.sort_unstable_by(|a, b| b.cmp(a));
    let (mut left, mut total, mut coins) = (value, 0, 0);
    for v in d {
        while left >= v && coins < MAX_COINS {
            left -= v;
            total += v;
            coins += 1;
        }
    }
    total
}

/// The merchant's per-creature counters (record program variables and slot
/// words +0x0C/+0x0E/+0x10 in the original).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    /// +0x0C: change still owed / reset marker.
    pub owed: u32,
    /// +0x0E: countdown, or the agreed price after acceptance.
    pub countdown: u32,
    /// +0x10: the previous offer.
    pub last_offer: u32,
}

/// Outcome of a merchant step: the action to play and the interpreter result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub action: Option<u8>,
    /// true = "done" (−2); false = stays in progress or fails.
    pub done: bool,
    pub failed: bool,
}

const fn play(a: u8) -> Outcome {
    Outcome { action: Some(a), done: false, failed: false }
}

/// `I`: waiting for a customer. `stray_goods` = non-money items in the near
/// quadrant; `customer_has_money` = the creature or party ahead holds coins
/// or gems.
pub fn wait(c: &mut Counters, stray_goods: bool, customer_has_money: bool, rng: &mut Rng) -> Outcome {
    if stray_goods {
        c.countdown = c.countdown.saturating_sub(1);
        if c.countdown <= 6 {
            c.countdown = 9 + rng.rand4() as u32;
            return play(action::REJECT_ITEM);
        }
        return play(action::WAIT);
    }
    if customer_has_money {
        return Outcome { action: None, done: true, failed: false };
    }
    if c.countdown == 0 {
        c.countdown = 5;
        return play(action::PROMPT);
    }
    c.countdown -= 1;
    play(action::WAIT)
}

/// The give-in decision used when patience runs out (0x28711).
fn give_in(ratio: u32, rng: &mut Rng) -> u8 {
    let r = rng.random((100u32.saturating_sub(ratio)).max(1) as u16);
    if r < 5 && rng.rand4() != 0 {
        action::GRUDGING
    } else {
        action::REFUSE
    }
}

/// `J`: haggling. `goods_value` is the raw value of the goods side;
/// `offer` the coins plus gems on the money side.
pub fn haggle(
    c: &mut Counters,
    stray_goods: bool,
    goods_value: u32,
    offer: u32,
    denominations: &[u32],
    rng: &mut Rng,
) -> Outcome {
    if stray_goods {
        return Outcome { action: None, done: false, failed: true };
    }
    if offer == 0 {
        c.last_offer = 0;
        return Outcome { action: None, done: true, failed: false };
    }
    let full = price(goods_value, denominations).max(1);
    let mut p = full;
    if p > 16 {
        p -= rng.random(16) as u32 * p / 100;
    }
    let p = p.max(1);
    let ratio = offer * 100 / p;
    let previous = c.last_offer;
    c.last_offer = offer;
    if offer >= p {
        c.countdown = offer.min(full);
        return Outcome { action: Some(action::ACCEPT), done: true, failed: false };
    }
    if offer == previous {
        if c.countdown == 0 {
            if ratio > 76 {
                return play(give_in(ratio, rng));
            }
            return play(action::REFUSE);
        }
        c.countdown -= 1;
        return play(action::WAIT);
    }
    if rng.rand4() != 0 && ratio <= 76 + (rng.rnd() & 7) {
        return play(action::WAIT);
    }
    c.owed = 0;
    play(give_in(ratio, rng))
}

/// `K`: settling. Returns the change still due in `c.owed`.
pub fn settle(
    c: &mut Counters,
    goods_in_money_side: bool,
    goods_value: u32,
    gems_on_goods_side: u32,
    paid: u32,
    denominations: &[u32],
) -> Outcome {
    if goods_in_money_side {
        return Outcome { action: Some(action::REJECT_ITEM), done: false, failed: true };
    }
    let p = price(goods_value, denominations) + gems_on_goods_side;
    if paid < gems_on_goods_side + c.owed {
        return play(action::REFUSE);
    }
    let mut a = None;
    if paid != c.countdown {
        a = Some(action::ACCEPT);
        c.last_offer = paid;
    }
    c.owed = paid.saturating_sub(p);
    Outcome { action: a, done: true, failed: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COINS: [u32; 3] = [1, 10, 100];

    #[test]
    fn price_is_limited_to_eighteen_coins() {
        assert_eq!(price(0, &COINS), 0);
        assert_eq!(price(345, &COINS), 345); // 3+4+5 = 12 coins
        // 999 would need 27 coins: greedily 9×100 then 9×10 = 990.
        assert_eq!(price(999, &COINS), 990);
        assert_eq!(price(5, &[]), 0);
    }

    #[test]
    fn haggle_accepts_full_offers_and_waits_on_low_ones() {
        let mut rng = Rng::new(1);
        let mut c = Counters { countdown: 3, ..Default::default() };
        let o = haggle(&mut c, false, 100, 120, &COINS, &mut rng);
        assert_eq!(o.action, Some(action::ACCEPT));
        assert!(o.done);
        assert_eq!(c.countdown, 100, "agreed price is capped at the undiscounted price");
        let mut c = Counters { countdown: 2, ..Default::default() };
        let o = haggle(&mut c, false, 100, 10, &COINS, &mut rng);
        assert!(!o.done && o.action.is_some());
        // An unchanged offer counts the patience down.
        let before = c.countdown;
        let o = haggle(&mut c, false, 100, 10, &COINS, &mut rng);
        assert_eq!(o.action, Some(action::WAIT));
        assert_eq!(c.countdown, before - 1);
    }

    #[test]
    fn settle_tracks_change() {
        let mut c = Counters::default();
        let o = settle(&mut c, false, 50, 0, 70, &COINS);
        assert!(o.done);
        assert_eq!(c.owed, 20);
        let mut c = Counters { owed: 30, ..Default::default() };
        assert_eq!(settle(&mut c, false, 50, 0, 10, &COINS).action, Some(action::REFUSE));
        assert!(settle(&mut c, true, 50, 0, 70, &COINS).failed);
    }

    #[test]
    fn waiting_merchant_prompts_and_rejects() {
        let mut rng = Rng::new(9);
        let mut c = Counters::default();
        assert_eq!(wait(&mut c, false, false, &mut rng).action, Some(action::PROMPT));
        assert_eq!(c.countdown, 5);
        assert!(wait(&mut c, false, true, &mut rng).done);
        let mut c = Counters { countdown: 7, ..Default::default() };
        assert_eq!(wait(&mut c, true, false, &mut rng).action, Some(action::REJECT_ITEM));
        assert!((9..13).contains(&c.countdown));
    }
}
