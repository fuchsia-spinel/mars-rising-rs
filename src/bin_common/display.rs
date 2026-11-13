//! The window side shared by the `play` and `viewer` binaries, on raylib (raylib-rs 6.0): the 640 x 480 picture the game composes in
//! software (palette-indexed -> 0xAARRGGBB) lives in one GPU texture with nearest-neighbour sampling and is stretched into the window
//! with `mars_rising::present::dest_rect` (whole-number scale by default, black bars around it). The game's pacing is in `present::Pacer`.
use mars_rising::present::{argb_to_rgba, dest_rect};
use raylib::prelude as rl;
use raylib::prelude::{RaylibDraw, RaylibTexture2D};

pub struct Display {
    /// Two textures used alternately: the one being uploaded is never the one the GPU may still be sampling for the picture on its way to the
    /// screen, so the upload does not have to wait for it (with a single texture the upload of every new picture blocked for a whole refresh).
    tex: [rl::Texture2D; 2],
    cur: usize,
    rgba: Vec<u8>,
    w: i32,
    h: i32,
    /// How much of the picture's width is shown (the game's window is only as wide as the playfield plus the side panel, 576; the title screen shows all 640).
    view_w: i32,
    /// Whole-number scale (true) or the largest scale that fits (false); toggled by the binaries.
    pub integer: bool,
}

/// Open the window: `scale` times the picture size at the start, resizable, vsynced, no key closes it by itself (Esc is handled by the caller).
pub fn open(title: &str, w: usize, h: usize, scale: i32) -> (rl::RaylibHandle, rl::RaylibThread) {
    let (mut handle, thread) = raylib::init()
        .size(w as i32 * scale.max(1), h as i32 * scale.max(1))
        .title(title)
        .vsync()
        .resizable()
        .log_level(rl::TraceLogLevel::LOG_WARNING)
        .build();
    handle.set_exit_key(None);
    (handle, thread)
}

impl Display {
    pub fn new(handle: &mut rl::RaylibHandle, thread: &rl::RaylibThread, w: usize, h: usize) -> Result<Display, String> {
        let img = rl::Image::gen_image_color(w as i32, h as i32, rl::Color::BLACK);
        let mut make = || -> Result<rl::Texture2D, String> {
            let tex = handle.load_texture_from_image(thread, &img).map_err(|e| format!("cannot create the screen texture: {:?}", e))?;
            tex.set_texture_filter(thread, rl::TextureFilter::TEXTURE_FILTER_POINT);
            Ok(tex)
        };
        let tex = [make()?, make()?];
        Ok(Display { tex, cur: 0, rgba: vec![0; w * h * 4], w: w as i32, h: h as i32, view_w: w as i32, integer: true })
    }

    /// Show only the leftmost `w` columns of the picture (the window is resized by the caller).
    pub fn set_view_width(&mut self, w: usize) { self.view_w = (w as i32).clamp(1, self.w); }
    pub fn view_width(&self) -> usize { self.view_w as usize }

    /// Send a new picture to the GPU.
    pub fn upload(&mut self, argb: &[u32]) -> Result<(), String> {
        argb_to_rgba(argb, &mut self.rgba);
        self.cur ^= 1;
        self.tex[self.cur].update_texture(&self.rgba).map_err(|e| format!("cannot update the screen texture: {:?}", e))
    }

    /// Send the picture that was uploaded last once more (to the other texture), without converting it again: the work of an upload on a refresh
    /// where the game did not step (`play` does this on every refresh so that all refreshes do the same GPU work).
    pub fn repeat(&mut self) -> Result<(), String> {
        self.cur ^= 1;
        self.tex[self.cur].update_texture(&self.rgba).map_err(|e| format!("cannot update the screen texture: {:?}", e))
    }

    /// Draw the current picture into a window of the given size (the caller owns `begin_drawing` / drop).
    pub fn present<D: RaylibDraw>(&self, d: &mut D, win_w: i32, win_h: i32) {
        d.clear_background(rl::Color::BLACK);
        let (x, y, w, h) = dest_rect(win_w, win_h, self.view_w, self.h, self.integer);
        d.draw_texture_pro(
            &self.tex[self.cur],
            rl::Rectangle::new(0.0, 0.0, self.view_w as f32, self.h as f32),
            rl::Rectangle::new(x as f32, y as f32, w as f32, h as f32),
            rl::Vector2::new(0.0, 0.0),
            0.0,
            rl::Color::WHITE,
        );
    }
}
