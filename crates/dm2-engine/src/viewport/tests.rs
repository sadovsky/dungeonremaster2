use super::*;
use crate::assets;

#[test]
fn slot_rotation_is_a_quarter_turn() {
    // Identity, and four quarter turns return to the start.
    for s in 0..25u8 {
        assert_eq!(rotate_slot(s, 0), s);
        let mut r = s;
        for _ in 0..4 {
            r = rotate_slot(r, 1);
        }
        assert_eq!(r, s);
    }
    assert_eq!(rotate_slot(12, 3), 12);
    // Front-left corner turns to the front-right corner and back.
    assert_eq!(rotate_slot(rotate_slot(0, 1), 3), 0);
    assert_eq!(rotate_slot(0, 2), 24);
}

#[test]
fn scaler_sizes_and_sampling() {
    let s = Sprite { w: 4, h: 2, px: vec![0, 1, 2, 3, 4, 5, 6, 7], cmap: None, off: (4, -4) };
    let half = s.scaled(32, 32).unwrap();
    assert_eq!((half.w, half.h), (2, 1));
    // Row (T/2 + j·T) >> 7 with T = 256 -> row 1; 8-bit columns
    // (S + 2·S·i) >> 8 with S = 256 -> 1, 3.
    assert_eq!(half.px, vec![5, 7]);
    assert_eq!(half.off, (2, -2));
    // 4-bit rule: column (S/2 + i·S) >> 7 -> 1, 3 as well for even S.
    let nib = Sprite { cmap: Some([0; 16]), ..s };
    assert_eq!(nib.scaled(32, 32).unwrap().px, vec![5, 7]);
    assert!(nib.scaled(1, 1).is_none());
}

fn fnv(px: &[u8]) -> u64 {
    px.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// Rendered views pinned by hash, so changes to the renderer are noticed.
/// The views cover walls, doors, pits, stairs, items, ornaments, creatures
/// and teleporters. Skipped when the original data is not present.
#[test]
fn pinned_views() {
    let Ok(mut a) = assets::Assets::load(&assets::default_data_dir()) else { return };
    let dg = a.dungeon.clone();
    let views: [((usize, i32, i32, u8), u64); 8] = [
        ((3, 10, 9, 0), 0x8cea3120aed4479b),
        ((6, 8, 8, 1), 0x8ed5fd131ea8de24),
        ((4, 6, 11, 2), 0xd788da890a57df50),
        ((8, 12, 3, 0), 0x63937f659a8ab17e),
        ((7, 12, 11, 1), 0xb6be0da00e90b5ff),
        ((0, 3, 4, 0), 0xa679cc154d628c53),
        ((1, 2, 9, 0), 0xb2205ef0cf9acf76),
        ((5, 12, 23, 0), 0x7b768a367eb43359),
    ];
    let mut bad = Vec::new();
    for ((m, x, y, d), want) in views {
        let got = fnv(&render(&mut a, &dg, m, x, y, d).px);
        if got != want {
            bad.push(format!("(({m}, {x}, {y}, {d}), {got:#x}),"));
        }
    }
    assert!(bad.is_empty(), "view hashes changed:\n{}", bad.join("\n"));
}
