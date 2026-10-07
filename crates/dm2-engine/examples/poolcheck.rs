//! Model the original's creature-slot pool after loading a save:
//! poolcheck FILE...
//!
//! Pool size (0x342F9): min(in-use creature records whose type has info
//! flag bit 0 clear + 100, creature record count). Game start (0x342A3 →
//! 0x34236 → 0x34106) then activates, on every map, the first creature of
//! each square's list if its type has flag bit 0 clear.
use std::rc::Rc;

use dm2_engine::{creatures, creatures::data::CreatureData, data::GameData, save};
use dm2_formats::dungeon::{ThingRef, ThingType};
use dm2_formats::gdat::Gdat;

fn main() {
    let gd = Rc::new(GameData::load_default().expect("game data"));
    let exe = std::fs::read(dm2_engine::exe_tables::default_exe_path()).unwrap();
    let gdat = Rc::new(Gdat::open(dm2_engine::assets::default_data_dir().join("GRAPHICS.DAT")).unwrap());
    let cd = Rc::new(CreatureData::load(gdat, &exe).unwrap());
    for path in std::env::args().skip(1) {
        let g = save::read(std::path::Path::new(&path), gd.clone(), Some(cd.clone())).expect("load");
        let total = g.dungeon.thing_count(ThingType::Creature);
        let flag0 = |t: ThingRef| {
            creatures::type_info(&g, &cd, creatures::creature_type(&g, t)).is_some_and(|(i, _)| i.raw[0] & 1 != 0)
        };
        let mut in_use = 0;
        let mut in_use_unflagged = 0;
        for i in 0..total {
            let t = ThingRef((ThingType::Creature as u16) << 10 | i as u16);
            if g.dungeon.record_word(t, 0) != Some(0xFFFF) {
                in_use += 1;
                if !flag0(t) {
                    in_use_unflagged += 1;
                }
            }
        }
        let pool = (in_use_unflagged + 100).min(total);
        let mut activations = 0;
        let mut per_map = vec![0; g.dungeon.maps.len()];
        for (m, md) in g.dungeon.maps.iter().enumerate() {
            for x in 0..md.width as i32 {
                for y in 0..md.height as i32 {
                    if let Some(c) = g.dungeon.things_at(m, x, y).into_iter().find(|t| t.kind() == ThingType::Creature) {
                        if !flag0(c) {
                            activations += 1;
                            per_map[m] += 1;
                        }
                    }
                }
            }
        }
        println!(
            "{path}: party map {}  records {total}  in use {in_use}  in use, flag0 clear {in_use_unflagged}  pool {pool}  activations {activations}",
            g.party.map
        );
        let busiest: Vec<_> = per_map.iter().enumerate().filter(|(_, n)| **n > 0).map(|(m, n)| format!("{m}:{n}")).collect();
        println!("  per map: {}", busiest.join(" "));
    }
}
