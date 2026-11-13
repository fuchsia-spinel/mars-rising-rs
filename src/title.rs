//! The title screen: its pictures, and the state machine that drives the verified button logic of `menu.rs` from a pointer, a keyboard or a pad.
//!
//! What is verified against the original executable is in `menu.rs` (which button is under the pointer, hover, press tracking, the sounds and the
//! drawing requests; `tests/menu_golden.rs`). What is *not* verified and is stated here as the assumption it is:
//! * the picture of the window: the background picture 9000, the logo 9300 in the 'Rect' 1 of the interface file (133, 116) and the buttons in
//!   the application's 'Rect' 1000..1010 are placed where the resources say (the original init `0x1aa68` draws them in this order);
//! * the transparency of the buttons: the original copies them with QuickDraw transfer mode 36 ("transparent": pixels of the background colour are
//!   not drawn). The extracted pictures are opaque, the "background" is the green (0, 238, 0) of the normal pictures and the white of most hilited
//!   ones (the original has only one value because the pictures are remapped to the game's palette), so both are treated as transparent;
//! * the button pictures are drawn 1:1. The original draws each picture scaled into a slightly larger off-screen slot (e.g. 76 x 29 into 79 x 32,
//!   `DrawPicture` with a destination rectangle) and copies it back scaled to the screen rectangle; the net effect is the same picture, resampled twice.
//!
//! Not on the title screen yet: the rotating text under the logo (pictures 9001 / 9002 and the idle cycle), the copyright box, the dialogs behind the
//! other buttons (the Credits slideshow is `credits.rs`), and the option-key level chooser.
use crate::assets::{read_png, Decoded, Error, Result};
use crate::menu::{action, Action, Menu, Op, Press, Rect};
use std::path::{Path, PathBuf};

pub const W: usize = 640;
pub const H: usize = 480;
/// Top-left corner of the logo picture (the interface file's 'Rect' 1, 133, 116 - 339, 301).
pub const LOGO_POS: (i32, i32) = (133, 116);
/// Picture ids of the normal button pictures, indexed like the screen rectangles (resource 1000 + k); the hilited picture is 100 higher.
pub const BUTTON_PICTURE: [u32; 11] = [9100, 9101, 9102, 9103, 9104, 9105, 9110, 9106, 9107, 9108, 9109];
/// The Controls button (k = 6) lives with the application pictures, the others with the title pictures.
pub const CONTROLS_K: usize = 6;
const GREEN_KEY: [u8; 3] = [0, 238, 0];
const WHITE_KEY: [u8; 3] = [255, 255, 255];

/// A picture with a transparency mask: 0xAARRGGBB, alpha 0 = not drawn.
#[derive(Debug, Clone)]
pub struct Art {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Art {
    /// From a decoded 8 bit RGB / RGBA PNG; pixels equal to one of `keys` become transparent.
    pub fn from_decoded(d: &Decoded, keys: &[[u8; 3]]) -> Result<Art> {
        let n = match d.color {
            png::ColorType::Rgba => 4,
            png::ColorType::Rgb => 3,
            other => return Err(Error::Msg(format!("title picture: unsupported PNG colour type {:?}", other))),
        };
        let (w, h) = (d.width as usize, d.height as usize);
        if d.data.len() != w * h * n {
            return Err(Error::Msg(format!("title picture: {} bytes for {} x {} pixels of {} bytes", d.data.len(), w, h, n)));
        }
        let px = d
            .data
            .chunks_exact(n)
            .map(|p| {
                let rgb = [p[0], p[1], p[2]];
                if keys.contains(&rgb) || (n == 4 && p[3] == 0) {
                    0
                } else {
                    0xFF00_0000 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32
                }
            })
            .collect();
        Ok(Art { w, h, px })
    }

    /// Copy the picture to `out` (`ow` pixels per row) with its top-left corner at (x, y); transparent pixels and everything outside are skipped.
    pub fn blit(&self, out: &mut [u32], ow: usize, x: i32, y: i32) {
        let oh = (out.len() / ow.max(1)) as i32;
        for row in 0..self.h as i32 {
            let yy = y + row;
            if yy < 0 || yy >= oh {
                continue;
            }
            for col in 0..self.w as i32 {
                let xx = x + col;
                if xx < 0 || xx >= ow as i32 {
                    continue;
                }
                let p = self.px[row as usize * self.w + col as usize];
                if p >> 24 != 0 {
                    out[yy as usize * ow + xx as usize] = p;
                }
            }
        }
    }
}

/// Everything the title screen shows.
pub struct TitleArt {
    pub background: Vec<u32>,
    pub logo: Art,
    pub normal: Vec<Option<Art>>,
    pub hilited: Vec<Option<Art>>,
    pub screen: [Rect; 11],
    /// The copyright box: picture 9400 at the 'Rect' 100 (left, top) and the inner 'Rect' 2 (left, top, right, bottom) the text is centred in.
    pub border: Option<(Art, i32, i32)>,
    pub text_box: Option<[i32; 4]>,
    /// The line in the box (`0x1dcc8`'s first stage: the copyright notice, `layout/title_text.txt` -> `string copyright`), set by the host.
    pub line: String,
}

fn find_picture(dir: &Path, id: u32) -> Result<PathBuf> {
    let prefix = format!("{}_", id);
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name.ends_with(".png") {
            return Ok(e.path());
        }
    }
    Err(Error::Msg(format!("picture {} not found in {}", id, dir.display())))
}

/// The eleven button rectangles from `layout/rects.json` (`rects.app["1000".."1010"]`, stored as left, top, right, bottom).
pub fn load_screen_rects(root: &Path) -> Result<[Rect; 11]> {
    let v: serde_json::Value = serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(root.join("layout/rects.json"))?))?;
    let mut out = [Rect::default(); 11];
    for (k, r) in out.iter_mut().enumerate() {
        let e = &v["rects"]["app"][(1000 + k).to_string()];
        let get = |f: &str| e[f].as_i64().and_then(|x| i16::try_from(x).ok()).ok_or_else(|| Error::Msg(format!("layout/rects.json: app {} has no {}", 1000 + k, f)));
        *r = Rect::new(get("top")?, get("left")?, get("bottom")?, get("right")?);
    }
    Ok(out)
}

impl TitleArt {
    pub fn load(root: &Path) -> Result<TitleArt> {
        let titles = root.join("pictures/titles");
        let app = root.join("pictures/app");
        let bg = read_png(&find_picture(&titles, 9000)?)?;
        let bg = Art::from_decoded(&bg, &[])?;
        if (bg.w, bg.h) != (W, H) {
            return Err(Error::Msg(format!("background picture is {} x {}, expected {} x {}", bg.w, bg.h, W, H)));
        }
        let logo = Art::from_decoded(&read_png(&find_picture(&titles, 9300)?)?, &[GREEN_KEY])?;
        let mut normal = Vec::new();
        let mut hilited = Vec::new();
        for (k, id) in BUTTON_PICTURE.iter().enumerate() {
            let dir = if k == CONTROLS_K { &app } else { &titles };
            let load = |id: u32| -> Result<Art> { Art::from_decoded(&read_png(&find_picture(dir, id)?)?, &[GREEN_KEY, WHITE_KEY]) };
            let (n, h) = (load(*id), load(*id + 100));
            normal.push(n.ok());
            hilited.push(h.ok());
        }
        // the buttons the original tests (1 Player, 2 Players, Register, Controls, Preferences, High Scores, Credits, Quit) need both pictures
        for k in [0usize, 1, 5, 6, 7, 8, 9, 10] {
            if normal[k].is_none() || hilited[k].is_none() {
                return Err(Error::Msg(format!("button picture {} (or its hilited version) is missing", BUTTON_PICTURE[k])));
            }
        }
        // the copyright box (0x1aa68 draws picture 9400 into 'Rect' 100; 0x1dcc8 darkens 'Rect' 2 and writes the line): optional, the title works without it
        let (mut border, mut text_box) = (None, None);
        if let (Ok(p), Ok(v)) = (find_picture(&titles, 9400).and_then(|p| read_png(&p)).and_then(|d| Art::from_decoded(&d, &[])),
                                  std::fs::File::open(root.join("layout/rects.json")).map_err(Error::from).and_then(|f| serde_json::from_reader::<_, serde_json::Value>(std::io::BufReader::new(f)).map_err(Error::from))) {
            let get = |id: &str, f: &str| v["rects"]["titles"][id][f].as_i64().map(|x| x as i32);
            if let (Some(l), Some(t)) = (get("100", "left"), get("100", "top")) { border = Some((p, l, t)); }
            if let (Some(l), Some(t), Some(r), Some(b)) = (get("2", "left"), get("2", "top"), get("2", "right"), get("2", "bottom")) { text_box = Some([l, t, r, b]); }
        }
        Ok(TitleArt { background: bg.px.iter().map(|p| p | 0xFF00_0000).collect(), logo, normal, hilited, screen: load_screen_rects(root)?, border, text_box, line: String::new() })
    }

    /// The picture of the title screen: the background, the logo and the buttons (`shown[k]` = hilited). `out` is `W * H` pixels.
    pub fn compose(&self, shown: &[bool; 11], registered: bool, out: &mut [u32]) {
        out[..W * H].copy_from_slice(&self.background);
        self.logo.blit(out, W, LOGO_POS.0, LOGO_POS.1);
        for k in [0usize, 1, 5, 6, 7, 8, 9, 10] {
            if k == 5 && registered {
                continue;
            }
            let art = if shown[k] { &self.hilited[k] } else { &self.normal[k] };
            if let Some(a) = art {
                a.blit(out, W, self.screen[k].left as i32, self.screen[k].top as i32);
            }
        }
        if let Some((b, x, y)) = &self.border {
            b.blit(out, W, *x, *y);
        }
    }

    /// `0x1dcc8`: the box inside the border is darkened (`PaintRect` in the blend mode with black and the `OpColor` 50000 / 65535: the pixel keeps
    /// 1 - 50000 / 65535 of its value; the port assumes that reading of the transfer mode) and `text` is written centred in it, 7 pixels above its bottom edge.
    pub fn draw_box_text(&self, out: &mut [u32], text: &str, color: u32) {
        let Some([l, t, r, b]) = self.text_box else { return; };
        for y in t.max(0)..b.min(H as i32) {
            for x in l.max(0)..r.min(W as i32) {
                let p = &mut out[y as usize * W + x as usize];
                let f = |c: u32| ((c & 0xff) * 15535 / 65535) & 0xff;
                *p = 0xFF00_0000 | f(*p >> 16) << 16 | f(*p >> 8) << 8 | f(*p);
            }
        }
        // the port's 5x7 font is wider than Geneva 9: a space is 3 pixels so that the 56 character notice fits the 322 pixel box
        let words: Vec<&str> = text.split(' ').collect();
        let wide = |t: &str| crate::font::text_width(t, 1) as i32 - 1;
        let total: i32 = words.iter().map(|w| if w.is_empty() { 0 } else { wide(w) }).sum::<i32>() + 3 * (words.len() as i32 - 1);
        let mut x = l + (r - l - total) / 2;
        for w in words {
            crate::font::draw_text(out, W, x, b - 7 - crate::font::GLYPH_H as i32, w, 1, color);
            x += wide(w).max(0) + 3;
        }
    }
}

/// The pictures and data of the Credits slideshow (`credits.rs`): font sheets 9001 / 9002 (white = transparent), 'IMAG' 128, the glyph table and the pages
/// from `layout/title_text.txt`.
pub fn load_credits(root: &Path, background: &[u32]) -> Result<(std::rc::Rc<crate::credits::CreditsData>, crate::credits::CreditsArt)> {
    use crate::credits::{CreditsArt, CreditsData, Sheet};
    let titles = root.join("pictures/titles");
    let app = root.join("pictures/app");
    let path = root.join("layout/title_text.txt");
    let text = std::fs::read_to_string(&path).map_err(|_| Error::Msg(format!("{} is missing: run the extractor (tools/mr_extract.py) again", path.display())))?;
    let data = CreditsData::parse(&text).map_err(Error::Msg)?;
    let sheet = |dir: &Path, id: u32, keys: &[[u8; 3]]| -> Result<Sheet> {
        let a = Art::from_decoded(&read_png(&find_picture(dir, id)?)?, keys)?;
        Ok(Sheet { w: a.w, h: a.h, px: a.px })
    };
    let imag = {
        let dir = std::fs::read_dir(&app)?
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_string_lossy().starts_with("imag_128_"))
            .ok_or_else(|| Error::Msg("pictures/app/imag_128_*.png is missing".into()))?;
        let a = Art::from_decoded(&read_png(&dir.path())?, &[])?;
        Sheet { w: a.w, h: a.h, px: a.px }
    };
    let art = CreditsArt { background: background.to_vec(), font: data.font.clone(), normal: sheet(&titles, 9001, &[WHITE_KEY])?, hilited: sheet(&titles, 9002, &[WHITE_KEY])?, imag128: imag };
    Ok((std::rc::Rc::new(data), art))
}

/// The picture of the ending (`0xe098(6000)`: 'IMAG' 130, `DrawPicture` at the top left of the black 640 x 480 window), as `0xAARRGGBB`.
pub fn load_ending(root: &Path) -> Result<Vec<u32>> {
    let app = root.join("pictures/app");
    let dir = std::fs::read_dir(&app)?
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().starts_with("imag_130_"))
        .ok_or_else(|| Error::Msg("pictures/app/imag_130_*.png is missing".into()))?;
    let a = Art::from_decoded(&read_png(&dir.path())?, &[])?;
    Ok(crate::overlay::ending_screen(&a.px, a.w, a.h))
}

/// The two player portraits of the high score name entry (pictures 11000 / 11001, 48 x 48).
pub fn load_portraits(root: &Path) -> Result<[crate::credits::Sheet; 2]> {
    let titles = root.join("pictures/titles");
    let one = |id: u32| -> Result<crate::credits::Sheet> {
        let a = Art::from_decoded(&read_png(&find_picture(&titles, id)?)?, &[])?;
        Ok(crate::credits::Sheet { w: a.w, h: a.h, px: a.px })
    };
    Ok([one(11000)?, one(11001)?])
}

/// The title screen's behaviour.
#[derive(Debug, Clone)]
pub struct Title {
    pub menu: Menu,
    /// Which buttons are drawn hilited (what the original's `CopyBits` calls leave on the screen).
    pub shown: [bool; 11],
    /// The picture changed since the flag was last cleared.
    pub dirty: bool,
    press: Option<Press>,
    down_prev: bool,
    last_pointer: Option<(i32, i32)>,
    sounds: Vec<(i32, i32, i32)>,
    idle_reset: bool,
}

impl Title {
    pub fn new(screen: [Rect; 11], registered: bool) -> Title {
        Title { menu: Menu::new(screen, registered, (0, 0)), shown: [false; 11], dirty: true, press: None, down_prev: false, last_pointer: None, sounds: Vec::new(), idle_reset: false }
    }

    fn apply(&mut self, ops: Vec<Op>) {
        for op in ops {
            match op {
                Op::Sound { id, prio, delay } => self.sounds.push((id, prio, delay)),
                Op::Blit { .. } => {
                    if let Some((k, h)) = op.button_state() {
                        if self.shown[k] != h {
                            self.shown[k] = h;
                            self.dirty = true;
                        }
                    }
                }
                Op::ResetIdle => self.idle_reset = true,
                Op::Refresh => {}
            }
        }
    }

    /// The main loop's idle timer (`r26`) was restarted (entering / leaving a button, a click) since the last call.
    pub fn take_idle_reset(&mut self) -> bool {
        std::mem::take(&mut self.idle_reset)
    }

    /// The sounds requested since the last call: (id, priority, delay) for `Mixer::play`.
    pub fn take_sounds(&mut self) -> Vec<(i32, i32, i32)> {
        std::mem::take(&mut self.sounds)
    }

    /// One look at the pointer (picture coordinates, left button state). A click is reported when the button is released inside the rectangle it
    /// was pressed in. The hover logic runs when the pointer moved (and no click is in progress), like the pointer-tracking block of the main loop.
    pub fn pointer(&mut self, h: i32, v: i32, down: bool) -> Option<Action> {
        let moved = self.last_pointer != Some((h, v));
        self.last_pointer = Some((h, v));
        let mut result = None;
        if let Some(p) = self.press {
            let (ops, done) = p.step(&self.menu, h, v, down);
            self.apply(ops);
            if let Some(inside) = done {
                self.press = None;
                if inside {
                    result = action(p.idx);
                }
            }
        } else if down && !self.down_prev {
            let idx = self.menu.hit(h, v);
            if idx != 0 {
                let (ops, p) = self.menu.press(idx);
                self.apply(ops);
                self.press = p;
                if let Some(p) = p {
                    let (ops, _) = p.step(&self.menu, h, v, true);
                    self.apply(ops);
                }
            }
        } else if moved && !down {
            let ops = self.menu.hover_to(h, v);
            self.apply(ops);
        }
        self.down_prev = down;
        result
    }

    /// Keyboard / pad: move the focus (an addition of the port).
    pub fn key_move(&mut self, delta: i32) {
        let ops = self.menu.focus_step(delta);
        self.apply(ops);
    }

    /// Keyboard / pad: press the focused button (press sound, then its action); `None` when nothing is focused.
    pub fn key_activate(&mut self) -> Option<Action> {
        let idx = self.menu.hover;
        if idx == 0 {
            return None;
        }
        let (ops, _) = self.menu.press(idx);
        self.apply(ops);
        action(idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rects() -> [Rect; 11] {
        let l = |t: i16, l: i16, b: i16, r: i16| Rect::new(t, l, b, r);
        [l(71, 428, 100, 504), l(115, 428, 144, 507), l(214, 428, 262, 589), l(287, 428, 335, 512), l(159, 428, 188, 484), l(159, 428, 188, 505),
         l(203, 428, 232, 505), l(247, 428, 276, 540), l(291, 428, 320, 533), l(335, 428, 364, 494), l(379, 428, 411, 464)]
    }

    fn title() -> Title {
        Title::new(rects(), true)
    }

    #[test]
    fn a_click_inside_starts_the_action_and_plays_the_verified_sounds() {
        let mut t = title();
        assert_eq!(t.pointer(450, 80, false), None);
        assert!(t.shown[0] && t.dirty);
        assert_eq!(t.take_sounds(), vec![(20, 3, 0)]);
        assert_eq!(t.pointer(450, 80, true), None);
        assert_eq!(t.take_sounds(), vec![(29, 3, 0)]);
        assert!(t.shown[0], "hilited while pressed inside");
        assert_eq!(t.pointer(450, 80, true), None);
        assert_eq!(t.pointer(450, 80, false), Some(Action::OnePlayer));
        assert!(!t.shown[0], "drawn normal after the release (like the original)");
        assert!(t.take_sounds().is_empty());
    }

    #[test]
    fn leaving_the_button_while_pressed_cancels_the_click() {
        let mut t = title();
        t.pointer(450, 120, false);
        t.take_sounds();
        t.pointer(450, 120, true);
        assert!(t.shown[1]);
        t.pointer(300, 300, true);
        assert!(!t.shown[1], "normal while the pointer is outside");
        t.pointer(450, 120, true);
        assert!(t.shown[1], "hilited again when it comes back");
        t.pointer(300, 300, true);
        assert_eq!(t.pointer(300, 300, false), None);
        // the same press released inside counts
        t.pointer(450, 120, true);
        assert_eq!(t.pointer(450, 120, false), Some(Action::TwoPlayers));
    }

    #[test]
    fn pressing_outside_every_button_and_unused_rectangles_do_nothing() {
        let mut t = title();
        assert_eq!(t.pointer(10, 10, true), None);
        assert_eq!(t.pointer(10, 10, false), None);
        assert!(t.take_sounds().is_empty());
        // the Host rectangle (ids 3..5) is not tested by the original
        assert_eq!(t.pointer(450, 240, true), None);
        assert_eq!(t.pointer(450, 240, false), None);
        assert!(t.take_sounds().is_empty());
    }

    #[test]
    fn dragging_onto_a_button_with_the_mouse_down_does_not_press_it() {
        let mut t = title();
        t.pointer(10, 10, true);
        assert_eq!(t.pointer(450, 80, true), None);
        assert_eq!(t.pointer(450, 80, false), None);
        assert!(t.take_sounds().is_empty());
    }

    #[test]
    fn keyboard_focus_and_activation() {
        let mut t = title();
        assert_eq!(t.key_activate(), None, "nothing focused");
        t.key_move(1);
        assert!(t.shown[0]);
        t.key_move(1);
        assert!(t.shown[1] && !t.shown[0]);
        t.take_sounds();
        assert_eq!(t.key_activate(), Some(Action::TwoPlayers));
        assert_eq!(t.take_sounds(), vec![(29, 3, 0)]);
        t.key_move(-1);
        t.key_move(-1);
        assert_eq!(t.menu.hover, 11, "wraps to Quit");
        assert_eq!(t.key_activate(), Some(Action::Quit));
    }

    #[test]
    fn art_keys_and_blits_are_clipped() {
        let d = Decoded { width: 2, height: 2, color: png::ColorType::Rgb, data: vec![0, 238, 0, 1, 2, 3, 255, 255, 255, 9, 9, 9] };
        let a = Art::from_decoded(&d, &[GREEN_KEY, WHITE_KEY]).unwrap();
        assert_eq!(a.px, vec![0, 0xFF01_0203, 0, 0xFF09_0909]);
        let mut out = vec![0xFF11_1111u32; 4 * 3];
        a.blit(&mut out, 4, 3, 2); // only the top-left pixel of the art is inside, and it is transparent
        assert!(out.iter().all(|&p| p == 0xFF11_1111));
        a.blit(&mut out, 4, 2, 1);
        assert_eq!(out[1 * 4 + 3], 0xFF01_0203);
        assert_eq!(out[2 * 4 + 2], 0xFF11_1111);
        assert_eq!(out[2 * 4 + 3], 0xFF09_0909);
        a.blit(&mut out, 4, -5, -5);
        a.blit(&mut out, 4, 100, 100);
        let bad = Decoded { width: 2, height: 2, color: png::ColorType::Rgb, data: vec![0; 5] };
        assert!(Art::from_decoded(&bad, &[]).is_err());
        let gray = Decoded { width: 1, height: 1, color: png::ColorType::Grayscale, data: vec![0] };
        assert!(Art::from_decoded(&gray, &[]).is_err());
    }

    #[test]
    fn compose_draws_the_hilited_picture_only_for_the_shown_button() {
        let solid = |w: usize, h: usize, c: u32| Art { w, h, px: vec![c; w * h] };
        let screen = rects();
        let mut normal: Vec<Option<Art>> = Vec::new();
        let mut hilited: Vec<Option<Art>> = Vec::new();
        for k in 0..11 {
            normal.push(Some(solid(screen[k].width() as usize, screen[k].height() as usize, 0xFF00_0001)));
            hilited.push(Some(solid(screen[k].width() as usize, screen[k].height() as usize, 0xFF00_0002)));
        }
        let art = TitleArt { background: vec![0xFF00_0000; W * H], logo: solid(206, 185, 0xFF00_0003), normal, hilited, screen, border: None, text_box: None, line: String::new() };
        let mut out = vec![0u32; W * H];
        let mut shown = [false; 11];
        shown[1] = true;
        art.compose(&shown, true, &mut out);
        assert_eq!(out[(71) * W + 428], 0xFF00_0001, "1 Player normal");
        assert_eq!(out[(115) * W + 428], 0xFF00_0002, "2 Players hilited");
        assert_eq!(out[(116) * W + 133], 0xFF00_0003, "logo corner");
        assert_eq!(out[(159) * W + 428], 0xFF00_0000, "no Register button in a registered copy (and the Editor rectangle is not drawn)");
        art.compose(&shown, false, &mut out);
        assert_eq!(out[(159) * W + 428], 0xFF00_0001, "Register button drawn");
        assert_eq!(out[(379) * W + 428], 0xFF00_0001, "Quit");
    }
}
