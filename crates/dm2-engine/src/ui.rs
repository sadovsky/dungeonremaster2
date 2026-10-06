//! Full-screen composition around the viewport (docs/10-ui-input.md,
//! docs/04-rendering.md). Every image and position comes from the user's
//! GRAPHICS.DAT at runtime: images are drawn at layout ids exactly as the
//! original's draw-at-layout routine (0x1BE6A) does.

use dm2_formats::gdat::Key;

use crate::assets::Assets;
use crate::font::Font;
use crate::gfx::{Bitmap, Sprite, SCREEN_H, SCREEN_W};
use crate::viewport::VP_SCREEN_POS;

/// What the interface shows for one champion.
#[derive(Clone, Debug, Default)]
pub struct ChampionView {
    pub name: Vec<u8>,
    /// Portrait index in category 22.
    pub portrait: u8,
    /// Spell-panel variant (champion record +0x1E), selects rune set.
    pub rune_set: u8,
    pub dead: bool,
    /// Runes entered so far (drawn as font characters).
    pub runes: Vec<u8>,
    /// Health, stamina and mana as (current, maximum).
    pub bars: [(u16, u16); 3],
    /// Damage to show in the starburst (champion +0x2C), if any.
    pub damage: Option<u16>,
    /// Wound bits (champion +0x34): bit n wounds body slot n, which picks
    /// the wounded slot frame and empty picture.
    pub wounds: u16,
}

/// An image in the archive: (category, index, sub-index of type 1).
pub type Icon = (u8, u8, u8);

/// The open inventory panel (0x48890 with the inventory flag).
#[derive(Clone, Debug, Default)]
pub struct InventoryView {
    pub champion: usize,
    /// Icons of the 30 inventory slots.
    pub slots: Vec<Option<Icon>>,
    /// The champion's wound bits (see `ChampionView::wounds`).
    pub wounds: u16,
    /// The champion is the leader: the name is drawn in colour 9, as in
    /// the champion box (0x48DD3), else colour 0xF.
    pub leader: bool,
    /// The 8 cells of an open container, if one is shown.
    pub container: Option<Vec<Option<Icon>>>,
    /// Name (+0x00) and title (+0x08); the panel shows them joined
    /// (0x48890).
    pub name: Vec<u8>,
    pub title: Vec<u8>,
    /// Health, stamina and mana as (current, maximum).
    pub stats: [(u16, u16); 3],
    /// Food and water (+0x44, +0x46), −1024..2048.
    pub food: i16,
    pub water: i16,
    pub poisoned: bool,
    /// Load and maximum load in tenths of a kilogram.
    pub load: (u16, u16),
    /// Name of the held item while the eye is pressed.
    pub info: Option<Vec<u8>>,
}

/// An open action menu (0x43759): the champion and its action names.
#[derive(Clone, Debug, Default)]
pub struct MenuView {
    pub champion: usize,
    /// Hand whose action menu is open (its slot gets the selected frame).
    pub hand: usize,
    pub names: Vec<Vec<u8>>,
}

/// Read-only view of the game the interface needs.
#[derive(Clone, Debug, Default)]
pub struct UiView {
    pub champions: [Option<ChampionView>; 4],
    pub leader: Option<usize>,
    /// Champion whose inventory is open.
    pub inventory_open: Option<usize>,
    /// Alternate arrow set (global 0x7F3A4).
    pub alt_arrows: bool,
    /// Item icons in each champion's two hands (None = empty hand).
    pub hands: [[Option<Icon>; 2]; 4],
    /// Hands still busy after an action (drawn greyed).
    pub busy: [[bool; 2]; 4],
    /// Party cell of each champion relative to the facing (0-3).
    pub cells: [u8; 4],
    pub inventory: Option<InventoryView>,
    pub menu: Option<MenuView>,
    /// Champion whose spell panel is open (0x7FB6E with hand 2 in the
    /// spell state). The panel is not shown otherwise.
    pub magic: Option<usize>,
    /// Item in the leader's hand, drawn as the cursor.
    pub held: Option<Icon>,
    /// Graphics set of the party's map; picks the formation grid's floor
    /// (8, set, 0xF5) drawn by 0x42EDD.
    pub map_set: u8,
    /// Invisibility active (0x7FFEE non-zero): the formation figures use
    /// the alternate half of their sheets.
    pub alt_figures: bool,
    /// Party asleep (0x7F234): hand cells and formation cells are shaded.
    pub asleep: bool,
    /// Hand cell drawn with the highlighted tile (champion, hand).
    pub hand_highlight: Option<(usize, usize)>,
}

/// Layout ids used below.
mod id {
    pub const ARROWS: u16 = 40; // 40-45
    pub const CHAMPION_BOX: u16 = 161; // + champion
    pub const PORTRAIT: u16 = 173; // + champion
    pub const DAMAGE: u16 = 177; // + champion
    pub const NAME: u16 = 165; // + champion
    pub const BARS: u16 = 193; // + champion + 4 * bar
    pub const LEADER_BAR: u16 = 60;
    pub const LEADER_BAR_END: u16 = 59;
    pub const LEADER_NAME: u16 = 61;
    pub const SPELL_PANEL: u16 = 92;
    pub const RUNE_SYMBOLS: u16 = 255; // 255-260
    pub const ENTERED_RUNES: u16 = 261; // 261-
    // Inventory panel (viewport-relative ids).
    pub const INVENTORY: u16 = 4;
    pub const MOUTH: u16 = 545;
    pub const EYE: u16 = 546;
    pub const STATS: u16 = 550; // 550-552
    pub const INV_NAME: u16 = 553;
    pub const LOAD: u16 = 555;
    pub const FOOD_PANEL: u16 = 494;
    pub const FOOD_BAR: u16 = 496;
    pub const WATER_BAR: u16 = 497;
    pub const POISON_BAR: u16 = 499;
    pub const FOOD_LABEL: u16 = 500;
    pub const WATER_LABEL: u16 = 501;
    pub const POISON_LABEL: u16 = 502;
    pub const CONTAINER_CELLS: u16 = 229; // 229-236
    // Action area (screen ids).
    pub const HAND0: u16 = 74; // + champion
    pub const HAND1: u16 = 70; // + champion
    pub const CELL_BACK: u16 = 0x57; // + party cell
    pub const CELL_FRONT: u16 = 0x53; // + party cell
    pub const FORMATION: u16 = 0x2F; // formation grid floor
    pub const FIGURE: u16 = 0x35; // + party cell
    pub const MENU_ROW: u16 = 0x3F; // + row
    pub const MENU_TEXT: u16 = 0x42; // + row
}

/// Number of entries in the slot table (0x75538): 8 portrait hand cells,
/// then the 30 inventory slots.
pub const SLOT_TABLE_LEN: usize = 38;

/// Small tables the interface reads from the user's SKULL.EXE.
#[derive(Clone, Copy, Debug)]
pub struct UiTables {
    /// Index into the UI colour table for each champion's bars (0x759CC).
    pub champion_colour: [u8; 4],
    /// Offset of the bar shadow (0x71724).
    pub shadow: (i32, i32),
    /// Slot table (0x75538, 8-byte records): layout id of each slot and
    /// the sub-index of its empty-slot picture in (7, 0), 0xFF for none.
    pub slots: [(u16, u8); SLOT_TABLE_LEN],
    /// Separator between a champion's name and title in the inventory
    /// (string pointed to by 0x760E0).
    pub name_sep: [u8; 4],
}

impl UiTables {
    pub fn load(exe: &crate::exe::Exe) -> Option<UiTables> {
        let champion_colour = [
            exe.u8_at(0x759CC)?,
            exe.u8_at(0x759CD)?,
            exe.u8_at(0x759CE)?,
            exe.u8_at(0x759CF)?,
        ];
        let shadow = (exe.i16_at(0x71724)? as i32, exe.i16_at(0x71726)? as i32);
        let mut slots = [(0u16, 0xFFu8); SLOT_TABLE_LEN];
        for (k, s) in slots.iter_mut().enumerate() {
            let a = 0x75538 + 8 * k as u32;
            *s = (exe.u16_at(a)?, exe.u8_at(a + 2)?);
        }
        let mut name_sep = [0u8; 4];
        // Unrelocated pointers are offsets from the data object's base.
        let raw = u32::from_le_bytes(exe.slice(0x760E0, 4)?.try_into().ok()?);
        let sep_addr = if raw >= exe.data_base() { raw } else { exe.data_base() + raw };
        for (k, b) in name_sep.iter_mut().enumerate() {
            let c = exe.u8_at(sep_addr + k as u32)?;
            if c == 0 {
                break;
            }
            *b = c;
        }
        Some(UiTables { champion_colour, shadow, slots, name_sep })
    }
}

impl Default for UiTables {
    /// Neutral stand-in when SKULL.EXE is unavailable.
    fn default() -> Self {
        // Slot layout ids follow the zone lists; no empty-slot pictures.
        let slots = std::array::from_fn(|k| (if k < 8 { 209 + k as u16 } else { 507 + (k as u16 - 8) }, 0xFF));
        UiTables { champion_colour: [1, 2, 3, 4], shadow: (1, 1), slots, name_sep: [b' ', 0, 0, 0] }
    }
}

fn fill(s: &mut Bitmap, x: i32, y: i32, w: i32, h: i32, c: u8) {
    for yy in y.max(0)..(y + h).min(s.h as i32) {
        for xx in x.max(0)..(x + w).min(s.w as i32) {
            s.px[yy as usize * s.w + xx as usize] = c;
        }
    }
}

/// Stat bar (0x481DC via 0x19BF2): the parent box scaled vertically by
/// value/max, placed through the bar record's own anchor, over a shadow.
fn bar(a: &Assets, s: &mut Bitmap, rid: u16, cur: u16, max: u16, shadow: (i32, i32), shade: u8, colour: u8) {
    if max == 0 {
        return;
    }
    let Some(rec) = a.layout.get(rid) else { return };
    let Some(par) = a.layout.get(rec.parent as u16) else { return };
    if par.kind != 9 {
        return;
    }
    let frac = (cur.min(max) as i32 * 10000) / max as i32;
    let w = par.x as i32;
    let mut h = par.y as i32 * frac / 10000;
    if h == 0 && frac != 0 {
        h = 1;
    }
    if h <= 0 || w <= 0 {
        return;
    }
    if let Some(p) = a.layout.resolve(rid, w, h, (w, h)) {
        fill(s, p.x + shadow.0, p.y + shadow.1, p.w, p.h, shade);
        fill(s, p.x, p.y, p.w, p.h, colour);
    }
}

/// The UI colour table, resident entry (1,0,13,254): 16 palette indices.
fn colours(a: &Assets) -> [u8; 16] {
    let mut c = [0u8; 16];
    if let Some(e) = a.gdat.get(Key::new(1, 0, 13, 254)) {
        let n = e.len().min(16);
        c[..n].copy_from_slice(&e[..n]);
    }
    c
}

/// Item icons and slot frames use colour key 12 (0x3815D).
const ICON_KEY: u8 = 12;

/// A slot (0x3815D): the slot frame (1, 2, 4), or 5 when its body part is
/// wounded, or 6 when it is the selected hand, for the champion box's hand
/// slots and the first six inventory slots (the original always redraws
/// the hand frames); then the item icon, or the slot's empty picture
/// (7, 0, n), offset by one when wounded.
fn draw_slot(a: &mut Assets, dst: &mut Bitmap, tables: &UiTables, k: usize, icon: Option<Icon>, selected: bool, wounded: bool) {
    let (rid, empty) = tables.slots[k];
    if k < 14 {
        let frame = if selected { 6 } else if wounded { 5 } else { 4 };
        slot_frame(a, dst, rid, frame);
    }
    match icon {
        Some((c, i, sub)) => {
            a.draw(dst, c, i, sub, rid, 0, Some(ICON_KEY));
        }
        None if empty != 0xFF => {
            a.draw(dst, 7, 0, empty + u8::from(wounded), rid, 0, Some(ICON_KEY));
        }
        None => {}
    }
}

/// A slot frame (1, 2, sub): the 18×18 frame is centred on the 16×16 slot
/// box at `rid` and not clipped to it (one pixel up and left of the slot).
fn slot_frame(a: &mut Assets, dst: &mut Bitmap, rid: u16, sub: u8) {
    if let (Some(f), Some(p)) = (a.sprite(1, 2, sub), a.layout.resolve(rid, 16, 16, (16, 16))) {
        let at = crate::layout::Placement {
            x: p.x + (16 - f.w as i32) / 2,
            y: p.y + (16 - f.h as i32) / 2,
            w: f.w as i32,
            h: f.h as i32,
            skip_x: 0,
            skip_y: 0,
        };
        f.blit(dst, &at, 0, Some(ICON_KEY));
    }
}

/// Horizontal bar filling a layout box by value/max (0x398FF).
fn hbar(a: &Assets, dst: &mut Bitmap, rid: u16, value: i16, lo: i32, hi: i32, colour: u8) {
    let Some(rec) = a.layout.get(rid) else { return };
    let Some(par) = a.layout.get(rec.parent as u16) else { return };
    let (w, h) = if par.kind == 9 { (par.x as i32, par.y as i32) } else { return };
    let Some(p) = a.layout.resolve(rid, w, h, (w, h)) else { return };
    let fill_w = ((value as i32 - lo).clamp(0, hi - lo) * w) / (hi - lo).max(1);
    fill(dst, p.x, p.y, fill_w, p.h, colour);
}

/// The inventory panel, drawn over the viewport area (0x48890, 0x39A4D).
pub fn inventory_panel(a: &mut Assets, font: &Font, tables: &UiTables, inv: &InventoryView) -> Bitmap {
    let mut b = Bitmap::new(crate::viewport::VP_W, crate::viewport::VP_H);
    let col = colours(a);
    a.draw(&mut b, 7, 0, 0, id::INVENTORY, 0, None);
    for (s, icon) in inv.slots.iter().enumerate().take(30) {
        draw_slot(a, &mut b, tables, 8 + s, *icon, false, s < 6 && inv.wounds & (1 << s) != 0);
    }
    // Mouth and eye, each in a slot frame.
    slot_frame(a, &mut b, id::MOUTH, 4);
    a.draw(&mut b, 7, 0, 0x25, id::MOUTH, 0, Some(ICON_KEY));
    slot_frame(a, &mut b, id::EYE, 4);
    a.draw(&mut b, 7, 0, 0x20 + u8::from(inv.container.is_some()), id::EYE, 0, Some(ICON_KEY));
    // Name-bar buttons (0x48863): image (7, 0, sub) at each id; a set state
    // bit selects the next sub. Drawn before the name: the first image is
    // the whole bar.
    for (sub, rid) in [(0x11u8, 0x238u16), (0x13, 0x267), (0x0F, 0x232), (0x0D, 0x234), (0x0B, 0x236)] {
        a.draw(&mut b, 7, 0, sub, rid, 0, None);
    }
    // Name and title (0x48890): joined by the separator unless the title
    // starts with ',', ';' or '-', drawn shadowed at 0x229.
    let mut full = inv.name.clone();
    if !inv.title.is_empty() {
        if !matches!(inv.title[0], b',' | b';' | b'-') {
            full.extend(tables.name_sep.iter().copied().take_while(|&c| c != 0));
        }
        full.extend_from_slice(&inv.title);
    }
    let fg = if inv.leader { col[9] } else { col[0xF] };
    font.draw_at_shadowed(&mut b, &a.layout, id::INV_NAME, &full, fg, 0);
    // Health, stamina (in tenths) and mana as "cur/max".
    for (k, &(cur, max)) in inv.stats.iter().enumerate() {
        let (cur, max) = if k == 1 { (cur / 10, max / 10) } else { (cur, max) };
        let t = format!("{cur:>3}/{max:>3}").into_bytes();
        font.draw_at(&mut b, &a.layout, id::STATS + k as u16, &t, col[0xD], None);
    }
    let (load, max) = inv.load;
    let lc = if load > max { 8 } else if load as u32 * 8 > max as u32 * 5 { 0xB } else { 0xD };
    // The load line is text (7,0,0x2a) with its numbers in context codes
    // 12 (kilograms), 13 (tenths) and 14 (maximum, whole kilograms).
    let ctx = crate::font::TextContext {
        slots: [None, None, Some((load / 10) as i32), Some((load % 10) as i32), Some((max / 10) as i32)],
        ..Default::default()
    };
    let t = crate::font::text(&a.gdat, 7, 0, 0x2A, &ctx)
        .unwrap_or_else(|| format!("{}.{}/{}", load / 10, load % 10, max / 10).into_bytes());
    font.draw_at(&mut b, &a.layout, id::LOAD, &t, col[lc], None);
    if let Some(info) = &inv.info {
        a.draw(&mut b, 7, 0, 1, id::FOOD_PANEL, 0, None);
        font.draw_at(&mut b, &a.layout, id::FOOD_LABEL, info, col[0xF], None);
    } else {
        // Food, water and poison bars (0x39A4D).
        a.draw(&mut b, 7, 0, 1, id::FOOD_PANEL, 0, None);
        hbar(a, &mut b, id::FOOD_BAR, inv.food, -1024, 2048, col[5]);
        hbar(a, &mut b, id::WATER_BAR, inv.water, -1024, 2048, col[0xE]);
        a.draw(&mut b, 7, 0, 6, id::FOOD_LABEL, 0, Some(ICON_KEY));
        a.draw(&mut b, 7, 0, 7, id::WATER_LABEL, 0, Some(ICON_KEY));
        if inv.poisoned {
            hbar(a, &mut b, id::POISON_BAR, 1, 0, 1, col[8]);
            a.draw(&mut b, 7, 0, 8, id::POISON_LABEL, 0, Some(ICON_KEY));
        }
    }
    b
}

/// The action area (0x4315D, 0x43759): one panel per champion with its two
/// hand icons, or the open action menu.
fn action_area(a: &mut Assets, font: &Font, view: &UiView, col: &[u8; 16], s: &mut Bitmap) {
    // An open container fills the action area with its 8 cells (list @174).
    if let Some(cont) = view.inventory.as_ref().and_then(|i| i.container.as_ref()) {
        for (k, icon) in cont.iter().enumerate().take(8) {
            let rid = id::CONTAINER_CELLS + k as u16;
            a.draw(s, 1, 2, 4, rid, 0, Some(ICON_KEY));
            if let Some((c, i, sub)) = icon {
                a.draw(s, *c, *i, *sub, rid, 0, Some(ICON_KEY));
            }
        }
        return;
    }
    if let Some(m) = &view.menu {
        for (row, name) in m.names.iter().enumerate().take(3) {
            a.draw(s, 1, 4, 0x15, id::MENU_ROW + row as u16, 0, None);
            font.draw_at(s, &a.layout, id::MENU_TEXT + row as u16, name, col[0xF], None);
        }
        return;
    }
    // Idle action area, in the original's order (0x3FE68): for each
    // champion its two hand cells (0x42DA6) and its formation cell
    // (0x4315D); then the formation grid with the figures (0x42EDD). All
    // positions are by party cell relative to the facing, not by champion.
    for (i, c) in view.champions.iter().enumerate() {
        let Some(c) = c else { continue };
        let rel = (view.cells[i] & 3) as u16;
        for h in 0..2 {
            let rid = if h == 1 { id::HAND1 } else { id::HAND0 } + rel;
            let lit = view.hand_highlight == Some((i, h));
            hand_cell(a, s, col, c.dead, view.hands[i][h], h, rid, lit, view.busy[i][h] || view.asleep);
        }
        if c.dead {
            continue;
        }
        let flip = u8::from(rel == 1 || rel == 2);
        let (back, front) = if rel < 2 { (6, 10) } else { (8, 12) };
        a.draw(s, 1, 4, back, id::CELL_BACK + rel, flip, Some(CELL_KEY));
        if view.asleep || c.damage.is_some() {
            shade_at(a, s, 1, 4, back, id::CELL_BACK + rel, col[0]);
        }
        let lead = u8::from(view.leader == Some(i));
        a.draw(s, 1, 4, front + lead, id::CELL_FRONT + rel, flip, Some(CELL_KEY));
        if view.asleep {
            shade_at(a, s, 1, 4, front + lead, id::CELL_FRONT + rel, col[0]);
        }
    }
    a.draw(s, 8, view.map_set, 0xF5, id::FORMATION, 0, None);
    for (i, c) in view.champions.iter().enumerate() {
        let Some(c) = c else { continue };
        if c.dead {
            continue;
        }
        let rel = (view.cells[i] & 3) as i32;
        let Some(sheet) = a.sprite(1, 6, i as u8) else { continue };
        let x0 = FIGURE * (rel + if view.alt_figures { 4 } else { 0 });
        if let Some(fig) = crop(&sheet, x0, 0, FIGURE, FIGURE) {
            if let Some(p) = a.layout.resolve(id::FIGURE + rel as u16, FIGURE, FIGURE, (FIGURE, FIGURE)) {
                fig.blit(s, &p, 0, Some(ICON_KEY));
            }
        }
    }
}

/// Formation-cell images (1, 4, 6-13) are keyed on nibble 4 (checked
/// against the original: their nibble-4 areas show what lies beneath).
const CELL_KEY: u8 = 4;

/// Size of one party figure in the (1, 6, champion) sheets (0x71726/0x7172A).
const FIGURE: i32 = 17;

/// A w×h piece of a sprite, keeping its colour map.
fn crop(sp: &Sprite, x0: i32, y0: i32, w: i32, h: i32) -> Option<Sprite> {
    if x0 < 0 || y0 < 0 || x0 + w > sp.w as i32 || y0 + h > sp.h as i32 {
        return None;
    }
    let mut px = Vec::with_capacity((w * h) as usize);
    for y in y0..y0 + h {
        let row = y as usize * sp.w;
        px.extend_from_slice(&sp.px[row + x0 as usize..row + (x0 + w) as usize]);
    }
    Some(Sprite { w: w as usize, h: h as usize, px, cmap: sp.cmap, off: (0, 0) })
}

/// Shade the box an image (cat, idx, sub) occupies at `rid` with a
/// checkerboard of colour `c`, like 0x1BDC3 does for busy hands and
/// sleeping or wounded champions.
fn shade_at(a: &mut Assets, s: &mut Bitmap, cat: u8, idx: u8, sub: u8, rid: u16, c: u8) {
    let Some(sp) = a.sprite(cat, idx, sub) else { return };
    let Some(p) = a.layout.resolve(rid, sp.w as i32, sp.h as i32, (sp.w as i32, sp.h as i32)) else { return };
    shade(s, p.x, p.y, p.w, p.h, c);
}

fn shade(s: &mut Bitmap, x: i32, y: i32, w: i32, h: i32, c: u8) {
    for yy in y.max(0)..(y + h).min(s.h as i32) {
        for xx in x.max(0)..(x + w).min(s.w as i32) {
            if (xx + yy) & 1 == 0 {
                s.px[yy as usize * s.w + xx as usize] = c;
            }
        }
    }
}

/// One hand cell of the action area (0x42DA6): the cell tile, then the
/// hand's icon centred on it (0x3844C). An item gets a one-pixel drop
/// shadow in colour 0 (16×16 icon in a 17×17 box); an empty hand shows
/// the bare-hand picture (1, 2, 7 + hand) plainly. A dead champion's cells
/// are cleared.
#[allow(clippy::too_many_arguments)]
fn hand_cell(a: &mut Assets, s: &mut Bitmap, col: &[u8; 16], dead: bool, item: Option<Icon>, hand: usize, rid: u16, lit: bool, shaded: bool) {
    let Some(tile) = a.sprite(1, 4, if lit { 4 } else { 2 }) else { return };
    let Some(p) = a.layout.resolve(rid, tile.w as i32, tile.h as i32, (tile.w as i32, tile.h as i32)) else { return };
    if dead {
        fill(s, p.x, p.y, p.w, p.h, col[0]);
        return;
    }
    tile.blit(s, &p, 0, None);
    // 0x3844C asks 0x37F76 with the "no context" flag, so action-area icons
    // always use the item's base frame: no animation, no equipped variant.
    let (icon, shadow) = match item {
        Some((c, i, _)) => (a.sprite(c, i, crate::items::ICON_BASE), true),
        None => (a.sprite(1, 2, 7 + hand as u8), false),
    };
    if let Some(icon) = icon {
        // Both paths centre a 17×17 box (16×16 icon plus its shadow).
        let (bw, bh) = (icon.w as i32 + 1, icon.h as i32 + 1);
        let x = p.x + ((tile.w as i32 + 1) >> 1) - ((bw + 1) >> 1);
        let y = p.y + ((tile.h as i32 + 1) >> 1) - ((bh + 1) >> 1);
        let at = |x, y| crate::layout::Placement { x, y, w: icon.w as i32, h: icon.h as i32, skip_x: 0, skip_y: 0 };
        if shadow {
            for yy in 0..icon.h {
                for xx in 0..icon.w {
                    if icon.px[yy * icon.w + xx] != ICON_KEY {
                        let (dx, dy) = (x + 1 + xx as i32, y + 1 + yy as i32);
                        if dx >= 0 && dy >= 0 && (dx as usize) < s.w && (dy as usize) < s.h {
                            s.px[dy as usize * s.w + dx as usize] = col[0];
                        }
                    }
                }
            }
        }
        // 0x1AF61: an item icon's colour map goes through the 256-byte remap
        // table (1, 0, 7, 1) before drawing, which gives the action area its
        // dimmed item icons.
        let mut dimmed = Sprite { w: icon.w, h: icon.h, px: icon.px.clone(), cmap: icon.cmap, off: (0, 0) };
        // Only items take this path; the bare-hand picture is drawn plainly.
        if let (true, Some(m), Some(t)) = (shadow, dimmed.cmap.as_mut(), a.gdat.get(Key::new(1, 0, 7, 1))) {
            if t.len() >= 256 {
                for c in m.iter_mut() {
                    *c = t[*c as usize];
                }
            }
        }
        dimmed.blit(s, &at(x, y), 0, Some(ICON_KEY));
    }
    if shaded {
        shade(s, p.x, p.y, p.w, p.h, col[0]);
    }
}

/// Draw the 320×200 game screen: interface plus the given viewport bitmap.
pub fn compose(a: &mut Assets, font: &Font, tables: &UiTables, view: &UiView, vp: &Bitmap) -> Bitmap {
    let mut s = Bitmap::new(SCREEN_W, SCREEN_H);
    match &view.inventory {
        Some(inv) => {
            let panel = inventory_panel(a, font, tables, inv);
            s.paste(&panel, VP_SCREEN_POS.0, VP_SCREEN_POS.1);
        }
        None => s.paste(vp, VP_SCREEN_POS.0, VP_SCREEN_POS.1),
    }
    let col = colours(a);

    // Movement arrows (0x42AE4): six images at ids 40-45.
    let base = if view.alt_arrows { 14 } else { 2 };
    for k in 0..6u8 {
        a.draw(&mut s, 1, 3, base + 2 * k, id::ARROWS + k as u16, 0, None);
    }
    // Opening an inventory (0x3A464) shades the arrows panel, layout id 9
    // sized as its rectangle 8, with colour 0 (0x13B7C).
    if view.inventory.is_some() {
        if let Some(r) = a.layout.get(8) {
            let (w, h) = (r.x as i32, r.y as i32);
            if let Some(p) = a.layout.resolve(9, w, h, (w, h)) {
                shade(&mut s, p.x, p.y, p.w, p.h, col[0]);
            }
        }
    }

    // Champion boxes along the top (0x48140, 0x487F9, 0x48733).
    for (i, c) in view.champions.iter().enumerate() {
        let Some(c) = c else { continue };
        let n = i as u16;
        let state = if c.dead {
            1
        } else if view.inventory_open == Some(i) {
            9
        } else {
            0
        };
        a.draw(&mut s, 1, 2, state, id::CHAMPION_BOX + n, 0, None);
        if c.dead {
            font.draw_at(&mut s, &a.layout, id::NAME + n, &c.name, col[0xF], None);
            continue;
        }
        // 0x48890: the portrait replaces name and hands only while this
        // champion's inventory is open (0x487F9); otherwise the box shows
        // the name (0x1C0BC at 0xA5 + n) and the two hand slots (0x484B0).
        if view.inventory_open == Some(i) {
            a.draw(&mut s, 22, c.portrait, 0, id::PORTRAIT + n, 0, None);
        } else {
            // Leader's name in colour 9, others in 0xF, on colour 1 (0x48DD3).
            let fg = if view.leader == Some(i) { col[9] } else { col[0xF] };
            font.draw_at_shadowed(&mut s, &a.layout, id::NAME + n, &c.name, fg, 0);
            for h in 0..2 {
                let sel = view.menu.as_ref().is_some_and(|m| m.champion == i && m.hand == h);
                draw_slot(a, &mut s, tables, i * 2 + h, view.hands[i][h], sel, c.wounds & (1 << h) != 0);
            }
        }
        let colour = col[(tables.champion_colour[i] & 15) as usize];
        for (k, &(cur, max)) in c.bars.iter().enumerate() {
            bar(a, &mut s, id::BARS + n + 4 * k as u16, cur, max, tables.shadow, col[0], colour);
        }
        if let Some(dmg) = c.damage {
            a.draw(&mut s, 1, 2, 3, id::DAMAGE + n, 0, Some(10));
            let txt = dmg.to_string().into_bytes();
            font.draw_at(&mut s, &a.layout, id::DAMAGE + n, &txt, col[0xF], Some(col[8]));
        }
    }

    // Right panel (0x3FE68): the selected champion's name bar (0x43332)
    // with either its action menu or its spell panel; the idle action area
    // when nobody is selected.
    let selected = view.menu.as_ref().map(|m| m.champion).or(view.magic);
    if let Some(sel) = selected {
        if let Some(c) = &view.champions[sel] {
            a.draw(&mut s, 1, 4, 0x14, id::LEADER_BAR, 0, None);
            a.draw(&mut s, 1, 4, 0x0E, id::LEADER_BAR_END, 0, None);
            let fg = if view.leader == Some(sel) { col[9] } else { col[0xF] };
            font.draw_at(&mut s, &a.layout, id::LEADER_NAME, &c.name, fg, None);
            if view.menu.is_none() {
                a.draw(&mut s, 1, 5, c.rune_set + 1, id::SPELL_PANEL, 0, None);
                if c.rune_set < 4 {
                    for k in 0..6u8 {
                        let glyph = [b'`' + c.rune_set * 6 + k];
                        font.draw_at(&mut s, &a.layout, id::RUNE_SYMBOLS + k as u16, &glyph, col[0], None);
                    }
                }
                for (k, &r) in c.runes.iter().enumerate() {
                    font.draw_at(&mut s, &a.layout, id::ENTERED_RUNES + k as u16, &[r], col[0], None);
                }
                return s;
            }
        }
    }
    action_area(a, font, view, &col, &mut s);
    s
}

/// Draw the cursor at screen (x, y): the held item's icon centred on the
/// pointer, or nothing (the frontend draws the system pointer).
pub fn draw_cursor(a: &mut Assets, s: &mut Bitmap, held: Option<Icon>, x: i32, y: i32) {
    let Some((c, i, sub)) = held else { return };
    let Some(sp) = a.sprite(c, i, sub) else { return };
    let p = crate::layout::Placement {
        x: x - sp.w as i32 / 2,
        y: y - sp.h as i32 / 2,
        w: sp.w as i32,
        h: sp.h as i32,
        skip_x: 0,
        skip_y: 0,
    };
    sp.blit(s, &p, 0, Some(ICON_KEY));
}

/// Title screen image (category 5), drawn full-screen.
pub fn title(a: &mut Assets, frame: u8) -> Bitmap {
    let mut s = Bitmap::new(SCREEN_W, SCREEN_H);
    if let Some(sp) = a.sprite(5, 0, frame) {
        let p = crate::layout::Placement { x: 0, y: 0, w: sp.w as i32, h: sp.h as i32, skip_x: 0, skip_y: 0 };
        sp.blit(&mut s, &p, 0, None);
    }
    s
}
