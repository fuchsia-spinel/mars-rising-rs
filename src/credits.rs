//! The title screen's Credits slideshow (button 10, routine `0x1ce24` -> `0x10b80`; the pages are drawn by `0x10dcc`).
//!
//! What the original does (read from the executable, verified against it by `tests/credits_golden.rs`):
//!   * the picture of the title window is replaced by a slideshow: for every page the off-screen buffer gets the background (picture 9000) and the page's
//!     text lines, drawn with the game's own bitmap font (`0x29054`: glyph rectangles from the table built by `0x28750` on the sheets 9001 = normal and
//!     9002 = highlighted, which sits 52 lines lower; centred on the 640 wide screen with x = -1; glyph advance = width + 1, or a fixed 12 with the
//!     "mono" flag; white is the transparent colour of the copies), then `0x1bafc(8)` shows it with a band wipe;
//!   * the wipe (`0x1bafc`): two bands of `h` lines, the first at the top (rows 0..h), the second at the bottom, are moved by `h` towards each other and
//!     the rows under them copied from the off-screen buffer to the window, once per tick (1/60 s), until the moved distance reaches `480 - 2h`
//!     (the first rows 0..h and the last h rows are never copied; every row between is copied once by each band);
//!   * a page stays 240 ticks (counted from the end of its wipe; for the first page from the moment the slideshow was started); then the next one comes;
//!   * the pages are chosen by the keys held when the button is released: none = 0..16 (credits), Control = 100..109 (reading), Option = 200..203
//!     (viewing, playing, listening, "not recommended"), Command = 300..302 (the last one is "In memorium" with the picture 'IMAG' 128); Command wins
//!     over Option over Control (the checks of `0x1ce24` run in the order Control, Option, Command and the last one that is down is kept);
//!   * a mouse click or any key ends it (sound 29, priority 3); the key N starts a one player game (`0x1cb80(0)`) instead; mouse-up / key-up / auto-key
//!     events are ignored; events that arrive during a wipe wait for its end; running out of pages ends without sound;
//!   * after the end (but not for N) the title window is drawn again (`0x1aa68`) and shown with `0x1bafc(12)`.
//! The logic works on 0xAARRGGBB pixel buffers of 640 x 480 and on ticks of 1/60 s (`Credits::tick`), so it does not depend on the window.
use std::collections::VecDeque;

pub const W: usize = 640;
pub const H: usize = 480;
/// Rectangle as in QuickDraw: top, left, bottom, right.
pub type Rect = [i16; 4];

/// One glyph copy of `0x29054`: source rectangle on the font sheets (the highlighted sheet starts 52 lines lower), destination rectangle on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blit {
    pub src: Rect,
    pub dst: Rect,
}

/// The glyph rectangle table (`0x28750`): 128 entries, all zero for characters without a glyph.
#[derive(Clone, Debug)]
pub struct FontTable {
    pub rects: [Rect; 128],
}

impl FontTable {
    pub fn empty() -> FontTable { FontTable { rects: [[0; 4]; 128] } }

    fn width(&self, c: u8) -> i32 { self.rects[c as usize][3] as i32 - self.rects[c as usize][1] as i32 }
}

/// `0x29054(string, x, y, hilite, mono)`: the glyph copies for a string. `x` = -1 centres it on the 640 wide screen, -2 on the 416 wide playfield (and moves it
/// 64 lines down). Bytes above 127 are skipped (no advance). The rectangles are 16 bit values as in the original.
pub fn layout_text(font: &FontTable, s: &[u8], x: i32, y: i32, hilite: bool, mono: bool) -> Vec<Blit> {
    let mut x = x;
    let mut y = y;
    if x == -1 || x == -2 {
        let total: i32 = s.iter().filter(|&&c| c < 128).map(|&c| font.width(c) + 1).sum();
        let total = total as i16 as i32;
        if x == -1 { x = (640 - total) / 2; } else { x = (416 - total) / 2 + 64; y += 64; }
    }
    let mut out = Vec::new();
    for &c in s {
        if c >= 128 { continue; }
        let r = font.rects[c as usize];
        let gx = x + 1;
        let (w, h) = (r[3] as i32 - r[1] as i32, r[2] as i32 - r[0] as i32);
        let off = if hilite { 52 } else { 0 };
        out.push(Blit {
            src: [(r[0] as i32 + off) as i16, r[1], (r[2] as i32 + off) as i16, r[3]],
            dst: [y as i16, gx as i16, (y + h) as i16, (gx + w) as i16],
        });
        x = if mono { gx + 11 } else { gx + w };
    }
    out
}

/// The band wipe of `0x1bafc(h)`: every `next()` is one tick, giving the two band copies (source rectangle in the off-screen buffer, destination rectangle
/// in the window, which is the source moved by the window's origin).
#[derive(Clone, Debug)]
pub struct Wipe {
    h: i32,
    origin: (i32, i32),
    moved: i32,
}

impl Wipe {
    pub fn new(h: i32, origin: (i32, i32)) -> Wipe { Wipe { h: h.max(1), origin, moved: 0 } }

    /// The copies of the next tick, or `None` when the wipe is over.
    pub fn next(&mut self) -> Option<[(Rect, Rect); 2]> {
        let h = self.h;
        if (self.moved as i16 as i32) >= 480 - 2 * h { return None; }
        self.moved += h;
        let k = self.moved;
        let (ox, oy) = self.origin;
        let top = [k as i16, 0, (k + h) as i16, 640];
        let bottom = [(480 - h - k) as i16, 0, (480 - k) as i16, 640];
        let mv = |r: Rect| -> Rect { [(r[0] as i32 + oy) as i16, (r[1] as i32 + ox) as i16, (r[2] as i32 + oy) as i16, (r[3] as i32 + ox) as i16] };
        Some([(top, mv(top)), (bottom, mv(bottom))])
    }

    /// Whether another tick follows (the original tests this after a copy, so the last copy is not followed by a wait).
    pub fn more(&self) -> bool { (self.moved as i16 as i32) < 480 - 2 * self.h }
}

/// A picture with a transparency mask for the copies: 0xAARRGGBB, alpha 0 = transparent.
#[derive(Clone, Debug)]
pub struct Sheet {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

/// Everything the slideshow draws with.
#[derive(Clone, Debug)]
pub struct CreditsArt {
    pub background: Vec<u32>,
    pub font: FontTable,
    pub normal: Sheet,
    pub hilited: Sheet,
    /// 'IMAG' 128, the picture of the last page.
    pub imag128: Sheet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Text { text: Vec<u8>, x: i32, y: i32, hilite: bool, mono: bool },
    /// DrawPicture of 'IMAG' `id` into the rectangle.
    Image { id: i32, rect: Rect },
}

#[derive(Clone, Debug)]
pub struct CreditsData {
    pub pages: Vec<(i32, Vec<Item>)>,
    /// (first, last) page of each mode 0..=3
    pub modes: [(i32, i32); 4],
    pub page_ticks: u32,
    pub wipe_credits: i32,
    pub wipe_title: i32,
    pub font: FontTable,
    /// named strings of the original's data (`string <name> <hex>`): the high score dialog's texts
    pub strings: Vec<(String, Vec<u8>)>,
}

impl CreditsData {
    /// From `layout/title_text.txt` (written by `tools/re/gen_credits_vectors.py --data` from the original code): lines `param <name> <n>`, `mode <k> <first> <last>`,
    /// `font <char> <top> <left> <bottom> <right>`, `page <n>` followed by its `text <x> <y> <hilite> <mono> <hex bytes>` / `image <id> <top> <left> <bottom> <right>` lines.
    pub fn parse(text: &str) -> Result<CreditsData, String> {
        let mut d = CreditsData { pages: Vec::new(), modes: [(0, 0); 4], page_ticks: 240, wipe_credits: 8, wipe_title: 12, font: FontTable::empty(), strings: Vec::new() };
        let mut have = [false; 3];
        for (ln, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            let bad = |what: &str| format!("title_text.txt line {}: {}", ln + 1, what);
            let f: Vec<&str> = line.split(' ').collect();
            let num = |i: usize| -> Result<i32, String> { f.get(i).and_then(|x| x.parse::<i32>().ok()).ok_or_else(|| bad("number expected")) };
            match f[0] {
                "param" => match f.get(1).copied() {
                    Some("page_ticks") => { d.page_ticks = num(2)? as u32; have[0] = true; }
                    Some("wipe_credits") => { d.wipe_credits = num(2)?; have[1] = true; }
                    Some("wipe_title") => { d.wipe_title = num(2)?; have[2] = true; }
                    _ => {}
                },
                "mode" => { let k = num(1)? as usize; if k >= 4 { return Err(bad("mode")); } d.modes[k] = (num(2)?, num(3)?); }
                "font" => { let k = num(1)? as usize; if k >= 128 { return Err(bad("char")); } d.font.rects[k] = [num(2)? as i16, num(3)? as i16, num(4)? as i16, num(5)? as i16]; }
                "string" => {
                    let hex = f.get(2).copied().unwrap_or("");
                    if hex.len() % 2 != 0 || !hex.is_ascii() { return Err(bad("hex")); }
                    let bytes: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).map_err(|_| bad("hex"))).collect::<Result<_, _>>()?;
                    d.strings.push((f.get(1).copied().unwrap_or("").to_string(), bytes));
                }
                "page" => d.pages.push((num(1)?, Vec::new())),
                "text" => {
                    let hex = f.get(5).copied().unwrap_or("");
                    if hex.len() % 2 != 0 || !hex.is_ascii() { return Err(bad("hex")); }
                    let bytes: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).map_err(|_| bad("hex"))).collect::<Result<_, _>>()?;
                    let it = Item::Text { text: bytes, x: num(1)?, y: num(2)?, hilite: num(3)? != 0, mono: num(4)? != 0 };
                    d.pages.last_mut().ok_or_else(|| bad("text before page"))?.1.push(it);
                }
                "image" => {
                    let it = Item::Image { id: num(1)?, rect: [num(2)? as i16, num(3)? as i16, num(4)? as i16, num(5)? as i16] };
                    d.pages.last_mut().ok_or_else(|| bad("image before page"))?.1.push(it);
                }
                _ => return Err(bad("unknown line")),
            }
        }
        if d.pages.is_empty() { return Err("title_text.txt: no pages".into()); }
        Ok(d)
    }

    /// A named string of the data (empty if the file has none).
    pub fn string(&self, name: &str) -> &[u8] {
        self.strings.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_slice()).unwrap_or(&[])
    }

    fn page(&self, n: i32) -> &[Item] {
        self.pages.iter().find(|(p, _)| *p == n).map(|(_, l)| l.as_slice()).unwrap_or(&[])
    }
}

/// The page range for the modifier keys held (Mac keys: Control 59, Option 58, Command 55; the last one tested that is down wins).
pub fn mode_for_keys(control: bool, option: bool, command: bool) -> usize {
    if command { 3 } else if option { 2 } else if control { 1 } else { 0 }
}

/// Input events the slideshow reacts to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// mouse button pressed
    Click,
    /// a key pressed; `n` = the N key
    Key { n: bool },
}

/// How the slideshow ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// back to the title screen (the window is shown again with the second wipe, see `Credits::screen`)
    Back,
    /// N: a one player game starts at once
    NewGame,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    /// a wipe is running; `then`: what follows it
    Wiping { returning: bool },
    Showing,
    Done(Outcome),
}

pub struct Credits {
    data: std::rc::Rc<CreditsData>,
    first: i32,
    last: i32,
    page: i32,
    /// what the window shows
    screen: Vec<u32>,
    /// the off-screen buffer the wipe copies from
    back: Vec<u32>,
    phase: Phase,
    wipe: Option<Wipe>,
    now: u32,
    /// `r30` of the original: the tick the current page's 240 start from
    since: u32,
    /// no page timer (the single page of `0x1e808` waits for a click or key for ever)
    forever: bool,
    pending: VecDeque<Event>,
    /// sounds to play: (id, priority, delay)
    sounds: Vec<(i32, i32, i32)>,
}

pub(crate) fn blit_sheet(back: &mut [u32], sheet: &Sheet, src: Rect, dst: Rect) {
    // CopyBits of the rectangle `src` of the sheet to `dst` (the sizes are equal for glyphs; transparent pixels are skipped)
    let (sw, sh) = ((src[3] as i32 - src[1] as i32).min(dst[3] as i32 - dst[1] as i32), (src[2] as i32 - src[0] as i32).min(dst[2] as i32 - dst[0] as i32));
    for r in 0..sh.max(0) {
        let (sy, dy) = (src[0] as i32 + r, dst[0] as i32 + r);
        if sy < 0 || sy >= sheet.h as i32 || dy < 0 || dy >= H as i32 { continue; }
        for c in 0..sw.max(0) {
            let (sx, dx) = (src[1] as i32 + c, dst[1] as i32 + c);
            if sx < 0 || sx >= sheet.w as i32 || dx < 0 || dx >= W as i32 { continue; }
            let p = sheet.px[sy as usize * sheet.w + sx as usize];
            if p >> 24 != 0 { back[dy as usize * W + dx as usize] = p; }
        }
    }
}

fn draw_page(art: &CreditsArt, data: &CreditsData, n: i32, back: &mut Vec<u32>) {
    back.clear();
    back.extend_from_slice(&art.background);
    for it in data.page(n) {
        match it {
            Item::Text { text, x, y, hilite, mono } => {
                for b in layout_text(&data.font, text, *x, *y, *hilite, *mono) {
                    if b.src[0] >= 52 {
                        blit_sheet(back, &art.hilited, [b.src[0] - 52, b.src[1], b.src[2] - 52, b.src[3]], b.dst);
                    } else {
                        blit_sheet(back, &art.normal, b.src, b.dst);
                    }
                }
            }
            Item::Image { id: 128, rect } => {
                let s = &art.imag128;
                blit_sheet(back, s, [0, 0, s.h as i16, s.w as i16], *rect);
            }
            Item::Image { .. } => {}
        }
    }
}

impl Credits {
    /// Start the slideshow of `mode` over the picture the window shows now (`window`, 640 x 480).
    pub fn new(data: std::rc::Rc<CreditsData>, art: &CreditsArt, mode: usize, window: &[u32]) -> Credits {
        let (first, last) = data.modes[mode.min(3)];
        let mut c = Credits {
            data, first, last, page: first, screen: window.to_vec(), back: Vec::new(), phase: Phase::Wiping { returning: false },
            wipe: None, now: 0, since: 0, forever: false, pending: VecDeque::new(), sounds: Vec::new(),
        };
        let data = c.data.clone();
        draw_page(art, &data, first, &mut c.back);
        c.wipe = Some(Wipe::new(data.wipe_credits, (0, 0)));
        c
    }

    /// The `X` key of the title (`0x1cef0` -> `0x1e808`): sound 65, then the single page `page` (400, the quotation) drawn with the same wipe as the slideshow's pages, no
    /// timer; a click or key plays sound 29 and returns to the title with the second wipe, N starts a game.
    pub fn single(data: std::rc::Rc<CreditsData>, art: &CreditsArt, page: i32, window: &[u32]) -> Credits {
        let mut c = Credits::new(data, art, 0, window);
        c.first = page;
        c.last = page;
        c.page = page;
        c.forever = true;
        let data = c.data.clone();
        draw_page(art, &data, page, &mut c.back);
        c.sounds.push((65, 4, 0));
        c
    }

    /// The picture the window shows.
    pub fn screen(&self) -> &[u32] { &self.screen }

    pub fn push(&mut self, e: Event) {
        if !matches!(self.phase, Phase::Done(_)) { self.pending.push_back(e); }
    }

    pub fn take_sounds(&mut self) -> Vec<(i32, i32, i32)> { std::mem::take(&mut self.sounds) }

    pub fn outcome(&self) -> Option<Outcome> { if let Phase::Done(o) = self.phase { Some(o) } else { None } }

    /// One tick (1/60 s). `title` is the picture of the title window, which the wipe at the end shows.
    pub fn tick(&mut self, art: &CreditsArt, title: &[u32]) {
        self.now += 1;
        match self.phase.clone() {
            Phase::Done(_) => {}
            Phase::Wiping { returning } => {
                let mut finished = false;
                if let Some(w) = self.wipe.as_mut() {
                    if let Some(bands) = w.next() {
                        for (src, dst) in bands {
                            for r in 0..(src[2] - src[0]) as i32 {
                                let (sy, dy) = (src[0] as i32 + r, dst[0] as i32 + r);
                                if sy < 0 || sy >= H as i32 || dy < 0 || dy >= H as i32 { continue; }
                                let (a, b) = (sy as usize * W, dy as usize * W);
                                self.screen[b..b + W].copy_from_slice(&self.back[a..a + W]);
                            }
                        }
                        finished = !w.more();
                    } else { finished = true; }
                }
                if finished {
                    self.wipe = None;
                    if returning { self.phase = Phase::Done(Outcome::Back); return; }
                    self.phase = Phase::Showing;
                    // the 240 ticks of a page are counted from the end of its wipe (`r30 = TickCount` after `0x10dcc`); the first page counts from the start
                    if self.page != self.first { self.since = self.now; }
                    self.poll(art, title);
                }
            }
            Phase::Showing => self.poll(art, title),
        }
    }

    /// The main loop of `0x10b80` (L10c74): the page timer, then one event; repeated until nothing is left to do this tick.
    fn poll(&mut self, art: &CreditsArt, title: &[u32]) {
        loop {
            if !self.forever && self.now > self.since + self.data.page_ticks {
                self.page += 1;
                if self.page > self.last { self.begin_return(title); return; }
                let data = self.data.clone();
                draw_page(art, &data, self.page, &mut self.back);
                self.wipe = Some(Wipe::new(data.wipe_credits, (0, 0)));
                self.phase = Phase::Wiping { returning: false };
                return;
            }
            match self.pending.pop_front() {
                None => return,
                Some(Event::Click) => { self.sounds.push((29, 3, 0)); self.begin_return(title); return; }
                Some(Event::Key { n }) => {
                    self.sounds.push((29, 3, 0));
                    if n { self.phase = Phase::Done(Outcome::NewGame); } else { self.begin_return(title); }
                    return;
                }
            }
        }
    }

    /// `0x1aa68` + `0x1bafc(12)`: the title window is drawn on the off-screen buffer and shown with the wipe.
    fn begin_return(&mut self, title: &[u32]) {
        self.back.clear();
        self.back.extend_from_slice(title);
        self.wipe = Some(Wipe::new(self.data.wipe_title, (0, 0)));
        self.phase = Phase::Wiping { returning: true };
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> FontTable {
        let mut f = FontTable::empty();
        f.rects[b'A' as usize] = [0, 0, 26, 10];
        f.rects[b'I' as usize] = [0, 104, 26, 110];
        f.rects[b' ' as usize] = [26, 325, 52, 335];
        f
    }

    #[test]
    fn text_advances_by_width_plus_one() {
        let b = layout_text(&font(), b"AI A", 100, 50, false, false);
        let lefts: Vec<i16> = b.iter().map(|b| b.dst[1]).collect();
        assert_eq!(lefts, vec![101, 112, 119, 130]);
        assert_eq!(b[0].dst, [50, 101, 76, 111]);
        assert_eq!(b[1].src, [0, 104, 26, 110]);
    }

    #[test]
    fn highlighted_glyphs_come_from_52_lines_lower_and_mono_advances_12() {
        let b = layout_text(&font(), b"AA", 0, 0, true, true);
        assert_eq!(b[0].src, [52, 0, 78, 10]);
        assert_eq!((b[0].dst[1], b[1].dst[1]), (1, 13));
    }

    #[test]
    fn centring() {
        // widths 10 + 1 and 6 + 1: total 18, (640 - 18) / 2 = 311
        let b = layout_text(&font(), b"AI", -1, 0, false, false);
        assert_eq!(b[0].dst[1], 312);
        let b = layout_text(&font(), b"AI", -2, 10, false, false);
        // (416 - 18) / 2 + 64 = 263, the glyph one to the right; 64 lines lower
        assert_eq!(b[0].dst, [74, 264, 100, 274]);
    }

    #[test]
    fn wipe_bands_move_towards_each_other() {
        let mut w = Wipe::new(8, (0, 0));
        let first = w.next().unwrap();
        assert_eq!(first[0].0, [8, 0, 16, 640]);
        assert_eq!(first[1].0, [464, 0, 472, 640]);
        let mut n = 1;
        while w.next().is_some() { n += 1; }
        assert_eq!(n, 58);
        let mut w = Wipe::new(12, (3, 5));
        let f = w.next().unwrap();
        assert_eq!(f[0].1, [17, 3, 29, 643]);
    }

    #[test]
    fn modes_prefer_command_then_option_then_control() {
        assert_eq!(mode_for_keys(false, false, false), 0);
        assert_eq!(mode_for_keys(true, false, false), 1);
        assert_eq!(mode_for_keys(true, true, false), 2);
        assert_eq!(mode_for_keys(true, true, true), 3);
    }
}
