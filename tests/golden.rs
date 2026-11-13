//! Checks the Rust renderer two ways. Run with:  MR_ASSETS=/path/to/extracted cargo test --release
//!  1. frames hashed (FNV-1a 64) against an independent Python reference renderer;
//!  2. level 5 against the ORIGINAL GAME's own gameplay screenshot - the strongest evidence the layer rules are right.
use mars_rising::assets::*;
use mars_rising::render::*;
use std::path::PathBuf;

fn assets() -> Option<PathBuf> {
    match std::env::var("MR_ASSETS") {
        Ok(v) => Some(PathBuf::from(v)),
        Err(_) => { eprintln!("MR_ASSETS not set; skipping"); None }
    }
}

// (level, scroll_y from the top of the level, hash of the 416x480 index frame)
const GOLDEN: &[(u32, i32, u64)] = &[
    (1, 4320, 0x8f345da5809b9053),
    (1, 0, 0x3d251d44f70085dc),
    (5, 2755, 0x007583472d44e9f4),
    (14, 1234, 0x4ac8f71420c3d8dc),
    (28, 8000, 0x212d9ee87b1979d6),
];

fn render(root: &PathBuf, lvl: u32, scroll: i32) -> Frame {
    let tiles = TileSet::load(root).expect("tiles");
    let pal = Palette::load_system(root).expect("palette");
    let mut bank = SpriteBank::new(root, &pal);
    let level = load_level(root, lvl).expect("level");
    let mut f = Frame::new(PLAYFIELD_W, SCREEN_H);
    draw_level(&mut f, &level, &tiles, &mut bank, scroll).expect("draw");
    f
}

#[test]
fn frames_match_python_reference() {
    let root = match assets() { Some(r) => r, None => return };
    for &(lvl, scroll, want) in GOLDEN {
        let f = render(&root, lvl, scroll);
        assert_eq!(fnv1a64(&f.pixels), want, "level {} scroll {}", lvl, scroll);
    }
}

#[test]
fn level5_matches_original_screenshot() {
    let root = match assets() { Some(r) => r, None => return };
    let pal = Palette::load_system(&root).expect("palette");
    let rev = pal.reverse_map();
    let shot = read_png(&root.join("pictures/screenshots/level_5_screenshot.png")).expect("screenshot");
    assert_eq!(shot.color, png::ColorType::Rgba);
    let f = render(&root, 5, 2755);
    let (mut same, mut total) = (0usize, 0usize);
    for y in 0..SCREEN_H {
        for x in 0..PLAYFIELD_W {
            let p = &shot.data[(y * shot.width as usize + x) * 4..(y * shot.width as usize + x) * 4 + 4];
            let idx = *rev.get(&[p[0], p[1], p[2]]).expect("screenshot colour in system palette");
            total += 1;
            if idx == f.pixels[y * PLAYFIELD_W + x] { same += 1; }
        }
    }
    // Static layers only (no player, enemies, bullets or ground objects): measured 0.9025 by the Python reference.
    let ratio = same as f64 / total as f64;
    assert!(ratio > 0.88, "only {:.3} of pixels match the original screenshot", ratio);
}

#[test]
fn every_sprite_maps_back_to_palette_indices() {
    let root = match assets() { Some(r) => r, None => return };
    let pal = Palette::load_system(&root).expect("palette");
    let mut bank = SpriteBank::new(&root, &pal);
    for id in [25000u32, 30463, 10000, 31987, 15000] {
        bank.get(id).expect("sprite");
    }
}

/// Ground objects (kinds verified so far) must reproduce the original screenshots' pixels under their opaque sprite area.
/// (level, scroll_y of that screenshot, ground-sprite record index)
const OBJECT_CASES: &[(u32, i32, u32)] = &[(5, 2755, 47), (5, 2755, 49), (17, 3644, 42), (17, 3644, 21), (15, 3738, 27), (8, 2019, 33)];

#[test]
fn ground_objects_match_original_screenshots() {
    let root = match assets() { Some(r) => r, None => return };
    let pal = Palette::load_system(&root).expect("palette");
    let rev = pal.reverse_map();
    let tiles = TileSet::load(&root).expect("tiles");
    let mut bank = SpriteBank::new(&root, &pal);
    let shadow = load_table(&root, "DARK", 128).expect("shadow table");
    for &(lvl, scroll, rec) in OBJECT_CASES {
        let level = load_level(&root, lvl).expect("level");
        let mut f = Frame::new(PLAYFIELD_W, SCREEN_H);
        draw_level(&mut f, &level, &tiles, &mut bank, scroll).expect("draw");
        draw_ground_objects(&mut f, &level, &mut bank, &shadow, scroll).expect("objects");
        let shot = read_png(&root.join(format!("pictures/screenshots/level_{}_screenshot.png", lvl))).expect("screenshot");
        let g = level.ground_sprites.iter().find(|g| g.index == rec).expect("record");
        let (x0, y0) = object_screen_pos(&level, g, scroll);
        let s = bank.get(g.sprite_id).expect("sprite");
        let (mut same, mut total) = (0usize, 0usize);
        for row in 0..s.height as i32 {
            for col in 0..s.width as i32 {
                let (x, y) = (x0 + col, y0 + row);
                if !s.opaque[row as usize * s.width + col as usize] || x < 0 || y < 0 || x >= PLAYFIELD_W as i32 || y >= SCREEN_H as i32 { continue; }
                let p = &shot.data[(y as usize * shot.width as usize + x as usize) * 4..][..4];
                let idx = *rev.get(&[p[0], p[1], p[2]]).expect("colour");
                total += 1;
                if idx == f.pixels[y as usize * PLAYFIELD_W + x as usize] { same += 1; }
            }
        }
        assert!(total > 0, "record {} not on screen", rec);
        let ratio = same as f64 / total as f64;
        assert!(ratio > 0.9, "level {} record {}: only {:.3} of the object's pixels match", lvl, rec, ratio);
    }
}

/// Shadows: the region under an object's silhouette shifted by SHADOW_OFFSET, excluding the object itself, must equal the
/// System Shadow remap of what was drawn before (measured 100 % on all five cases).
// Static-frame objects only (kind 4 turrets aim at runtime, so their on-screen frame differs from the initial one).
const SHADOW_CASES: &[(u32, i32, u32)] = &[(15, 3738, 27), (17, 3644, 21), (17, 3644, 42), (8, 2019, 33), (15, 3738, 43)];

#[test]
fn object_shadows_match_original_screenshots() {
    let root = match assets() { Some(r) => r, None => return };
    let pal = Palette::load_system(&root).expect("palette");
    let rev = pal.reverse_map();
    let tiles = TileSet::load(&root).expect("tiles");
    let mut bank = SpriteBank::new(&root, &pal);
    let shadow = load_table(&root, "DARK", 128).expect("shadow table");
    for &(lvl, scroll, rec) in SHADOW_CASES {
        let level = load_level(&root, lvl).expect("level");
        let mut f = Frame::new(PLAYFIELD_W, SCREEN_H);
        draw_level(&mut f, &level, &tiles, &mut bank, scroll).expect("draw");
        draw_ground_objects(&mut f, &level, &mut bank, &shadow, scroll).expect("objects");
        let shot = read_png(&root.join(format!("pictures/screenshots/level_{}_screenshot.png", lvl))).expect("screenshot");
        let g = level.ground_sprites.iter().find(|g| g.index == rec).expect("record");
        assert!(g.casts_shadow);
        let (x0, y0) = object_screen_pos(&level, g, scroll);
        let s = bank.get(g.sprite_id).expect("sprite");
        let (mut same, mut total) = (0usize, 0usize);
        for row in 0..s.height as i32 {
            for col in 0..s.width as i32 {
                if !s.opaque[row as usize * s.width + col as usize] { continue; }
                let (x, y) = (x0 + col + SHADOW_OFFSET.0, y0 + row + SHADOW_OFFSET.1);
                let (cx, cy) = (x - x0, y - y0);
                let covered = cx >= 0 && cy >= 0 && (cx as usize) < s.width && (cy as usize) < s.height && s.opaque[cy as usize * s.width + cx as usize];
                if covered || x < 0 || y < 0 || x >= PLAYFIELD_W as i32 || y >= SCREEN_H as i32 { continue; }
                let p = &shot.data[(y as usize * shot.width as usize + x as usize) * 4..][..4];
                let idx = *rev.get(&[p[0], p[1], p[2]]).expect("colour");
                total += 1;
                if idx == f.pixels[y as usize * PLAYFIELD_W + x as usize] { same += 1; }
            }
        }
        assert!(total > 0);
        assert!(same as f64 / total as f64 > 0.9, "level {} record {}: shadow match {}/{}", lvl, rec, same, total);
    }
}

/// Cross-check of the enemy table against the extracted sprites: each type's animation must start with a sprite of the size the
/// type's init code declares (verified for 20 of 22 types when this was written; types 1 and 2 are known exceptions).
#[test]
fn enemy_animation_sprites_have_the_declared_size() {
    let root = match assets() { Some(r) => r, None => return };
    let pal = Palette::load_system(&root).expect("palette");
    let mut bank = SpriteBank::new(&root, &pal);
    let sets: serde_json::Value = serde_json::from_reader(std::fs::File::open(root.join("sprites/sets.json")).unwrap()).unwrap();
    let anim = sets["sets"].as_array().unwrap().iter().find(|s| s["id"] == 1000).unwrap()["entries"].as_array().unwrap().clone();
    let mut ok = 0;
    for t in mars_rising::enemies::ENEMY_TYPES.iter() {
        let first = anim[t.anim_index as usize - 1]["first_id"].as_u64().unwrap() as u32;
        let s = bank.get(first).expect("sprite");
        if s.width as i32 == t.sprite_w && s.height as i32 == t.sprite_h { ok += 1; }
    }
    assert!(ok >= 20, "only {} of 22 enemy types match their sprite size", ok);
}
