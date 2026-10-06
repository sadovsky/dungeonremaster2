//! Party position and movement (docs/05-timeline.md, "Party movement").

use dm2_formats::dungeon::{Dungeon, Element};

use crate::viewport::{DX, DY};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartyPos {
    pub map: usize,
    pub x: i32,
    pub y: i32,
    /// 0 north, 1 east, 2 south, 3 west.
    pub dir: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Forward,
    Back,
    Left,
    Right,
}

/// Blocking rules (0x4AF72).
pub fn blocks(dg: &Dungeon, map: usize, x: i32, y: i32) -> bool {
    let sq = dg.square(map, x, y);
    match sq.element() {
        Element::Wall | Element::Rock => true,
        Element::Door => matches!(sq.0 & 7, 2..=4),
        Element::TrickWall => sq.0 & 0b101 == 0,
        _ => false,
    }
}

/// Map on layer `depth` that contains the global position (gx, gy).
fn map_at(dg: &Dungeon, depth: u8, gx: i32, gy: i32) -> Option<usize> {
    dg.maps.iter().position(|m| {
        let (ox, oy) = (m.origin_x as i32, m.origin_y as i32);
        m.depth == depth && gx >= ox && gy >= oy && gx < ox + m.width as i32 && gy < oy + m.height as i32
    })
}

impl PartyPos {
    pub fn turn_left(&mut self) {
        self.dir = (self.dir + 3) & 3;
    }

    pub fn turn_right(&mut self) {
        self.dir = (self.dir + 1) & 3;
    }

    fn is_stairs(dg: &Dungeon, map: usize, x: i32, y: i32) -> bool {
        dg.square(map, x, y).element() == Element::Stairs
    }

    /// Take the stairs at the party's square (0x232DD): bit 2 of the stairs
    /// square picks layer +1 (0) or −1 (1).
    fn take_stairs(&mut self, dg: &Dungeon) {
        let sq = dg.square(self.map, self.x, self.y);
        let m = &dg.maps[self.map];
        let depth = if sq.0 & 4 == 0 { m.depth.wrapping_add(1) } else { m.depth.wrapping_sub(1) };
        let (gx, gy) = (m.origin_x as i32 + self.x, m.origin_y as i32 + self.y);
        if let Some(nm) = map_at(dg, depth, gx, gy) {
            let n = &dg.maps[nm];
            self.map = nm;
            self.x = gx - n.origin_x as i32;
            self.y = gy - n.origin_y as i32;
            // TODO(0x1CE6F): the original takes the facing from the arrival
            // stairs; approximate by facing the first open neighbour.
            for d in 0..4u8 {
                let (nx, ny) = (self.x + DX[d as usize], self.y + DY[d as usize]);
                if !blocks(dg, nm, nx, ny) && !Self::is_stairs(dg, nm, nx, ny) {
                    self.dir = d;
                    break;
                }
            }
        }
    }

    /// Try a step; returns true if the party moved.
    pub fn step(&mut self, dg: &Dungeon, mv: Move) -> bool {
        let d = (self.dir
            + match mv {
                Move::Forward => 0,
                Move::Right => 1,
                Move::Back => 2,
                Move::Left => 3,
            })
            & 3;
        // Moving backward off stairs takes them.
        if mv == Move::Back && Self::is_stairs(dg, self.map, self.x, self.y) {
            self.take_stairs(dg);
            return true;
        }
        let (nx, ny) = (self.x + DX[d as usize], self.y + DY[d as usize]);
        if blocks(dg, self.map, nx, ny) {
            return false;
        }
        self.x = nx;
        self.y = ny;
        if Self::is_stairs(dg, self.map, nx, ny) {
            self.take_stairs(dg);
        }
        true
    }
}
