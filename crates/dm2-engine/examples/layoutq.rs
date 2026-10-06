//! Resolve one layout placement: layoutq RID W H IMG_W IMG_H
use dm2_engine::assets;

fn main() {
    let a: Vec<i32> = std::env::args().skip(1).map(|s| s.parse().unwrap()).collect();
    let assets = assets::Assets::load(&assets::default_data_dir()).unwrap();
    println!("{:?}", assets.layout.resolve(a[0] as u16, a[1], a[2], (a[3], a[4])));
}
