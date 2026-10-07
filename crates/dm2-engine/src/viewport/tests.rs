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
    // Views with wall ornaments changed when ornaments started honouring the
    // anchor override from attribute 5 (0 = centred on the grid point),
    // checked against the original's start view in DOSBox.
    let views: [((usize, i32, i32, u8), u64); 8] = [
        ((3, 10, 9, 0), 0x8cea3120aed4479b),
        // A door panel drawn from its per-depth image is now lit at depth 0
        // (ambient only), as 0x5346E passes it to 0x4E502; the changed
        // pixels are confined to the centre door panel. Verified against the
        // original in DOSBox at map 2 (19,12,N): the door view went from
        // 3,413 differing pixels to 0 with this and the door frame.
        ((6, 8, 8, 1), 0xd50ccd4aa6e11435),
        // A depth-4 front-face ornament is now drawn (cells 16-20 show ornaments).
        // two open pits in view: pits are now keyed and lit by depth (pit view verified
        // pixel-exact against the original at (4, 5, 6, N)).
        ((4, 6, 11, 2), 0x97a23aa9bbe135b2),
        // Stairs ahead: keyed with the set's default colour and lit by depth,
        // verified pixel-exact against the original at (8, 12, 2, N).
        ((8, 12, 3, 0), 0x66acb89dc0519ff1),
        ((7, 12, 11, 1), 0x8a9dbdd9e46d85e9),
        // Ornament attribute 10 is a kind (1 alcove, 3 portrait mirror), not
        // a flag: the starting map's mirror now shows its champion and no
        // longer its items (alcove view (0,1,1,W) against the original in
        // DOSBox: 2,552 -> 392 differing pixels).
        ((0, 3, 4, 0), 0x67a035016ee93515),
        // Creature in cell 6 now offset by its descriptor shift byte (0x50DEE).
        // The outdoor set's backdrop scripts (horizon strip, landmarks) now
        // draw (0x54699); against the original in DOSBox this view's diff
        // fell from 26,124 to 22,952 pixels, the rest being time-of-day
        // colour and light, which are not modelled yet.
        // Backdrops now use the map set's colour key (0x544BE): the brown
        // box around the castle silhouettes is gone (gallery view 04, same
        // square: 9,957 -> 1,695 differing pixels with the save's weather).
        ((1, 2, 9, 0), 0xe3ec12f8202fd6b9),
        // four open pits in view: pits are now keyed and lit by depth (pit view verified
        // pixel-exact against the original at (4, 5, 6, N)).
        // Floor ornaments take their colour key from attribute 4 with the
        // map set's key as fallback (0x50081), not from attribute 0x11; the
        // 263 changed pixels are exactly floor ornament 12's sprite at
        // (168,74), whose background is now transparent.
        // Floor ornaments now pass attribute 5's anchor kind to the drawer
        // (0x50081; default anchor 0, centred, when the attribute is 0), so
        // ornament 12 is centred on its grid point. The same rule took the
        // map 3 tiled-floor view (gallery 08) from 11,057 to 480 differing
        // viewport pixels against the original.
        ((5, 12, 23, 0), 0x5e7b7b3e05f6b092),
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

#[test]
fn clip_keeps_source_offsets() {
    let p = Placement { x: 10, y: 0, w: 100, h: 50, skip_x: 0, skip_y: 0 };
    let c = clip(p, MID_STEP_CLIP).unwrap();
    assert_eq!((c.x, c.y, c.skip_x, c.skip_y), (21, 8, 11, 8));
    assert_eq!((c.w, c.h), (89, 42));
    assert!(clip(Placement { x: 0, y: 0, w: 5, h: 5, skip_x: 0, skip_y: 0 }, MID_STEP_CLIP).is_none());
}

#[test]
fn creature_grid_geometry() {
    // The party cell's back row and the square ahead's front row are the
    // same grid row; centre points of neighbouring cells are 4 apart.
    assert_eq!(creature::grid_point(0, 12), (10, 2));
    assert_eq!(creature::grid_point(3, 12), (10, 6));
    assert_eq!(creature::grid_point(0, 2).1, creature::grid_point(3, 22).1);
    assert_eq!(creature::grid_point(1, 12), (6, 2));
}

#[test]
fn quadrants_and_hits() {
    assert_eq!([6, 8, 18, 16, 12, 7].map(quadrant_of), [Some(0), Some(1), Some(2), Some(3), Some(4), None]);
    let mut t = hits::HitTable::default();
    t.item(hits::HitKind::FloorItem, 3, 1, 0x1400, (10, 10, 5, 5));
    t.item(hits::HitKind::FloorItem, 3, 1, 0x1401, (12, 8, 6, 4));
    t.item(hits::HitKind::FloorItem, 3, 2, 0x1402, (40, 40, 4, 4));
    assert_eq!(t.hits.len(), 2, "a pile in one quadrant shares a record");
    let h = t.at(17, 9).unwrap();
    assert_eq!((h.x, h.y, h.w, h.h, h.thing), (10, 8, 8, 7, Some(0x1400)));
    assert!(t.at(0, 0).is_none());
    assert_eq!(t.at(41, 41).unwrap().thing, Some(0x1402));
}

/// Some wall-writing thing in the dungeon resolves to non-empty text
/// (its content is not checked or printed).
#[test]
fn wall_writing_resolves() {
    let Ok(a) = assets::Assets::load(&assets::default_data_dir()) else { return };
    let dg = &a.dungeon;
    let found = (0..dg.maps.len()).any(|m| {
        let md = &dg.maps[m];
        (0..md.width as i32).any(|x| {
            (0..md.height as i32).any(|y| {
                dg.square(m, x, y).element() == Element::Wall
                    && dg.things_at(m, x, y).into_iter().any(|t| {
                        t.kind() as u16 == 2
                            && dg.record_word(t, 1).is_some_and(|w| w & 7 == 3 && w >> 11 == 14)
                            && walltext::text_of(&a, dg, t).is_some_and(|s| !s.is_empty())
                    })
            })
        })
    });
    assert!(found);
}
