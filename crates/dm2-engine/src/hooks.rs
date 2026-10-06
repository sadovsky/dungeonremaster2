//! Champion-side hooks used by the dungeon mechanics.
//!
//! These stand in for the champions module (docs/06): each documents the
//! original routine and formula, keeps the RNG call order where it is
//! known, and reports outcomes as effects. Replace the bodies when the
//! champion records exist.

use crate::effects::Effect;
use crate::state::GameState;

/// Number of living champions (0x7F276 counts recruited champions; the
/// mechanics only ask "is there a party at all").
pub fn champion_count(g: &GameState) -> u16 {
    g.champions.iter().filter(|c| c.is_alive()).count() as u16
}

/// Damage every champion (0x4766B with attack kind 4, mode 2). Returns the
/// mask of champions hurt.
pub fn damage_party(g: &mut GameState, amount: u16) -> u16 {
    if amount == 0 || champion_count(g) == 0 {
        return 0;
    }
    // TODO(docs/06): per-champion defence and wounds.
    let mask = (1u16 << champion_count(g)) - 1;
    g.effects.push(Effect::PartyDamaged { amount, mask });
    mask
}

/// Fall damage after `falls` levels (0x4A34A): each living champion takes
/// `(min(max_hp / 4, 17) + rand4()) * falls`, attack kind 0x30.
pub fn fall_damage(g: &mut GameState, falls: u16) {
    for i in 0..g.champions.len() {
        if !g.champions[i].is_alive() {
            continue;
        }
        let base = (g.champions[i].max_health().max(0) as u16 / 4).min(17);
        let amount = (base + g.rng.rand4()) * falls;
        g.effects.push(Effect::PartyDamaged { amount, mask: 1 << i });
    }
}

/// Bash strength of the two front champions (0x235BF, case "blocked"):
/// for each, `curve(strength_term + (random & 15))`, summed.
pub fn bash_power(g: &mut GameState) -> u16 {
    let mut total = 0;
    for _ in 0..champion_count(g).min(2) {
        let r = (g.rng.rnd() & 15) as u16;
        // TODO(docs/06): 0x466AB / 0x4667A strength terms.
        total += r;
    }
    total
}

/// Does any champion (or the leader's hand) hold item number `kind`?
/// (0x4BD6C; used by floor sensor type 8.)
pub fn party_carries(_g: &GameState, _kind: u16) -> bool {
    false // TODO(docs/09): search the 30 slots and the hand.
}
