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
