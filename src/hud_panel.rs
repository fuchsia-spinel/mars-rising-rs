//! The pixels behind the HUD: executes the [`HudCall`](crate::hud::HudCall) stream of `hud.rs` on two palette-indexed surfaces, like the original's CopyBits /
//! sprite-blit / PaintRect calls do on its offscreen buffers.
//!
//! * `panel` (the original's 704 x 608 buffer `D.8200`, here 640 x 480: panel coordinates minus 64) holds the side-panel picture at x = 416 and everything drawn on
//!   it. The original's sprite blitters write straight into that buffer whatever the QuickDraw port is, so every [`HudCall::Blit`] lands here.
//! * `store` (`D.81fc`, the strip bitmap) keeps clean copies of the backgrounds for [`HudCall::Restore`].
//! * `Present` copies a rectangle of the panel to the screen buffer at the *same* position (the original's origins make the two coincide: panel left 480 - 64
//!   = screen left 416, panel top 64 - 64 = 0), so the finished frame simply shows the panel region of `panel`.
//! `PaintRect` in pen mode 32 (blend) with the foreground colour 33 (black) and an `OpColor` of 50 % darkens a rectangle (the dimmed panel of an absent player).
use crate::assets::{read_png, AnimTable, Palette, Result, SpriteBank};
use crate::compose::Blitter;
use crate::draw::{Draw, Prim};
use crate::hud::{HudCall, Rect, ANIM};
use crate::render::Frame;
use std::path::Path;

/// Where the panel picture sits on the 640 x 480 screen.
pub const PANEL_X: usize = 416;

pub struct HudPanel {
    pub panel: Frame,
    store: Frame,
    black: u8,
}

/// The side-panel picture (`pictures/titles/10000_score_bar.png`, 160 x 480) as palette indices.
pub fn load_panel_picture(root: &Path, palette: &Palette) -> Result<Frame> {
    let img = read_png(&root.join("pictures/titles/10000_score_bar.png"))?;
    let rev = palette.reverse_map();
    let (w, h) = (img.width as usize, img.height as usize);
    let channels = if img.color == png::ColorType::Rgba { 4 } else { 3 };
    let mut f = Frame::new(w, h);
    for i in 0..w * h {
        let p = &img.data[i * channels..i * channels + 3];
        f.pixels[i] = match rev.get(&[p[0], p[1], p[2]]) {
            Some(&v) => v,
            None => nearest(palette, [p[0], p[1], p[2]]),
        };
    }
    Ok(f)
}

fn nearest(palette: &Palette, c: [u8; 3]) -> u8 {
    let mut best = (u32::MAX, 0u8);
    for (i, p) in palette.colors.iter().enumerate() {
        let d: u32 = (0..3).map(|k| { let e = p[k] as i32 - c[k] as i32; (e * e) as u32 }).sum();
        if d < best.0 { best = (d, i as u8); }
    }
    best.1
}

impl HudPanel {
    pub fn new(picture: &Frame, palette: &Palette) -> HudPanel {
        let mut panel = Frame::new(640, 480);
        for y in 0..picture.height.min(480) {
            for x in 0..picture.width {
                if PANEL_X + x < 640 { panel.pixels[y * 640 + PANEL_X + x] = picture.pixels[y * picture.width + x]; }
            }
        }
        let black = palette.colors.iter().position(|c| *c == [0, 0, 0]).unwrap_or(255) as u8;
        HudPanel { panel, store: Frame::new(128, 352), black }
    }

    fn copy(src: &Frame, sx: i32, sy: i32, dst: &mut Frame, dx: i32, dy: i32, w: i32, h: i32) {
        for y in 0..h {
            for x in 0..w {
                let (px, py, qx, qy) = (sx + x, sy + y, dx + x, dy + y);
                if px < 0 || py < 0 || qx < 0 || qy < 0 { continue; }
                let (px, py, qx, qy) = (px as usize, py as usize, qx as usize, qy as usize);
                if px >= src.width || py >= src.height || qx >= dst.width || qy >= dst.height { continue; }
                dst.pixels[qy * dst.width + qx] = src.pixels[py * src.width + px];
            }
        }
    }

    fn size(r: Rect) -> (i32, i32) { ((r.right as i32 - r.left as i32).max(0), (r.bottom as i32 - r.top as i32).max(0)) }

    /// Execute a stream of HUD operations.
    pub fn apply(&mut self, calls: &[HudCall], blitter: &Blitter, bank: &mut SpriteBank) -> Result<()> {
        for c in calls {
            match *c {
                HudCall::Restore(strip, rect) => {
                    let (w, h) = Self::size(strip);
                    Self::copy(&self.store, strip.left as i32, strip.top as i32, &mut self.panel, rect.left as i32 - 64, rect.top as i32 - 64, w, h);
                }
                HudCall::Store(rect, strip) => {
                    let (w, h) = Self::size(rect);
                    Self::copy(&self.panel, rect.left as i32 - 64, rect.top as i32 - 64, &mut self.store, strip.left as i32, strip.top as i32, w, h);
                }
                HudCall::Blit { addr, x, y, w, h, anim, frame } => {
                    let prim = if addr == ANIM { Prim::Anim } else { Prim::Flash };
                    // the clipped variants add the 64 pixel margin of the original's buffer themselves
                    blitter.draw(&mut self.panel, bank, &Draw { prim, x, y, w, h, anim, frame })?;
                }
                HudCall::Paint(r) => {
                    let (w, h) = Self::size(r);
                    for yy in 0..h {
                        for xx in 0..w {
                            let (px, py) = (r.left as i32 - 64 + xx, r.top as i32 - 64 + yy);
                            if px < 0 || py < 0 || px >= 640 || py >= 480 { continue; }
                            let i = py as usize * 640 + px as usize;
                            let below = self.panel.pixels[i] as usize;
                            self.panel.pixels[i] = if blitter.blend.len() >= 65536 { blitter.blend[below * 256 + self.black as usize] } else { self.panel.pixels[i] };
                        }
                    }
                }
                HudCall::Present(..) | HudCall::Port(_) | HudCall::OpColor(_) | HudCall::PenMode(_) | HudCall::ForeColor(_) => {}
            }
        }
        Ok(())
    }

    /// Copy the panel region onto a screen frame (640 x 480).
    pub fn paste_onto(&self, screen: &mut Frame) {
        for y in 0..480.min(screen.height) {
            let a = y * 640 + PANEL_X;
            let b = y * screen.width + PANEL_X;
            let n = (640 - PANEL_X).min(screen.width.saturating_sub(PANEL_X));
            screen.pixels[b..b + n].copy_from_slice(&self.panel.pixels[a..a + n]);
        }
    }
}

#[allow(dead_code)]
fn _unused(_: &AnimTable) {}
