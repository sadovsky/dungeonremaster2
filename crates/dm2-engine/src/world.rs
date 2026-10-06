//! Party position and static square rules (docs/05-timeline.md, "Party
//! movement"). The full move logic with sensors lives in `movement`.

use dm2_formats::dungeon::{Dungeon, Element, ThingType};

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

impl Move {
    /// Direction offset relative to the facing (forward 0, right 1, back 2, left 3).
    pub fn offset(self) -> u8 {
        match self {
            Move::Forward => 0,
            Move::Right => 1,
            Move::Back => 2,
            Move::Left => 3,
        }
    }
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

/// Map on the layer `delta` away from `map` that holds the same global
/// position (0x1CC7E). Candidate maps are tried in order; a map matches if
/// the position lies within one square of its bounds and the square there is
/// not rock (nor a teleporter whose record has word 2 bit 0 set). Returns the
/// map and the position converted to its coordinates.
pub fn layer_map(dg: &Dungeon, map: usize, delta: i32, x: i32, y: i32) -> Option<(usize, i32, i32)> {
    let m = &dg.maps[map];
    let depth = m.depth as i32 + delta;
    if !(0..0x3F).contains(&depth) {
        return None;
    }
    let (gx, gy) = (x + m.origin_x as i32, y + m.origin_y as i32);
    for (i, n) in dg.maps.iter().enumerate() {
        if n.depth as i32 != depth {
            continue;
        }
        let (ox, oy) = (n.origin_x as i32, n.origin_y as i32);
        if gx < ox - 1 || gx > ox + n.width as i32 || gy < oy - 1 || gy > oy + n.height as i32 {
            continue;
        }
        let (lx, ly) = (gx - ox, gy - oy);
        let sq = dg.square(i, lx, ly);
        let mut element = sq.element();
        if element == Element::Teleporter {
            let flagged = dg
                .things_at(i, lx, ly)
                .into_iter()
                .find(|t| t.kind() == ThingType::Teleporter)
                .and_then(|t| dg.record_word(t, 2))
                .is_some_and(|w| w & 1 != 0);
            if flagged {
                element = Element::Rock;
            }
        }
        if element != Element::Rock {
            return Some((i, lx, ly));
        }
    }
    None
}

/// Facing when leaving a stairs square (0x1CE6F). Bit 3 of the stairs square
/// picks the axis (clear: east-west, set: north-south); the party faces away
/// from the neighbour on that axis when it is a wall or stairs.
pub fn stairs_exit_dir(dg: &Dungeon, map: usize, x: i32, y: i32) -> u8 {
    let ew = dg.square(map, x, y).0 & 8 == 0;
    let probe = if ew { 1 } else { 0 };
    let n = dg.square(map, x + DX[probe], y + DY[probe]).element();
    let solid = matches!(n, Element::Wall | Element::Stairs);
    u8::from(solid) * 2 + u8::from(ew)
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
    /// square picks layer +1 (clear) or −1 (set).
    pub fn take_stairs(&mut self, dg: &Dungeon) -> bool {
        let sq = dg.square(self.map, self.x, self.y);
        let delta = if sq.0 & 4 == 0 { 1 } else { -1 };
        match layer_map(dg, self.map, delta, self.x, self.y) {
            Some((m, x, y)) => {
                *self = PartyPos { map: m, x, y, dir: stairs_exit_dir(dg, m, x, y) };
                true
            }
            None => false,
        }
    }

    /// Geometry-only step without sensors or timeline effects (debug and
    /// tests). Returns true if the party moved.
    pub fn step(&mut self, dg: &Dungeon, mv: Move) -> bool {
        let d = (self.dir + mv.offset()) & 3;
        if mv == Move::Back && Self::is_stairs(dg, self.map, self.x, self.y) {
            return self.take_stairs(dg);
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
