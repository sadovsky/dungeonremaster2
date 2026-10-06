//! Full-screen composition around the viewport (docs/10-ui-input.md,
//! docs/04-rendering.md). Every image and position comes from the user's
//! GRAPHICS.DAT at runtime: images are drawn at layout ids exactly as the
//! original's draw-at-layout routine (0x1BE6A) does.

use dm2_formats::gdat::Key;

use crate::assets::Assets;
use crate::font::Font;
use crate::gfx::{Bitmap, SCREEN_H, SCREEN_W};
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
}

/// Small tables the interface reads from the user's SKULL.EXE.
#[derive(Clone, Copy, Debug)]
pub struct UiTables {
    /// Index into the UI colour table for each champion's bars (0x759CC).
    pub champion_colour: [u8; 4],
    /// Offset of the bar shadow (0x71724).
    pub shadow: (i32, i32),
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
        Some(UiTables { champion_colour, shadow })
    }
}

impl Default for UiTables {
    /// Neutral stand-in when SKULL.EXE is unavailable.
    fn default() -> Self {
        UiTables { champion_colour: [1, 2, 3, 4], shadow: (1, 1) }
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

/// Draw the 320×200 game screen: interface plus the given viewport bitmap.
pub fn compose(a: &mut Assets, font: &Font, tables: &UiTables, view: &UiView, vp: &Bitmap) -> Bitmap {
    let mut s = Bitmap::new(SCREEN_W, SCREEN_H);
    s.paste(vp, VP_SCREEN_POS.0, VP_SCREEN_POS.1);
    let col = colours(a);

    // Movement arrows (0x42AE4): six images at ids 40-45.
    let base = if view.alt_arrows { 14 } else { 2 };
    for k in 0..6u8 {
        a.draw(&mut s, 1, 3, base + 2 * k, id::ARROWS + k as u16, 0, None);
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
        a.draw(&mut s, 22, c.portrait, 0, id::PORTRAIT + n, 0, None);
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

    // Leader bar and spell panel (0x43332, 0x43686, 0x435D3).
    if let Some(l) = view.leader {
        if let Some(c) = &view.champions[l] {
            a.draw(&mut s, 1, 4, 0x14, id::LEADER_BAR, 0, None);
            a.draw(&mut s, 1, 4, 0x0E, id::LEADER_BAR_END, 0, None);
            font.draw_at(&mut s, &a.layout, id::LEADER_NAME, &c.name, col[9], None);
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
        }
    }
    s
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
