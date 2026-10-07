//! Print key fields of the 60-byte globals record of save files:
//! saveglobals FILE...
use std::rc::Rc;

use dm2_engine::{creatures::data::CreatureData, data::GameData, save};
use dm2_formats::gdat::Gdat;

fn main() {
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = CreatureData::load(gdat, &exe).ok().map(Rc::new);
    for path in std::env::args().skip(1) {
        let g = save::read(std::path::Path::new(&path), gd.clone(), cd.clone()).expect("load");
        let r = &g.legacy.globals;
        let w = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
        println!(
            "{path}: map {} ({},{}) dir {}  0x1E {:#06x}  0x20 {:#06x}  0x22 {:#06x}  0x28 {:#06x}",
            g.party.map, g.party.x, g.party.y, g.party.dir, w(0x1E), w(0x20), w(0x22), w(0x28)
        );
    }
}
