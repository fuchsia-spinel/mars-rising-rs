//! Software renderer: an 8-bit palette-indexed frame buffer, as the original used.
//! The layer rules below were recovered from the original's PowerPC map-draw code and verified against its own
//! screenshot (level 5 matches ~90% pixel-exactly with only the static layers drawn).
use crate::assets::{Error, GroundSprite, Level, Result, Sprite, SpriteBank, TileSet};

pub const TILE: i32 = 32;
pub const PLAYFIELD_W: usize = 416; // 13 tiles
pub const SCREEN_W: usize = 640;
pub const SCREEN_H: usize = 480;
/// Fringe overlay sprites are sprite ids 10000 + (value-1); shadow overlays are 15000 + (value-1).
pub const FRINGE_BASE: u32 = 10000;
pub const SHADOW_BASE: u32 = 15000;

pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>, // palette indices
}

impl Frame {
    pub fn new(width: usize, height: usize) -> Frame {
        Frame { width, height, pixels: vec![0; width * height] }
    }
    pub fn clear(&mut self, index: u8) {
        for p in self.pixels.iter_mut() { *p = index; }
    }
    /// Opaque 32x32 blit with clipping.
    pub fn blit_tile(&mut self, tile: &[u8], x: i32, y: i32) {
        self.blit_tile_v(tile, x, y, false);
    }
    /// Opaque 32x32 blit with clipping, optionally flipped vertically.
    pub fn blit_tile_v(&mut self, tile: &[u8], x: i32, y: i32, vflip: bool) {
        for row in 0..TILE {
            let dy = y + row;
            if dy < 0 || dy >= self.height as i32 { continue; }
            let src_row = if vflip { TILE - 1 - row } else { row };
            for col in 0..TILE {
                let dx = x + col;
                if dx < 0 || dx >= self.width as i32 { continue; }
                self.pixels[dy as usize * self.width + dx as usize] = tile[(src_row * TILE + col) as usize];
            }
        }
    }
    /// Masked sprite blit with clipping.
    pub fn blit_sprite(&mut self, s: &Sprite, x: i32, y: i32) {
        for row in 0..s.height as i32 {
            let dy = y + row;
            if dy < 0 || dy >= self.height as i32 { continue; }
            for col in 0..s.width as i32 {
                let dx = x + col;
                if dx < 0 || dx >= self.width as i32 { continue; }
                let si = row as usize * s.width + col as usize;
                if s.opaque[si] { self.pixels[dy as usize * self.width + dx as usize] = s.index[si]; }
            }
        }
    }
}

/// Draw the static map layers: base tiles (with flip flags), then fringe sprites, then shadow sprites.
///
/// `scroll_y` = distance in pixels from the TOP of the level to the top of the frame. Row 0 is the bottom of the
/// level (the player's start); the screen y of row `r` is `(height-1-r)*32 - scroll_y`.
/// Valid scroll range is 0 ..= height*32 - frame.height; the level starts at the maximum.
pub fn draw_level(frame: &mut Frame, level: &Level, tiles: &TileSet, bank: &mut SpriteBank, scroll_y: i32) -> Result<()> {
    let h = level.height as i32;
    let fh = frame.height as i32;
    let cols = level.width.min(frame.width / TILE as usize);
    let row_y = |r: i32| (h - 1 - r) * TILE - scroll_y;

    for r in 0..h {
        let y0 = row_y(r);
        if y0 >= fh || y0 + TILE <= 0 { continue; }
        for tx in 0..cols {
            let i = r as usize * level.width + tx;
            let t = level.base_tile[i] as usize;
            if t >= tiles.count { continue; }
            let flip = level.base_flip[i];
            let src = if flip & 2 != 0 { &tiles.mirrored[t] } else { &tiles.tiles[t] };
            frame.blit_tile_v(src, tx as i32 * TILE, y0, flip & 1 != 0);
        }
    }
    for (layer, base) in [(&level.fringe, FRINGE_BASE), (&level.shadow, SHADOW_BASE)] {
        for r in 0..h {
            let y0 = row_y(r);
            if y0 >= fh || y0 + 64 <= 0 { continue; } // overlay sprites are at most 64 px tall
            for tx in 0..cols {
                let v = layer[r as usize * level.width + tx];
                if v == 255 { continue; }
                let id = base + (v.max(1) as u32) - 1;
                // a map cell may name a sprite the set does not have (level 11's shadow layer uses index 54 of 53): nothing is drawn there
                let s = match bank.get(id) {
                    Ok(s) => s,
                    Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e),
                };
                frame.blit_sprite(s, tx as i32 * TILE, y0);
            }
        }
    }
    Ok(())
}

/// Object kinds whose animation strip, frame rule and anchor were confirmed pixel-exactly against the original screenshots
/// (kind 4 = turret, which aims at runtime, so only its position/anchor is confirmed).
pub const VERIFIED_OBJECT_KINDS: [i32; 6] = [2, 4, 8, 16, 18, 21];

/// Screen position (top-left) where a ground object's sprite is drawn.
pub fn object_screen_pos(level: &Level, g: &GroundSprite, scroll_y: i32) -> (i32, i32) {
    let row_top = (level.height as i32 - 1 - g.y as i32) * TILE - scroll_y;
    (g.x as i32 + g.draw_offset[0], row_top + g.draw_offset[1])
}

/// Offset of an object's drop shadow (verified on 10 screenshot objects: 97-100 % pixel match).
pub const SHADOW_OFFSET: (i32, i32) = (-6, 8);

impl Frame {
    /// Drop shadow: for every opaque pixel of `s` placed at (x, y), replace the pixel already in the frame at the
    /// shadow position by `lut[pixel]` (the "System Shadow" DARK table).
    pub fn blit_shadow(&mut self, s: &Sprite, x: i32, y: i32, lut: &[u8]) {
        for row in 0..s.height as i32 {
            let dy = y + row;
            if dy < 0 || dy >= self.height as i32 { continue; }
            for col in 0..s.width as i32 {
                let dx = x + col;
                if dx < 0 || dx >= self.width as i32 { continue; }
                if s.opaque[row as usize * s.width + col as usize] {
                    let p = &mut self.pixels[dy as usize * self.width + dx as usize];
                    *p = lut[*p as usize];
                }
            }
        }
    }
}

/// Draw static ground objects (initial animation frame) over the map layers: shadow first (if `casts_shadow`), then the sprite.
/// `shadow_lut` is `load_table(root, "DARK", 128)`. Only `VERIFIED_OBJECT_KINDS` are drawn.
/// Not yet modelled: runtime state (turret aiming, destroyed/active), linked partner records, kind 1 and other kinds.
pub fn draw_ground_objects(frame: &mut Frame, level: &Level, bank: &mut SpriteBank, shadow_lut: &[u8], scroll_y: i32) -> Result<()> {
    for g in &level.ground_sprites {
        if g.sprite_id == 0 || !VERIFIED_OBJECT_KINDS.contains(&g.kind) { continue; }
        let (x, y) = object_screen_pos(level, g, scroll_y);
        if y >= frame.height as i32 || y + 64 <= 0 || x >= frame.width as i32 || x + 64 <= 0 { continue; }
        let s = bank.get(g.sprite_id)?;
        if g.casts_shadow { frame.blit_shadow(s, x + SHADOW_OFFSET.0, y + SHADOW_OFFSET.1, shadow_lut); }
        frame.blit_sprite(s, x, y);
    }
    Ok(())
}

/// FNV-1a 64-bit; used to check frames against the Python reference renderer (tests/golden.rs).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
    }

    #[test]
    fn tile_blit_clips_on_all_sides() {
        let mut f = Frame::new(40, 40);
        let tile = vec![7u8; 1024];
        f.blit_tile(&tile, -16, -16);
        assert_eq!(f.pixels[0], 7);
        assert_eq!(f.pixels[15 * 40 + 15], 7);
        assert_eq!(f.pixels[16 * 40 + 16], 0);
        f.blit_tile(&tile, 30, 30);
        assert_eq!(f.pixels[39 * 40 + 39], 7);
    }

    #[test]
    fn vertical_flip_reverses_rows() {
        let mut tile = vec![0u8; 1024];
        for c in 0..32 { tile[c] = 1; tile[31 * 32 + c] = 2; } // top row 1, bottom row 2
        let mut f = Frame::new(32, 32);
        f.blit_tile_v(&tile, 0, 0, true);
        assert_eq!(f.pixels[0], 2);
        assert_eq!(f.pixels[31 * 32], 1);
    }
}
