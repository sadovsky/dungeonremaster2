//! Render a view from every walkable square of every map, facing each way,
//! and report how many pixels each feature layer changes. Optional args:
//! `vpsweep OUT_DIR M X Y D ...` also writes those views as raw buffers.
use dm2_engine::viewport::{self, layers, ViewExtras};
use dm2_engine::{assets, world};

fn main() {
    let mut a = assets::Assets::load(&assets::default_data_dir()).unwrap();
    let dg = a.dungeon.clone();
    let names = [
        (layers::ORNAMENTS, "ornaments"),
        (layers::ITEMS, "items"),
        (layers::CREATURES, "creatures"),
        (layers::MISSILES, "missiles"),
        (layers::TELEPORTERS, "teleporters"),
        (layers::DOORS, "doors"),
        (layers::PITS_STAIRS, "pits/stairs"),
    ];
    let mut totals = vec![(0usize, 0usize); names.len()];
    let mut lit_changed = 0usize;
    let mut views = 0usize;
    for m in 0..dg.maps.len() {
        let md = &dg.maps[m];
        for x in 0..md.width as i32 {
            for y in 0..md.height as i32 {
                if world::blocks(&dg, m, x, y) {
                    continue;
                }
                for d in 0..4u8 {
                    views += 1;
                    let full = ViewExtras { lighting: false, ..Default::default() };
                    let all = viewport::render_ex(&mut a, &dg, m, x, y, d, &full).px;
                    for (i, &(bit, _)) in names.iter().enumerate() {
                        let ex = ViewExtras { lighting: false, layers: layers::ALL & !bit, ..Default::default() };
                        let without = viewport::render_ex(&mut a, &dg, m, x, y, d, &ex).px;
                        let diff = all.iter().zip(&without).filter(|(p, q)| p != q).count();
                        if diff > 0 {
                            totals[i].0 += 1;
                            totals[i].1 += diff;
                        }
                    }
                    let lit = viewport::render_ex(&mut a, &dg, m, x, y, d, &ViewExtras::default()).px;
                    if lit != all {
                        lit_changed += 1;
                    }
                }
            }
        }
    }
    println!("{views} views rendered");
    for (i, &(_, n)) in names.iter().enumerate() {
        println!("{n:12} visible in {:6} views, {:9} pixels", totals[i].0, totals[i].1);
    }
    println!("lighting changes {lit_changed} views");
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(out) = args.first() {
        for v in args[1..].chunks(4) {
            let n: Vec<i32> = v.iter().map(|s| s.parse().unwrap()).collect();
            let px = viewport::render(&mut a, &dg, n[0] as usize, n[1], n[2], n[3] as u8).px;
            std::fs::write(format!("{out}/vp_{}_{}_{}_{}.raw", n[0], n[1], n[2], n[3]), px).unwrap();
        }
    }
}
