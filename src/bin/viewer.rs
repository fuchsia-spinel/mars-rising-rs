//! Level viewer: `cargo run --release --bin viewer -- [<extracted-assets-dir>] [level 1..28]`
//! The level auto-scrolls like the original (1 px per frame, row 0 at the bottom). Space pauses, R restarts,
//! Left/Right change level, F4 whole-number / free scaling, F11 fullscreen, Esc quits. Window: raylib (vsync, resizable). Shows static layers, verified ground objects and the original score bar.
//! Spawn triggers are reported in the title bar only (enemy behaviour is not ported yet).
use mars_rising::assets::*;
use mars_rising::render::*;
use mars_rising::scroll::Scroller;
use mars_rising::game::FRAME_MICROS;
use mars_rising::present::Pacer;
use raylib::prelude::KeyboardKey as Key;
use std::path::{Path, PathBuf};

#[path = "../bin_common/display.rs"]
mod display;

fn main() {
    let mut args = std::env::args().skip(1).peekable();
    // the first argument is the assets folder unless it is a level number (or there is none): then `assets` next to the executable
    let named = args.peek().map_or(false, |f| f.parse::<u32>().is_err());
    let root = if named { PathBuf::from(args.next().unwrap_or_default()) } else { mars_rising::assets::default_assets_dir() };
    let level_id: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    if let Err(e) = run(&root, level_id) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn argb_from_rgba(d: &Decoded) -> Vec<u32> {
    d.data.chunks(4).map(|p| 0xFF00_0000 | ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32).collect()
}

fn run(root: &Path, mut level_id: u32) -> Result<()> {
    let palette = Palette::load_system(root)?;
    let tiles = TileSet::load(root)?;
    let mut level = load_level(root, level_id)?;
    let mut bank = SpriteBank::new(root, &palette);
    bank.preload_all()?;
    let shadow_lut = load_table(root, "DARK", 128)?; // "System Shadow"
    let bar = read_png(&root.join("pictures/titles/10000_score_bar.png"))?;
    let bar_px = argb_from_rgba(&bar);

    let (mut win, thread) = display::open("Mars Rising (Rust port) - level viewer", SCREEN_W, SCREEN_H, 2);
    let mut screen = display::Display::new(&mut win, &thread, SCREEN_W, SCREEN_H).map_err(Error::Msg)?;
    let mut pacer = Pacer::new(FRAME_MICROS as f64 / 1e6, 4); // the original runs 30 frames per second (2 Mac ticks per frame)

    let mut frame = Frame::new(PLAYFIELD_W, SCREEN_H);
    let mut buf = vec![0xFF00_0000u32; SCREEN_W * SCREEN_H];
    let mut scroller = Scroller::new(level.height);
    let mut paused = false;
    let mut last_spawn: Option<(usize, u32)> = None;
    let mut dirty = true;
    let mut title_age = 0u32;

    while !win.window_should_close() && !win.is_key_pressed(Key::KEY_ESCAPE) {
        let mut reload = false;
        if win.is_key_pressed(Key::KEY_SPACE) { paused = !paused; pacer.reset(); dirty = true; title_age = 0; }
        if win.is_key_pressed(Key::KEY_R) { reload = true; }
        if win.is_key_pressed(Key::KEY_RIGHT) && level_id < 28 { level_id += 1; reload = true; }
        if win.is_key_pressed(Key::KEY_LEFT) && level_id > 1 { level_id -= 1; reload = true; }
        if win.is_key_pressed(Key::KEY_F4) { screen.integer = !screen.integer; }
        if win.is_key_pressed(Key::KEY_F11) { win.toggle_fullscreen(); }
        if reload {
            level = load_level(root, level_id)?;
            scroller = Scroller::new(level.height);
            last_spawn = None;
            pacer.reset();
            dirty = true;
        }
        let due = if paused { 0 } else { pacer.advance(win.get_frame_time() as f64) };
        for _ in 0..due {
            let r = scroller.tick(&level.spawn_trigger_by_row);
            if let (Some(row), Some(g)) = (r.row_completed, r.spawn_group) { last_spawn = Some((row, g)); }
            dirty = true;
        }

        if dirty {
            dirty = false;
            frame.clear(0);
            let top = scroller.view_top_y(SCREEN_H as i32);
            draw_level(&mut frame, &level, &tiles, &mut bank, top)?;
            draw_ground_objects(&mut frame, &level, &mut bank, &shadow_lut, top)?;

            for y in 0..SCREEN_H {
                for x in 0..PLAYFIELD_W { buf[y * SCREEN_W + x] = palette.argb(frame.pixels[y * PLAYFIELD_W + x]); }
                for x in PLAYFIELD_W..SCREEN_W { buf[y * SCREEN_W + x] = 0xFF00_0000; }
            }
            for y in 0..(bar.height as usize).min(SCREEN_H) {
                for x in 0..(bar.width as usize).min(SCREEN_W - PLAYFIELD_W) {
                    buf[y * SCREEN_W + PLAYFIELD_W + x] = bar_px[y * bar.width as usize + x];
                }
            }
            screen.upload(&buf).map_err(Error::Msg)?;
            let spawn = match last_spawn { Some((row, g)) => format!("last spawn: group {} at row {}", g, row), None => "no spawn yet".to_string() };
            if title_age == 0 { win.set_window_title(&thread, &format!("Mars Rising (Rust port) - {} - row {}/{} - {}{}", level.name, scroller.row(), level.height, spawn,
                                                   if paused { " [paused]" } else { "" })); }
            title_age = (title_age + 1) % 30; // the title changes once a second at most (changing it on every frame can stall the window on some systems)
        }

        let (ww, wh) = (win.get_screen_width(), win.get_screen_height());
        let mut d = win.begin_drawing(&thread);
        screen.present(&mut d, ww, wh);
    }
    Ok(())
}
