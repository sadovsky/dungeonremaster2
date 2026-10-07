//! The leader's hand and the panels it works through (docs/10-ui-input.md,
//! docs/09-items.md): inventory slots and containers, eating, taking,
//! dropping and throwing items in the viewport, wall clicks, the action
//! menus and spell entry.
//!
//! The frontend turns clicks into `Command::Ui` (dispatcher numbers) and
//! `Command::Viewport` (a region of the 3D view); `dispatch` and
//! `viewport_click` below are the dispatcher (0x21D6C) for those.

use dm2_formats::dungeon::{ThingRef, ThingType};

use crate::champions::{self, EMPTY, INVENTORY_SLOTS};
use crate::combat::{self, ActionContext, ActionSpec};
use crate::creatures;
use crate::effects::Effect;
use crate::magic::{self, CastResult};
use crate::movement;
use crate::state::GameState;
use crate::viewport::{DX, DY};

/// Slots 30-37 are the cells of an open container.
pub const CONTAINER_FIRST: usize = 30;
pub const CONTAINER_CELLS: usize = 8;
/// The inventory's action hand: a container placed there is shown open.
const CONTAINER_HAND: usize = 1;
/// Skill used for throwing (docs/06 numbering).
const SKILL_THROW: usize = 10;

/// Interface state that belongs to the game rather than the screen.
#[derive(Clone, Debug)]
pub struct HandState {
    /// Item in the leader's hand, drawn as the cursor (0x7FBB4); EMPTY when none.
    pub held: u16,
    /// Champion whose inventory is open (0x7F970 − 1).
    pub inventory_open: Option<usize>,
    /// Open action menu.
    pub menu: Option<ActionMenu>,
    /// Champion whose spell panel is open (0x7FB6E in the spell state).
    pub magic: Option<usize>,
    /// Tick until which each champion's hand is busy after an action.
    pub busy_until: [[u32; 2]; 4],
    /// The eye was clicked: show the held item's details (0x3A409).
    pub show_info: bool,
    /// Last hand cell selected in the action area (champion, hand), drawn
    /// with the highlighted tile. Presentation only: the original sets
    /// 0x7FB50/0x7FB4C when a hand or party cell is clicked and never
    /// redraws the cell until its contents change, so the highlight stays.
    pub highlight: Option<(usize, usize)>,
}

impl Default for HandState {
    fn default() -> Self {
        HandState { held: EMPTY, inventory_open: None, menu: None, magic: None, busy_until: [[0; 2]; 4], show_info: false, highlight: None }
    }
}

/// A champion's action menu: up to three actions of the item in a hand
/// (text subs 8-10 of its category and index).
#[derive(Clone, Debug)]
pub struct ActionMenu {
    pub champion: usize,
    pub hand: usize,
    pub actions: Vec<ActionSpec>,
}

/// Regions of the 3D view a click can land in (layout ids 0x2F8-0x2FE,
/// docs/10 "Viewport clicks").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewRegion {
    NearLeft,
    NearRight,
    AheadLeft,
    AheadRight,
    WallLeft,
    WallRight,
    /// Anywhere else; `right` tells which half of the view.
    Elsewhere { right: bool },
}

impl ViewRegion {
    pub const LAYOUT_IDS: [(u16, ViewRegion); 6] = [
        (0x2F8, ViewRegion::NearLeft),
        (0x2F9, ViewRegion::NearRight),
        (0x2FA, ViewRegion::AheadRight),
        (0x2FB, ViewRegion::AheadLeft),
        (0x2FD, ViewRegion::WallLeft),
        (0x2FE, ViewRegion::WallRight),
    ];
}

fn held(g: &GameState) -> Option<ThingRef> {
    (g.hand.held != EMPTY).then_some(ThingRef(g.hand.held))
}

fn refresh_load(g: &mut GameState, idx: usize) {
    let Some(data) = g.data.clone() else { return };
    let db = data.item_db(&g.dungeon);
    if let Some(c) = g.champions.get_mut(idx) {
        champions::recompute_load(c, &db);
    }
}

fn leader(g: &GameState) -> Option<usize> {
    g.leader.filter(|&l| g.champions.get(l).is_some_and(|c| c.is_alive()))
}

/// Category and index of a thing's images and attributes.
pub fn item_key(g: &GameState, t: ThingRef) -> Option<(u8, u8)> {
    let data = g.data.as_ref()?;
    data.item_db(&g.dungeon).key(t)
}

// ---------------------------------------------------------------------------
// Containers

/// The container shown in the open inventory: a container in the open
/// champion's action hand.
pub fn open_container(g: &GameState) -> Option<ThingRef> {
    let c = g.champions.get(g.hand.inventory_open?)?;
    let t = c.inventory(CONTAINER_HAND);
    (t != EMPTY && ThingRef(t).kind() == ThingType::Container).then_some(ThingRef(t))
}

/// Things inside a container (record word 1 starts the list), up to 8.
pub fn container_contents(g: &GameState, cont: ThingRef) -> Vec<ThingRef> {
    let mut out = Vec::new();
    let mut t = ThingRef(g.dungeon.record_word(cont, 1).unwrap_or(0xFFFE));
    while t.is_thing() && out.len() < 64 {
        out.push(t);
        t = ThingRef(g.dungeon.record_word(t, 0).unwrap_or(0xFFFE));
    }
    out
}

fn set_container_contents(g: &mut GameState, cont: ThingRef, items: &[ThingRef]) {
    let mut next = ThingRef::END.0;
    for &t in items.iter().rev() {
        g.dungeon.set_record_word(t, 0, next);
        next = t.0 & 0x3FFF;
    }
    g.dungeon.set_record_word(cont, 1, next);
}

/// Item in an inventory or container slot (EMPTY when none).
pub fn slot_item(g: &GameState, champion: usize, slot: usize) -> u16 {
    if slot < INVENTORY_SLOTS {
        return g.champions.get(champion).map_or(EMPTY, |c| c.inventory(slot));
    }
    let Some(cont) = open_container(g) else { return EMPTY };
    container_contents(g, cont).get(slot - CONTAINER_FIRST).map_or(EMPTY, |t| t.0 & 0x3FFF)
}

/// May the held item go in `slot`? Containers take anything that isn't
/// marked too large (attribute 4 bit 15) and isn't a container itself.
fn fits(g: &GameState, t: ThingRef, slot: usize) -> bool {
    if slot < INVENTORY_SLOTS {
        return crate::party::slot_fits(g, t, slot);
    }
    let Some(data) = g.data.as_ref() else { return false };
    let big = data.item_db(&g.dungeon).attr(t, crate::items::ATTR_SLOTS) & 0x8000 != 0;
    !big && t.kind() != ThingType::Container
}

/// Click on an inventory, hand or container slot (0x46029): swap the held
/// item with the slot's contents when the held item may go there.
pub fn click_slot(g: &mut GameState, champion: usize, slot: usize) -> bool {
    if champion >= g.champions.len() || !g.champions[champion].is_alive() {
        return false;
    }
    if let Some(t) = held(g) {
        if !fits(g, t, slot) {
            return false;
        }
    }
    let here = slot_item(g, champion, slot);
    let incoming = g.hand.held;
    if slot < INVENTORY_SLOTS {
        if here == EMPTY && incoming == EMPTY {
            return false;
        }
        g.champions[champion].set_inventory(slot, incoming);
    } else {
        let Some(cont) = open_container(g) else { return false };
        let mut items = container_contents(g, cont);
        let i = slot - CONTAINER_FIRST;
        match (i < items.len(), incoming != EMPTY) {
            (true, true) => items[i] = ThingRef(incoming),
            (true, false) => {
                items.remove(i);
            }
            (false, true) if items.len() < CONTAINER_CELLS => items.push(ThingRef(incoming)),
            _ => return false,
        }
        set_container_contents(g, cont, &items);
    }
    g.hand.held = here;
    g.hand.show_info = false;
    refresh_load(g, champion);
    if let Some(l) = leader(g) {
        refresh_load(g, l);
    }
    true
}

// ---------------------------------------------------------------------------
// Inventory panel commands

/// Commands 7-10 and 0x0B (0x3A464): open champion n's inventory, or close it.
pub fn toggle_inventory(g: &mut GameState, champion: Option<usize>) {
    match champion {
        Some(c) if g.hand.inventory_open != Some(c) => {
            if g.champions.get(c).is_some_and(|ch| ch.is_alive()) {
                g.hand.inventory_open = Some(c);
                g.hand.menu = None;
            }
        }
        _ => g.hand.inventory_open = None,
    }
    g.hand.show_info = false;
}

/// Command 0x46 (0x39C3F): feed the held item to the open champion.
pub fn eat_held(g: &mut GameState) -> bool {
    let (Some(idx), Some(t)) = (g.hand.inventory_open, held(g)) else { return false };
    let Some(data) = g.data.clone() else { return false };
    let db = data.item_db(&g.dungeon);
    let food = db.attr(t, crate::items::ATTR_FOOD) as i16;
    if t.kind() == ThingType::Potion {
        let w1 = g.dungeon.record_word(t, 1).unwrap_or(0);
        let (kind, power) = ((w1 >> 8) & 0x7F, (w1 & 0xFF) as i16);
        if !crate::potions::drink(g, idx, kind, power as u16) {
            return false;
        }
        // The potion becomes an empty flask (misc kind 0x14; item numbers
        // 256-383 are misc items).
        g.dungeon.free_thing(t);
        let flask = crate::actuators::create_item(g, 256 + 0x14);
        g.hand.held = flask.map_or(EMPTY, |f| f.0 & 0x3FFF);
    } else if food > 0 {
        champions::eat(&mut g.champions[idx], food);
        g.dungeon.free_thing(t);
        g.hand.held = EMPTY;
    } else {
        return false;
    }
    // Eating and drinking play (9, 0x5B, 0xFB) at volume 200 on the
    // interface queue (mode 0), as the original's eat routine does.
    let p = g.party;
    g.effects.push(crate::effects::Effect::SoundAt { cat: 9, idx: 0x5B, sub: 0xFB, map: p.map, x: p.x, y: p.y, vol: 200, mode: 0 });
    refresh_load(g, idx);
    true
}

// ---------------------------------------------------------------------------
// The viewport (0x22A68)

fn square_ahead(g: &GameState) -> (usize, i32, i32) {
    let p = g.party;
    (p.map, p.x + DX[p.dir as usize], p.y + DY[p.dir as usize])
}

/// Cell of a region: the party square's front cells, or the near cells of
/// the square ahead.
fn region_cell(dir: u8, r: ViewRegion) -> u8 {
    match r {
        ViewRegion::NearLeft => dir,
        ViewRegion::NearRight => (dir + 1) & 3,
        ViewRegion::AheadLeft => (dir + 3) & 3,
        ViewRegion::AheadRight => (dir + 2) & 3,
        _ => dir,
    }
}

fn is_item(t: ThingRef) -> bool {
    matches!(
        t.kind(),
        ThingType::Weapon | ThingType::Clothing | ThingType::Scroll | ThingType::Potion | ThingType::Container | ThingType::Misc
    )
}

fn ahead_open(g: &GameState) -> bool {
    let (m, x, y) = square_ahead(g);
    !crate::world::blocks(&g.dungeon, m, x, y) && creatures::group_at(g, m, x, y).is_none()
}

/// Handle a click that landed on a drawn thing (the viewport hit table,
/// 0x22A68): with an empty hand, take exactly the floor or alcove item
/// that was clicked. Returns false when the hit does not apply, so the
/// caller can fall back to `viewport_click`.
pub fn viewport_hit(g: &mut GameState, hit: &crate::viewport::hits::Hit) -> bool {
    use crate::viewport::hits::HitKind;
    let (Some(t), None) = (hit.thing, held(g)) else { return false };
    let (m, x, y) = match (hit.kind, hit.cell) {
        (HitKind::FloorItem, 0) => (g.party.map, g.party.x, g.party.y),
        (HitKind::FloorItem, 3) | (HitKind::AlcoveItem, _) => square_ahead(g),
        _ => return false,
    };
    let Some(t) = g.dungeon.things_at(m, x, y).into_iter().find(|s| s.0 & 0x3FFF == t & 0x3FFF && is_item(*s)) else {
        return false;
    };
    movement::move_thing(g, t, Some((m, x, y)), None);
    g.hand.held = t.0 & 0x3FFF;
    if let Some(l) = leader(g) {
        refresh_load(g, l);
    }
    true
}

/// Handle a click in the 3D view. Returns true if something happened.
pub fn viewport_click(g: &mut GameState, r: ViewRegion) -> bool {
    let p = g.party;
    let near = matches!(r, ViewRegion::NearLeft | ViewRegion::NearRight);
    let ahead = matches!(r, ViewRegion::AheadLeft | ViewRegion::AheadRight);
    let (am, ax, ay) = square_ahead(g);
    match (held(g), r) {
        // Take the top item of a floor cell.
        (None, _) if near || (ahead && ahead_open(g)) => {
            let (m, x, y) = if near { (p.map, p.x, p.y) } else { (am, ax, ay) };
            let cell = region_cell(p.dir, r);
            let Some(t) = g.dungeon.things_at(m, x, y).into_iter().rev().find(|t| t.cell() == cell && is_item(*t)) else {
                return false;
            };
            movement::move_thing(g, t, Some((m, x, y)), None);
            g.hand.held = t.0 & 0x3FFF;
            if let Some(l) = leader(g) {
                refresh_load(g, l);
            }
            true
        }
        // Wall in front: buttons and other wall sensors, by hand or with an item.
        (item, ViewRegion::WallLeft | ViewRegion::WallRight) => {
            let face = (p.dir + 2) & 3;
            let res = crate::actuators::click_wall(g, am, ax, ay, face, item);
            if res.consume_item {
                if let Some(t) = item {
                    g.dungeon.free_thing(t);
                    g.hand.held = EMPTY;
                }
            }
            if res.stored {
                g.hand.held = EMPTY;
            }
            if let Some(t) = res.take {
                g.hand.held = t.0 & 0x3FFF;
            }
            // Pressing with an empty hand clicks (3, 0, 0x88) at the party's
            // square (0x22A68). Tentative: the original keys this on flag
            // 0x40 of the square ahead, which isn't traced; the remake plays
            // it when the press fires something.
            if item.is_none() && res.fired {
                g.effects.push(Effect::Sound { cat: 3, idx: 0, sub: 0x88, map: p.map, x: p.x, y: p.y });
            }
            res.fired
        }
        (Some(t), _) if near || (ahead && ahead_open(g)) => {
            let (m, x, y) = if near { (p.map, p.x, p.y) } else { (am, ax, ay) };
            let cell = region_cell(p.dir, r);
            g.hand.held = EMPTY;
            movement::move_thing(g, ThingRef((t.0 & 0x3FFF) | (cell as u16) << 14), None, Some((m, x, y)));
            crate::party::item_dropped(g, m, x, y, t);
            if let Some(l) = leader(g) {
                refresh_load(g, l);
            }
            true
        }
        (Some(_), ViewRegion::Elsewhere { right }) => throw_held(g, right),
        (Some(_), _) => throw_held(g, matches!(r, ViewRegion::AheadRight)),
        _ => false,
    }
}

/// Throw the held item into the view (0x22942 → 0x478A1), from the left
/// or right half of the party square.
/// Stamina a throw costs for an item of `weight` (0x4663A): with h = w/2,
/// clamp(h, 1, 10), plus half of every positive h − 10k.
pub fn throw_stamina_cost(weight: i32) -> i16 {
    let mut h = weight >> 1;
    let mut cost = h.clamp(1, 10);
    loop {
        h -= 10;
        if h <= 0 {
            break;
        }
        cost += h >> 1;
    }
    cost as i16
}

pub fn throw_held(g: &mut GameState, right: bool) -> bool {
    let (Some(l), Some(t)) = (leader(g), held(g)) else { return false };
    let Some(data) = g.data.clone() else { return false };
    let db = data.item_db(&g.dungeon);
    let p = g.party;
    let mut s = combat::strength(&g.champions[l], &g.party_status, 1, SKILL_THROW as i32, &db, &mut g.rng) as i32;
    // Experience: 8, or 12 + attribute 9 / 4 for throwing weapons.
    let a9 = db.attr(t, crate::items::ATTR_DAMAGE);
    let xp = if a9 != 0 { (a9 >> 2) as u32 + 12 } else { 8 };
    let mult = champions::map_experience_multiplier(&g.dungeon, p.map);
    champions::add_experience(&mut g.champions, &mut g.party_status, l, SKILL_THROW, xp, g.tick, mult, &mut g.rng);
    let level = champions::level(&g.champions[l], &g.party_status, SKILL_THROW, true) as i32;
    s += g.rng.random(((s >> 2) + 8).max(1) as u16) as i32 + level;
    let energy = s.clamp(0, 255) as u8;
    let attack = ((g.rng.rnd() & 31) as i32 + level * 8).clamp(40, 200) as u8;
    let a12 = db.attr(t, 0x0C);
    let step = if a12 != 0 { a12 as u8 } else { (11 - level).max(5) as u8 };
    let cost = throw_stamina_cost(db.weight(t) as i32);
    champions::stamina_loss(&mut g.champions, &mut g.party_status, l, cost);
    g.hand.held = EMPTY;
    let cell = (p.dir + u8::from(right)) & 3;
    crate::missiles::launch(g, t.0 & 0x3FFF, p.map, p.x, p.y, cell, p.dir, energy, attack, step, false);
    refresh_load(g, l);
    true
}

// ---------------------------------------------------------------------------
// Actions (0x3FD77, 0x40DEC, 0x414A5)

/// (category, index) whose action strings a hand uses: the item, or the
/// champion's bare hand (category 22, portrait).
fn action_key(g: &GameState, champion: usize, hand: usize) -> Option<(u8, u8)> {
    let c = g.champions.get(champion)?;
    let t = c.inventory(hand);
    if t == EMPTY {
        Some((22, c.portrait()))
    } else {
        item_key(g, ThingRef(t))
    }
}

pub fn hand_busy(g: &GameState, champion: usize, hand: usize) -> bool {
    champion < 4 && g.tick < g.hand.busy_until[champion][hand.min(1)]
}

/// 0x3FC6D for the bare-hand command 0x11: the action hand needs an item
/// in slot 12, or in one of slots 7-9 (approximated the same way for the
/// ready hand).
fn has_something_to_use(g: &GameState, champion: usize, hand: usize) -> bool {
    let c = &g.champions[champion];
    (hand == 1 && c.inventory(12) != EMPTY) || (7..10).any(|s| c.inventory(s) != EMPTY)
}

/// Commands 0x74-0x7B: open the action menu for a champion's hand.
pub fn open_menu(g: &mut GameState, champion: usize, hand: usize) -> bool {
    if champion >= g.champions.len() || !g.champions[champion].is_alive() || hand_busy(g, champion, hand) {
        return false;
    }
    let Some(data) = g.data.clone() else { return false };
    let Some((cat, idx)) = action_key(g, champion, hand) else { return false };
    // 0x3F9F5: try action strings 8-11 and keep the first three that have
    // a command (CM), suit this hand (WH is 0 or hand + 1), and whose skill
    // level (SK) reaches the required level (LV). A bare hand's command
    // 0x11 also needs something to use (0x3FC6D).
    // A held item also needs enough charges (code 8, 0x1F606): 0x12 means
    // "only when empty"; 0x10 and 0x11 count as 1; any other non-zero
    // value is the number of charges the action needs.
    // TODO(0x3F927): containers whose word 2 has (bits 1-2) == 2 limit
    // commands 0x2C-0x30 by their subtype and contents.
    let held = g.champions[champion].inventory(hand);
    let bare = held == EMPTY;
    let charges = if bare { 0 } else { data.item_db(&g.dungeon).charges(ThingRef(held)) as i16 };
    let code = |a: &ActionSpec, slot: usize| a.codes.get(slot).copied().unwrap_or(0);
    let mut actions: Vec<ActionSpec> = Vec::new();
    for n in 0..4u8 {
        if actions.len() >= 3 {
            break;
        }
        let Some(a) = combat::action_spec(&data.gdat, &data.tables, cat, idx, n) else { continue };
        let cm = code(&a, 2);
        let wh = code(&a, 0x11);
        if a.name.is_empty() || cm == 0 || (wh != 0 && wh - 1 != hand as i16) {
            continue;
        }
        if bare && cm == 0x11 && !has_something_to_use(g, champion, hand) {
            continue;
        }
        if !bare {
            match code(&a, 8) {
                0x12 if charges != 0 => continue,
                0x12 | 0 => {}
                n => {
                    let need = if n == 0x10 || n == 0x11 { 1 } else { n };
                    if charges < need {
                        continue;
                    }
                }
            }
        }
        let level = champions::level(&g.champions[champion], &g.party_status, code(&a, 0).max(0) as usize, true);
        if (code(&a, 1) as i32) <= level as i32 {
            actions.push(a);
        }
    }
    if actions.is_empty() {
        return false;
    }
    g.hand.highlight = Some((champion, hand));
    g.hand.menu = Some(ActionMenu { champion, hand, actions });
    g.hand.magic = None;
    true
}

/// Commands 0x71-0x73: run action `n` of the open menu.
pub fn choose_action(g: &mut GameState, n: usize) -> bool {
    let Some(menu) = g.hand.menu.take() else { return false };
    let Some(spec) = menu.actions.get(n).cloned() else { return false };
    let Some(data) = g.data.clone() else { return false };
    let db = data.item_db(&g.dungeon);
    let (am, ax, ay) = square_ahead(g);
    let group = creatures::group_at(g, am, ax, ay);
    let target = group.and_then(|c| creatures::defence(g, c));
    let ctx = ActionContext {
        db: &db,
        tables: &data.tables,
        tick: g.tick,
        map_multiplier: champions::map_experience_multiplier(&g.dungeon, g.party.map),
        light_term: 0,
        target,
        target_untouchable: group.is_some_and(|c| creatures::is_non_material(g, c)),
    };
    let r = combat::do_action(&mut g.champions, &mut g.party_status, menu.champion, menu.hand, &spec, &ctx, &mut g.rng);
    if menu.champion < 4 {
        g.hand.busy_until[menu.champion][menu.hand.min(1)] = g.tick + r.busy as u32;
    }
    if let (Some(sub), Some((cat, idx))) = (r.sound, action_key(g, menu.champion, menu.hand)) {
        let p = g.party;
        g.effects.push(Effect::Sound { cat, idx, sub: sub as u8, map: p.map, x: p.x, y: p.y });
    }
    crate::apply::apply_action(g, menu.champion, &r.effects);
    true
}

// ---------------------------------------------------------------------------
// Spells (0x42924, 0x429F5, 0x428A2)

pub fn add_rune(g: &mut GameState, column: usize) -> bool {
    let (Some(l), Some(data)) = (leader(g), g.data.clone()) else { return false };
    magic::enter_rune(&mut g.champions[l], column, &data.tables)
}

pub fn delete_rune(g: &mut GameState) {
    if let Some(l) = leader(g) {
        magic::remove_rune(&mut g.champions[l]);
    }
}

pub fn cast(g: &mut GameState) -> Option<CastResult> {
    let (l, data) = (leader(g)?, g.data.clone()?);
    if magic::runes(&g.champions[l]).is_empty() {
        return None;
    }
    let has_flask = (0..2).any(|h| {
        let t = g.champions[l].inventory(h);
        t != EMPTY && item_key(g, ThingRef(t)) == Some((21, 0x14))
    });
    let mult = champions::map_experience_multiplier(&g.dungeon, g.party.map);
    let r = magic::cast(&mut g.champions, &mut g.party_status, l, &data.tables, has_flask, g.tick, mult, &mut g.rng);
    if let CastResult::Success { effects, .. } = &r {
        crate::apply::apply_cast(g, l, effects.clone());
    }
    Some(r)
}

// ---------------------------------------------------------------------------
// Dispatcher (0x21D6C) for the interface commands handled here

/// Run an interface command number. Returns true if it was handled.
pub fn dispatch(g: &mut GameState, cmd: u16) -> bool {
    match cmd {
        0x07..=0x0A => toggle_inventory(g, Some((cmd - 0x07) as usize)),
        0x0B => toggle_inventory(g, None),
        0x14..=0x1B => {
            let n = (cmd - 0x14) as usize;
            return click_slot(g, n / 2, n % 2);
        }
        0x1C..=0x39 | 0x3A..=0x41 => {
            let Some(c) = g.hand.inventory_open else { return false };
            return click_slot(g, c, (cmd - 0x1C) as usize);
        }
        0x46 => return eat_held(g),
        // The eye: item details when holding something, else the
        // champion's skills and stats (0x3A409 / 0x3A12A).
        0x47 => g.hand.show_info = true,
        0x5F..=0x62 => {
            // Party cell (0x458F4 then 0x3FE03): select the champion standing
            // there; the original then shows that champion's action menu.
            let cell = ((cmd - 0x5F) as u8 + g.party.dir) & 3;
            let Some(i) = g.champions.iter().position(|c| c.is_alive() && c.cell() & 3 == cell) else { return false };
            return open_menu(g, i, 1) || open_menu(g, i, 0);
        }
        0x65..=0x6A => return add_rune(g, (cmd - 0x65) as usize),
        0x6B => delete_rune(g),
        0x6C => return cast(g).is_some(),
        0x70 => {
            // 0x3FD17: deselect.
            g.hand.menu = None;
            g.hand.magic = None;
        }
        0x71..=0x73 => return choose_action(g, (cmd - 0x71) as usize),
        0x74..=0x7B => {
            let n = (cmd - 0x74) as usize;
            return open_menu(g, n / 2, n % 2);
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests;
