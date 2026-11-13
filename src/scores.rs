//! The high scores: the table, the name entry and the High Scores screen of the title (original: `0x199dc`, `0x19b28`, `0x194ac`, `0x19894`).
//!
//! Verified against the original code running in `tools/re/ppcemu.py` (`tools/re/gen_scores_vectors.py`, `tests/vectors/scores_vectors.txt`,
//! `tests/scores_golden.rs`): the qualifying test, the insertion and name rules of the name entry, and the whole sequence of drawing operations,
//! delays and sounds of the screen together with its exit rules.
//!
//! The table is the 332 byte `SCOR` resource: 12 Pascal strings of 21 bytes (the last name entered by player 1 and by player 2, then the 10 names), then
//! 10 scores and 10 levels as big endian 32 bit values (the level is the game's level number at the end of the game).
//! What is the port's own choice: the name entry in the original is an operating system dialog (`DLOG` 1000: text, an edit field, OK / Cancel,
//! the portrait 11000 / 11001); `NameEntry` has the same rules (the key filter `0x1a0cc`: at most 20 characters, Return / Enter = OK, the click
//! sounds) but the port draws its own panel (`play.rs`). The table is saved to a file of the port's own (the original keeps it in its preferences file).
use crate::credits::{blit_sheet, layout_text, CreditsArt, CreditsData, Rect, Sheet, H, W};

pub const TABLE_LEN: usize = 332;
pub const ROWS: usize = 10;
const NAME_AT: usize = 42;
const SCORE_AT: usize = 252;
const LEVEL_AT: usize = 292;

/// The score table in the original's byte layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    raw: [u8; TABLE_LEN],
}

fn be32(b: &[u8]) -> i32 { i32::from_be_bytes([b[0], b[1], b[2], b[3]]) }

impl Table {
    pub fn from_bytes(b: &[u8]) -> Option<Table> {
        if b.len() != TABLE_LEN { return None; }
        let mut raw = [0u8; TABLE_LEN];
        raw.copy_from_slice(b);
        Some(Table { raw })
    }

    pub fn to_bytes(&self) -> &[u8; TABLE_LEN] { &self.raw }

    fn pstr(&self, at: usize) -> &[u8] {
        let n = (self.raw[at] as usize).min(20);
        &self.raw[at + 1..at + 1 + n]
    }

    /// The name of row `i` (0..10).
    pub fn name(&self, i: usize) -> &[u8] { self.pstr(NAME_AT + 21 * i) }
    pub fn score(&self, i: usize) -> i32 { be32(&self.raw[SCORE_AT + 4 * i..]) }
    pub fn level(&self, i: usize) -> i32 { be32(&self.raw[LEVEL_AT + 4 * i..]) }
    /// The name player 1 / 2 (1 or 2) entered last: the edit field's start text.
    pub fn last_name(&self, player: u8) -> &[u8] { self.pstr(if player == 1 { 0 } else { 21 }) }

    /// `0x199dc`: does the score beat the last row (strictly)? The game over code asks this before it opens the name entry.
    pub fn qualifies(&self, score: i32) -> bool { score > self.score(9) }

    /// The check at the start of the name entry `0x19b28`: it wants at least the last row's score.
    pub fn wants(&self, score: i32) -> bool { score >= self.score(9) }

    /// The insertion of the name entry (after OK): `typed` is the edit field's text. Returns the new row and whether the name was the author's
    /// (replaced; the original plays sound 5 once more). Scores equal to an existing one go below it; the last row falls off.
    pub fn enter(&mut self, player: u8, score: i32, level: i32, typed: &[u8], strings: &Strings) -> (usize, bool) {
        let mut at = 8i32;
        while at >= 0 && score > self.score(at as usize) {
            let a = at as usize;
            let (s, l) = (self.raw[SCORE_AT + 4 * a..SCORE_AT + 4 * a + 4].to_vec(), self.raw[LEVEL_AT + 4 * a..LEVEL_AT + 4 * a + 4].to_vec());
            self.raw[SCORE_AT + 4 * a + 4..SCORE_AT + 4 * a + 8].copy_from_slice(&s);
            self.raw[LEVEL_AT + 4 * a + 4..LEVEL_AT + 4 * a + 8].copy_from_slice(&l);
            let n: Vec<u8> = self.raw[NAME_AT + 21 * a..NAME_AT + 21 * a + 21].to_vec();
            self.raw[NAME_AT + 21 * a + 21..NAME_AT + 21 * a + 42].copy_from_slice(&n);
            at -= 1;
        }
        let row = (at + 1) as usize;
        self.raw[SCORE_AT + 4 * row..SCORE_AT + 4 * row + 4].copy_from_slice(&score.to_be_bytes());
        self.raw[LEVEL_AT + 4 * row..LEVEL_AT + 4 * row + 4].copy_from_slice(&level.to_be_bytes());
        // the name: empty -> the default, the author's name -> another one (with a sound)
        let (name, swoop): (Vec<u8>, bool) = if typed.is_empty() {
            (strings.default_name.clone(), false)
        } else if typed == strings.taboo_name.as_slice() {
            (strings.taboo_replacement.clone(), true)
        } else {
            (typed.to_vec(), false)
        };
        let mut entry = [0u8; 21];
        entry[0] = name.len().min(255) as u8;
        for (k, &c) in name.iter().take(20).enumerate() { entry[1 + k] = c; }
        self.raw[NAME_AT + 21 * row..NAME_AT + 21 * row + 21].copy_from_slice(&entry);
        // the last name of this player (a name of exactly 20 characters loses the last one)
        let at_last = if player == 1 { 0 } else { 21 };
        self.raw[at_last..at_last + 21].copy_from_slice(&entry);
        if self.raw[at_last] == 20 {
            self.raw[at_last] = 19;
            self.raw[at_last + 20] = 0;
        }
        (row, swoop)
    }
}

/// The texts of the name entry (the original's data, in `layout/title_text.txt`).
#[derive(Clone, Debug, Default)]
pub struct Strings {
    pub prompt_single: Vec<u8>,
    pub prompt_p1: Vec<u8>,
    pub prompt_p2: Vec<u8>,
    pub default_name: Vec<u8>,
    pub taboo_name: Vec<u8>,
    pub taboo_replacement: Vec<u8>,
}

impl Strings {
    pub fn from_data(d: &CreditsData) -> Strings {
        Strings {
            prompt_single: d.string("prompt_single").to_vec(),
            prompt_p1: d.string("prompt_p1").to_vec(),
            prompt_p2: d.string("prompt_p2").to_vec(),
            default_name: d.string("default_name").to_vec(),
            taboo_name: d.string("taboo_name").to_vec(),
            taboo_replacement: d.string("taboo_replacement").to_vec(),
        }
    }

    /// The dialog's text: one player game, or player 1 / 2 of a two player game.
    pub fn prompt(&self, single: bool, player: u8) -> &[u8] {
        if single { &self.prompt_single } else if player == 1 { &self.prompt_p1 } else { &self.prompt_p2 }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// the name entry

/// What the player did in the name entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKey {
    /// a character (Mac Roman code)
    Char(u8),
    Backspace,
    /// Return or Enter
    Return,
}

/// The name entry's edit field and its key rules (the dialog's filter `0x1a0cc`).
#[derive(Clone, Debug)]
pub struct NameEntry {
    pub player: u8,
    pub single: bool,
    pub text: Vec<u8>,
    pub score: i32,
    pub level: i32,
    sounds: Vec<(i32, i32, i32)>,
    done: Option<bool>,
}

impl NameEntry {
    /// Opens (sound 5, priority 3); the field starts with the last name this player entered.
    pub fn new(table: &Table, player: u8, single: bool, score: i32, level: i32) -> NameEntry {
        NameEntry { player, single, text: table.last_name(player).to_vec(), score, level, sounds: vec![(5, 3, 0)], done: None }
    }

    /// Portrait picture id (11000 / 11001).
    pub fn portrait(&self) -> i32 { if self.player == 1 { 11000 } else { 11001 } }

    /// A key: Return / Enter ends with OK; Backspace plays sound 20 (priority 5); a character is taken while there are fewer than 20, with sound 29
    /// (priority 5); with 20 characters it is refused (the original beeps).
    pub fn key(&mut self, k: EntryKey) {
        match k {
            EntryKey::Return => self.done = Some(true),
            EntryKey::Backspace => { self.sounds.push((20, 5, 0)); self.text.pop(); }
            EntryKey::Char(c) => {
                if self.text.len() < 20 { self.sounds.push((29, 5, 0)); self.text.push(c); }
            }
        }
    }

    /// The Cancel button.
    pub fn cancel(&mut self) { self.done = Some(false); }
    pub fn ok(&mut self) { self.done = Some(true); }
    /// `Some(true)` after OK, `Some(false)` after Cancel.
    pub fn done(&self) -> Option<bool> { self.done }
    pub fn take_sounds(&mut self) -> Vec<(i32, i32, i32)> { std::mem::take(&mut self.sounds) }

    /// After OK: inserts the entry. Returns the new row and the sounds (30, and 5 once more for the author's name).
    pub fn commit(&self, table: &mut Table, strings: &Strings) -> (usize, Vec<(i32, i32, i32)>) {
        let (row, swoop) = table.enter(self.player, self.score, self.level, &self.text, strings);
        let mut s = vec![(30, 3, 0)];
        if swoop { s.push((5, 3, 0)); }
        (row, s)
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// the High Scores screen

/// One drawing operation / sound of the screen, as the original performs them (`tools/re/gen_scores_vectors.py` logs the same).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// the back buffer black
    Clear,
    /// the picture (9000, the title background) drawn into the back buffer
    Pic(i32),
    /// the back buffer is shown in the window
    Show,
    /// the event queue is emptied
    Flush,
    Snd(i32, i32, i32),
    Text { s: Vec<u8>, x: i32, y: i32, hilite: bool, mono: bool },
    /// copy a rectangle of the back buffer into the strip buffer (`dst` in the strip)
    Save { src: Rect, dst: Rect },
    /// copy a strip rectangle back into the back buffer
    Restore { src: Rect, dst: Rect },
    Delay(u32),
}

fn rect_token(r: &Rect) -> String { format!("{}.{}.{}.{}", r[0], r[1], r[2], r[3]) }

impl Op {
    /// The line format of the golden vectors.
    pub fn token(&self, at: u32) -> String {
        let hex = |s: &[u8]| s.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        match self {
            Op::Clear => format!("clear@{}", at),
            Op::Pic(i) => format!("pic:{}@{}", i, at),
            Op::Show => format!("show@{}", at),
            Op::Flush => format!("flush@{}", at),
            Op::Snd(a, b, c) => format!("snd:{}:{}:{}@{}", a, b, c, at),
            Op::Text { s, x, y, hilite, mono } => format!("text:{}:{}:{}:{}:{}@{}", hex(s), x, y, *hilite as u8, *mono as u8, at),
            Op::Save { src, dst } => format!("save:{}:{}@{}", rect_token(src), rect_token(dst), at),
            Op::Restore { src, dst } => format!("restore:{}:{}@{}", rect_token(src), rect_token(dst), at),
            Op::Delay(n) => format!("delay:{}@{}", n, at),
        }
    }
}

/// Row `i`'s three texts (`0x19894`): the rank with a point, the name, the score (the fixed width digits).
fn row_ops(t: &Table, i: usize, y: i32, out: &mut Vec<Op>) {
    out.push(Op::Text { s: format!("{}.", i + 1).into_bytes(), x: 120, y, hilite: false, mono: false });
    out.push(Op::Text { s: t.name(i).iter().copied().take_while(|&c| c != 0).collect(), x: 160, y, hilite: false, mono: false });
    out.push(Op::Text { s: t.score(i).to_string().into_bytes(), x: 450, y, hilite: false, mono: true });
}

fn row_rect(y: i32) -> Rect { [y as i16, 100, (y + 26) as i16, 580] }

/// The sequence before the key loop (`0x194ac` up to the event flush): (time in ticks, operation), and the time of the flush.
/// `idx` = the rows of the new scores of player 1 and 2 (-1 none): those rows blink 5 times (hidden 8 ticks, shown 8 ticks).
pub fn build(t: &Table, idx: [i32; 2]) -> (Vec<(u32, Op)>, u32) {
    let mut v: Vec<(u32, Op)> = Vec::new();
    let mut now = 0u32;
    let push = |now: u32, op: Op, v: &mut Vec<(u32, Op)>| v.push((now, op));
    let mut ops = Vec::new();
    push(now, Op::Clear, &mut v);
    push(now, Op::Pic(9000), &mut v);
    for i in 0..ROWS { row_ops(t, i, 70 + 35 * i as i32, &mut ops); }
    for o in ops.drain(..) { push(now, o, &mut v); }
    push(now, Op::Show, &mut v);
    if idx[0] >= 0 || idx[1] >= 0 {
        let y = [70 + 35 * idx[0], 70 + 35 * idx[1]];
        let strip = [[0, 0, 26, 480], [26, 0, 52, 480]];
        push(now, Op::Pic(9000), &mut v);
        for p in 0..2 {
            if idx[p] >= 0 { push(now, Op::Save { src: row_rect(y[p]), dst: strip[p] }, &mut v); }
        }
        for i in 0..ROWS { row_ops(t, i, 70 + 35 * i as i32, &mut ops); }
        for o in ops.drain(..) { push(now, o, &mut v); }
        push(now, Op::Delay(20), &mut v);
        now += 20;
        for _ in 0..5 {
            push(now, Op::Delay(8), &mut v);
            now += 8;
            for p in 0..2 {
                if idx[p] >= 0 { push(now, Op::Restore { src: strip[p], dst: row_rect(y[p]) }, &mut v); }
            }
            push(now, Op::Show, &mut v);
            push(now, Op::Delay(8), &mut v);
            now += 8;
            for p in 0..2 {
                if idx[p] >= 0 {
                    row_ops(t, idx[p] as usize, y[p], &mut ops);
                    for o in ops.drain(..) { push(now, o, &mut v); }
                    push(now, Op::Show, &mut v);
                }
            }
        }
    }
    push(now, Op::Flush, &mut v);
    (v, now)
}

/// Input of the key loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// mouse button pressed
    Click,
    /// a key went down (character code)
    Key(u8),
}

/// How the screen ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// back to the title
    Back,
    /// the N key: start a one player game
    NewGame,
}

/// The screen's logic, one `tick()` per tick (1/60 s). After the sequence the screen waits 10 s (600 ticks from the start) for a click (leave) or the
/// N key (sound 30, then a new game); any other key also leaves; input given while the sequence runs is dropped.
#[derive(Clone, Debug)]
pub struct ScoreScreen {
    script: Vec<(u32, Op)>,
    next: usize,
    loop_start: u32,
    t: u32,
    outcome: Option<Outcome>,
    end: u32,
}

impl ScoreScreen {
    pub fn new(t: &Table, idx: [i32; 2]) -> ScoreScreen {
        let (script, loop_start) = build(t, idx);
        ScoreScreen { script, next: 0, loop_start, t: 0, outcome: None, end: 0 }
    }

    pub fn outcome(&self) -> Option<Outcome> { self.outcome }
    /// The tick count after the last tick (the original's clock after leaving).
    pub fn end(&self) -> u32 { self.end }

    /// One tick: the operations due now (with their time) and the sounds.
    pub fn tick(&mut self, input: Option<Input>) -> Vec<(u32, Op)> {
        let mut out = Vec::new();
        if self.outcome.is_some() { return out; }
        let t = self.t;
        while self.next < self.script.len() && self.script[self.next].0 <= t {
            out.push(self.script[self.next].clone());
            self.next += 1;
        }
        if self.next >= self.script.len() && t >= self.loop_start {
            if t > 600 {
                self.outcome = Some(Outcome::Back);
                self.end = t + 1;
            } else {
                match input {
                    Some(Input::Click) => { self.outcome = Some(Outcome::Back); self.end = t + 1; }
                    Some(Input::Key(c)) if c == b'N' || c == b'n' => {
                        out.push((t + 1, Op::Snd(30, 3, 0)));
                        self.outcome = Some(Outcome::NewGame);
                        self.end = t + 1;
                    }
                    Some(Input::Key(_)) => { self.outcome = Some(Outcome::Back); self.end = t + 1; }
                    None => {}
                }
            }
        }
        self.t += 1;
        out
    }
}

/// The off-screen buffers the operations act on: back buffer, the strip with the hidden rows' background, and what the window shows.
#[derive(Clone, Debug)]
pub struct ScoreCanvas {
    back: Vec<u32>,
    strip: Vec<u32>,
    screen: Vec<u32>,
}

const STRIP_H: usize = 52;
const STRIP_W: usize = 480;

impl ScoreCanvas {
    /// `window` = the picture the window shows when the screen opens.
    pub fn new(window: &[u32]) -> ScoreCanvas {
        ScoreCanvas { back: vec![0xFF00_0000; W * H], strip: vec![0xFF00_0000; STRIP_W * STRIP_H], screen: window.to_vec() }
    }

    pub fn screen(&self) -> &[u32] { &self.screen }

    pub fn apply(&mut self, op: &Op, art: &CreditsArt, data: &CreditsData) {
        match op {
            Op::Clear => self.back.iter_mut().for_each(|p| *p = 0xFF00_0000),
            Op::Pic(_) => {
                let n = art.background.len().min(self.back.len());
                self.back[..n].copy_from_slice(&art.background[..n]);
            }
            Op::Show => self.screen.copy_from_slice(&self.back),
            Op::Text { s, x, y, hilite, mono } => {
                for b in layout_text(&data.font, s, *x, *y, *hilite, *mono) {
                    if b.src[0] >= 52 {
                        blit_sheet(&mut self.back, &art.hilited, [b.src[0] - 52, b.src[1], b.src[2] - 52, b.src[3]], b.dst);
                    } else {
                        blit_sheet(&mut self.back, &art.normal, b.src, b.dst);
                    }
                }
            }
            Op::Save { src, dst } => copy_rect(&self.back, W, H, *src, &mut self.strip, STRIP_W, STRIP_H, *dst),
            Op::Restore { src, dst } => copy_rect(&self.strip, STRIP_W, STRIP_H, *src, &mut self.back, W, H, *dst),
            Op::Flush | Op::Snd(..) | Op::Delay(_) => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn copy_rect(src: &[u32], sw: usize, sh: usize, sr: Rect, dst: &mut [u32], dw: usize, dh: usize, dr: Rect) {
    let w = (sr[3] as i32 - sr[1] as i32).min(dr[3] as i32 - dr[1] as i32);
    let h = (sr[2] as i32 - sr[0] as i32).min(dr[2] as i32 - dr[0] as i32);
    for r in 0..h.max(0) {
        let (sy, dy) = (sr[0] as i32 + r, dr[0] as i32 + r);
        if sy < 0 || sy >= sh as i32 || dy < 0 || dy >= dh as i32 { continue; }
        for c in 0..w.max(0) {
            let (sx, dx) = (sr[1] as i32 + c, dr[1] as i32 + c);
            if sx < 0 || sx >= sw as i32 || dx < 0 || dx >= dw as i32 { continue; }
            dst[dy as usize * dw + dx as usize] = src[sy as usize * sw + sx as usize];
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// the name entry's panel (the port's own drawing of the original's dialog `DLOG` 1000: same size, same item rectangles)

pub const PANEL_W: i32 = 480;
pub const PANEL_H: i32 = 232;
// item rectangles in the panel (x0, y0, x1, y1): the original's dialog is 316 x 134 for a 12 point system font; the game's bitmap font is twice as high,
// so the same items (portrait, text, edit field, OK, Cancel) are laid out larger
const OK_RECT: [i32; 4] = [340, 176, 456, 212];
const CANCEL_RECT: [i32; 4] = [208, 176, 324, 212];
const FIELD_RECT: [i32; 4] = [100, 120, 456, 156];
const PROMPT_AT: (i32, i32) = (100, 22);
const PORTRAIT_AT: (i32, i32) = (28, 28); // 48 x 48

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Ok,
    Cancel,
}

/// Where the panel sits in the 640 x 480 picture and how it is drawn (dark panel, the game's bitmap font: capitals and lower case look the same).
#[derive(Clone, Copy, Debug)]
pub struct EntryPanel {
    pub x: i32,
    pub y: i32,
}

pub(crate) fn fill(buf: &mut [u32], r: [i32; 4], c: u32) {
    for y in r[1].max(0)..r[3].min(H as i32) {
        for x in r[0].max(0)..r[2].min(W as i32) { buf[y as usize * W + x as usize] = c; }
    }
}

pub(crate) fn frame(buf: &mut [u32], r: [i32; 4], c: u32, t: i32) {
    fill(buf, [r[0], r[1], r[2], r[1] + t], c);
    fill(buf, [r[0], r[3] - t, r[2], r[3]], c);
    fill(buf, [r[0], r[1], r[0] + t, r[3]], c);
    fill(buf, [r[2] - t, r[1], r[2], r[3]], c);
}

/// Width of a string in the game's font (advance = glyph width + 1).
pub fn text_width(data: &CreditsData, s: &[u8]) -> i32 {
    layout_text(&data.font, s, 0, 0, false, false).last().map(|b| b.dst[3] as i32 - 1).unwrap_or(0)
}

pub(crate) fn put_text(buf: &mut Vec<u32>, art: &CreditsArt, data: &CreditsData, s: &[u8], x: i32, y: i32, hilite: bool) {
    for b in layout_text(&data.font, s, x, y, hilite, false) {
        if b.src[0] >= 52 {
            blit_sheet(buf, &art.hilited, [b.src[0] - 52, b.src[1], b.src[2] - 52, b.src[3]], b.dst);
        } else {
            blit_sheet(buf, &art.normal, b.src, b.dst);
        }
    }
}

impl EntryPanel {
    /// Centred in a picture `view_w` x `view_h` (the game's picture is 576 wide).
    pub fn centered(view_w: i32, view_h: i32) -> EntryPanel { EntryPanel { x: (view_w - PANEL_W) / 2, y: (view_h - PANEL_H) / 2 } }

    pub fn hit(&self, px: i32, py: i32) -> Option<Button> {
        let (x, y) = (px - self.x, py - self.y);
        let inside = |r: [i32; 4]| x >= r[0] && x < r[2] && y >= r[1] && y < r[3];
        if inside(OK_RECT) { Some(Button::Ok) } else if inside(CANCEL_RECT) { Some(Button::Cancel) } else { None }
    }

    /// Draws the panel into `buf` (640 x 480, over what is in it).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, buf: &mut Vec<u32>, entry: &NameEntry, strings: &Strings, art: &CreditsArt, data: &CreditsData, portrait: Option<&Sheet>, caret: bool, hover: Option<Button>) {
        let (ox, oy) = (self.x, self.y);
        let r = |a: [i32; 4]| [a[0] + ox, a[1] + oy, a[2] + ox, a[3] + oy];
        fill(buf, [ox, oy, ox + PANEL_W, oy + PANEL_H], 0xFF14_1824);
        frame(buf, [ox, oy, ox + PANEL_W, oy + PANEL_H], 0xFFB8_C0D8, 2);
        // portrait and its frame
        frame(buf, [ox + PORTRAIT_AT.0 - 3, oy + PORTRAIT_AT.1 - 3, ox + PORTRAIT_AT.0 + 51, oy + PORTRAIT_AT.1 + 51], 0xFF60_6C90, 1);
        if let Some(p) = portrait {
            blit_sheet(buf, p, [0, 0, p.h as i16, p.w as i16], [(oy + PORTRAIT_AT.1) as i16, (ox + PORTRAIT_AT.0) as i16, (oy + PORTRAIT_AT.1 + 48) as i16, (ox + PORTRAIT_AT.0 + 48) as i16]);
        }
        // the prompt: its lines are separated by a carriage return
        let lh = layout_text(&data.font, b"A", 0, 0, false, false).first().map(|b| (b.dst[2] - b.dst[0]) as i32).unwrap_or(12) + 10;
        for (i, line) in strings.prompt(entry.single, entry.player).split(|&c| c == b'\r').enumerate() {
            put_text(buf, art, data, line, ox + PROMPT_AT.0, oy + PROMPT_AT.1 + i as i32 * lh, false);
        }
        // the edit field
        fill(buf, r(FIELD_RECT), 0xFF00_0000);
        frame(buf, r(FIELD_RECT), 0xFFB8_C0D8, 1);
        let tx = ox + FIELD_RECT[0] + 6;
        let ty = oy + FIELD_RECT[1] + 6;
        put_text(buf, art, data, &entry.text, tx, ty, true);
        if caret {
            let cx = tx + text_width(data, &entry.text) + 2;
            fill(buf, [cx, ty, cx + 2, oy + FIELD_RECT[3] - 6], 0xFFFF_FFFF);
        }
        // buttons (OK is the default button: a double frame)
        for (b, rect, label) in [(Button::Ok, OK_RECT, &b"OK"[..]), (Button::Cancel, CANCEL_RECT, &b"CANCEL"[..])] {
            let rr = r(rect);
            fill(buf, rr, if hover == Some(b) { 0xFF38_4470 } else { 0xFF24_2C48 });
            frame(buf, rr, 0xFFB8_C0D8, if b == Button::Ok { 2 } else { 1 });
            let w = text_width(data, label);
            put_text(buf, art, data, label, rr[0] + (rr[2] - rr[0] - w) / 2, rr[1] + 6, hover == Some(b));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings() -> Strings {
        Strings { default_name: b"Swoop".to_vec(), taboo_name: b"x y".to_vec(), taboo_replacement: b"Swoop!".to_vec(), ..Default::default() }
    }

    fn table() -> Table {
        let mut raw = [0u8; TABLE_LEN];
        for i in 0..ROWS {
            raw[NAME_AT + 21 * i] = 1;
            raw[NAME_AT + 21 * i + 1] = b'a' + i as u8;
            raw[SCORE_AT + 4 * i..SCORE_AT + 4 * i + 4].copy_from_slice(&(10000 - 1000 * i as i32).to_be_bytes());
        }
        Table::from_bytes(&raw).unwrap()
    }

    #[test]
    fn name_entry_key_rules() {
        let t = table();
        let mut e = NameEntry::new(&t, 1, true, 5500, 4);
        assert_eq!(e.take_sounds(), vec![(5, 3, 0)]);
        for _ in 0..25 { e.key(EntryKey::Char(b'q')); }
        assert_eq!(e.text.len(), 20);
        assert_eq!(e.take_sounds().len(), 20, "refused keys are silent");
        e.key(EntryKey::Backspace);
        assert_eq!(e.take_sounds(), vec![(20, 5, 0)]);
        assert_eq!(e.text.len(), 19);
        assert_eq!(e.done(), None);
        e.key(EntryKey::Return);
        assert_eq!(e.done(), Some(true));
    }

    #[test]
    fn insertion_ties_go_below_and_the_last_row_falls_off() {
        let mut t = table();
        let (row, _) = t.enter(2, 9000, 7, b"New", &strings());
        assert_eq!(row, 2, "equal to row 2 (9000): goes below it, row index 2 holds the old 8000 afterwards");
        assert_eq!(t.score(1), 9000);
        assert_eq!(t.score(2), 9000);
        assert_eq!(t.name(2), b"New");
        assert_eq!(t.score(9), 2000, "the 1000 row fell off");
        assert_eq!(t.last_name(2), b"New");
        assert_eq!(t.level(2), 7);
    }

    #[test]
    fn empty_name_and_the_authors_name() {
        let mut t = table();
        let (_, swoop) = t.enter(1, 20000, 1, b"", &strings());
        assert!(!swoop);
        assert_eq!(t.name(0), b"Swoop");
        let (_, swoop) = t.enter(1, 30000, 1, b"x y", &strings());
        assert!(swoop);
        assert_eq!(t.name(0), b"Swoop!");
    }

    #[test]
    fn a_20_character_name_is_shortened_in_the_last_name_field_only() {
        let mut t = table();
        t.enter(1, 20000, 1, &[b'z'; 20], &strings());
        assert_eq!(t.name(0).len(), 20);
        assert_eq!(t.last_name(1).len(), 19);
    }
}
