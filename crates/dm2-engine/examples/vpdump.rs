//! Render one viewport to a raw indexed buffer: vpdump MAP X Y DIR OUT
use dm2_engine::{assets, viewport};
fn main() {
    let a: Vec<i32> = std::env::args().skip(1).take(4).map(|s| s.parse().unwrap()).collect();
    let out = std::env::args().nth(5).unwrap();
    let mut assets = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let bm = { let dg = assets.dungeon.clone(); viewport::render(&mut assets, &dg, a[0] as usize, a[1], a[2], a[3] as u8) };
    std::fs::write(out, bm.px).unwrap();
}
