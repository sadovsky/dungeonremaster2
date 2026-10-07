//! Where is a thing in a save? thingwhere SAVE THING_HEX
//! Loads the save the way the original does and prints the square holding
//! the thing, the square byte, and the creature's status word if it is one.
use std::rc::Rc;
use dm2_engine::{creatures::data::CreatureData, data::GameData, save};
use dm2_formats::dungeon::ThingRef;
use dm2_formats::gdat::Gdat;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = CreatureData::load(gdat, &exe).ok().map(Rc::new);
    let g = save::read_as_original(std::path::Path::new(&a[0]), gd, cd).expect("load");
    if a[1] == "sq" {
        // thingwhere SAVE sq MAP X Y: print a square byte and its things.
        let (m, x, y): (usize, i32, i32) = (a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap());
        let things: Vec<String> = g.dungeon.things_at(m, x, y).iter().map(|t| format!("{:#06x}", t.0)).collect();
        println!("map {} ({},{}) square {:#04x} things {:?}", m, x, y, g.dungeon.square(m, x, y).0, things);
        return;
    }
    let want = u16::from_str_radix(a[1].trim_start_matches("0x"), 16).unwrap() & 0x3FFF;
    for (m, md) in g.dungeon.maps.iter().enumerate() {
        for x in 0..md.width as i32 {
            for y in 0..md.height as i32 {
                if g.dungeon.things_at(m, x, y).iter().any(|t| t.0 & 0x3FFF == want) {
                    println!("thing {:#x} on map {} ({},{}) square {:#04x} party map {} ({},{})", want, m, x, y, g.dungeon.square(m, x, y).0, g.party.map, g.party.x, g.party.y);
                }
            }
        }
    }
    let t = ThingRef(want);
    if let Some(r) = g.dungeon.record(t) {
        println!("record {:02x?}", r);
    }
}
