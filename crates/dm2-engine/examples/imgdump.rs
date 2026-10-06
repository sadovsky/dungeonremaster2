//! Dump one decoded image (optionally scaled) as raw source values:
//!   imgdump CAT IDX SUB SCALE OUT    (SCALE in 64ths; 64 = unscaled)
use dm2_engine::assets;

fn main() {
    let a: Vec<i32> = std::env::args().skip(1).take(4).map(|s| s.parse().unwrap()).collect();
    let out = std::env::args().nth(5).unwrap();
    let mut assets = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let s = assets.sprite_scaled(a[0] as u8, a[1] as u8, a[2] as u8, a[3], a[3]).unwrap();
    println!("{}x{} off {:?}", s.w, s.h, s.off);
    std::fs::write(out, &s.px).unwrap();
}
