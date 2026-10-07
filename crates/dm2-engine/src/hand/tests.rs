//! Tests for the leader hand and item moves (docs/10), on the user's data.

use std::rc::Rc;

use dm2_formats::dungeon::{Dungeon, ThingRef, ThingType};

use super::*;
use crate::assets::default_data_dir;
use crate::data::GameData;

fn game() -> Option<GameState> {
    let dg = Dungeon::parse(&std::fs::read(default_data_dir().join("DUNGEON.DAT")).ok()?).ok()?;
    let data = Rc::new(GameData::load_default()?);
    let g = GameState::new_game_with(&dg, data);
    (!g.champions.is_empty()).then_some(g)
}

/// Some item the starting champion carries, and its slot.
fn carried(g: &GameState) -> Option<(usize, ThingRef)> {
    (0..INVENTORY_SLOTS).find_map(|s| {
        let t = g.champions[0].inventory(s);
        (t != EMPTY).then_some((s, ThingRef(t)))
    })
}

#[test]
fn pick_up_and_put_back_an_inventory_item() {
    let Some(mut g) = game() else { return };
    let Some((slot, t)) = carried(&g) else { return };
    let load = g.champions[0].load();
    assert!(dispatch(&mut g, 0x07), "open the inventory");
    assert_eq!(g.hand.inventory_open, Some(0));
    // Take the item into the hand: the slot empties.
    assert!(click_slot(&mut g, 0, slot));
    assert_eq!(g.hand.held, t.0 & 0x3FFF);
    assert_eq!(g.champions[0].inventory(slot), EMPTY);
    // Clicking another empty slot with an empty hand does nothing.
    let empty = (13..INVENTORY_SLOTS).find(|&s| g.champions[0].inventory(s) == EMPTY);
    // Put it back; load is unchanged overall.
    assert!(click_slot(&mut g, 0, slot));
    if let Some(e) = empty {
        assert!(!click_slot(&mut g, 0, e));
    }
    assert_eq!(g.hand.held, EMPTY);
    assert_eq!(g.champions[0].inventory(slot), t.0 & 0x3FFF);
    assert_eq!(g.champions[0].load(), load);
    assert!(dispatch(&mut g, 0x0B));
    assert_eq!(g.hand.inventory_open, None);
}

#[test]
fn slot_restrictions_are_enforced() {
    let Some(mut g) = game() else { return };
    let Some((slot, t)) = carried(&g) else { return };
    click_slot(&mut g, 0, slot);
    assert_eq!(g.hand.held, t.0 & 0x3FFF);
    // At least one equipment slot (2-12) refuses this item, unless it fits
    // everywhere; slot_fits decides.
    for s in 2..=12 {
        if g.champions[0].inventory(s) != EMPTY {
            continue;
        }
        let ok = crate::party::slot_fits(&g, t, s);
        assert_eq!(click_slot(&mut g, 0, s), ok, "slot {s}");
        if ok {
            // Take it back out for the next slot.
            assert!(click_slot(&mut g, 0, s));
        }
    }
}

#[test]
fn drop_on_the_floor_and_take_back() {
    let Some(mut g) = game() else { return };
    let Some((slot, t)) = carried(&g) else { return };
    click_slot(&mut g, 0, slot);
    let p = g.party;
    let before = g.dungeon.things_at(p.map, p.x, p.y).len();
    assert!(viewport_click(&mut g, ViewRegion::NearRight));
    assert_eq!(g.hand.held, EMPTY);
    let cell = (p.dir + 1) & 3;
    let here = g.dungeon.things_at(p.map, p.x, p.y);
    assert_eq!(here.len(), before + 1);
    assert!(here.iter().any(|x| x.0 & 0x3FFF == t.0 & 0x3FFF && x.cell() == cell));
    // The other near cell is empty of it; take from the right one.
    assert!(viewport_click(&mut g, ViewRegion::NearRight));
    assert_eq!(g.hand.held, t.0 & 0x3FFF);
    assert_eq!(g.dungeon.things_at(p.map, p.x, p.y).len(), before);
}

#[test]
fn throwing_launches_a_missile() {
    let Some(mut g) = game() else { return };
    let Some((slot, _)) = carried(&g) else { return };
    click_slot(&mut g, 0, slot);
    let missiles = g.dungeon.thing_count(ThingType::Missile);
    let events = g.timeline.len();
    assert!(throw_held(&mut g, false));
    assert_eq!(g.hand.held, EMPTY);
    // A missile record was taken (or the item dropped if none were free).
    assert!(g.timeline.len() > events || g.dungeon.thing_count(ThingType::Missile) >= missiles);
}

#[test]
fn container_cells_hold_items() {
    let Some(mut g) = game() else { return };
    // Make a container from a free record and put it in the action hand.
    let Some(cont) = g.dungeon.alloc_thing(ThingType::Container) else { return };
    g.dungeon.set_record_word(cont, 1, ThingRef::END.0);
    let Some((slot, t)) = carried(&g) else { return };
    let action_hand = g.champions[0].inventory(1);
    if action_hand != EMPTY && action_hand != t.0 {
        return;
    }
    g.champions[0].set_inventory(1, cont.0 & 0x3FFF);
    dispatch(&mut g, 0x07);
    assert_eq!(open_container(&g).map(|c| c.0 & 0x3FFF), Some(cont.0 & 0x3FFF));
    if slot == 1 {
        return;
    }
    click_slot(&mut g, 0, slot);
    if crate::party::slot_fits(&g, t, 0) && fits(&g, t, CONTAINER_FIRST) {
        assert!(click_slot(&mut g, 0, CONTAINER_FIRST));
        assert_eq!(container_contents(&g, cont), vec![ThingRef(t.0 & 0x3FFF)]);
        assert_eq!(slot_item(&g, 0, CONTAINER_FIRST), t.0 & 0x3FFF);
        // And back out again.
        assert!(click_slot(&mut g, 0, CONTAINER_FIRST));
        assert!(container_contents(&g, cont).is_empty());
        assert_eq!(g.hand.held, t.0 & 0x3FFF);
    }
}

#[test]
fn action_menu_lists_the_bare_hand_or_item_actions() {
    let Some(mut g) = game() else { return };
    for hand in 0..2 {
        if open_menu(&mut g, 0, hand) {
            let m = g.hand.menu.clone().unwrap();
            assert_eq!((m.champion, m.hand), (0, hand));
            assert!(!m.actions.is_empty() && m.actions.len() <= 3);
            assert!(dispatch(&mut g, 0x70));
            assert!(g.hand.menu.is_none());
        }
    }
    // Running an action makes the hand busy for a while.
    if open_menu(&mut g, 0, 1) {
        assert!(choose_action(&mut g, 0));
        assert!(g.hand.menu.is_none());
    }
}

#[test]
fn runes_are_entered_and_removed() {
    let Some(mut g) = game() else { return };
    let mana = g.champions[0].mana();
    if add_rune(&mut g, 0) {
        assert_eq!(crate::magic::runes(&g.champions[0]).len(), 1);
        assert!(g.champions[0].mana() <= mana);
        delete_rune(&mut g);
        assert!(crate::magic::runes(&g.champions[0]).is_empty());
    }
}

#[test]
fn throw_stamina_cost_follows_weight() {
    use super::throw_stamina_cost as c;
    assert_eq!(c(0), 1);
    assert_eq!(c(8), 4);
    assert_eq!(c(20), 10);
    // h = 25: 10 + (15 >> 1) + (5 >> 1) = 19.
    assert_eq!(c(50), 19);
    // h = 60: 10 + 25 + 20 + 15 + 10 + 5 = 85.
    assert_eq!(c(120), 85);
}

#[test]
fn eating_plays_the_eating_sound() {
    let Some(mut g) = game() else { return };
    let data = g.data.clone().unwrap();
    // Some misc item with a food value (misc item numbers are 256-383).
    let food = (256..384u16).find_map(|n| {
        let t = crate::actuators::create_item(&mut g, n)?;
        if data.item_db(&g.dungeon).attr(t, crate::items::ATTR_FOOD) > 0 {
            Some(t)
        } else {
            g.dungeon.free_thing(t);
            None
        }
    });
    let Some(t) = food else { return };
    g.hand.held = t.0 & 0x3FFF;
    g.hand.inventory_open = Some(0);
    g.effects.clear();
    assert!(eat_held(&mut g));
    assert!(g.effects.iter().any(|e| matches!(e,
        crate::effects::Effect::SoundAt { cat: 9, idx: 0x5B, sub: 0xFB, vol: 200, mode: 0, .. })));
}

/// A launcher and an ammunition item from the dungeon's weapons, as 0x408A8
/// pairs them: the launcher's attribute 5 has bit 15, the ammunition's not,
/// and their class bits overlap. Also some weapon that doesn't fit.
fn launcher_and_ammo(g: &GameState) -> Option<(u16, u16, u16)> {
    let data = g.data.clone()?;
    let db = data.item_db(&g.dungeon);
    let n = g.dungeon.thing_count(ThingType::Weapon) as u16;
    let weapon = |i: u16| (ThingType::Weapon as u16) << 10 | i;
    let attr5 = |i: u16| db.attr(ThingRef(weapon(i)), 5);
    let l = (0..n).find(|&i| attr5(i) & 0x8000 != 0)?;
    let a = (0..n).find(|&i| attr5(i) & 0x8000 == 0 && attr5(i) & 0x7FFF & attr5(l) != 0)?;
    let other = (0..n).find(|&i| attr5(i) & 0x8000 == 0 && attr5(i) & 0x7FFF & attr5(l) == 0)?;
    Some((weapon(l), weapon(a), weapon(other)))
}

/// Finish hand `hand`'s action `action` through the busy countdown.
fn finish_action(g: &mut GameState, hand: usize, action: u8) {
    g.champions[0].raw[0x20 + hand] = action;
    g.champions[0].raw[0x2A + hand] = 1;
    champions::count_down_busy(g);
}

#[test]
fn shooting_reloads_the_other_hand_from_slot_12() {
    let Some(mut g) = game() else { return };
    let (launcher, ammo, _) = launcher_and_ammo(&g).expect("the data has a launcher, its ammunition and an unrelated weapon");
    let c = &mut g.champions[0];
    c.set_inventory(0, launcher);
    c.set_inventory(1, EMPTY);
    c.set_inventory(12, ammo);
    finish_action(&mut g, 0, 0x20);
    assert_eq!(g.champions[0].inventory(1), ammo, "ammunition moves into the empty hand");
    assert_eq!(g.champions[0].inventory(12), EMPTY);
    assert_eq!(g.champions[0].raw[0x20], 0xFF, "the action still ends");
}

#[test]
fn shooting_leaves_unsuitable_items_and_full_hands_alone() {
    let Some(mut g) = game() else { return };
    let (launcher, ammo, other) = launcher_and_ammo(&g).expect("the data has a launcher, its ammunition and an unrelated weapon");
    // An item whose class doesn't fit the launcher stays in slot 12.
    let c = &mut g.champions[0];
    c.set_inventory(0, launcher);
    c.set_inventory(1, EMPTY);
    c.set_inventory(12, other);
    for s in 7..10 {
        c.set_inventory(s, EMPTY);
    }
    finish_action(&mut g, 0, 0x20);
    assert_eq!(g.champions[0].inventory(1), EMPTY);
    assert_eq!(g.champions[0].inventory(12), other);
    // With the other hand already full nothing moves.
    let c = &mut g.champions[0];
    c.set_inventory(1, other);
    c.set_inventory(12, ammo);
    finish_action(&mut g, 0, 0x20);
    assert_eq!(g.champions[0].inventory(1), other);
    assert_eq!(g.champions[0].inventory(12), ammo);
    // Ammunition in slots 7-9 is found when slot 12 has none.
    let c = &mut g.champions[0];
    c.set_inventory(1, EMPTY);
    c.set_inventory(12, EMPTY);
    c.set_inventory(8, ammo);
    finish_action(&mut g, 0, 0x20);
    assert_eq!(g.champions[0].inventory(1), ammo);
    assert_eq!(g.champions[0].inventory(8), EMPTY);
}
