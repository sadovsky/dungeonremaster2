//! Render one viewport to a raw indexed buffer:
//!   vpdump MAP X Y DIR OUT [LAYERS] [LIGHT]
//! LAYERS is a `viewport::layers` bit mask (default all); LIGHT is 0 or 1
//! (default 1).
use dm2_engine::{assets, viewport};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let n: Vec<i32> = args[..4].iter().map(|s| s.parse().unwrap()).collect();
    let out = &args[4];
    let mut ex = viewport::ViewExtras::default();
    if let Some(l) = args.get(5) {
        ex.layers = l.parse().unwrap();
    }
    if let Some(l) = args.get(6) {
        ex.lighting = l != "0";
    }
    let mut assets = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let dg = assets.dungeon.clone();
    let bm = viewport::render_ex(&mut assets, &dg, n[0] as usize, n[1], n[2], n[3] as u8, &ex);
    std::fs::write(out, bm.px).unwrap();
}
