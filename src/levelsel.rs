//! The level chooser: "Start at which level?" (`DLOG` 160 "Level Select", routine `0x1b254(maxLevel)`, its key filter `0x7e04`).
//!
//! The title's 1 Player / 2 Players buttons open it when the Option key is held (`0x1cb80` / `0x1cbe8`; the number of levels comes from counting the
//! `DIME` resources, `0xfcb8`). The dialog opens with "1" selected in the edit field (`0x486cc` / `0x48778`: Return is the default item 1, Esc the cancel item 2),
//! plays sound (34, 4, 0), and on OK reads the text (`StringToNum`): below 1 the field shows 0 for 30 ticks and the dialog returns 0 (nothing starts); otherwise the
//! number is clamped to the last level, shown for 30 ticks and returned. High scores are off for a game that starts above level 1 (see `scores.rs`).
//!
//! [`filter`] is the dialog's event filter, [`LevelSelect`] the rest of `0x1b254` as a state machine; both are verified against the original
//! (`tools/re/gen_levelsel_vectors.py`, `tests/vectors/levelsel_vectors.txt`). [`LevelUi`] is the port's drawing of the dialog and its input rules.

use crate::credits::{CreditsArt, CreditsData};
use crate::prefs_panel::DialogText;
use crate::scores::{fill, frame, put_text, text_width};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// `0x45bac(id, priority, delay)`.
    Snd(i32, i32, i32),
    /// `0x487fc`: the text of the edit field (Pascal string contents).
    SetText(Vec<u8>),
    /// `0x488a8`: select the whole text.
    Select,
    /// `0x967c(ticks)`.
    Delay(i32),
    /// `0x4a240(dlg, item)`: the button flashes.
    Flash(i16),
    /// `SysBeep`.
    Beep,
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Op::Snd(a, b, c) => write!(f, "snd:{}:{}:{}", a, b, c),
            Op::SetText(t) => write!(f, "settext:{}", t.iter().map(|b| format!("{:02x}", b)).collect::<String>()),
            Op::Select => write!(f, "select"),
            Op::Delay(n) => write!(f, "delay:{}", n),
            Op::Flash(i) => write!(f, "flash:{}", i),
            Op::Beep => write!(f, "beep"),
        }
    }
}

/// What the event filter decided: `handled` = the filter's result (the event does not reach the dialog), `item` = the item hit it reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filtered {
    pub handled: bool,
    pub item: Option<i16>,
    pub ops: Vec<Op>,
}

pub const KEY_DOWN: u16 = 3;
pub const AUTO_KEY: u16 = 5;
pub const CMD: u16 = 0x100;

/// `0x7e04`: Return / Enter = item 1, Esc and Command-. = item 2 (the button flashes), digits and Backspace go to the field, every other key beeps and is swallowed.
/// `dialog_ok` is the result of `0x4a3c0` (the dialog is one of the program's); without it nothing is handled.
pub fn filter(dialog_ok: bool, what: u16, message: u32, modifiers: u16) -> Filtered {
    let pass = Filtered { handled: false, item: None, ops: Vec::new() };
    if !dialog_ok { return pass; }
    if what != KEY_DOWN && what != AUTO_KEY { return pass; }
    let ch = (message & 255) as u8;
    if modifiers & CMD != 0 {
        // 0x95e8: lower case to upper case
        let up = if (b'a'..=b'z').contains(&ch) { ch - 32 } else { ch };
        if up == b'.' { return Filtered { handled: true, item: Some(2), ops: vec![Op::Flash(2)] }; }
        return Filtered { handled: true, item: None, ops: Vec::new() };
    }
    match ch {
        13 | 3 => Filtered { handled: true, item: Some(1), ops: Vec::new() },
        27 => Filtered { handled: true, item: Some(2), ops: vec![Op::Flash(2)] },
        b'0'..=b'9' | 8 => pass,
        _ => Filtered { handled: true, item: None, ops: vec![Op::Beep] },
    }
}

/// `StringToNum` of the edit field (digits only; the port's field takes at most nine).
pub fn string_to_num(text: &[u8]) -> i32 {
    text.iter().take_while(|c| c.is_ascii_digit()).fold(0i32, |n, &c| n.wrapping_mul(10).wrapping_add((c - b'0') as i32))
}

fn num_text(n: i32) -> Vec<u8> { n.to_string().into_bytes() }

/// `0x1b254`.
#[derive(Debug, Clone)]
pub struct LevelSelect {
    max: i32,
    result: Option<i32>,
}

impl LevelSelect {
    /// The start: the field shows "1" (selected), sound (34, 4, 0).
    pub fn open(max: i32) -> (LevelSelect, Vec<Op>) {
        (LevelSelect { max, result: None }, vec![Op::SetText(b"1".to_vec()), Op::Select, Op::Snd(34, 4, 0)])
    }

    /// The level to start (0 = none) once the dialog is over.
    pub fn result(&self) -> Option<i32> { self.result }

    /// One `ModalDialog` result: `typed` = the text of the field (read when OK is hit).
    pub fn hit(&mut self, item: i16, typed: &[u8]) -> Vec<Op> {
        if self.result.is_some() { return Vec::new(); }
        match item {
            2 => { self.result = Some(0); Vec::new() }
            1 => {
                let mut n = string_to_num(typed);
                let ok = n >= 1;
                if !ok { n = 0; } else if n > self.max { n = self.max; }
                self.result = Some(if ok { n } else { 0 });
                vec![Op::SetText(num_text(n)), Op::Delay(30)]
            }
            _ => Vec::new(),
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------------------------------
// the port's screen for it

pub const PANEL_W: i32 = 460;
pub const PANEL_H: i32 = 196;
const FIELD: [i32; 4] = [330, 22, 436, 56];
const OK_RECT: [i32; 4] = [316, 132, 436, 172];
const CANCEL_RECT: [i32; 4] = [180, 132, 300, 172];
const PROMPT_AT: (i32, i32) = (24, 22);
const MAX_DIGITS: usize = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button { Ok, Cancel }

/// A key the screen understands (typed characters come separately).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key { Return, Escape, Backspace }

#[derive(Debug, Clone)]
pub struct LevelUi {
    dialog: LevelSelect,
    text: DialogText,
    pub field: Vec<u8>,
    /// the text is selected: the next character replaces it
    selected: bool,
    /// ticks left of the 30 tick display of the clamped number after OK
    shown: u32,
    pub x: i32,
    pub y: i32,
}

impl LevelUi {
    pub fn new(max: i32, text: DialogText, view_w: i32, view_h: i32) -> (LevelUi, Vec<Op>) {
        let (dialog, ops) = LevelSelect::open(max);
        (LevelUi { dialog, text, field: b"1".to_vec(), selected: true, shown: 0, x: (view_w - PANEL_W) / 2, y: (view_h - PANEL_H) / 2 }, ops)
    }

    /// The level to start (> 0), 0 = nothing, `None` while the dialog is open (and during the 30 ticks after OK).
    pub fn result(&self) -> Option<i32> { if self.shown > 0 { None } else { self.dialog.result() } }

    pub fn busy(&self) -> bool { self.shown > 0 }

    /// One tick (1/60 s of the original; the caller passes 2 per logic step) of the display after OK.
    pub fn tick(&mut self) { if self.shown > 0 { self.shown -= 1; } }

    fn apply(&mut self, ops: &[Op]) {
        for op in ops {
            match op {
                Op::SetText(t) => { self.field = t.clone(); self.selected = false; }
                Op::Select => self.selected = true,
                Op::Delay(n) => self.shown = *n as u32,
                _ => {}
            }
        }
    }

    fn ok(&mut self) -> Vec<Op> {
        let f = self.field.clone();
        let ops = self.dialog.hit(1, &f);
        self.apply(&ops);
        ops
    }

    fn cancel(&mut self) -> Vec<Op> { self.dialog.hit(2, &[]) }

    /// A typed character (the event filter decides: digits reach the field, everything else beeps).
    pub fn typed(&mut self, c: u8) -> Vec<Op> {
        if self.dialog.result().is_some() { return Vec::new(); }
        let r = filter(true, KEY_DOWN, c as u32, 0);
        if r.handled { return r.ops; }
        if c.is_ascii_digit() {
            if self.selected { self.field.clear(); self.selected = false; }
            if self.field.len() < MAX_DIGITS { self.field.push(c); }
        }
        Vec::new()
    }

    pub fn key(&mut self, k: Key) -> Vec<Op> {
        if self.dialog.result().is_some() { return Vec::new(); }
        let ch = match k { Key::Return => 13, Key::Escape => 27, Key::Backspace => 8 };
        let r = filter(true, KEY_DOWN, ch, 0);
        let mut ops = r.ops.clone();
        match r.item {
            Some(1) => ops.extend(self.ok()),
            Some(2) => ops.extend(self.cancel()),
            _ => if ch == 8 { if self.selected { self.field.clear(); self.selected = false; } else { self.field.pop(); } },
        }
        ops
    }

    pub fn hit(&self, px: i32, py: i32) -> Option<Button> {
        let (x, y) = (px - self.x, py - self.y);
        let inside = |r: [i32; 4]| x >= r[0] && x < r[2] && y >= r[1] && y < r[3];
        if inside(OK_RECT) { Some(Button::Ok) } else if inside(CANCEL_RECT) { Some(Button::Cancel) } else { None }
    }

    pub fn click(&mut self, px: i32, py: i32) -> Vec<Op> {
        if self.dialog.result().is_some() { return Vec::new(); }
        match self.hit(px, py) {
            Some(Button::Ok) => self.ok(),
            Some(Button::Cancel) => self.cancel(),
            None => Vec::new(),
        }
    }

    /// Draws the panel into `buf` (the 640 x 480 picture).
    pub fn draw(&self, buf: &mut Vec<u32>, art: &CreditsArt, data: &CreditsData, hover: Option<Button>, caret: bool) {
        const EDGE: u32 = 0xFFB8_C0D8;
        let (ox, oy) = (self.x, self.y);
        let r = |a: [i32; 4]| [a[0] + ox, a[1] + oy, a[2] + ox, a[3] + oy];
        fill(buf, [ox, oy, ox + PANEL_W, oy + PANEL_H], 0xFF14_1824);
        frame(buf, [ox, oy, ox + PANEL_W, oy + PANEL_H], EDGE, 2);
        let lh = 26;
        for (i, line) in self.text.label(160, 3).split(|&c| c == b'\r').enumerate() {
            put_text(buf, art, data, line, ox + PROMPT_AT.0, oy + PROMPT_AT.1 + i as i32 * lh, false);
        }
        let f = r(FIELD);
        fill(buf, f, 0xFF00_0000);
        frame(buf, f, EDGE, 1);
        let (tx, ty) = (f[0] + 8, f[1] + 9);
        if self.selected && !self.field.is_empty() {
            let w = text_width(data, &self.field);
            fill(buf, [tx - 2, ty - 3, tx + w + 3, ty + 17], 0xFF38_4470);
        }
        put_text(buf, art, data, &self.field, tx, ty, true);
        if caret && !self.selected {
            let cx = tx + text_width(data, &self.field) + 2;
            fill(buf, [cx, ty - 1, cx + 2, ty + 15], 0xFFFF_FFFF);
        }
        for (b, rect, item) in [(Button::Ok, OK_RECT, 1u16), (Button::Cancel, CANCEL_RECT, 2)] {
            let rr = r(rect);
            fill(buf, rr, if hover == Some(b) { 0xFF38_4470 } else { 0xFF24_2C48 });
            frame(buf, rr, EDGE, if b == Button::Ok { 2 } else { 1 });
            let s = self.text.label(160, item);
            let w = text_width(data, s);
            put_text(buf, art, data, s, rr[0] + (rr[2] - rr[0] - w) / 2, rr[1] + 12, hover == Some(b));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text() -> DialogText {
        DialogText::parse("ditl 160 1 4 0 0 0 0 4f4b\nditl 160 2 4 0 0 0 0 43\nditl 160 3 8 0 0 0 0 41\nmenu 1 1 41\n").unwrap()
    }

    #[test]
    fn typing_replaces_the_selected_one_and_ok_clamps() {
        let (mut u, ops) = LevelUi::new(28, text(), 640, 480);
        assert_eq!(ops, vec![Op::SetText(b"1".to_vec()), Op::Select, Op::Snd(34, 4, 0)]);
        u.typed(b'4');
        u.typed(b'5');
        assert_eq!(u.field, b"45");
        let ops = u.key(Key::Return);
        assert_eq!(ops, vec![Op::SetText(b"28".to_vec()), Op::Delay(30)]);
        assert_eq!(u.result(), None, "the clamped number stays on screen for 30 ticks");
        for _ in 0..30 { u.tick(); }
        assert_eq!(u.result(), Some(28));
    }

    #[test]
    fn zero_or_empty_starts_nothing_and_cancel_is_silent() {
        let (mut u, _) = LevelUi::new(28, text(), 640, 480);
        u.key(Key::Backspace);
        assert!(u.field.is_empty());
        let ops = u.key(Key::Return);
        assert_eq!(ops, vec![Op::SetText(b"0".to_vec()), Op::Delay(30)]);
        for _ in 0..30 { u.tick(); }
        assert_eq!(u.result(), Some(0));
        let (mut u, _) = LevelUi::new(28, text(), 640, 480);
        assert!(u.key(Key::Escape).iter().all(|o| matches!(o, Op::Flash(2))));
        assert_eq!(u.result(), Some(0));
    }

    #[test]
    fn letters_beep_and_the_field_stays_numeric() {
        let (mut u, _) = LevelUi::new(28, text(), 640, 480);
        assert_eq!(u.typed(b'x'), vec![Op::Beep]);
        for _ in 0..20 { u.typed(b'7'); }
        assert_eq!(u.field.len(), MAX_DIGITS);
        assert!(u.field.iter().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn buttons_are_clickable() {
        let (mut u, _) = LevelUi::new(28, text(), 640, 480);
        let (x, y) = (u.x + OK_RECT[0] + 3, u.y + OK_RECT[1] + 3);
        assert_eq!(u.hit(x, y), Some(Button::Ok));
        u.click(x, y);
        for _ in 0..30 { u.tick(); }
        assert_eq!(u.result(), Some(1));
    }
}
