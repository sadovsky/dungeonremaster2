//! The one place where queued outcomes from combat, magic and the dungeon
//! mechanics change the game state. Presentation-only effects (sounds,
//! text, notices) are left in `GameState::effects` for the frontend.

use dm2_formats::dungeon::{ThingRef, ThingType};

use crate::champions::{self, EMPTY};
use crate::combat::{self, Effect as Action};
use crate::creatures;
use crate::doors;
use crate::effects::Effect;
use crate::magic::CastEffect;
use crate::missiles;
use crate::party;
use crate::state::GameState;
use crate::timeline::Event;
use crate::viewport::{DX, DY};

/// Light expiry (handler 0x5917D): adds +6 back to the light level.
pub const EVENT_LIGHT: u8 = 0x46;

/// Misc item kind of an empty flask (docs/09).
const EMPTY_FLASK: u16 = 0x14;

/// Damage one champion through the real formula (0x4722A). Without the
/// shared data, armour can't be read and the damage is queued as is.
pub fn damage_champion(g: &mut GameState, idx: usize, amount: i16, parts: u16, atype: u16) -> i16 {
    let Some(data) = g.data.clone() else {
        return champions::add_pending_damage(&g.champions, &mut g.party_status, idx, amount);
    };
    let db = data.item_db(&g.dungeon);
    combat::damage_champion(
        &mut g.champions,
        &mut g.party_status,
        idx,
        amount,
        parts,
        atype,
        &db,
        &data.tables,
        &mut g.rng,
    )
}

/// Damage every living champion (0x4766B). Returns the mask of champions hurt.
pub fn damage_party(g: &mut GameState, amount: i16, parts: u16, atype: u16) -> u16 {
    let mut mask = 0;
    for i in 0..g.champions.len() {
        if g.champions[i].is_alive() && damage_champion(g, i, amount, parts, atype) > 0 {
            mask |= 1 << i;
        }
    }
    mask
}

/// The cell a champion's missile starts from: their own cell if it is in
/// the front row, else the front cell on the same side (0x47773).
fn launch_cell(champion_cell: u8, dir: u8) -> u8 {
    let c = champion_cell & 3;
    if c == dir || c == (dir + 1) & 3 {
        c
    } else if c == (dir + 2) & 3 {
        (dir + 1) & 3
    } else {
        dir
    }
}

fn ahead(g: &GameState) -> (usize, i32, i32) {
    let p = g.party;
    (p.map, p.x + DX[p.dir as usize], p.y + DY[p.dir as usize])
}

/// Light or darkness (0x412E1). Kind 6 darkens, kinds 0x26 and 0x27 light
/// up; event 0x46 undoes the change when the duration ends.
pub fn light(g: &mut GameState, kind: u8, strength: u16) {
    let s = (strength as i32 + 1).clamp(32, 256);
    let mut level = (s >> 3).max(8);
    let (duration, sign) = match kind {
        6 => ((level - 8) * 16 + 16, -2),
        0x26 => {
            let d = (level - 3) * 128 + 2000;
            level = (level >> 2) + 1;
            (d, 1)
        }
        0x27 => ((level - 8) * 512 + 10000, 1),
        _ => return,
    };
    if kind != 0x26 {
        level = (level >> 1) - 1;
    }
    // TODO(0x756FE): the immediate change is a table value indexed by the
    // strength; the level itself is used here.
    g.light = g.light.saturating_add((level * sign) as i16);
    let undo = if kind == 6 { level } else { -level };
    let mut ev = Event::new(EVENT_LIGHT, g.party.map as u8, g.tick.wrapping_add(duration as u32));
    [ev.x, ev.y] = (undo as i16 as u16).to_le_bytes();
    g.schedule(ev);
}

/// Event 0x46: a light or darkness effect ends.
pub fn light_expired(g: &mut GameState, ev: Event) {
    let v = i16::from_le_bytes([ev.x, ev.y]);
    g.light = g.light.saturating_add(v);
}

/// Apply what a champion's action produced (0x414A5 results).
pub fn apply_action(g: &mut GameState, champion: usize, effects: &[Action]) {
    for e in effects {
        apply_one(g, champion, e.clone());
    }
}

fn apply_one(g: &mut GameState, champion: usize, e: Action) {
    let p = g.party;
    match e {
        Action::DamageCreature { amount, .. } => {
            let (m, x, y) = ahead(g);
            if let Some(c) = creatures::group_at(g, m, x, y) {
                if amount > 0 {
                    // A blow that lands plays (15, creature type, 0x8D) at
                    // the creature's square (0x414A5).
                    let ty = creatures::creature_type(g, c);
                    g.effects.push(crate::effects::Effect::Sound { cat: 15, idx: ty, sub: 0x8D, map: m, x, y });
                    creatures::damage(g, c, m, x, y, amount as u16);
                }
            }
        }
        Action::LaunchMissile { champion: who, what, energy, attack, step } => {
            let cell = g.champions.get(who).map_or(p.dir, |c| launch_cell(c.cell(), p.dir));
            missiles::launch(g, what, p.map, p.x, p.y, cell, p.dir, energy, attack, step, false);
        }
        Action::Explosion { kind, strength } => {
            let cell = g.champions.get(champion).map_or(p.dir, |c| launch_cell(c.cell(), p.dir));
            missiles::explode(g, kind, strength.min(255) as u8, p.map, p.x, p.y, cell);
        }
        Action::Light { kind, strength } => light(g, kind, strength),
        Action::PartyEffect { kind, strength } => {
            party::champion_party_effect(g, champion, kind, strength, true);
        }
        Action::Invisibility { ticks } => {
            g.magic_counter = g.magic_counter.saturating_add(1);
            let ev = Event::new(party::EVENT_MAGIC_COUNTER, p.map as u8, g.tick.wrapping_add(ticks as u32));
            g.schedule(ev);
        }
        Action::BashDoor => {
            let (m, x, y) = ahead(g);
            let power = crate::hooks::bash_power(g);
            doors::bash(g, m, x, y, power, 0, false);
        }
        Action::Unsupported { .. } => {}
    }
}

/// Apply a successful cast (0x422F5 results).
pub fn apply_cast(g: &mut GameState, champion: usize, effects: Vec<CastEffect>) {
    for e in effects {
        match e {
            CastEffect::MakePotion { kind, power } => make_potion(g, champion, kind, power),
            CastEffect::CreateItem => create_item_from_spell(g, champion),
            CastEffect::Summon { kind, .. } => {
                let (m, x, y) = ahead(g);
                creatures::spawn(g, kind as u16, m, x, y, g.party.dir);
            }
            CastEffect::Other(a) => apply_one(g, champion, a),
        }
    }
}

/// Fill the empty flask in one of the champion's hands (0x4225E).
fn make_potion(g: &mut GameState, idx: usize, kind: u8, power: u8) {
    let Some(data) = g.data.clone() else { return };
    for hand in 0..2 {
        let Some(c) = g.champions.get(idx) else { return };
        let t = c.inventory(hand);
        if t == EMPTY {
            continue;
        }
        let item = ThingRef(t);
        let is_flask = item.kind() == ThingType::Misc
            && data.item_db(&g.dungeon).key(item).is_some_and(|(cat, i)| cat == 21 && i as u16 == EMPTY_FLASK);
        if !is_flask {
            continue;
        }
        let Some(p) = g.dungeon.alloc_thing(ThingType::Potion) else { return };
        g.dungeon.set_record_word(p, 1, 0x8000 | (kind as u16 & 0x7F) << 8 | power as u16);
        g.dungeon.free_thing(item);
        g.champions[idx].set_inventory(hand, p.0);
        crate::party::refresh_load(g, idx);
        return;
    }
}

/// Spell kind 0x0F: create the item named by attribute (13, 15, 11, 0x42),
/// into an empty hand or onto the party square.
fn create_item_from_spell(g: &mut GameState, idx: usize) {
    let n = g.attrs.get(13, 15, 0x42);
    let Some(t) = crate::actuators::create_item(g, n) else { return };
    if let Some(c) = g.champions.get_mut(idx) {
        if let Some(hand) = (0..2).find(|&h| c.inventory(h) == EMPTY) {
            c.set_inventory(hand, t.0);
            crate::party::refresh_load(g, idx);
            return;
        }
    }
    let p = g.party;
    let cell = g.champions.get(idx).map_or(p.dir, |c| c.cell() & 3);
    g.dungeon.add_thing(p.map, p.x, p.y, ThingRef(t.0 | (cell as u16) << 14));
}

/// Shooter actuators (0x57A63). The event's square (x, y) and direction
/// `dir` place the shot: missiles start one square ahead in `dir`, in cell
/// (dir + 2) & 3 (and the next cell for double shooters), with kinetic
/// energy from word 3 bits 4-11, step energy from bits 12-15 and a fixed
/// attack byte of 100. Kinds 0x0E/0x0F launch items lying on the event
/// square in cells `dir` and `dir + 1`.
fn shoot(g: &mut GameState, map: usize, x: i32, y: i32, dir: u8, actuator: ThingRef) {
    const ATTACK: u8 = 100;
    let a = crate::actuators::Actuator::load(g, actuator);
    let energy = (a.w3 >> 4 & 0xFF) as u8;
    let step = (a.w3 >> 12) as u8;
    let single = matches!(a.kind(), 0x07 | 0x08 | 0x0E);
    let dir = dir & 3;
    let (sx, sy) = (x + crate::viewport::DX[dir as usize], y + crate::viewport::DY[dir as usize]);
    let md = &g.dungeon.maps[map];
    if sx < 0 || sy < 0 || sx >= md.width as i32 || sy >= md.height as i32 {
        return; // nowhere for the shot to start; create and take nothing
    }
    // What to fire: one or two things, gathered before the cell roll.
    let mut what: Vec<u16> = Vec::new();
    match a.kind() {
        0x07 | 0x09 => {
            for _ in 0..if single { 1 } else { 2 } {
                match crate::actuators::create_item(g, a.data()) {
                    Some(t) => what.push(t.0 & 0x3FFF),
                    None => return,
                }
            }
        }
        0x08 | 0x0A => {
            let spell = 0xFF80u16.wrapping_add(a.data());
            what.push(spell);
            if !single {
                what.push(spell);
            }
        }
        _ => {
            let lying = |g: &GameState| {
                g.dungeon.things_at(map, x, y).into_iter().find(|t| {
                    (5..=10).contains(&(t.kind() as u8)) && (t.cell() == dir || t.cell() == (dir + 1) & 3)
                })
            };
            for _ in 0..if single { 1 } else { 2 } {
                let Some(t) = lying(g) else { break };
                g.dungeon.remove_thing(map, x, y, t);
                what.push(t.0 & 0x3FFF);
            }
            if what.is_empty() {
                return;
            }
        }
    }
    let mut cell = (dir + 2) & 3;
    if single {
        // A single shot drifts one cell sideways at random.
        cell = (cell + g.rng.bit() as u8) & 3;
    }
    for (i, w) in what.into_iter().enumerate() {
        missiles::launch(g, w, map, sx, sy, (cell + i as u8) & 3, dir, energy, ATTACK, step, true);
    }
}

/// Apply the queued mechanics effects that change state; leave the
/// presentation ones for the frontend.
pub fn apply_effects(g: &mut GameState) {
    let queued = std::mem::take(&mut g.effects);
    let mut keep = Vec::new();
    for e in queued {
        match e {
            Effect::Shoot { map, x, y, dir, actuator, .. } => shoot(g, map, x, y, dir, actuator),
            other => keep.push(other),
        }
    }
    // Effects queued while applying (e.g. sounds) follow the kept ones.
    keep.append(&mut g.effects);
    g.effects = keep;
}

/// Party-wide damage requested by the mechanics (falls, doors, bumps).
pub fn mechanics_damage(g: &mut GameState, idx: Option<usize>, amount: u16, atype: u16) {
    match idx {
        Some(i) => {
            damage_champion(g, i, amount as i16, 0x3F, atype);
        }
        None => {
            damage_party(g, amount as i16, 0x3F, atype);
        }
    }
}
