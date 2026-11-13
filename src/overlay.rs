//! The in-game message overlay: `0x28518`, called once per frame by the draw part of the session loop (0xd384, right after the ships' score and charge
//! bar), with its state byte `D.172c` (set by `0x28510(n)`, read by `0x28508`) and its y position `D.1728` (206, set by the level reset `0x284a8`).
//!
//! | state | when it is set | what is drawn |
//! |---|---|---|
//! | 0 | the level reset; a ship goes into play (not in a demo) | nothing |
//! | 1 | the level starts (0xdb04) | "Sector N" (highlighted font, y = 206) and "Get Ready!" (y = 246) |
//! | 2 | the pause key is pressed (0xcf78, the old state is put back when the game goes on, 0xdfac) | "Game Paused" and "Press CAPS LOCK to Resume" |
//! | 3 | the level starts in a demo (0xdaf8); the last ship was destroyed (0xcc0c, 0xcc38, 0xceb0; the port: `Ship::session_step`, with sound 28) | two sprites: animation 34, frames 1 and 2, 64 x 33 at x = 153 and 217, y = 206 (GAME OVER) |
//! | 4 | five rows before the end of the level, after sound 21 (0xc568) | "Sector N Cleared" (highlighted, y = 220) |
//!
//! The texts are centred with `0x29054(string, -2, y, hilite, 0)` (`credits::layout_text`); `N` is `D.13ec` (`NumToString`). The sprites go through `0x3170` when the
//! translucency option is on (`0x29a4`) and through `0x2f88` otherwise. The strings are the ones in the data segment (0x6901 "Sector ", 0x6a01 "Get Ready!",
//! 0x6a0c "Game Paused", 0x6a18 "Press CAPS LOCK to Resume", 0x6a32 " Cleared").

use crate::credits::{layout_text, Blit, CreditsArt, CreditsData, Sheet};
use crate::draw::{Draw, Prim};

/// `D.1728`: the y position of the messages (the level reset stores 206).
pub const Y: i32 = 206;

/// The pause key of the original is Caps Lock; the port pauses with this key and says so in the text.
pub const RESUME_KEY: &str = "P";

pub const NONE: u8 = 0;
pub const GET_READY: u8 = 1;
pub const PAUSED: u8 = 2;
pub const GAME_OVER: u8 = 3;
pub const CLEARED: u8 = 4;

/// One line of text: the bytes, the y position given to `0x29054`, the highlighted font.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub text: Vec<u8>,
    pub y: i32,
    pub hilite: bool,
}

fn sector(level: i32) -> Vec<u8> { format!("Sector {}", level).into_bytes() }

/// The text lines of a state (empty for 0 and 3). `level` is `D.13ec`.
pub fn lines(state: u8, level: i32) -> Vec<Line> {
    match state {
        GET_READY => vec![
            Line { text: sector(level), y: Y, hilite: true },
            Line { text: b"Get Ready!".to_vec(), y: Y + 40, hilite: false },
        ],
        PAUSED => vec![
            Line { text: b"Game Paused".to_vec(), y: Y, hilite: true },
            Line { text: format!("Press {} to Resume", RESUME_KEY).into_bytes(), y: Y + 40, hilite: false },
        ],
        CLEARED => {
            let mut t = sector(level);
            t.extend_from_slice(b" Cleared");
            vec![Line { text: t, y: 220, hilite: true }]
        }
        _ => Vec::new(),
    }
}

/// The two GAME OVER sprites of state 3 (the draw list entries; `translucent` is the option tested by 0x29a4).
pub fn sprites(state: u8, translucent: bool) -> Vec<Draw> {
    if state != GAME_OVER { return Vec::new(); }
    let prim = if translucent { Prim::Trans } else { Prim::Anim };
    [(153, 1), (153 + 64, 2)].iter().map(|&(x, frame)| Draw { prim, x, y: Y, w: 64, h: 33, anim: 34, frame }).collect()
}

/// The glyph copies of a line, in the coordinates of the original's off-screen buffer (the playfield starts at 64, 64).
pub fn blits(data: &CreditsData, l: &Line) -> Vec<Blit> { layout_text(&data.font, &l.text, -2, l.y, l.hilite, false) }

/// Draws the lines of `state` into a picture `stride` pixels wide whose (0, 0) is the playfield's top left (the off-screen buffer minus 64, 64).
pub fn draw_text(buf: &mut [u32], stride: usize, art: &CreditsArt, data: &CreditsData, state: u8, level: i32) {
    for l in lines(state, level) {
        for b in blits(data, &l) {
            if b.src[0] >= 52 {
                copy(buf, stride, &art.hilited, [b.src[0] - 52, b.src[1], b.src[2] - 52, b.src[3]], b.dst);
            } else {
                copy(buf, stride, &art.normal, b.src, b.dst);
            }
        }
    }
}

type Rect = [i16; 4];

fn copy(buf: &mut [u32], stride: usize, sheet: &Sheet, src: Rect, dst: Rect) {
    let h = buf.len() / stride;
    let (sw, sh) = ((src[3] as i32 - src[1] as i32).min(dst[3] as i32 - dst[1] as i32), (src[2] as i32 - src[0] as i32).min(dst[2] as i32 - dst[0] as i32));
    for r in 0..sh.max(0) {
        let (sy, dy) = (src[0] as i32 + r, dst[0] as i32 + r - 64);
        if sy < 0 || sy >= sheet.h as i32 || dy < 0 || dy >= h as i32 { continue; }
        for c in 0..sw.max(0) {
            let (sx, dx) = (src[1] as i32 + c, dst[1] as i32 + c - 64);
            if sx < 0 || sx >= sheet.w as i32 || dx < 0 || dx >= stride as i32 { continue; }
            let p = sheet.px[sy as usize * sheet.w + sx as usize];
            if p >> 24 != 0 { buf[dy as usize * stride + dx as usize] = p; }
        }
    }
}

/// The ending (`0xe098(6000)`): the window is painted black (`0x1650`), picture 130 is drawn at its top left (`0x1898`), sound 71, then the picture waits for
/// `ENDING_TICKS` ticks (1/60 s) or a mouse click or key press (events 1 and 3 only; the event queue was flushed before). A click or key plays sound 29, then
/// sound 56 plays three times (delays 0, 1, 2) and the end of game flow follows.
pub const ENDING_TICKS: u32 = 3200;

/// The 640 x 480 window with the picture (`w` x `h`, `0xAARRGGBB`) at its top left on black.
pub fn ending_screen(px: &[u32], w: usize, h: usize) -> Vec<u32> {
    let mut out = vec![0xFF00_0000u32; 640 * 480];
    for y in 0..h.min(480) {
        for x in 0..w.min(640) {
            let p = px[y * w + x];
            if p >> 24 != 0 { out[y * 640 + x] = p | 0xFF00_0000; }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credits::FontTable;

    #[test]
    fn texts_of_the_states() {
        assert!(lines(NONE, 3).is_empty() && lines(GAME_OVER, 3).is_empty());
        let l = lines(GET_READY, 7);
        assert_eq!((l[0].text.as_slice(), l[0].y, l[0].hilite), (&b"Sector 7"[..], 206, true));
        assert_eq!((l[1].text.as_slice(), l[1].y, l[1].hilite), (&b"Get Ready!"[..], 246, false));
        let l = lines(CLEARED, 12);
        assert_eq!((l[0].text.as_slice(), l[0].y), (&b"Sector 12 Cleared"[..], 220));
        let l = lines(PAUSED, 1);
        assert_eq!(l[0].text, b"Game Paused");
        assert_eq!(l[1].y, 246);
    }

    #[test]
    fn the_ending_picture_is_drawn_at_the_top_left_on_black() {
        let px = vec![0xFF11_2233u32; 4 * 2];
        let s = ending_screen(&px, 4, 2);
        assert_eq!(s.len(), 640 * 480);
        assert_eq!((s[0], s[3], s[4], s[640], s[643], s[644], s[640 * 480 - 1]), (0xFF11_2233, 0xFF11_2233, 0xFF00_0000, 0xFF11_2233, 0xFF11_2233, 0xFF00_0000, 0xFF00_0000));
        assert_eq!(s[1280], 0xFF00_0000);
    }

    #[test]
    fn game_over_sprites() {
        let s = sprites(GAME_OVER, false);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].x, s[0].y, s[0].w, s[0].h, s[0].anim, s[0].frame, s[0].prim), (153, 206, 64, 33, 34, 1, Prim::Anim));
        assert_eq!((s[1].x, s[1].frame), (217, 2));
        assert!(sprites(GAME_OVER, true).iter().all(|d| d.prim == Prim::Trans));
        assert!(sprites(CLEARED, false).is_empty());
    }

    #[test]
    fn text_is_centred_on_the_playfield() {
        // a font of 10 wide glyphs: "Get Ready!" = 10 glyphs of advance 11 = 110 -> x = (416 - 110) / 2 + 64
        let mut f = FontTable::empty();
        for c in 0..128 { f.rects[c] = [0, 0, 12, 10]; }
        let l = Line { text: b"Get Ready!".to_vec(), y: 246, hilite: false };
        let b = layout_text(&f, &l.text, -2, l.y, false, false);
        assert_eq!(b[0].dst[1] as i32, (416 - 110) / 2 + 64 + 1);
        assert_eq!(b[0].dst[0] as i32, 246 + 64);
    }
}
