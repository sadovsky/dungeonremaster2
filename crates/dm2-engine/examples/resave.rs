//! Load a save and write it back with the remake's writer: resave IN OUT
use std::rc::Rc;
use dm2_engine::{creatures::data::CreatureData, data::GameData, save};
use dm2_formats::gdat::Gdat;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = CreatureData::load(gdat, &exe).ok().map(Rc::new);
    let mut g = save::read(std::path::Path::new(&a[0]), gd, cd).expect("load");
    let bytes = save::to_bytes(&mut g, "REMAKE SAVE").expect("save");
    std::fs::write(&a[1], &bytes).unwrap();
    let orig = std::fs::read(&a[0]).unwrap();
    let n = orig.len().min(bytes.len());
    let same = orig[..n].iter().zip(&bytes[..n]).filter(|(x, y)| x == y).count();
    println!("wrote {} bytes ({} original); {} of the first {} bytes identical", bytes.len(), orig.len(), same, n);
}
