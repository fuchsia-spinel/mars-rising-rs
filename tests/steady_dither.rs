//! The drawn-in shadow layer is a 1-pixel checkerboard fixed to the ground; `draw_level_with(.., true)` keeps its phase constant on the screen while the
//! ground scrolls 1 px per step (see render.rs). Run with:  MR_ASSETS=/path/to/extracted cargo test --release
use mars_rising::assets::*;
use mars_rising::render::*;
use std::path::PathBuf;

fn assets() -> Option<PathBuf> {
    match std::env::var("MR_ASSETS") {
        Ok(v) => Some(PathBuf::from(v)),
        Err(_) => { eprintln!("MR_ASSETS not set; skipping"); None }
    }
}

/// Parities (x + y) % 2 of the pixels the shadow layer changed: (even count, odd count).
fn shadow_parity(level: &Level, bare: &Level, tiles: &TileSet, bank: &mut SpriteBank, top: i32, steady: bool) -> (usize, usize) {
    let mut a = Frame::new(PLAYFIELD_W, SCREEN_H);
    let mut b = Frame::new(PLAYFIELD_W, SCREEN_H);
    draw_level_with(&mut a, level, tiles, bank, top, steady).expect("draw");
    draw_level_with(&mut b, bare, tiles, bank, top, steady).expect("draw");
    let (mut even, mut odd) = (0, 0);
    for y in 0..SCREEN_H {
        for x in 0..PLAYFIELD_W {
            if a.pixels[y * PLAYFIELD_W + x] != b.pixels[y * PLAYFIELD_W + x] {
                if (x + y) % 2 == 0 { even += 1 } else { odd += 1 }
            }
        }
    }
    (even, odd)
}

#[test]
fn shadow_dither_keeps_its_phase_on_the_screen() {
    let root = match assets() { Some(r) => r, None => return };
    let tiles = TileSet::load(&root).expect("tiles");
    let pal = Palette::load_system(&root).expect("palette");
    let mut bank = SpriteBank::new(&root, &pal);
    for lvl in [1u32, 5, 14] {
        let level = load_level(&root, lvl).expect("level");
        let mut bare = level.clone();
        for v in bare.shadow.iter_mut() { *v = 255; }
        let top0 = (level.height as i32 * 32 - SCREEN_H as i32 - 3) & !1; // an even scroll position near the start
        let mut seen_normal = [0usize; 2];
        for k in 0..8 {
            let top = top0 - k;
            // the reference path flips with the scroll parity ...
            let (e, o) = shadow_parity(&level, &bare, &tiles, &mut bank, top, false);
            assert!(e == 0 || o == 0, "level {} top {}: the shadow dither is not a pure checkerboard ({} / {})", lvl, top, e, o);
            if e > 0 { seen_normal[0] += 1 } else if o > 0 { seen_normal[1] += 1 }
            // ... the steady path never does: always the odd phase it has on even scroll positions
            let (e, o) = shadow_parity(&level, &bare, &tiles, &mut bank, top, true);
            assert_eq!(e, 0, "level {} top {}: steady dither changed phase", lvl, top);
            assert!(o > 0, "level {} top {}: no shadow pixels in view", lvl, top);
        }
        assert!(seen_normal[0] > 0 && seen_normal[1] > 0, "level {}: the reference path should flip ({:?})", lvl, seen_normal);
        // on even scroll positions both paths are identical, and the option off is the reference
        let (mut f1, mut f2, mut f3) = (Frame::new(PLAYFIELD_W, SCREEN_H), Frame::new(PLAYFIELD_W, SCREEN_H), Frame::new(PLAYFIELD_W, SCREEN_H));
        draw_level(&mut f1, &level, &tiles, &mut bank, top0).unwrap();
        draw_level_with(&mut f2, &level, &tiles, &mut bank, top0, true).unwrap();
        draw_level_with(&mut f3, &level, &tiles, &mut bank, top0 - 1, false).unwrap();
        let mut f4 = Frame::new(PLAYFIELD_W, SCREEN_H);
        draw_level(&mut f4, &level, &tiles, &mut bank, top0 - 1).unwrap();
        assert_eq!(f1.pixels, f2.pixels);
        assert_eq!(f3.pixels, f4.pixels);
    }
}

#[test]
fn a_map_cell_naming_a_missing_overlay_sprite_draws_nothing() {
    // level 11's shadow layer has a cell with index 54, but the shadow set ends at 53 (sprite ids 15000..=15052)
    let root = match assets() { Some(r) => r, None => return };
    let tiles = TileSet::load(&root).expect("tiles");
    let pal = Palette::load_system(&root).expect("palette");
    let mut bank = SpriteBank::new(&root, &pal);
    let level = load_level(&root, 11).expect("level");
    let mut f = Frame::new(PLAYFIELD_W, SCREEN_H);
    for top in (0..level.height as i32 * 32 - SCREEN_H as i32).step_by(480) {
        draw_level(&mut f, &level, &tiles, &mut bank, top).expect("a missing overlay sprite must not be an error");
    }
}
