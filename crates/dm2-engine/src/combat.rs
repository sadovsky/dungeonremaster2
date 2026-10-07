//! Champion combat: strength, armour, damage to champions, melee and the
//! action executor (docs/07-combat-magic.md).
//!
//! World-side consequences (damaging a creature, launching a missile, an
//! explosion, a party effect) are returned as `Effect`s for the engine to
//! apply, so this module stays independent of creatures and missiles.

use dm2_formats::dungeon::ThingRef;
use dm2_formats::gdat::{Gdat, Key};

use crate::champions::{self, part, skill, stat, Champion, PartyStatus, EMPTY};
use crate::exe_tables::ExeTables;
use crate::items::{self, ItemDb};
use crate::rng::Rng;

/// Attack types (docs/07 "Attack types").
pub mod attack {
    pub const UNBLOCKABLE: u16 = 0;
    pub const FIRE: u16 = 1;
    pub const SELF: u16 = 2;
    pub const BLUNT: u16 = 3;
    pub const SHARP: u16 = 4;
    pub const MAGIC: u16 = 5;
    pub const PSYCHIC: u16 = 6;
    pub const LIGHTNING: u16 = 7;
    /// Set on the attack type when a parry can absorb the blow.
    pub const PARRYABLE: u16 = 0x8000;
}

/// Armour contribution of an item's attribute 0x0B (0x46BAD).
pub fn item_armour(attr: u16, sharp: bool) -> i32 {
    let base = (attr & 0xFF) as i32;
    if sharp {
        (base * (((attr >> 8) & 7) as i32 + 4)) >> 3
    } else {
        base
    }
}

/// Strength behind an action with the item in `hand` (0x46A19).
/// `sk` < 0 skips the skill and weapon bonus.
pub fn strength(c: &Champion, party: &PartyStatus, hand: usize, sk: i32, db: &ItemDb, rng: &mut Rng) -> i16 {
    let r = rng.rnd();
    let mut s = champions::stat_current(c, stat::STRENGTH, rng) as i32 + (r & 15) as i32;
    let item = ThingRef(c.inventory(hand));
    let w = if c.inventory(hand) == EMPTY { 0 } else { db.weight(item) as i32 };
    let lim = (champions::max_load(c, rng) >> 4) as i32;
    s += w - 12;
    if lim < w {
        s -= (w - lim) >> 1;
        let lim2 = lim + ((lim - 12) >> 1);
        if lim2 < w {
            s -= 2 * (w - lim2);
        }
    }
    if sk >= 0 {
        let sk = sk as usize;
        s += 2 * champions::level(c, party, sk, true) as i32;
        let bonus = match sk {
            0 | 4..=7 | 9 => db.attr(item, items::ATTR_MELEE_BONUS) as i16 as i32,
            1 | 10 | 11 => {
                let dmg = db.attr(item, items::ATTR_DAMAGE) as i16 as i32;
                let launcher = db.attr(item, items::ATTR_LAUNCHER) & 0x8000 != 0;
                if dmg != 0 && launcher == (sk == skill::SHOOT) { dmg } else { 0 }
            }
            _ => 0,
        };
        s += bonus;
    }
    let mut s = champions::stamina_adjusted(c, s as i16) as i32;
    if c.wounds() & (1 << hand) != 0 {
        s >>= 1;
    }
    (s >> 1).clamp(0, 100) as i16
}

/// Defence of one body part, 0..100 (0x46BDC).
pub fn armour_value(
    c: &Champion,
    party: &PartyStatus,
    slot: usize,
    sharp: bool,
    db: &ItemDb,
    tables: &ExeTables,
    rng: &mut Rng,
) -> i16 {
    let mut shields = 0i32;
    for hand in 0..2 {
        let item = ThingRef(c.inventory(hand));
        if c.inventory(hand) == EMPTY {
            continue;
        }
        let a = db.attr(item, items::ATTR_ARMOUR);
        if a & 0x8000 != 0 {
            // Hand-held shields: weighted by the body part, half as much
            // when the shield is in the other hand.
            let st = strength(c, party, hand, skill::PARRY as i32, db, rng) as i32;
            let shift = 4 + u32::from(hand != slot);
            shields += ((item_armour(a, sharp) + st) * tables.slot_defence[slot] as i32) >> shift;
        }
    }
    let vit = champions::stat_current(c, stat::VITALITY, rng) as i32;
    let mut v = rng.random(((vit >> 3) + 1) as u16) as i32;
    if sharp {
        v >>= 1;
    }
    if c.shield_kind() == champions::shield::ARMOUR {
        v += c.shield_value() as i32;
    }
    v += c.hand_defence(0) as i32 + c.hand_defence(1) as i32 + shields;
    if slot > 1 && c.inventory(slot) != EMPTY {
        v += item_armour(db.attr(ThingRef(c.inventory(slot)), items::ATTR_ARMOUR), sharp);
    }
    if c.wounds() & (1 << slot) != 0 {
        v -= rng.rand4() as i32 + 8;
    }
    if party.asleep {
        v >>= 1;
    }
    (v >> 1).clamp(0, 100) as i16
}

/// Damage a champion (0x4722A). `parts` selects body parts (bits 0-5);
/// `atype` is an attack type, optionally with `attack::PARRYABLE`.
/// Returns the damage queued (applied by `champions::apply_pending`).
#[allow(clippy::too_many_arguments)]
pub fn damage_champion(
    champions: &mut [Champion],
    party: &mut PartyStatus,
    idx: usize,
    amount: i16,
    parts: u16,
    atype: u16,
    db: &ItemDb,
    tables: &ExeTables,
    rng: &mut Rng,
) -> i16 {
    let Some(c) = champions.get(idx) else { return 0 };
    if party.recruiting == Some(idx) || party.invulnerable || amount < 1 || !c.is_alive() {
        return 0;
    }
    let t = atype & 0x7FFF;
    if t == attack::UNBLOCKABLE {
        party.pending_damage[idx] = party.pending_damage[idx].saturating_add(amount);
        return amount;
    }
    let mut amount = amount as i32;
    let (mut n, mut def) = (0i32, 0i32);
    for b in 0..6 {
        if parts & (1 << b) != 0 {
            n += 1;
            def += armour_value(c, party, b, t == attack::SHARP, db, tables, rng) as i32;
        }
    }
    if n != 0 {
        def /= n;
    }
    let ta: i32 = (0..2).filter(|&h| c.hand_state(h) == 1).map(|h| c.hand_defence(h) as i32).sum();
    if ta != 0 {
        let lv = champions::level(c, party, skill::PARRY, true) as i32;
        if ((rng.rnd() & 15) as i32) < lv + (ta >> 3) {
            if atype & attack::PARRYABLE != 0 {
                amount -= ta;
                if amount < 1 {
                    return 0;
                }
            }
            def += ta >> 2;
        }
    }
    let scaled = match t {
        attack::FIRE => {
            let mut a = champions::stat_adjusted(c, stat::ANTI_FIRE, amount as i16, rng) as i32;
            if c.shield_kind() == champions::shield::FIRE {
                a -= c.shield_value() as i32;
            }
            (a > 0).then(|| (a * (130 - def)) >> 6)
        }
        attack::MAGIC => {
            let mut a = champions::stat_adjusted(c, stat::ANTI_MAGIC, amount as i16, rng) as i32;
            if c.shield_kind() == champions::shield::SPELL {
                a -= c.shield_value() as i32;
            }
            Some(a)
        }
        attack::PSYCHIC => {
            let d = 115 - champions::stat_current(c, stat::WISDOM, rng) as i32;
            if d < 1 {
                return 0;
            }
            Some((amount * d) >> 6)
        }
        attack::SELF | 8 => {
            let def = (def >> 1) + champions::level(c, party, skill::NINJA, true) as i32;
            (amount > 0).then(|| (amount * (130 - def)) >> 6)
        }
        _ => (amount > 0).then(|| (amount * (130 - def)) >> 6),
    };
    let Some(amount) = scaled.filter(|&a| a > 0) else { return 0 };
    // Wounds: the harder the hit relative to vitality, the more body parts.
    let r = rng.rnd();
    let mut v = champions::stat_adjusted(c, stat::VITALITY, ((r & 0x7F) + 10) as i16, rng) as i32;
    if v < amount {
        loop {
            let r = rng.rnd();
            party.pending_wounds[idx] |= (1u16 << (r & 7)) & parts;
            v *= 2;
            if amount <= v || v == 0 {
                break;
            }
        }
    }
    party.asleep = false;
    party.pending_damage[idx] = party.pending_damage[idx].saturating_add(amount as i16);
    amount as i16
}

/// The parts of a creature type's info record (36 bytes, SKULL.EXE 0x71968)
/// that melee reads. See docs/08 for the full layout.
#[derive(Clone, Copy, Debug, Default)]
pub struct CreatureDefence {
    /// +0: flags; 0x20 = non-material.
    pub flags: u8,
    /// +2: armour.
    pub armour: u8,
    /// +8: dexterity; 0xFF means it cannot be hit.
    pub dexterity: u8,
    /// +0x16: bits 8-11 scale the experience a hit earns.
    pub xp_word: u16,
    /// +0x19: 0x10 = takes reduced damage unless attacked with fire.
    pub flags19: u8,
}

impl CreatureDefence {
    pub fn from_info(info: &[u8]) -> CreatureDefence {
        CreatureDefence {
            flags: info[0],
            armour: info[2],
            dexterity: info[8],
            xp_word: u16::from_le_bytes([info[0x16], info[0x17]]),
            flags19: info[0x19],
        }
    }
}

/// One melee blow against a creature, champion side (0x18A57).
pub struct MeleeArgs {
    pub hand: usize,
    /// Hit probability (PB).
    pub probability: u16,
    /// Can hit non-material creatures (HN).
    pub hits_non_material: bool,
    /// Damage factor (DM).
    pub damage: u16,
    /// Skill used and trained (SK).
    pub skill: usize,
    /// Attack type (AT).
    pub attack_type: u16,
    /// The creature is in a state that cannot be struck (0x2FBA9/0x30185).
    pub untouchable: bool,
    /// Bonus damage from the weapon's attribute 0x0D against this creature
    /// (0x31574). TODO: the slayer calculation is not traced yet.
    pub slayer_bonus: i16,
    /// Map experience multiplier.
    pub map_multiplier: u32,
    /// Light term at 0x7F282 (docs/07 open question).
    pub light_term: i16,
    pub tick: u32,
}

/// Resolve a melee blow and return the damage to deal to the creature
/// (0 for a miss). Applies stamina cost and experience to the champion.
pub fn melee(
    champions: &mut [Champion],
    party: &mut PartyStatus,
    idx: usize,
    target: &CreatureDefence,
    a: &MeleeArgs,
    db: &ItemDb,
    rng: &mut Rng,
) -> i16 {
    if idx >= champions.len() || !champions[idx].is_alive() {
        return 0;
    }
    let d2 = 2 * a.map_multiplier as i32;
    let mut damage: i32 = 0;
    let mut hit = target.dexterity != 0xFF && !a.untouchable;
    if hit && target.flags & 0x20 != 0 && !a.hits_non_material {
        hit = false;
    }
    if hit {
        let r = rng.rnd();
        let threshold = (((r & 31) as i32) + target.dexterity as i32 + d2 + 2 * a.light_term as i32 - 16) >> 1;
        hit = champions::dexterity(&champions[idx], party, rng) as i32 > threshold
            || rng.rand4() == 0
            || champions::lucky(&mut champions[idx], 75u16.wrapping_sub(a.probability), rng);
    }
    let stamina_cost;
    if hit {
        let c = &champions[idx];
        let s = strength(c, party, a.hand, a.skill as i32, db, rng) as i32;
        let mut d: i32 = 0;
        let mut weak = s == 0;
        if s != 0 {
            let s = s + rng.random(((s >> 1) + 1) as u16) as i32;
            d = (a.damage as i32 * s) >> 5;
            let defend = (rng.rnd() & 31) as i32 + target.armour as i32 + d2;
            d += (rng.rnd() & 31) as i32;
            d -= defend;
            weak = d <= 1;
        }
        let mut miss = false;
        if weak {
            let r4 = rng.rand4() as i32;
            if r4 == 0 {
                miss = true;
            } else {
                let mut x = r4 + 1;
                d += (rng.rnd() & 15) as i32;
                if d > 0 || rng.bit() != 0 {
                    x += rng.rand4() as i32;
                    if rng.rand4() == 0 {
                        x += ((rng.rnd() & 15) as i32 + d).max(0);
                    }
                }
                d = x;
            }
        }
        if miss {
            stamina_cost = rng.bit() as i32 + 2;
        } else {
            d >>= 1;
            d += rng.random(d as u16) as i32 + rng.rand4() as i32;
            d += rng.random(d as u16) as i32;
            d >>= 2;
            d += rng.rand4() as i32 + 1;
            if ((rng.rnd() & 63) as i32) < champions::level(c, party, a.skill, true) as i32 {
                d += d + 10;
            }
            let weapon = ThingRef(c.inventory(a.hand));
            if c.inventory(a.hand) != EMPTY
                && db.attr(weapon, items::ATTR_SLAYER) != 0
                && ((rng.rnd() & 31) as i32) < d
            {
                d += a.slayer_bonus as i32;
            }
            let xp = ((((target.xp_word >> 8) & 15) as i32 * d) >> 4) + 3;
            let mult = a.map_multiplier;
            champions::add_experience(champions, party, idx, a.skill, xp.max(0) as u32, a.tick, mult, rng);
            stamina_cost = rng.rand4() as i32 + 4;
            damage = d;
        }
    } else {
        stamina_cost = rng.bit() as i32 + 2;
    }
    champions::stamina_loss(champions, party, idx, stamina_cost as i16);
    if target.flags19 & 0x10 != 0 && a.attack_type != attack::FIRE {
        damage >>= rng.bit() as i32 + 1;
    }
    damage.clamp(0, i16::MAX as i32) as i16
}

/// Decoded action string: menu label plus code values by slot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActionSpec {
    pub name: String,
    /// Value per code slot (docs/07 table), 0 when absent.
    pub codes: Vec<i16>,
}

impl ActionSpec {
    /// Parse `NAME:XX1YY-2...` given the code names by slot.
    pub fn parse(text: &str, code_names: &[String]) -> ActionSpec {
        let (name, rest) = text.split_once(':').unwrap_or((text, ""));
        let mut codes = vec![0i16; code_names.len()];
        let b = rest.as_bytes();
        let mut i = 0;
        while i + 2 <= b.len() {
            let code = &rest[i..i + 2];
            i += 2;
            let start = i;
            if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                i += 1;
            }
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let v: i16 = rest[start..i].parse().unwrap_or(0);
            if let Some(slot) = code_names.iter().position(|c| c == code) {
                codes[slot] = v;
            }
        }
        ActionSpec { name: name.to_string(), codes }
    }

    pub fn get(&self, tables: &ExeTables, code: &str) -> i16 {
        tables.code_slot(code).and_then(|s| self.codes.get(s).copied()).unwrap_or(0)
    }
}

/// Fetch and parse action `n` (0-3) of an item category/index, or the bare
/// hand when there is no item (0x3F7FA reads text sub 8 + n).
pub fn action_spec(g: &Gdat, tables: &ExeTables, cat: u8, idx: u8, n: u8) -> Option<ActionSpec> {
    let raw = g.get(Key::new(cat, idx, 5, 8 + n))?;
    let obf = g.lookup(Key::new(0, 0, 11, 0)).unwrap_or(0) & 0x08 != 0;
    let text: Vec<u8> = raw
        .iter()
        .enumerate()
        .map(|(i, &b)| if obf { (!b).wrapping_sub(i as u8) } else { b })
        .take_while(|&c| c != 0)
        .collect();
    Some(ActionSpec::parse(&String::from_utf8_lossy(&text), &tables.action_codes))
}

/// World consequences of an action or spell, applied by the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Damage the creature ahead of the champion.
    DamageCreature { amount: i16, attack_type: u16 },
    /// Launch a missile from the champion's side of the party square.
    /// `what` is a thing or an explosion type 0xFF80+.
    LaunchMissile { champion: usize, what: u16, energy: u8, attack: u8, step: u8 },
    /// Explosion on the party square.
    Explosion { kind: u16, strength: u16 },
    /// Light (+) or darkness (-) change of the given strength (0x412E1).
    Light { kind: u8, strength: u16 },
    /// Timed party effect (0x45815): shields and the like.
    PartyEffect { kind: u8, strength: u16 },
    /// Invisibility for `ticks` (timeline event 0x47).
    Invisibility { ticks: u16 },
    /// Bash the door ahead.
    BashDoor,
    /// A command this executor does not implement yet.
    Unsupported { command: i16 },
}

/// Result of `do_action`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActionResult {
    pub effects: Vec<Effect>,
    pub success: bool,
    /// Ticks the hand is busy (BZ, halved on failure).
    pub busy: u16,
    /// Sound sub-index to play (SD), if any.
    pub sound: Option<i16>,
}

/// Context the executor needs from the engine.
pub struct ActionContext<'a> {
    pub db: &'a ItemDb<'a>,
    pub tables: &'a ExeTables,
    pub tick: u32,
    pub map_multiplier: u32,
    pub light_term: i16,
    /// The creature standing ahead within reach, if any.
    pub target: Option<CreatureDefence>,
    pub target_untouchable: bool,
}

/// Run a chosen action for a champion's hand (0x414A5).
pub fn do_action(
    champions: &mut [Champion],
    party: &mut PartyStatus,
    idx: usize,
    hand: usize,
    spec: &ActionSpec,
    ctx: &ActionContext,
    rng: &mut Rng,
) -> ActionResult {
    let t = ctx.tables;
    let get = |c: &str| spec.get(t, c);
    let (cm, sd, bz, sk, tr, ex, st, at, ta) =
        (get("CM"), get("SD"), get("BZ"), get("SK"), get("TR"), get("EX"), get("ST"), get("AT"), get("TA"));
    let sk = (sk.max(0) as usize).min(19);
    let mut out = ActionResult { sound: (sd != 0).then_some(sd), ..Default::default() };
    if idx >= champions.len() || !champions[idx].is_alive() {
        return out;
    }
    // The stamina cost's random bit is drawn in the executor's prologue
    // (0x4161A), before the command runs and makes its own draws.
    // The hand's action byte (+0x20) takes the command (0x4155A) until its
    // busy time runs out (0x40AA6).
    champions[idx].raw[0x20 + hand.min(1)] = cm as u8;
    let tr_cost = tr as i32 + rng.bit() as i32;
    champions[idx].set_hand_defence(hand, ta as i8);
    let mut success = true;
    let mut xp = ex.max(0) as u32;
    match cm {
        2 => out.effects.push(Effect::Invisibility { ticks: st.max(32) as u16 }),
        3 => {
            // Cast the item's projectile: mana cost 7 - min(6, level).
            let lv = champions::level(&champions[idx], party, sk, true) as i32;
            let cost = 7 - lv.min(6);
            let c = &mut champions[idx];
            let mut energy = st as i32;
            if (c.mana() as i32) < cost {
                energy = energy * c.mana() as i32 / cost.max(1);
                c.set_mana(0);
            } else {
                c.set_mana(c.mana() - cost as i16);
            }
            if energy <= 0 {
                success = false;
                xp >>= 1;
            } else {
                let pa = get("PA");
                out.effects.push(projectile(champions, idx, 0xFF80u16.wrapping_add(pa as u16), energy.min(255) as u8));
            }
        }
        4 | 8 => match ctx.target {
            Some(target) => {
                let args = MeleeArgs {
                    hand,
                    probability: get("PB").max(0) as u16,
                    hits_non_material: get("HN") != 0,
                    damage: get("DM").max(0) as u16,
                    skill: sk,
                    attack_type: at as u16,
                    untouchable: ctx.target_untouchable,
                    slayer_bonus: 0,
                    map_multiplier: ctx.map_multiplier,
                    light_term: ctx.light_term,
                    tick: ctx.tick,
                };
                let d = melee(champions, party, idx, &target, &args, ctx.db, rng);
                success = d > 0;
                // Melee grants its own experience; EX applies on top only on a hit.
                if !success {
                    xp >>= 1;
                }
                out.effects.push(Effect::DamageCreature { amount: d, attack_type: at as u16 });
            }
            None if cm == 8 => out.effects.push(Effect::BashDoor),
            None => {
                success = false;
                xp >>= 1;
            }
        },
        6 | 0x26 | 0x27 => out.effects.push(Effect::Light { kind: cm as u8, strength: st.max(0) as u16 }),
        7 => out.effects.push(Effect::Explosion { kind: 0xFF8E, strength: st.max(2) as u16 }),
        9 => party.haste = (party.haste as i32 + 32 * st as i32).clamp(0, 255) as u8,
        0x0B => party.counter_0b = (party.counter_0b as i32 + st as i32).clamp(0, 200) as u8,
        0x0C..=0x0F => out.effects.push(Effect::PartyEffect {
            kind: (cm - 0x0C) as u8,
            strength: (4 * st.max(32) as i32) as u16,
        }),
        0x21..=0x23 => {
            let c = &mut champions[idx];
            let mut strength = 3 * st.max(32) as i32;
            if c.mana() < 4 {
                strength >>= 1;
                c.set_mana(0);
            } else {
                c.set_mana(c.mana() - 4);
            }
            out.effects.push(Effect::PartyEffect { kind: (cm - 0x21) as u8, strength: strength as u16 });
        }
        0x20 => {
            // Shoot (0x414A5 case 0x20): the launcher in this hand fires the
            // ammunition from the other hand. A launcher has attribute 5 bit
            // 15 set; the ammunition has it clear and shares a class bit
            // (0x408A8). With L the shoot level: energy = L + both items'
            // attribute 9, attack = launcher attribute 0x0A + 2L, step =
            // the ammunition's attribute 0x0C.
            let launcher = champions[idx].inventory(hand);
            let other = 1 - hand.min(1);
            let ammo = champions[idx].inventory(other);
            let class = |t: u16| ctx.db.attr(ThingRef(t), crate::items::ATTR_LAUNCHER);
            let fits = launcher != EMPTY && ammo != EMPTY && class(launcher) & 0x8000 != 0 && class(ammo) & 0x8000 == 0 && class(ammo) & class(launcher) & 0x7FFF != 0;
            if fits {
                let l = champions::level(&champions[idx], party, skill::SHOOT, true) as i32;
                champions[idx].set_inventory(other, EMPTY);
                let a9 = |t: u16| ctx.db.attr(ThingRef(t), crate::items::ATTR_DAMAGE) as i32;
                let energy = (l + a9(launcher) + a9(ammo)).clamp(0, 255) as u8;
                let attack = (ctx.db.attr(ThingRef(launcher), 0x0A) as i32 + 2 * l).clamp(0, 255) as u8;
                let step = ctx.db.attr(ThingRef(ammo), 0x0C) as u8;
                out.effects.push(Effect::LaunchMissile { champion: idx, what: ammo, energy, attack, step });
            } else {
                success = false;
                xp >>= 1;
            }
        }
        0x24 => {
            // Heal: 2 mana per step of min(10, heal level).
            let lv = champions::level(&champions[idx], party, skill::HEAL, true) as i16;
            let c = &mut champions[idx];
            let step = lv.min(10);
            let mut healed = false;
            while c.health() < c.max_health() && c.mana() >= 2 {
                c.set_mana(c.mana() - 2);
                c.set_health((c.health() + step).min(c.max_health()));
                healed = true;
            }
            success = healed;
        }
        _ => out.effects.push(Effect::Unsupported { command: cm }),
    }
    out.success = success;
    out.busy = if success { bz.max(0) as u16 } else { (bz.max(0) as u16) >> 1 };
    if champions[idx].is_alive() {
        champions::stamina_loss(champions, party, idx, tr_cost as i16);
        if xp > 0 {
            let mult = ctx.map_multiplier;
            champions::add_experience(champions, party, idx, sk, xp, ctx.tick, mult, rng);
        }
    }
    out
}

/// A missile launched by a champion (0x47773): energies capped at 255.
pub fn projectile(champions: &[Champion], idx: usize, what: u16, energy: u8) -> Effect {
    let _ = champions;
    Effect::LaunchMissile { champion: idx, what, energy, attack: energy, step: (energy / 4).max(1) }
}

/// Body-part mask for a hit to the head and torso (missiles).
pub const HEAD_TORSO: u16 = part::HEAD | part::TORSO;

#[cfg(test)]
mod tests {
    use super::*;

    fn codes() -> Vec<String> {
        ["SK", "LV", "CM", "BZ", "TR", "ST", "PA", "TA", "NC", "EX", "PB", "DM", "MS", "SD", "RP", "HN", "AT", "WH"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn parses_action_strings() {
        let a = ActionSpec::parse("CHOP:CM4SK4BZ6TR3EX5PB40DM20TA-2", &codes());
        assert_eq!(a.name, "CHOP");
        assert_eq!(a.codes[2], 4);
        assert_eq!(a.codes[0], 4);
        assert_eq!(a.codes[10], 40);
        assert_eq!(a.codes[11], 20);
        assert_eq!(a.codes[7], -2);
        assert_eq!(a.codes[16], 0);
    }

    #[test]
    fn item_armour_sharp_scaling() {
        assert_eq!(item_armour(0x0320, false), 0x20);
        // sharp: 32 * (3 + 4) / 8 = 28
        assert_eq!(item_armour(0x0320, true), 28);
    }

    fn with_db(f: impl FnOnce(&ItemDb, &ExeTables)) {
        let Ok(g) = Gdat::open(dm2_formats::gdat::default_path()) else { return };
        let Ok(t) = ExeTables::load(&crate::exe_tables::default_exe_path()) else { return };
        let path = dm2_formats::gdat::default_path().with_file_name("DUNGEON.DAT");
        let Ok(raw) = std::fs::read(path) else { return };
        let dg = dm2_formats::dungeon::Dungeon::parse(&raw).unwrap();
        let db = ItemDb { gdat: &g, dungeon: &dg, categories: &t.thing_category };
        f(&db, &t);
    }

    fn fighter() -> Champion {
        let mut c = Champion::default();
        c.set_health(80);
        c.set_max_health(80);
        c.set_stamina(600);
        c.set_max_stamina(600);
        for s in 0..7 {
            c.set_stat_raw(s, 0, 50);
            c.set_stat_raw(s, 1, 50);
        }
        for slot in 0..crate::champions::INVENTORY_SLOTS {
            c.set_inventory(slot, EMPTY);
        }
        c
    }

    fn blow() -> MeleeArgs {
        MeleeArgs {
            hand: 1,
            probability: 40,
            hits_non_material: false,
            damage: 20,
            skill: 4,
            attack_type: attack::SHARP,
            untouchable: false,
            slayer_bonus: 0,
            map_multiplier: 0,
            light_term: 0,
            tick: 100,
        }
    }

    #[test]
    fn melee_misses_cost_one_random_number() {
        with_db(|db, _| {
            for target in [
                CreatureDefence { dexterity: 0xFF, ..Default::default() },
                CreatureDefence { flags: 0x20, dexterity: 10, ..Default::default() },
            ] {
                let mut champs = vec![fighter()];
                let mut party = PartyStatus::default();
                let mut rng = Rng::new(3);
                let mut e = Rng::new(3);
                let d = melee(&mut champs, &mut party, 0, &target, &blow(), db, &mut rng);
                assert_eq!(d, 0);
                let cost = e.bit() as i16 + 2;
                assert_eq!(rng.state, e.state);
                assert_eq!(champs[0].stamina(), 600 - cost);
            }
        });
    }

    #[test]
    fn melee_hits_hurt_and_train() {
        with_db(|db, _| {
            let target = CreatureDefence { dexterity: 0, armour: 0, xp_word: 0x0400, ..Default::default() };
            let mut total = 0;
            for seed in 0..50 {
                let mut champs = vec![fighter()];
                let mut party = PartyStatus::default();
                let mut rng = Rng::new(seed);
                let d = melee(&mut champs, &mut party, 0, &target, &blow(), db, &mut rng);
                total += d as i32;
                if d > 0 {
                    assert!(champs[0].experience(4) > 0);
                    assert!(champs[0].stamina() < 600);
                }
            }
            assert!(total > 0, "a defenceless target is hit sometimes");
        });
    }

    #[test]
    fn damage_types() {
        with_db(|db, t| {
            let mut champs = vec![fighter()];
            let mut party = PartyStatus::default();
            let mut rng = Rng::new(9);
            assert_eq!(damage_champion(&mut champs, &mut party, 0, 7, 0, attack::UNBLOCKABLE, db, t, &mut rng), 7);
            assert_eq!(party.pending_damage[0], 7);
            assert_eq!(rng.state, 9, "unblockable damage draws no random numbers");
            // psychic damage is ignored at wisdom >= 115
            champs[0].set_stat_raw(stat::WISDOM, 0, 200);
            assert_eq!(damage_champion(&mut champs, &mut party, 0, 50, 0, attack::PSYCHIC, db, t, &mut rng), 0);
            party.invulnerable = true;
            assert_eq!(damage_champion(&mut champs, &mut party, 0, 50, 0x3F, attack::BLUNT, db, t, &mut rng), 0);
            party.invulnerable = false;
            let hit = damage_champion(&mut champs, &mut party, 0, 50, 0x3F, attack::BLUNT, db, t, &mut rng);
            assert!(hit > 0 && hit <= 102, "armour scales 50 by (130 - defence) / 64");
        });
    }
}
