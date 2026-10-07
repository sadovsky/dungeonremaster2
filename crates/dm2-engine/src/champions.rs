//! Champions: record, skills, experience, regeneration (docs/06-champions.md).
//!
//! A champion is kept as its original 0x107-byte record so save games can
//! store it unchanged; typed accessors cover the documented fields. Every
//! random call goes through the shared `Rng` in the order the original
//! makes it, so fixed-seed runs reproduce the original's numbers.

use dm2_formats::gdat::{Gdat, Key};

use crate::rng::Rng;
use crate::state::GameState;
use crate::timeline::Event;

pub const RECORD_SIZE: usize = 0x107;
pub const MAX_CHAMPIONS: usize = 4;
pub const INVENTORY_SLOTS: usize = 30;
pub const EMPTY: u16 = 0xFFFF;
/// Timeline event type that delivers the next dose of poison.
pub const EVENT_POISON: u8 = 0x4B;

/// Stat numbers (index into the seven stat pairs at +0x4A).
pub mod stat {
    pub const LUCK: usize = 0;
    pub const STRENGTH: usize = 1;
    pub const DEXTERITY: usize = 2;
    pub const WISDOM: usize = 3;
    pub const VITALITY: usize = 4;
    pub const ANTI_MAGIC: usize = 5;
    pub const ANTI_FIRE: usize = 6;
}

/// Skill numbers: 0-3 classes, 4-19 sub-skills (class = (skill - 4) / 4).
pub mod skill {
    pub const FIGHTER: usize = 0;
    pub const NINJA: usize = 1;
    pub const PRIEST: usize = 2;
    pub const WIZARD: usize = 3;
    pub const PARRY: usize = 7;
    pub const STEAL: usize = 8;
    pub const THROW: usize = 10;
    pub const SHOOT: usize = 11;
    pub const HEAL: usize = 13;
    pub const INFLUENCE: usize = 14;
}

/// Wound and body-part bits (+0x34, and the damage body-part mask).
pub mod part {
    pub const READY_HAND: u16 = 1;
    pub const ACTION_HAND: u16 = 2;
    pub const HEAD: u16 = 4;
    pub const TORSO: u16 = 8;
    pub const LEGS: u16 = 0x10;
    pub const FEET: u16 = 0x20;
}

/// Kind byte (+0x102) of the value at +0x103: personal shields and stat boosts.
pub mod shield {
    pub const FIRE: u8 = 0;
    pub const SPELL: u8 = 1;
    pub const ARMOUR: u8 = 2;
    /// Kinds 3-6 boost stats 1-4 (strength, dexterity, wisdom, vitality).
    pub const FIRST_BOOST: u8 = 3;
}

#[derive(Clone)]
pub struct Champion {
    pub raw: [u8; RECORD_SIZE],
}

impl Default for Champion {
    /// A blank record. Inventory slots hold EMPTY, not 0: thing reference 0
    /// is a real thing, and dropping it on death would corrupt a list.
    fn default() -> Self {
        let mut c = Champion { raw: [0; RECORD_SIZE] };
        for slot in 0..INVENTORY_SLOTS {
            c.set_inventory(slot, EMPTY);
        }
        c
    }
}

impl std::fmt::Debug for Champion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Champion")
            .field("name", &self.name())
            .field("health", &(self.health(), self.max_health()))
            .field("stamina", &(self.stamina(), self.max_stamina()))
            .field("mana", &(self.mana(), self.max_mana()))
            .finish()
    }
}

macro_rules! word_field {
    ($get:ident, $set:ident, $off:expr) => {
        pub fn $get(&self) -> i16 {
            self.i16_at($off)
        }
        pub fn $set(&mut self, v: i16) {
            self.set_i16($off, v)
        }
    };
}

impl Champion {
    pub fn from_bytes(b: &[u8]) -> Option<Champion> {
        let raw: [u8; RECORD_SIZE] = b.get(..RECORD_SIZE)?.try_into().ok()?;
        Some(Champion { raw })
    }

    pub fn to_bytes(&self) -> [u8; RECORD_SIZE] {
        self.raw
    }

    pub fn u16_at(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.raw[o], self.raw[o + 1]])
    }

    pub fn i16_at(&self, o: usize) -> i16 {
        self.u16_at(o) as i16
    }

    pub fn set_u16(&mut self, o: usize, v: u16) {
        self.raw[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }

    pub fn set_i16(&mut self, o: usize, v: i16) {
        self.set_u16(o, v as u16);
    }

    fn text(&self, o: usize, n: usize) -> String {
        let s = &self.raw[o..o + n];
        let end = s.iter().position(|&c| c == 0).unwrap_or(n);
        String::from_utf8_lossy(&s[..end]).into_owned()
    }

    pub fn name(&self) -> String {
        self.text(0, 8)
    }

    pub fn title(&self) -> String {
        self.text(8, 20)
    }

    pub fn facing(&self) -> u8 {
        self.raw[0x1C]
    }

    pub fn cell(&self) -> u8 {
        self.raw[0x1D]
    }

    word_field!(health, set_health, 0x36);
    word_field!(max_health, set_max_health, 0x38);
    word_field!(stamina, set_stamina, 0x3A);
    word_field!(max_stamina, set_max_stamina, 0x3C);
    word_field!(mana, set_mana, 0x3E);
    word_field!(max_mana, set_max_mana, 0x40);
    word_field!(food, set_food, 0x44);
    word_field!(water, set_water, 0x46);
    word_field!(poison_pool, set_poison_pool, 0x48);
    word_field!(shield_value, set_shield_value, 0x103);

    pub fn is_alive(&self) -> bool {
        self.health() != 0
    }

    pub fn wounds(&self) -> u16 {
        self.u16_at(0x34)
    }

    pub fn set_wounds(&mut self, v: u16) {
        self.set_u16(0x34, v)
    }

    /// Raw stat byte: `which` 0 = current, 1 = maximum.
    pub fn stat_raw(&self, s: usize, which: usize) -> u8 {
        self.raw[0x4A + 2 * s + which]
    }

    pub fn set_stat_raw(&mut self, s: usize, which: usize, v: u8) {
        self.raw[0x4A + 2 * s + which] = v;
    }

    /// Signed temporary modifier for stat `s` (+0x58).
    pub fn stat_modifier(&self, s: usize) -> i8 {
        self.raw[0x58 + s] as i8
    }

    pub fn experience(&self, sk: usize) -> u32 {
        let o = 0x5F + 4 * sk;
        u32::from_le_bytes([self.raw[o], self.raw[o + 1], self.raw[o + 2], self.raw[o + 3]])
    }

    pub fn set_experience(&mut self, sk: usize, v: u32) {
        let o = 0x5F + 4 * sk;
        self.raw[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// Signed temporary level modifier for skill `sk` (+0xAF).
    pub fn skill_modifier(&self, sk: usize) -> i8 {
        self.raw[0xAF + sk] as i8
    }

    pub fn inventory(&self, slot: usize) -> u16 {
        self.u16_at(0xC3 + 2 * slot)
    }

    pub fn set_inventory(&mut self, slot: usize, t: u16) {
        self.set_u16(0xC3 + 2 * slot, t)
    }

    /// Cached load in tenths of a kilogram (+0xFF), kept up to date by
    /// whoever changes the inventory (see `recompute_load`).
    pub fn load(&self) -> u16 {
        self.u16_at(0xFF)
    }

    pub fn set_load(&mut self, v: u16) {
        self.set_u16(0xFF, v)
    }

    pub fn portrait(&self) -> u8 {
        self.raw[0x101]
    }

    pub fn shield_kind(&self) -> u8 {
        self.raw[0x102]
    }

    /// Signed movement-speed modifier (+0x105).
    pub fn speed_modifier(&self) -> i8 {
        self.raw[0x105] as i8
    }

    /// Per-hand action state (+0x20/+0x21): 1 while the hand's defensive
    /// bonus is active, 0xFF idle.
    pub fn hand_state(&self, hand: usize) -> u8 {
        self.raw[0x20 + hand]
    }

    /// Signed defence bonus of a hand's current action (+0x42/+0x43).
    pub fn hand_defence(&self, hand: usize) -> i8 {
        self.raw[0x42 + hand] as i8
    }

    pub fn set_hand_defence(&mut self, hand: usize, v: i8) {
        self.raw[0x42 + hand] = v as u8;
    }

    pub fn poison_count(&self) -> u8 {
        self.raw[0x1F]
    }

    /// Mark parts of the champion's panel for redraw (+0x32).
    pub fn flag_redraw(&mut self, bits: u16) {
        let v = self.u16_at(0x32) | bits;
        self.set_u16(0x32, v);
    }
}

/// Party-wide state that the champion formulas read (docs/06 "Party globals").
#[derive(Clone, Debug, Default)]
pub struct PartyStatus {
    pub asleep: bool,
    /// Champions take no damage (0x7F23C).
    pub invulnerable: bool,
    /// Champion currently being recruited (0x7F284), excluded from updates.
    pub recruiting: Option<usize>,
    /// Tick a creature last attacked the party (0x716A0).
    pub last_attacked: u32,
    /// Tick the party formed (0x7F19C): set by the recruit routine (0x49A17)
    /// when the first champion joins, and not touched by moving. Upkeep
    /// regenerates faster once 0x50 and again 0xFA ticks have passed since.
    pub party_formed: u32,
    /// Shared regeneration threshold (0x7FFF4).
    pub regen_counter: u16,
    /// Damage and wounds waiting for the screen update (0x7FBAC, 0x7FBA4).
    pub pending_damage: [i16; MAX_CHAMPIONS],
    pub pending_wounds: [u16; MAX_CHAMPIONS],
    /// While non-zero every champion's move time is 1 (0x7FFF0).
    pub haste: u8,
    /// Second party counter raised by action 0x0B (0x7FFEF); meaning open.
    pub counter_0b: u8,
    /// Per-champion, per-class level-up counters (0x7FFF8).
    pub level_ups: [[u8; 4]; MAX_CHAMPIONS],
    /// Notices for the interface (level-ups, deaths).
    pub notices: Vec<Notice>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// Champion gained a level in class (message text (1, 0, 6 + class)).
    LevelUp { champion: usize, class: usize },
    Died { champion: usize },
}

/// Skill level (0x46241).
pub fn level(c: &Champion, party: &PartyStatus, sk: usize, with_modifiers: bool) -> u16 {
    if party.asleep {
        return 1;
    }
    let mut xp = c.experience(sk) as u64;
    if sk >= 4 {
        let class = (sk - 4) / 4;
        let k: i64 = if with_modifiers { c.skill_modifier(class) as i64 + 1 } else { 1 };
        xp = ((xp as i64 + k * c.experience(class) as i64).max(0) as u64) >> 1;
    }
    let mut lv: i32 = 1;
    while xp > 511 {
        xp >>= 1;
        lv += 1;
    }
    if with_modifiers {
        lv = (lv + c.skill_modifier(sk) as i32).max(1);
    }
    lv as u16
}

/// Current value of a stat including boosts and modifiers (0x466AB with
/// which = 0), clamped to 10..220. A stat-boost "shield" (kinds 3-6) adds a
/// random amount, consuming one random number.
pub fn stat_current(c: &Champion, s: usize, rng: &mut Rng) -> i16 {
    let base = c.stat_raw(s, 0) as i32;
    let mut v = base;
    let sv = c.shield_value();
    let kind = c.shield_kind();
    if sv != 0 && kind >= shield::FIRST_BOOST && kind < 7 && s == (kind - 2) as usize {
        let sv = sv.min(100) as i32;
        v = base + rng.random(((sv * base) >> 7) as u16 + 1) as i32 + 4;
    }
    (v + c.stat_modifier(s) as i32).clamp(10, 220) as i16
}

/// Maximum value of a stat plus its modifier, clamped (0x466AB, which = 1).
pub fn stat_max(c: &Champion, s: usize) -> i16 {
    (c.stat_raw(s, 1) as i32 + c.stat_modifier(s) as i32).clamp(10, 220) as i16
}

/// Scale a value down by a resistance stat (0x46745).
pub fn stat_adjusted(c: &Champion, s: usize, value: i16, rng: &mut Rng) -> i16 {
    let d = 170 - stat_current(c, s, rng) as i32;
    if d < 16 {
        (value as i32 >> 3) as i16
    } else {
        ((value as i32 * d) >> 7) as i16
    }
}

/// Reduce a value when the champion is below half stamina (0x4667A).
pub fn stamina_adjusted(c: &Champion, value: i16) -> i16 {
    let half = c.max_stamina() as i32 >> 1;
    let st = c.stamina() as i32;
    if st < half {
        let v = value as i32 >> 1;
        (st * v / half + v) as i16
    } else {
        value
    }
}

/// Maximum load in tenths of a kilogram, rounded up to a whole kilogram (0x46824).
pub fn max_load(c: &Champion, rng: &mut Rng) -> u16 {
    let s = stat_current(c, stat::STRENGTH, rng);
    let mut v = stamina_adjusted(c, s * 8 + 100) as u16;
    if c.wounds() != 0 {
        let shift = if c.wounds() & part::LEGS != 0 { 2 } else { 3 };
        v = v.wrapping_sub(v >> shift);
    }
    let r = v.wrapping_add(9);
    r - r % 10
}

/// Recompute the cached load from the inventory.
pub fn recompute_load(c: &mut Champion, items: &crate::items::ItemDb) {
    let mut total: u16 = 0;
    for slot in 0..INVENTORY_SLOTS {
        let t = c.inventory(slot);
        if t != EMPTY {
            total = total.wrapping_add(items.weight(dm2_formats::dungeon::ThingRef(t)));
        }
    }
    c.set_load(total);
}

/// Ticks a champion needs per step (0x46892).
pub fn move_time(c: &Champion, party: &PartyStatus, rng: &mut Rng) -> u16 {
    if party.haste != 0 {
        return 1;
    }
    let max = max_load(c, rng) as i32;
    let load = c.load() as i32;
    let (mut t, inc) = if load < max {
        (if max * 5 < load * 8 { 3 } else { 2 }, 1)
    } else {
        ((load - max) * 4 / max.max(1) + 4, 2)
    };
    if c.wounds() & part::FEET != 0 {
        t += inc;
    }
    let mut t = (t - c.speed_modifier() as i32).max(1);
    if t > 2 {
        t = (t + 1) & !1;
    }
    t as u16
}

/// The party's move cooldown: half the slowest living champion's move time
/// (docs/05 "Cooldown"). At least 1 tick.
pub fn party_move_time(champions: &[Champion], party: &PartyStatus, rng: &mut Rng) -> u16 {
    let slowest = champions.iter().filter(|c| c.is_alive()).map(|c| move_time(c, party, rng)).max().unwrap_or(2);
    (slowest / 2).max(1)
}

/// Luck roll (0x46786): true on success. Adjusts current luck.
pub fn lucky(c: &mut Champion, threshold: u16, rng: &mut Rng) -> bool {
    if rng.bit() != 0 && rng.random(100) > threshold {
        return true;
    }
    let luck = stat_current(c, stat::LUCK, rng);
    let r = rng.random((luck * 2) as u16);
    let cap = stat_max(c, stat::LUCK).min(220) as i32;
    let ok = r > threshold;
    let cur = c.stat_raw(stat::LUCK, 0) as i32 + if ok { -2 } else { 2 };
    c.set_stat_raw(stat::LUCK, 0, cur.clamp(10, cap.max(10)) as u8);
    ok
}

/// Effective dexterity (0x46968).
pub fn dexterity(c: &Champion, party: &PartyStatus, rng: &mut Rng) -> i16 {
    let r = rng.rnd();
    let mut d = ((r & 7) as i32 + stat_current(c, stat::DEXTERITY, rng) as i32) >> 1;
    let load = c.load() as i32;
    let max = (max_load(c, rng) as i32).max(1);
    d = (d - load * d / max).max(2);
    if party.asleep {
        d >>= 1;
    }
    let hi = 100 - (rng.rnd() & 7) as i32;
    let lo = (rng.rnd() & 7) as i32 + 1;
    d.clamp(lo, hi.max(lo)) as i16
}

/// Add pending, unblockable damage (attack type 0).
pub fn add_pending_damage(champions: &[Champion], party: &mut PartyStatus, idx: usize, amount: i16) -> i16 {
    let Some(c) = champions.get(idx) else { return 0 };
    if amount < 1 || !c.is_alive() || party.recruiting == Some(idx) || party.invulnerable {
        return 0;
    }
    party.pending_damage[idx] = party.pending_damage[idx].saturating_add(amount);
    amount
}

/// Spend stamina (0x47707). Overflow below zero becomes half as much
/// unblockable damage; negative amounts restore stamina up to the maximum.
pub fn stamina_loss(champions: &mut [Champion], party: &mut PartyStatus, idx: usize, amount: i16) {
    let Some(c) = champions.get_mut(idx) else { return };
    let st = c.stamina() as i32 - amount as i32;
    if st < 1 {
        c.set_stamina(0);
        add_pending_damage(champions, party, idx, ((-st) >> 1) as i16);
    } else {
        c.set_stamina(st.min(c.max_stamina() as i32) as i16);
    }
    if amount.unsigned_abs() > 9 {
        champions[idx].flag_redraw(0x0800);
    }
}

/// Experience multiplier of a map: descriptor word +12, bits 12-15 (the
/// parser's `difficulty` field).
pub fn map_experience_multiplier(dg: &dm2_formats::dungeon::Dungeon, map: usize) -> u32 {
    dg.maps.get(map).map_or(0, |m| m.difficulty as u32)
}

/// Grant experience (0x462FA), rolling stat gains for each class level won.
pub fn add_experience(
    champions: &mut [Champion],
    party: &mut PartyStatus,
    idx: usize,
    sk: usize,
    amount: u32,
    tick: u32,
    map_multiplier: u32,
    rng: &mut Rng,
) {
    let mut xp = amount;
    let recent = |within: u32| tick.wrapping_sub(party.last_attacked) < within;
    if (4..12).contains(&sk) && !recent(150) {
        xp >>= 1;
    }
    if xp == 0 {
        return;
    }
    if map_multiplier != 0 {
        xp = xp.wrapping_mul(map_multiplier);
    }
    let class = if sk >= 4 { (sk - 4) / 4 } else { sk };
    let before = level(&champions[idx], party, class, false);
    if sk >= 4 && recent(40) {
        xp = xp.wrapping_mul(2);
    }
    let c = &mut champions[idx];
    c.set_experience(sk, c.experience(sk).wrapping_add(xp));
    if sk >= 4 {
        c.set_experience(class, c.experience(class).wrapping_add(xp));
    }
    let after = level(c, party, class, false);
    let a = after as i32;
    for _ in before..after {
        let b = rng.bit() as u8;
        let r = rng.bit() as u8 + 1;
        let mut v = rng.bit() as u8;
        if class != skill::PRIEST {
            v &= a as u8;
        }
        let bump = |c: &mut Champion, s: usize, n: u8| {
            let m = c.stat_raw(s, 1).wrapping_add(n);
            c.set_stat_raw(s, 1, m);
        };
        bump(c, stat::VITALITY, v);
        let f = rng.bit() as u8 & !(a as u8);
        bump(c, stat::ANTI_FIRE, f);
        let (hp_gain, st_base) = match class {
            skill::FIGHTER => {
                bump(c, stat::STRENGTH, r);
                bump(c, stat::DEXTERITY, b);
                (3 * a, c.max_stamina() as i32 / 16)
            }
            skill::NINJA => {
                bump(c, stat::STRENGTH, b);
                bump(c, stat::DEXTERITY, r);
                (2 * a, c.max_stamina() as i32 / 21)
            }
            skill::PRIEST => {
                c.set_max_mana(c.max_mana() + a as i16);
                bump(c, stat::WISDOM, b);
                (a + ((a + 1) >> 1), c.max_stamina() as i32 / 25)
            }
            _ => {
                c.set_max_mana(c.max_mana() + (a + (a >> 1)) as i16);
                bump(c, stat::WISDOM, r);
                (a, c.max_stamina() as i32 / 32)
            }
        };
        if class >= skill::PRIEST {
            let extra = (rng.rand4() as i32).min(a - 1).max(0);
            c.set_max_mana((c.max_mana() as i32 + extra).min(900) as i16);
            let am = rng.rand4() as u8;
            bump(c, stat::ANTI_MAGIC, am);
        }
        let hp = c.max_health() as i32 + hp_gain + rng.random(((hp_gain >> 1) + 1) as u16) as i32;
        c.set_max_health(hp.min(999) as i16);
        let st = c.max_stamina() as i32 + st_base + rng.random(((st_base >> 1) + 1) as u16) as i32;
        c.set_max_stamina(st.min(9999) as i16);
        c.flag_redraw(0x3800);
        party.level_ups[idx][class] = party.level_ups[idx][class].wrapping_add(1);
        party.notices.push(Notice::LevelUp { champion: idx, class });
    }
}

/// Undo the champion-text obfuscation (docs/13): byte i -> (!b - i).
fn decode_text(raw: &[u8], obfuscated: bool) -> Vec<u8> {
    let mut out: Vec<u8> = raw
        .iter()
        .enumerate()
        .map(|(i, &b)| if obfuscated { (!b).wrapping_sub(i as u8) } else { b })
        .collect();
    if let Some(p) = out.iter().position(|&c| c == 0) {
        out.truncate(p);
    }
    out
}

/// Build a newly recruited champion (0x49242).
///
/// `taken_cells` marks formation cells already occupied. Consumes two
/// random numbers (food, then water).
pub fn recruit(g: &Gdat, portrait: u8, party_facing: u8, taken_cells: &[bool; 4], rng: &mut Rng) -> Option<Champion> {
    let stats = g.get(Key::new(22, portrait, 8, 0))?;
    if stats.len() < 52 {
        return None;
    }
    let w = |i: usize| u16::from_le_bytes([stats[2 * i], stats[2 * i + 1]]);
    let mut c = Champion::default();
    c.raw[0x1C] = party_facing;
    c.raw[0x28] = party_facing;
    c.raw[0x1D] = (0..4u8).map(|k| (party_facing + k) & 3).find(|&cell| !taken_cells[cell as usize]).unwrap_or(0);
    let obf = g.lookup(Key::new(0, 0, 11, 0)).unwrap_or(0) & 0x08 != 0;
    if let Some(t) = g.get(Key::new(22, portrait, 5, 24)) {
        let text = decode_text(t, obf);
        let split = text.iter().position(|&ch| ch == b' ').unwrap_or(text.len());
        let name = &text[..split.min(7)];
        c.raw[..name.len()].copy_from_slice(name);
        let title = text.get(split + 1..).unwrap_or(&[]);
        let n = title.len().min(19);
        c.raw[8..8 + n].copy_from_slice(&title[..n]);
    }
    for slot in 0..INVENTORY_SLOTS {
        c.set_inventory(slot, EMPTY);
    }
    c.set_health(w(0) as i16);
    c.set_max_health(w(0) as i16);
    c.set_stamina(w(1) as i16);
    c.set_max_stamina(w(1) as i16);
    c.set_mana(w(2) as i16);
    c.set_max_mana(w(2) as i16);
    for s in 0..7 {
        let v = w(3 + s).max(30).min(255) as u8;
        c.set_stat_raw(s, 0, v);
        c.set_stat_raw(s, 1, v);
    }
    for sub in 0..16 {
        let n = w(10 + sub);
        let xp = if n == 0 { 0 } else { 64u32 << n.min(25) };
        c.set_experience(4 + sub, xp);
        let class = sub / 4;
        c.set_experience(class, c.experience(class) + xp);
    }
    c.set_food(1500 + (rng.rnd() & 0xFF) as i16);
    c.set_water(1500 + (rng.rnd() & 0xFF) as i16);
    c.raw[0x101] = portrait;
    c.set_u16(0x2E, 0xFFFF);
    Some(c)
}

/// Periodic regeneration for every champion (0x47CC3); see `regen_due`
/// for how often the main loop runs it.
pub fn regenerate(champions: &mut [Champion], party: &mut PartyStatus, tick: u32, rng: &mut Rng) {
    if champions.is_empty() {
        return;
    }
    let g = party.regen_counter + 0x38;
    party.regen_counter = if g > 0x80 { party.regen_counter.wrapping_sub(0x48) } else { g };
    let gc = party.regen_counter as i32;
    for idx in 0..champions.len() {
        if !champions[idx].is_alive() || party.recruiting == Some(idx) {
            continue;
        }
        // Mana.
        let c = &champions[idx];
        if c.mana() < c.max_mana() {
            let l = level(c, party, skill::WIZARD, true) as i32 + level(c, party, skill::PRIEST, true) as i32;
            let wis = stat_current(c, stat::WISDOM, rng) as i32;
            if gc < wis + l {
                let mut gain = c.max_mana() as i32 / 40 + 1;
                if party.asleep {
                    gain *= 2;
                }
                stamina_loss(champions, party, idx, ((16 - l).max(7) * gain) as i16);
                let c = &mut champions[idx];
                let add = gain.min(c.max_mana() as i32 - c.mana() as i32);
                c.set_mana(c.mana() + add as i16);
            }
        } else if c.mana() > c.max_mana() {
            let m = c.mana() - 1;
            champions[idx].set_mana(m);
        }
        // Stamina, food and water.
        let c = &mut champions[idx];
        let mut k: i32 = 4;
        let mut s = c.max_stamina() as i32;
        loop {
            s >>= 1;
            if (c.stamina() as i32) < s {
                k += 2;
            } else {
                break;
            }
        }
        let mut rate = ((c.max_stamina() as i32 >> 8) - 1).clamp(1, 6);
        let rested = tick.wrapping_sub(party.party_formed) as u16;
        if rested > 0x50 {
            rate += 1;
            if rested > 0xFA {
                rate += 1;
            }
        }
        if party.asleep {
            rate *= 2;
        }
        let mut loss: i32 = 0;
        loop {
            let low = k < 5;
            let food = c.food() as i32;
            if food < -0x200 {
                if low {
                    loss += rate;
                    c.set_food((food - 2) as i16);
                }
            } else {
                if food >= 0 {
                    loss -= rate;
                }
                c.set_food((food - if low { 2 } else { k >> 1 }) as i16);
            }
            let water = c.water() as i32;
            if water < -0x200 {
                if low {
                    loss += rate;
                    c.set_water((water - 1) as i16);
                }
            } else {
                if water >= 0 {
                    loss -= rate;
                }
                c.set_water((water - if low { 1 } else { k >> 2 }) as i16);
            }
            k -= 1;
            if k == 0 || c.stamina() as i32 - loss >= c.max_stamina() as i32 {
                break;
            }
        }
        stamina_loss(champions, party, idx, loss as i16);
        let c = &mut champions[idx];
        c.set_food(c.food().max(-0x400));
        c.set_water(c.water().max(-0x400));
        // Health.
        if c.health() < c.max_health() && (c.max_stamina() >> 2) <= c.stamina() {
            let vit = stat_current(c, stat::VITALITY, rng) as i32;
            if gc < vit + 12 {
                let mut gain = (c.max_health() >> 7) as i32 + 1;
                if party.asleep {
                    gain *= 2;
                }
                let add = gain.min(c.max_health() as i32 - c.health() as i32);
                c.set_health(c.health() + add as i16);
            }
        }
        // Stats drift back toward their maximum.
        let mask = if party.asleep { 0x3F } else { 0xFF };
        if tick & mask == 0 {
            for s in 0..7 {
                let (cur, max) = (c.stat_raw(s, 0), c.stat_raw(s, 1));
                if cur < max {
                    c.set_stat_raw(s, 0, cur + 1);
                } else if max < cur && max != 0 {
                    c.set_stat_raw(s, 0, cur - cur / max);
                }
            }
        }
        c.flag_redraw(0x0800);
    }
}

/// Apply pending damage and wounds, and handle deaths (the screen update's
/// part of 0x4722A; death is 0x46ECA).
/// Returns the champions that took damage and survived, with the amount, in
/// champion order: the caller starts their damage display (0x47113).
pub fn apply_pending(champions: &mut [Champion], party: &mut PartyStatus) -> Vec<(usize, i16)> {
    let mut hurt = Vec::new();
    for idx in 0..champions.len() {
        let dmg = std::mem::take(&mut party.pending_damage[idx]);
        let wounds = std::mem::take(&mut party.pending_wounds[idx]);
        let c = &mut champions[idx];
        if !c.is_alive() {
            continue;
        }
        c.set_wounds(c.wounds() | wounds);
        if dmg > 0 {
            let hp = c.health() as i32 - dmg as i32;
            if hp <= 0 {
                die(c);
                party.notices.push(Notice::Died { champion: idx });
            } else {
                c.set_health(hp as i16);
                hurt.push((idx, dmg));
            }
            c.flag_redraw(0x0800);
        }
    }
    hurt
}

/// Start or extend a champion's damage display (0x47113): show the amount
/// (+0x30), flag the box (+0x33 bit 3) and end the display with event 0x0C
/// five ticks from now. The event's record is kept in +0x2E; while one is
/// pending its time is moved instead of scheduling another.
fn start_damage_display(g: &mut GameState, idx: usize, dmg: i16) {
    let due = g.tick.wrapping_add(5);
    let map = g.party.map as u8;
    let c = &mut g.champions[idx];
    c.set_u16(0x30, dmg as u16);
    c.raw[0x33] |= 8;
    let rec = c.u16_at(0x2E);
    if rec == 0xFFFF {
        let ev = crate::timeline::Event {
            prio: idx as u8,
            ..crate::timeline::Event::new(crate::party::EVENT_DAMAGE_DISPLAY, map, due)
        };
        let slot = g.schedule(ev).unwrap_or(0xFFFF);
        g.champions[idx].set_u16(0x2E, slot);
    } else {
        g.timeline.modify(rec, |e| {
            e.tick = due;
            e.map = map;
        });
    }
}

/// Death (0x46ECA), champion side only.
// The dungeon side (possessions, bones, leadership) is party::on_death.
fn die(c: &mut Champion) {
    c.set_health(0);
    c.raw[0x1E] = 0;
    c.raw[0x22..0x26].fill(0);
    c.set_poison_pool(0);
    c.raw[0x1F] = 0;
    c.flag_redraw(0x40);
}

/// Poison a champion (0x474FC): immediate damage, then the rest of the
/// dose is delivered by repeating timeline events 36 ticks apart.
pub fn poison(g: &mut GameState, idx: usize, amount: i16) {
    if amount < 1 || idx >= g.champions.len() {
        return;
    }
    let hit = ((amount as i32 + 30) / 64).max(1) as i16;
    add_pending_damage(&g.champions, &mut g.party_status, idx, hit);
    let rest = amount - 1;
    if rest == 0 {
        return;
    }
    let c = &mut g.champions[idx];
    c.set_poison_pool(((c.poison_pool() as i32 + rest as i32).min(3072)) as i16);
    c.raw[0x1F] = c.raw[0x1F].wrapping_add(1);
    let mut ev = Event::new(EVENT_POISON, g.party.map as u8, g.tick.wrapping_add(36));
    ev.prio = idx as u8;
    let _ = g.timeline.schedule(ev);
}

/// Handler for timeline event 0x4B: deliver the next dose of poison. The
/// events module calls this for that type.
pub fn poison_event(g: &mut GameState, ev: &crate::timeline::Event) {
    let idx = ev.prio as usize;
    let Some(c) = g.champions.get_mut(idx) else { return };
    c.raw[0x1F] = c.raw[0x1F].saturating_sub(1);
    let pool = c.poison_pool();
    if !c.is_alive() || pool <= 0 {
        return;
    }
    c.set_poison_pool(0);
    poison(g, idx, pool);
}

/// Cure poison (0x475D3) by up to `strength` points of the pool.
pub fn cure_poison(c: &mut Champion, strength: i16) {
    let left = (c.poison_pool() - strength).max(0);
    c.set_poison_pool(left);
    if left == 0 {
        c.raw[0x1F] = 0;
    }
}

/// Eat or drink (0x39C3F, command 0x10): `food_value` from item attribute 3;
/// water adds 800. Both cap at 2048.
pub fn eat(c: &mut Champion, food_value: i16) {
    c.set_food((c.food() as i32 + food_value as i32).min(2048) as i16);
}

pub fn drink_water(c: &mut Champion) {
    c.set_water((c.water() as i32 + 800).min(2048) as i16);
}

/// The main loop (0x24691) runs regeneration only when the game tick is a
/// multiple of 64, or of 16 while the party sleeps (flag 0x7F234).
pub fn regen_due(tick: u32, asleep: bool) -> bool {
    tick & if asleep { 0x0F } else { 0x3F } == 0
}

/// Per-tick champion work called from `GameState::advance`.
/// Add an action's busy time to a hand's busy counter (0x40A0A, +0x2A+hand;
/// hand -1 adds it to all three counters). The time is scaled by 5/4, cut
/// to a quarter while the haste counter (0x7FFF0) runs, plus 2; the larger
/// of the old count and the new time is kept plus half the smaller, capped
/// at 255.
pub fn add_busy(c: &mut Champion, hand: i16, busy: u16, haste: bool) {
    let mut t = busy.wrapping_add(busy >> 2);
    if haste {
        t >>= 2;
    }
    t = t.wrapping_add(2);
    let hands: &[usize] = if hand < 0 { &[0, 1, 2] } else { &[0] };
    for &k in hands {
        let i = 0x2A + if hand < 0 { k } else { hand as usize };
        let cur = c.raw[i] as u16;
        let v = if t > cur { t + (cur >> 1) } else { cur + (t >> 1) };
        c.raw[i] = v.min(0xFF) as u8;
    }
}

/// Count down every recruited champion's three busy counters (0x3FE68, run
/// once a tick by the main loop through 0x4904F). A counter reaching zero
/// finishes that hand's action (0x40AA6): for a living champion's hands 0-1
/// the hand's action byte (+0x20) returns to 0xFF and its defence bonus
/// (+0x42) to 0.
pub fn count_down_busy(g: &mut GameState) {
    for c in g.champions.iter_mut() {
        count_down_hands(c);
    }
}

/// One champion's part of `count_down_busy`.
pub fn count_down_hands(c: &mut Champion) {
    for h in 0..3 {
        let b = c.raw[0x2A + h];
        if b == 0 {
            continue;
        }
        c.raw[0x2A + h] = b - 1;
        if b == 1 && c.is_alive() && h < 2 {
            // TODO(0x40B09): finishing actions 0x20 and 0x2A also reloads
            // the hand from the quiver or a container.
            c.raw[0x20 + h] = 0xFF;
            c.raw[0x42 + h] = 0;
        }
    }
}

pub fn tick(g: &mut GameState) {
    let tick = g.tick;
    if regen_due(tick, g.party_status.asleep) {
        regenerate(&mut g.champions, &mut g.party_status, tick, &mut g.rng);
    }
    let seen = g.party_status.notices.len();
    for (idx, dmg) in apply_pending(&mut g.champions, &mut g.party_status) {
        start_damage_display(g, idx, dmg);
    }
    let died: Vec<usize> = g.party_status.notices[seen..]
        .iter()
        .filter_map(|n| match n {
            Notice::Died { champion } => Some(*champion),
            _ => None,
        })
        .collect();
    for idx in died {
        crate::party::on_death(g, idx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regeneration_runs_every_64_ticks_or_16_asleep() {
        let awake: Vec<u32> = (0..200).filter(|&t| regen_due(t, false)).collect();
        assert_eq!(awake, vec![0, 64, 128, 192]);
        assert_eq!((0..64).filter(|&t| regen_due(t, true)).count(), 4);
    }

    /// Regression: regeneration used to run every tick, so a fed champion
    /// starved within ~750 ticks (about 100 seconds of play).
    #[test]
    fn idle_party_does_not_starve_quickly() {
        let Some(data) = crate::data::GameData::load_default() else { return };
        let Ok(bytes) = std::fs::read(crate::assets::default_data_dir().join("DUNGEON.DAT")) else { return };
        let dg = dm2_formats::dungeon::Dungeon::parse(&bytes).unwrap();
        let mut g = GameState::new_game_with(&dg, std::rc::Rc::new(data));
        let Some(c) = g.champions.first() else { return };
        let (food, water) = (c.food(), c.water());
        for _ in 0..640 {
            g.advance();
        }
        let c = &g.champions[0];
        // 10 or 11 upkeep calls at a few points each, not 640.
        assert!(food - c.food() < 100, "food {food} -> {}", c.food());
        assert!(water - c.water() < 100, "water {water} -> {}", c.water());
        assert!(c.is_alive());
    }

    fn sample() -> Champion {
        let mut c = Champion::default();
        c.set_health(50);
        c.set_max_health(60);
        c.set_stamina(800);
        c.set_max_stamina(1000);
        c.set_mana(5);
        c.set_max_mana(20);
        c.set_food(1600);
        c.set_water(1600);
        for s in 0..7 {
            c.set_stat_raw(s, 0, 45);
            c.set_stat_raw(s, 1, 50);
        }
        for slot in 0..INVENTORY_SLOTS {
            c.set_inventory(slot, EMPTY);
        }
        c
    }

    #[test]
    fn record_round_trip() {
        let mut c = sample();
        c.set_experience(9, 0x1234_5678);
        let back = Champion::from_bytes(&c.to_bytes()).unwrap();
        assert_eq!(back.experience(9), 0x1234_5678);
        assert_eq!(back.max_stamina(), 1000);
        assert_eq!(back.stat_raw(stat::WISDOM, 1), 50);
    }

    #[test]
    fn levels_double() {
        let mut c = sample();
        let p = PartyStatus::default();
        assert_eq!(level(&c, &p, 0, false), 1);
        c.set_experience(0, 512);
        assert_eq!(level(&c, &p, 0, false), 2);
        c.set_experience(0, 1024);
        assert_eq!(level(&c, &p, 0, false), 3);
        // sub-skill folds in its class: (xp + class) / 2
        assert_eq!(level(&c, &p, 4, false), 2); // (0 + 1024) / 2 = 512
        c.set_experience(4, 1024);
        assert_eq!(level(&c, &p, 4, false), 3); // (1024 + 1024) / 2 = 1024
        let asleep = PartyStatus { asleep: true, ..Default::default() };
        assert_eq!(level(&c, &asleep, 0, false), 1);
    }

    #[test]
    fn stamina_and_load_formulas() {
        let mut c = sample();
        // full stamina: unchanged
        c.set_stamina(1000);
        assert_eq!(stamina_adjusted(&c, 100), 100);
        // a quarter stamina: v/2 + 250*50/500
        c.set_stamina(250);
        assert_eq!(stamina_adjusted(&c, 100), 75);
        c.set_stamina(1000);
        let mut rng = Rng::new(1);
        // strength 45 (no boost): 45*8+100 = 460, rounded up to 460
        assert_eq!(max_load(&c, &mut rng), 460);
        c.set_wounds(part::LEGS);
        // 460 - 460/4 = 345 -> 350
        assert_eq!(max_load(&c, &mut rng), 350);
        assert_eq!(rng.state, 1, "no random numbers without a stat boost");
    }

    #[test]
    fn move_time_bands() {
        let mut c = sample();
        c.set_stamina(1000);
        let p = PartyStatus::default();
        let mut rng = Rng::new(0);
        c.set_load(100);
        assert_eq!(move_time(&c, &p, &mut rng), 2);
        c.set_load(300); // over 5/8 of 460
        assert_eq!(move_time(&c, &p, &mut rng), 4); // 3 rounds up to even
        c.set_load(460 * 2); // overloaded: (460*4/460)+4 = 8
        assert_eq!(move_time(&c, &p, &mut rng), 8);
        let haste = PartyStatus { haste: 1, ..Default::default() };
        assert_eq!(move_time(&c, &haste, &mut rng), 1);
    }

    #[test]
    fn stamina_overflow_becomes_damage() {
        let mut champs = vec![sample()];
        let mut p = PartyStatus::default();
        champs[0].set_stamina(10);
        stamina_loss(&mut champs, &mut p, 0, 30);
        assert_eq!(champs[0].stamina(), 0);
        assert_eq!(p.pending_damage[0], 10);
        apply_pending(&mut champs, &mut p);
        assert_eq!(champs[0].health(), 40);
    }

    #[test]
    fn experience_and_level_up_rng_order() {
        let mut champs = vec![sample()];
        let mut p = PartyStatus { last_attacked: 1000, ..Default::default() };
        let mut rng = Rng::new(0x1234);
        let mut expect = Rng::new(0x1234);
        // Fighter sub-skill 4 within 40 ticks of an attack: no halving, doubled.
        add_experience(&mut champs, &mut p, 0, 4, 300, 1010, 0, &mut rng);
        let c = &champs[0];
        assert_eq!(c.experience(4), 600);
        assert_eq!(c.experience(0), 600);
        // class level 1 -> 2: one round of rolls, A = 2
        let b = expect.bit() as u8;
        let r = expect.bit() as u8 + 1;
        let v = expect.bit() as u8 & 2;
        let f = expect.bit() as u8 & !2u8;
        let hp = 6 + expect.random(4) as i16;
        let st = 1000 / 16 + expect.random((1000 / 16 / 2 + 1) as u16) as i16;
        assert_eq!(rng.state, expect.state);
        assert_eq!(c.stat_raw(stat::STRENGTH, 1), 50 + r);
        assert_eq!(c.stat_raw(stat::DEXTERITY, 1), 50 + b);
        assert_eq!(c.stat_raw(stat::VITALITY, 1), 50 + v);
        assert_eq!(c.stat_raw(stat::ANTI_FIRE, 1), 50 + f);
        assert_eq!(c.max_health(), 60 + hp);
        assert_eq!(c.max_stamina(), 1000 + st);
        assert_eq!(p.notices, vec![Notice::LevelUp { champion: 0, class: 0 }]);
    }

    #[test]
    fn experience_halved_without_combat() {
        let mut champs = vec![sample()];
        let mut p = PartyStatus::default();
        let mut rng = Rng::new(0);
        add_experience(&mut champs, &mut p, 0, 5, 100, 10_000, 3, &mut rng);
        // halved (no recent attack), then x3 map multiplier
        assert_eq!(champs[0].experience(5), 150);
    }

    #[test]
    fn regeneration_food_water_and_counter() {
        let mut champs = vec![sample()];
        let mut p = PartyStatus::default();
        let mut rng = Rng::new(7);
        champs[0].set_mana(20); // full mana: no mana branch
        champs[0].set_stamina(1000);
        champs[0].set_health(60);
        regenerate(&mut champs, &mut p, 1, &mut rng);
        assert_eq!(p.regen_counter, 0x38);
        // k = 4 (full stamina), one pass: food -2, water -1
        assert_eq!(champs[0].food(), 1598);
        assert_eq!(champs[0].water(), 1599);
        regenerate(&mut champs, &mut p, 2, &mut rng);
        assert_eq!(p.regen_counter, 0x70);
        regenerate(&mut champs, &mut p, 3, &mut rng);
        assert_eq!(p.regen_counter, 0x70 - 0x48);
    }

    #[test]
    fn luck_roll_order() {
        let mut c = sample();
        let mut rng = Rng::new(99);
        let mut e = Rng::new(99);
        let ok = lucky(&mut c, 75, &mut rng);
        let early = e.bit() != 0 && e.random(100) > 75;
        if !early {
            let r = e.random(90);
            assert_eq!(ok, r > 75);
        }
        assert_eq!(rng.state, e.state);
    }

    #[test]
    fn recruits_from_users_data() {
        let Ok(g) = Gdat::open(dm2_formats::gdat::default_path()) else { return };
        let mut rng = Rng::new(0);
        let mut seen = 0;
        for portrait in 0..=255u8 {
            if g.get(Key::new(22, portrait, 8, 0)).is_none() {
                continue;
            }
            let c = recruit(&g, portrait, 1, &[true, false, false, false], &mut rng).unwrap();
            seen += 1;
            assert!(!c.name().is_empty() && c.name().len() <= 7);
            assert!(c.health() > 0 && c.health() == c.max_health());
            assert!((0..7).all(|s| c.stat_raw(s, 0) >= 30));
            assert!((1500..1756).contains(&c.food()) && (1500..1756).contains(&c.water()));
            assert_eq!(c.cell(), 1, "first free cell clockwise from facing");
            for class in 0..4 {
                let sum: u32 = (0..4).map(|k| c.experience(4 + 4 * class + k)).sum();
                assert_eq!(c.experience(class), sum);
            }
        }
        assert!(seen > 0);
    }

    /// 0x40A0A: busy time scaled by 5/4 plus 2, the larger of old and new
    /// kept plus half the smaller, capped at 255; a quarter while hasted.
    #[test]
    fn busy_time_combines_like_the_original() {
        let mut c = Champion::default();
        add_busy(&mut c, 0, 8, false);
        assert_eq!(c.raw[0x2A], 12, "8 + 2 + 2");
        add_busy(&mut c, 0, 8, false);
        assert_eq!(c.raw[0x2A], 18, "12 + 12/2");
        add_busy(&mut c, 1, 8, true);
        assert_eq!(c.raw[0x2B], 4, "hasted: 10/4 + 2");
        add_busy(&mut c, -1, 400, false);
        assert_eq!(&c.raw[0x2A..0x2D], &[255, 255, 255], "all hands, capped");
    }

    /// 0x3FE68 / 0x40AA6: a hand's counter runs down one a tick; reaching
    /// zero ends its action and drops its defence bonus.
    #[test]
    fn busy_countdown_finishes_the_hands_action() {
        let mut c = Champion::default();
        c.set_health(10);
        c.raw[0x20] = 4;
        c.raw[0x42] = 3;
        c.raw[0x2A] = 2;
        count_down_hands(&mut c);
        assert_eq!((c.raw[0x2A], c.raw[0x20], c.raw[0x42]), (1, 4, 3));
        count_down_hands(&mut c);
        assert_eq!((c.raw[0x2A], c.raw[0x20], c.raw[0x42]), (0, 0xFF, 0));
    }
}
