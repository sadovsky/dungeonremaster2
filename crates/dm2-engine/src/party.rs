//! Party-level champion events: the starting champion, giving items,
//! death and bones, resurrection at altars, timed party effects and the
//! remaining champion timeline events (docs/06, docs/07).

use dm2_formats::dungeon::{ThingRef, ThingType};

use crate::champions::{self, Champion, EMPTY, INVENTORY_SLOTS};
use crate::effects::Effect;
use crate::state::GameState;
use crate::timeline::Event;

/// Wall actuator type that holds a recruitable champion (portrait in the
/// bits above 7 of word 1).
const ACTUATOR_PORTRAIT: u16 = 0x7E;
/// Misc item flagged as a champion's bones: kind bit 7, champion in bits 14-15.
const BONES_FLAG: u16 = 0x0080;
/// GRAPHICS.DAT category and index of the bones item.
const BONES_KEY: (u8, u8) = (21, 0);
/// Attribute (9, ornament, 11, ALTAR_ATTR) marks a resurrection altar.
const ALTAR_ATTR: u8 = 0x0C;

pub const EVENT_DAMAGE_DISPLAY: u8 = 0x0C;
pub const EVENT_REBIRTH: u8 = 0x0D;
pub const EVENT_MAGIC_COUNTER: u8 = 0x47;
pub const EVENT_PARTY_EFFECT: u8 = 0x48;
pub const EVENT_STATUS: u8 = 0x54;

fn is_item(t: ThingRef) -> bool {
    (5..=10).contains(&(t.kind() as u8))
}

/// Recruit the champion whose portrait actuator sits on square (0, 0) of
/// the party's map, as the new-game branch of 0x49D46 does: silently, as
/// the leader, facing the party's way, taking the items in cell 2 there.
pub fn recruit_starting_champion(g: &mut GameState) {
    let Some(data) = g.data.clone() else { return };
    let map = g.party.map;
    let portrait = g.dungeon.things_at(map, 0, 0).into_iter().find_map(|t| {
        (t.kind() == ThingType::Actuator)
            .then(|| g.dungeon.record_word(t, 1))
            .flatten()
            .filter(|w| w & 0x7F == ACTUATOR_PORTRAIT)
            .map(|w| (w >> 7) as u8)
    });
    let Some(portrait) = portrait else { return };
    let Some(mut c) = champions::recruit(&data.gdat, portrait, g.party.dir, &[false; 4], &mut g.rng) else { return };
    // 0x49D46 sets both the facing and the cell byte to the party facing.
    c.raw[0x1C] = g.party.dir;
    c.raw[0x1D] = g.party.dir;
    g.champions.push(c);
    g.leader = Some(0);
    // Belongings: items in the cell opposite the recruiting direction
    // (north, so cell 2) of the portrait square.
    let items: Vec<ThingRef> =
        g.dungeon.things_at(map, 0, 0).into_iter().filter(|t| is_item(*t) && t.cell() == 2).collect();
    for t in items {
        if give_item(g, 0, t) {
            g.dungeon.remove_thing(map, 0, 0, t);
        }
    }
    refresh_load(g, 0);
}

/// May item `t` go in inventory slot `slot` (0x152C4, mode 0)?
pub fn slot_fits(g: &GameState, t: ThingRef, slot: usize) -> bool {
    let Some(data) = g.data.as_ref() else { return false };
    match slot {
        0 | 1 | 13..=29 => true,
        2..=12 => {
            let db = data.item_db(&g.dungeon);
            db.attr(t, crate::items::ATTR_SLOTS) & data.slot_mask(slot) != 0
        }
        _ => false,
    }
}

/// Put an item into the first suitable empty slot (0x4916B): the five
/// slot ranges of the table are tried in order, each optionally limited to
/// one thing type. Returns false if nothing fits.
pub fn give_item(g: &mut GameState, idx: usize, t: ThingRef) -> bool {
    let Some(data) = g.data.clone() else { return false };
    if idx >= g.champions.len() {
        return false;
    }
    for (lo, hi, ty) in data.starting_slot_ranges() {
        for slot in lo..=hi.min(INVENTORY_SLOTS as u16 - 1) {
            let slot = slot as usize;
            if g.champions[idx].inventory(slot) != EMPTY || !slot_fits(g, t, slot) {
                continue;
            }
            if ty != 0xFFFF && t.kind() as u16 != ty {
                continue;
            }
            g.champions[idx].set_inventory(slot, t.0 & 0x3FFF);
            return true;
        }
    }
    false
}

/// Recompute a champion's cached load after its inventory changed.
pub(crate) fn refresh_load(g: &mut GameState, idx: usize) {
    let Some(data) = g.data.clone() else { return };
    let db = data.item_db(&g.dungeon);
    if let Some(c) = g.champions.get_mut(idx) {
        champions::recompute_load(c, &db);
    }
}

/// The parts of death (0x46ECA) that touch the dungeon and the party:
/// drop possessions, leave bones, pass leadership or end the game.
pub fn on_death(g: &mut GameState, idx: usize) {
    let Some(c) = g.champions.get(idx) else { return };
    let cell = c.cell() & 3;
    let (map, x, y) = (g.party.map, g.party.x, g.party.y);
    // Possessions are dropped on the party square, in the champion's cell.
    for slot in 0..INVENTORY_SLOTS {
        let t = g.champions[idx].inventory(slot);
        if t == EMPTY {
            continue;
        }
        g.champions[idx].set_inventory(slot, EMPTY);
        g.dungeon.add_thing(map, x, y, ThingRef((t & 0x3FFF) | (cell as u16) << 14));
    }
    g.champions[idx].set_load(0);
    // Bones: a misc item carrying the champion's index.
    if let Some(b) = g.dungeon.alloc_thing(ThingType::Misc) {
        g.dungeon.set_record_word(b, 1, BONES_FLAG | (idx as u16) << 14);
        g.dungeon.add_thing(map, x, y, ThingRef(b.0 | (cell as u16) << 14));
    }
    if g.leader == Some(idx) || g.leader.is_none() {
        g.leader = g.champions.iter().position(Champion::is_alive);
    }
    if !g.champions.iter().any(Champion::is_alive) {
        g.game_over = true;
        g.effects.push(Effect::EndGame);
    }
}

/// Is `t` a champion's bones? Returns the champion index.
pub fn bones_owner(g: &GameState, t: ThingRef) -> Option<usize> {
    let data = g.data.as_ref()?;
    if t.kind() != ThingType::Misc || data.item_db(&g.dungeon).key(t)? != BONES_KEY {
        return None;
    }
    Some((g.dungeon.record_word(t, 1)? >> 14) as usize)
}

/// Does square (x, y) hold an altar (an actuator whose wall ornament has
/// the altar attribute; 0x1FDF0 via 0x1FC2C)?
pub fn is_altar(g: &GameState, map: usize, x: i32, y: i32) -> bool {
    let lists = g.dungeon.map_lists(map);
    g.dungeon.things_at(map, x, y).into_iter().any(|t| {
        if t.kind() != ThingType::Actuator {
            return false;
        }
        let slot = g.dungeon.record_word(t, 2).unwrap_or(0) >> 12;
        if slot == 0 {
            return false;
        }
        let Some(&orn) = lists.wall_ornaments.get(slot as usize - 1) else { return false };
        g.attrs.get(9, orn, ALTAR_ATTR) != 0
    })
}

/// An item landed on (x, y): if it is a champion's bones on an altar,
/// start the three-stage rebirth (trigger in floor sensors, 0x4CDCC).
pub fn item_dropped(g: &mut GameState, map: usize, x: i32, y: i32, t: ThingRef) {
    let Some(idx) = bones_owner(g, t) else { return };
    if idx >= g.champions.len() || !is_altar(g, map, x, y) {
        return;
    }
    let mut ev = Event::new(EVENT_REBIRTH, map as u8, g.tick.wrapping_add(1));
    ev.prio = idx as u8;
    ev.x = x as u8;
    ev.y = y as u8;
    ev.b8 = t.cell();
    ev.b9 = 2;
    g.schedule(ev);
}

/// Event 0x0D (0x59050): stage 2 rebirth effect, stage 1 consume the bones,
/// stage 0 revive.
pub fn rebirth_event(g: &mut GameState, ev: Event) {
    let (map, x, y, idx) = (ev.map as usize, ev.x as i32, ev.y as i32, ev.prio as usize);
    match ev.b9 {
        2 => {
            crate::missiles::explode(g, 0xFFE4, 0, map, x, y, ev.b8);
            g.schedule(Event { tick: g.tick.wrapping_add(5), b9: 1, ..ev });
        }
        1 => {
            let bones = g
                .dungeon
                .things_at(map, x, y)
                .into_iter()
                .find(|&t| t.cell() == ev.b8 && bones_owner(g, t) == Some(idx));
            if let Some(b) = bones {
                g.dungeon.remove_thing(map, x, y, b);
                g.dungeon.free_thing(b);
            }
            g.schedule(Event { tick: g.tick.wrapping_add(1), b9: 0, ..ev });
        }
        _ => revive(g, idx),
    }
}

/// Bring a dead champion back (0x49CBB).
pub fn revive(g: &mut GameState, idx: usize) {
    let Some(c) = g.champions.get_mut(idx) else { return };
    if c.is_alive() {
        return;
    }
    // TODO(docs/06 open question): which fields 0x46E4D resets.
    c.set_load(0);
    for slot in 0..INVENTORY_SLOTS {
        c.set_inventory(slot, EMPTY);
    }
    let max = c.max_health();
    let new_max = (max - max / 64 - 1).max(25);
    c.set_max_health(new_max);
    c.set_health(new_max / 2);
    c.raw[0x33] |= 0x40;
    c.raw[0x102] = 0;
    c.set_shield_value(0);
    c.flag_redraw(0x0800);
    if g.leader.is_none() {
        g.leader = Some(idx);
    }
    g.game_over = false;
}

/// Timed party effect (0x4565A): champions in `mask` gain `strength` of
/// effect `kind` (stored at +0x102/+0x103) for `ticks`; event 0x48 takes
/// it away again. A different kind replaces the old one and cancels its
/// pending expiries.
pub fn party_effect(g: &mut GameState, mask: u8, kind: u8, strength: i16, ticks: u16) {
    let mut mask = mask;
    let mut strength = strength;
    let mut strong = false;
    for i in 0..4usize {
        let bit = 1u8 << i;
        if let Some(c) = g.champions.get_mut(i) {
            if mask & bit != 0 {
                if !c.is_alive() {
                    mask &= !bit;
                }
                if c.raw[0x102] != kind || !c.is_alive() {
                    c.set_shield_value(0);
                    // Cancel or trim this champion's pending expiries.
                    let pending: Vec<(u16, u8)> = g
                        .timeline
                        .iter()
                        .filter(|(_, e)| e.kind == EVENT_PARTY_EFFECT && e.prio & bit != 0)
                        .map(|(s, e)| (s, e.prio))
                        .collect();
                    for (slot, prio) in pending {
                        if prio & !mask == 0 {
                            g.timeline.delete(slot);
                        } else {
                            g.timeline.modify(slot, |e| e.prio &= !mask);
                        }
                    }
                }
            }
        }
        if g.champions.get(i).is_some_and(|c| c.shield_value() > 50) {
            strong = true;
        }
    }
    if strong {
        strength >>= 2;
    }
    for (i, c) in g.champions.iter_mut().enumerate() {
        if mask & (1 << i) != 0 {
            c.raw[0x102] = kind;
            c.set_shield_value(c.shield_value().saturating_add(strength));
        }
    }
    let mut ev = Event::new(EVENT_PARTY_EFFECT, g.party.map as u8, g.tick.wrapping_add(ticks as u32));
    ev.prio = mask;
    [ev.x, ev.y] = (strength as u16).to_le_bytes();
    g.schedule(ev);
}

/// Party effect cast from an item or spell on one champion (0x45815):
/// item use costs 4 mana (with less, the strength halves and mana empties).
/// Returns false if the champion had no mana at all.
pub fn champion_party_effect(g: &mut GameState, idx: usize, kind: u8, strength: u16, from_item: bool) -> bool {
    let mut strength = strength;
    let mut ok = true;
    if from_item {
        let Some(c) = g.champions.get_mut(idx) else { return false };
        if c.mana() == 0 {
            return false;
        }
        if c.mana() < 4 {
            strength >>= 1;
            c.set_mana(0);
            ok = false;
        } else {
            c.set_mana(c.mana() - 4);
        }
    }
    party_effect(g, 0x0F, kind, (strength >> 5) as i16, strength);
    ok
}

/// Event 0x48: the effect strength in +6 expires for the champions in +5.
fn party_effect_expired(g: &mut GameState, ev: Event) {
    let amount = u16::from_le_bytes([ev.x, ev.y]) as i16;
    for (i, c) in g.champions.iter_mut().enumerate() {
        if ev.prio & (1 << i) != 0 {
            c.set_shield_value((c.shield_value() - amount).max(0));
        }
    }
}

/// Champion timeline events other than poison (0x4B, champions.rs).
pub fn event(g: &mut GameState, ev: Event) {
    match ev.kind {
        // End of the damage display: reset the shown-damage word and redraw.
        EVENT_DAMAGE_DISPLAY => {
            if let Some(c) = g.champions.get_mut(ev.prio as usize) {
                c.set_u16(0x2E, 0xFFFF);
                if c.is_alive() {
                    c.raw[0x33] |= 8;
                }
            }
        }
        EVENT_REBIRTH => rebirth_event(g, ev),
        // Duration counter (invisibility and the like).
        EVENT_MAGIC_COUNTER => {
            g.magic_counter = g.magic_counter.saturating_sub(1);
        }
        EVENT_PARTY_EFFECT => party_effect_expired(g, ev),
        // Status refresh: redraw every champion box.
        EVENT_STATUS => {
            for c in g.champions.iter_mut() {
                c.flag_redraw(0x0800);
            }
        }
        _ => {}
    }
}
