//! Screen layout table, GRAPHICS.DAT key (1,0,4,0).
//!
//! Port of the resolver documented in docs/04-rendering.md (SKULL.EXE
//! 0x1936F); `tools/layout.py` is the reference implementation.

use std::collections::HashMap;

use dm2_formats::gdat::{Gdat, Key};

const MAGIC: u16 = 0xFC0D;
const RECT: i16 = 9;
const TRANSLATE: i16 = 1;

#[derive(Clone, Copy, Debug)]
pub struct Rec {
    pub kind: i16,
    pub parent: i16,
    pub x: i16,
    pub y: i16,
}

/// Visible destination box plus how much of the source was clipped off the
/// left and top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub skip_x: i32,
    pub skip_y: i32,
}

pub struct Layout {
    recs: HashMap<u16, Rec>,
}

fn anchor_offset(anchor: i16, w: i32, h: i32) -> (i32, i32) {
    let (hw, hh) = ((w + 1) >> 1, (h + 1) >> 1);
    let dx = match anchor {
        0 | 5 | 7 => hw,
        2 | 3 | 6 => w - 1,
        _ => 0,
    };
    let dy = match anchor {
        0 | 6 | 8 => hh,
        3 | 4 | 7 => h - 1,
        _ => 0,
    };
    (dx, dy)
}

/// Top-left of a w×h box whose anchor point is at (px, py).
fn box_origin(anchor: i16, px: i32, py: i32, w: i32, h: i32) -> (i32, i32) {
    let (dx, dy) = anchor_offset(anchor, w, h);
    (px - dx, py - dy)
}

impl Layout {
    pub fn load(g: &Gdat) -> Option<Layout> {
        let d = g.get(Key::new(1, 0, 4, 0))?;
        let r16 = |o: usize| i16::from_le_bytes([d[o], d[o + 1]]);
        if r16(0) as u16 != MAGIC {
            return None;
        }
        let n = r16(2) as usize;
        let mut off = 4 + 4 * n;
        let mut recs = HashMap::new();
        for i in 0..n {
            let (lo, hi) = (r16(4 + 4 * i), r16(6 + 4 * i));
            for id in lo..=hi {
                recs.insert(
                    id as u16,
                    Rec { kind: r16(off), parent: r16(off + 2), x: r16(off + 4), y: r16(off + 6) },
                );
                off += 8;
            }
        }
        Some(Layout { recs })
    }

    pub fn get(&self, id: u16) -> Option<Rec> {
        self.recs.get(&id).copied()
    }

    /// Place a w×h object at layout id `rid`. If bit 15 of `rid` is set, w
    /// and h are extra x/y offsets instead, and the size comes from `img`.
    pub fn resolve(&self, rid: u16, w: i32, h: i32, img: (i32, i32)) -> Option<Placement> {
        self.resolve_anchored(rid, w, h, img, None)
    }

    /// `resolve` with the resolver's sixth argument: `Some(kind)` replaces
    /// the record's own anchor kind (0x1936F; 0xFFFF in the original means
    /// keep the record's kind, which is `None` here).
    pub fn resolve_anchored(&self, rid: u16, mut w: i32, mut h: i32, img: (i32, i32), anchor: Option<i16>) -> Option<Placement> {
        let extra = rid & 0x8000 != 0;
        let mut rec = self.get(rid & 0x7FFF)?;
        if let Some(k) = anchor {
            rec.kind = k;
        }
        let mut kind = rec.kind;
        let (mut ox, mut oy);
        if kind < 9 {
            ox = rec.x as i32;
            oy = rec.y as i32;
        } else if kind == RECT {
            return None;
        } else {
            ox = 0;
            oy = 0;
            kind -= 10;
        }
        if extra {
            ox += w;
            oy += h;
            w = 0;
            h = 0;
        }
        let (mut cx, mut cy, mut cw, mut ch) = (-10000i32, -10000i32, 20000i32, 20000i32);
        let mut pending = false;
        let mut cur = rec;
        let mut guard = 0;
        while cur.parent != 0 && guard < 64 {
            guard += 1;
            let ck = cur.kind;
            if !(10..=18).contains(&ck) {
                let Some(par) = self.get(cur.parent as u16) else { break };
                let (pw, ph) = (par.x as i32, par.y as i32);
                if par.kind == TRANSLATE {
                    ox += pw;
                    oy += ph;
                    cx += pw;
                    cy += ph;
                } else if par.kind == RECT {
                    let (rx, ry) = if ck <= 8 {
                        if ck == TRANSLATE {
                            (cur.x as i32, cur.y as i32)
                        } else {
                            box_origin(ck, cur.x as i32, cur.y as i32, pw, ph)
                        }
                    } else {
                        (pw, ph)
                    };
                    if pending {
                        pending = false;
                        ox += rx;
                        oy += ry;
                        cx += rx;
                        cy += ry;
                    }
                    if cx < rx {
                        cx = rx;
                    }
                    if rx + pw <= cx + cw - 1 {
                        cw = pw - cx + rx;
                    }
                    if cy < ry {
                        cy = ry;
                    }
                    if ry + ph <= cy + ch - 1 {
                        ch = ry + ph - cy;
                    }
                } else if par.kind < 9 {
                    pending = true;
                }
                cur = par;
            } else {
                let Some(par) = self.get(cur.parent as u16) else { break };
                let Some(rect) = self.get(par.parent as u16) else { break };
                let (rw, rh) = (rect.x as i32, rect.y as i32);
                let (rx, ry) = if par.kind == TRANSLATE {
                    (par.x as i32, par.y as i32)
                } else {
                    box_origin(par.kind, par.x as i32, par.y as i32, rw, rh)
                };
                cx += rx;
                if cx < rx {
                    cx = rx;
                }
                if rx + rw <= cx + cw - 1 {
                    cw = rw - cx + rx;
                }
                cy += ry;
                if cy < ry {
                    cy = ry;
                }
                if ry + rh <= cy + ch - 1 {
                    ch = ry + rh - cy;
                }
                let (ax, ay) = anchor_offset(ck - 10, rw, rh);
                ox += rx + ax + cur.x as i32;
                oy += ry + ay + cur.y as i32;
                cur = rect;
            }
        }
        if w == 0 {
            w = img.0;
        }
        if h == 0 {
            h = img.1;
        }
        if kind > 8 {
            return None;
        }
        let (mut x, mut y) = box_origin(kind, ox, oy, w, h);
        let (mut skip_x, mut skip_y) = (0, 0);
        let d = cx - x;
        let avail_w = if d < 1 {
            cw + d
        } else {
            skip_x = d;
            x = cx;
            w -= d;
            cw
        };
        w = w.min(avail_w);
        let d = cy - y;
        let avail_h = if d < 1 {
            ch + d
        } else {
            skip_y = d;
            y = cy;
            h -= d;
            ch
        };
        h = h.min(avail_h);
        if w <= 0 || h <= 0 {
            return None;
        }
        Some(Placement { x, y, w, h, skip_x, skip_y })
    }
}
