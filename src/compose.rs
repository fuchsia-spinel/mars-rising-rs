//! Turns the draw requests of the game (`draw::Draw`) into pixels of a palette-indexed `Frame` (needs the extracted assets).
use crate::assets::{load_table, AnimTable, Result, SpriteBank};
use crate::blit::{blit, Luts, SpriteView};
use crate::draw::{Draw, Prim};
use crate::render::Frame;
use std::path::Path;

/// The animation table and the three lookup tables the blit effects need.
pub struct Blitter {
    pub anims: AnimTable,
    /// `DARK 129` Orange LUT (hit flash / blink).
    pub flash: Vec<u8>,
    /// `DARK 128` System Shadow.
    pub shadow: Vec<u8>,
    /// `TRAN 1000` 50 % blend (the table the original loads at start-up, 0xbdb8).
    pub blend: Vec<u8>,
}

impl Blitter {
    pub fn load(root: &Path) -> Result<Blitter> {
        Ok(Blitter {
            anims: AnimTable::load(root)?,
            flash: load_table(root, "DARK", 129)?,
            shadow: load_table(root, "DARK", 128)?,
            blend: load_table(root, "TRAN", 1000)?,
        })
    }

    /// Execute one blit request. Requests for frames outside the animation table draw nothing (the original ignores frame <= 0).
    pub fn draw(&self, frame: &mut Frame, bank: &mut SpriteBank, d: &Draw) -> Result<()> {
        if d.prim == Prim::Pixels {
            // debris fragment: `anim` is the palette index, 3 x 3 pixels, clipped to the frame
            for dy in 0..d.h.max(0) {
                for dx in 0..d.w.max(0) {
                    let (px, py) = (d.x + dx, d.y + dy);
                    if px >= 0 && py >= 0 && (px as usize) < frame.width && (py as usize) < frame.height {
                        frame.pixels[py as usize * frame.width + px as usize] = d.anim as u8;
                    }
                }
            }
            return Ok(());
        }
        let Some(id) = self.anims.sprite_id(d.anim, d.frame) else { return Ok(()) };
        let s = bank.get(id)?;
        let view = SpriteView { width: s.width, height: s.height, index: &s.index, opaque: &s.opaque };
        let luts = Luts { flash: &self.flash, shadow: &self.shadow, blend: &self.blend };
        blit(&mut frame.pixels, frame.width, frame.height, &view, d.x, d.y, d.prim, &luts);
        Ok(())
    }

    /// Execute a whole frame's requests in order.
    pub fn draw_all(&self, frame: &mut Frame, bank: &mut SpriteBank, draws: &[Draw]) -> Result<()> {
        for d in draws { self.draw(frame, bank, d)?; }
        Ok(())
    }
}
