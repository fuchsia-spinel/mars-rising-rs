//! Level 11's shadow layer has a cell with index 54, but the shadow set ends at 53 (sprite ids 15000..=15052): nothing is drawn there (it used to be an
//! error that would have stopped the game). Run with:  MR_ASSETS=/path/to/extracted cargo test --release
use mars_rising::assets::*;
use mars_rising::render::*;
use std::path::PathBuf;

#[test]
fn a_map_cell_naming_a_missing_overlay_sprite_draws_nothing() {
    let root = match std::env::var("MR_ASSETS") { Ok(v) => PathBuf::from(v), Err(_) => { eprintln!("MR_ASSETS not set; skipping"); return; } };
    let tiles = TileSet::load(&root).expect("tiles");
    let pal = Palette::load_system(&root).expect("palette");
    let mut bank = SpriteBank::new(&root, &pal);
    let level = load_level(&root, 11).expect("level");
    let mut f = Frame::new(PLAYFIELD_W, SCREEN_H);
    for top in (0..level.height as i32 * 32 - SCREEN_H as i32).step_by(480) {
        draw_level(&mut f, &level, &tiles, &mut bank, top).expect("a missing overlay sprite must not be an error");
    }
}
