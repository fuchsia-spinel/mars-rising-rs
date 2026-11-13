//! Software blitters for the draw commands of `draw.rs` (std-only).
//!
//! The original stores every sprite as a run stream (maximal runs of opaque / transparent pixels per row, every row covering the full width)
//! and has one blitter per effect. This module reproduces each blitter pixel for pixel on palette-indexed pixels; the run structure is rebuilt
//! from the opacity mask (all 1248 original streams use maximal, non-empty, alternating runs, so the mask determines it exactly).
//! Verified against the original routines by `tests/blit_golden.rs` (600 vectors from the emulator, every prim).
//!
//! * `Anim`   (wrapper 0x2f88, blitter 0x3f08): opaque pixels are copied.
//! * `Trans`  (0x3170, 0x4078): 50 % blend through the `TRAN` table, `blend[below * 256 + sprite]`.
//! * `Dither` (0x3264, 0x486c): checkerboard. The original keeps a parity flag that starts at 0 for each sprite and flips for every pixel
//!   (every odd-length run, every row end); a run is copied in 4-pixel and 2-pixel chunks that take the odd (parity 1) or even (parity 0)
//!   positions, and the single leftover pixel of an odd-length run is copied when the parity is 1 -- the opposite phase. This quirk is kept.
//! * `Flash`  (0x3540, 0x4a90): the pixels under the sprite's opaque area are replaced through the orange `DARK 129` table.
//! * `Shadow` (0x344c, 0x46a8): the sprite is drawn squashed -- every third column (`col % 3 == 2`) and every third row from the fourth on
//!   (`row % 3 == 0`, `row >= 3`) are dropped and the rest closed up, so the shadow covers 2/3 x 3/4 of the sprite's size -- and the pixels
//!   under it are replaced through the `DARK 128` "System Shadow" table.
//!
//! * `Cast`   (0x3358, 0x444c): the same recolouring without the squashing: the drop shadow of ground objects, the sprite's own silhouette.
//!
//! The wrappers add a 64 px margin and reject (rather than clip) a sprite that crosses the edge of the 704 x 608 off-screen buffer; as every
//! sprite is at most 64 x 64 that only ever rejects sprites that are completely outside the playfield, so here every pixel is simply clipped.
use crate::draw::Prim;

/// Borrowed view of a sprite: palette indices plus an opacity mask (index 0 is a real colour, so transparency cannot live in the indices).
pub struct SpriteView<'a> {
    pub width: usize,
    pub height: usize,
    pub index: &'a [u8],
    pub opaque: &'a [bool],
}

/// The lookup tables the effects need.
pub struct Luts<'a> {
    /// `DARK 129` orange tint (256 entries).
    pub flash: &'a [u8],
    /// `DARK 128` shadow (256 entries).
    pub shadow: &'a [u8],
    /// `TRAN 1000` 50 % blend (65536 entries). May be empty: `Trans` then falls back to the dither (what the game does with translucency off).
    pub blend: &'a [u8],
}

/// Draw `s` with its top-left corner at (x, y) into the palette-indexed `canvas` (`cw` x `ch`), clipped to the canvas.
pub fn blit(canvas: &mut [u8], cw: usize, ch: usize, s: &SpriteView, x: i32, y: i32, prim: Prim, luts: &Luts) {
    let prim = if prim == Prim::Trans && luts.blend.len() < 65536 { Prim::Dither } else { prim };
    let w = s.width;
    let mut parity = false; // dither phase, see the module comment
    for row in 0..s.height {
        let mut col = 0;
        while col < w {
            let opaque = s.opaque[row * w + col];
            let mut end = col;
            while end < w && s.opaque[row * w + end] == opaque { end += 1; }
            let n = end - col;
            if opaque {
                for k in 0..n {
                    let c = col + k;
                    let si = row * w + c;
                    let (dx, dy) = match prim {
                        Prim::Shadow => {
                            if (row >= 3 && row % 3 == 0) || c % 3 == 2 { continue; }
                            (x + (c - c / 3) as i32, y + (row - row / 3) as i32)
                        }
                        _ => (x + c as i32, y + row as i32),
                    };
                    if prim == Prim::Dither {
                        let tail = n % 2 == 1 && k == n - 1;
                        let copy = if tail { parity } else { (k % 2 == 1) == parity };
                        if !copy { continue; }
                    }
                    if dx < 0 || dy < 0 || dx >= cw as i32 || dy >= ch as i32 { continue; }
                    let d = &mut canvas[dy as usize * cw + dx as usize];
                    *d = match prim {
                        Prim::Anim | Prim::Dither | Prim::Pixels => s.index[si],
                        Prim::Trans => luts.blend[(*d as usize) * 256 + s.index[si] as usize],
                        Prim::Flash => luts.flash[*d as usize],
                        Prim::Shadow | Prim::Cast => luts.shadow[*d as usize],
                    };
                }
            }
            if n % 2 == 1 { parity = !parity; }
            col = end;
        }
        parity = !parity;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lut(f: impl Fn(usize) -> u8) -> Vec<u8> { (0..256).map(f).collect() }

    #[test]
    fn effects_on_a_small_sprite() {
        let flash = lut(|i| (i as u8).wrapping_add(100));
        let shadow = lut(|i| (i as u8) / 2);
        let blend: Vec<u8> = (0..65536usize).map(|i| (((i >> 8) + (i & 255)) / 2) as u8).collect();
        let luts = Luts { flash: &flash, shadow: &shadow, blend: &blend };
        // 3x2 sprite, one transparent pixel
        let idx = [10u8, 20, 30, 40, 50, 60];
        let opq = [true, true, false, true, true, true];
        let s = SpriteView { width: 3, height: 2, index: &idx, opaque: &opq };
        let mk = || vec![200u8; 5 * 4];
        let mut c = mk();
        blit(&mut c, 5, 4, &s, 1, 1, Prim::Anim, &luts);
        assert_eq!(&c[5 + 1..5 + 4], &[10, 20, 200]);
        assert_eq!(&c[10 + 1..10 + 4], &[40, 50, 60]);
        let mut c = mk();
        blit(&mut c, 5, 4, &s, 1, 1, Prim::Trans, &luts);
        assert_eq!(&c[5 + 1..5 + 3], &[105, 110]);
        let mut c = mk();
        blit(&mut c, 5, 4, &s, 1, 1, Prim::Flash, &luts);
        assert_eq!(&c[5 + 1..5 + 4], &[44, 44, 200]); // 200 + 100 wraps to 44
        // dither: the parity starts at 0, so a 2-pixel chunk copies its first pixel; the 1-pixel skip flips the parity and the row end flips it
        // back, so row 1 starts at 0 again: its 2-pixel chunk copies the first pixel and the odd tail (parity 0) is not copied
        let mut c = mk();
        blit(&mut c, 5, 4, &s, 1, 1, Prim::Dither, &luts);
        assert_eq!(&c[5 + 1..5 + 4], &[10, 200, 200]);
        assert_eq!(&c[10 + 1..10 + 4], &[40, 200, 200]);
    }

    #[test]
    fn shadow_is_squashed_to_two_thirds_by_three_quarters() {
        let ident = lut(|i| i as u8);
        let shadow = lut(|i| (i as u8).wrapping_add(1));
        let luts = Luts { flash: &ident, shadow: &shadow, blend: &[] };
        let (w, h) = (6usize, 7usize);
        let idx = vec![9u8; w * h];
        let opq = vec![true; w * h];
        let s = SpriteView { width: w, height: h, index: &idx, opaque: &opq };
        let mut c = vec![0u8; 10 * 10];
        blit(&mut c, 10, 10, &s, 2, 1, Prim::Shadow, &luts);
        let mut touched = 0;
        for yy in 0..10 { for xx in 0..10 { if c[yy * 10 + xx] != 0 { touched += 1; } } }
        // 6 columns -> 4, 7 rows -> 5 (rows 3 and 6 dropped)
        assert_eq!(touched, 4 * 5);
        let at = |x: usize, y: usize| c[y * 10 + x];
        assert_eq!(at(2, 1), 1);
        assert_eq!(at(5, 1), 1);
        assert_eq!(at(6, 1), 0);
        assert_eq!(at(2, 5), 1);
        assert_eq!(at(2, 6), 0);
    }

    #[test]
    fn clipping_on_every_side_and_missing_blend_table() {
        let flash = lut(|i| i as u8);
        let luts = Luts { flash: &flash, shadow: &flash, blend: &[] };
        let idx = [7u8; 16];
        let opq = [true; 16];
        let s = SpriteView { width: 4, height: 4, index: &idx, opaque: &opq };
        let mut c = vec![0u8; 6 * 6];
        blit(&mut c, 6, 6, &s, -2, -2, Prim::Anim, &luts);
        assert_eq!(c[0], 7);
        assert_eq!(c[2], 0);
        blit(&mut c, 6, 6, &s, 4, 4, Prim::Anim, &luts);
        assert_eq!(c[35], 7);
        // no blend table: Trans degrades to the dither
        let mut c1 = vec![0u8; 16];
        let mut c2 = vec![0u8; 16];
        blit(&mut c1, 4, 4, &s, 0, 0, Prim::Trans, &luts);
        blit(&mut c2, 4, 4, &s, 0, 0, Prim::Dither, &luts);
        assert_eq!(c1, c2);
    }
}
