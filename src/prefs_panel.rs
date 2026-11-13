//! The Preferences screen of this port: the original's dialog (`DLOG` 190 with the item lists 191 / 192, see `prefs.rs`) drawn with the game's bitmap font.
//!
//! The logic is the original's ([`crate::prefs::Dialog`], verified against the original's code); this file only lays the items out, turns clicks and keys into
//! `Dialog::hit` calls and follows the returned [`Op`]s (control values, enabled items, enabled menu items) to know what to draw. The labels and the pop-up
//! entries are the original's (`ui/dialog_text.txt`, made by the extractor). Options that the port cannot honour (the screen resizing, line skipping, the music
//! quality / mode / surround / filtering of the Mac sound engine) are shown greyed: their values are kept in the file but nothing reads them.

use crate::credits::{CreditsArt, CreditsData};
use crate::prefs::{item, Closed, Dialog, Op, Prefs};
use crate::scores::{fill, frame, put_text, text_width};
use std::collections::{BTreeMap, BTreeSet};

pub const PANEL_W: i32 = 560;
pub const PANEL_H: i32 = 448;
const TAB_Y: i32 = 16;
const TAB_H: i32 = 30;
const TAB_W: i32 = 200;
const ROW_Y: i32 = 66;
const ROW_H: i32 = 30;
const BTN_Y: i32 = 400;
const BTN_H: i32 = 32;
const NOTE_Y: i32 = 308;
/// The hover help box (the original's static text item 10): below the page frame, above the buttons.
const HELP_Y: i32 = 344;
const HELP_H: i32 = 50;

/// The texts of the dialogs (Level Select 160, Preferences 190..192, pop-up menus) from `ui/dialog_text.txt` (hex, MacRoman).
#[derive(Debug, Clone, Default)]
pub struct DialogText {
    ditl: BTreeMap<(u16, u16), Vec<u8>>,
    menu: BTreeMap<(u16, u16), Vec<u8>>,
    help: BTreeMap<(u16, u16), Vec<u8>>,
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s == "-" { return Some(Vec::new()); }
    if s.len() % 2 != 0 { return None; }
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()).collect()
}

impl DialogText {
    pub fn parse(text: &str) -> Option<DialogText> {
        let mut t = DialogText::default();
        for line in text.lines() {
            let f: Vec<&str> = line.split(' ').collect();
            match f.first().copied() {
                Some("ditl") if f.len() == 9 => { t.ditl.insert((f[1].parse().ok()?, f[2].parse().ok()?), unhex(f[8])?); }
                Some("menu") if f.len() == 4 => { t.menu.insert((f[1].parse().ok()?, f[2].parse().ok()?), unhex(f[3])?); }
                Some("help") if f.len() == 4 => { t.help.insert((f[1].parse().ok()?, f[2].parse().ok()?), unhex(f[3])?); }
                _ => {}
            }
        }
        if t.ditl.is_empty() || t.menu.is_empty() { None } else { Some(t) }
    }

    pub fn label(&self, ditl: u16, item: u16) -> &[u8] { self.ditl.get(&(ditl, item)).map(|v| v.as_slice()).unwrap_or(b"") }
    pub fn entry(&self, menu: u16, item: u16) -> &[u8] { self.menu.get(&(menu, item)).map(|v| v.as_slice()).unwrap_or(b"") }
    /// `GetIndString(STR# res, index)` of the hover help (res 132 / 133 by page, 135 for the buttons); empty when missing.
    pub fn help(&self, res: u16, index: u16) -> &[u8] { self.help.get(&(res, index)).map(|v| v.as_slice()).unwrap_or(b"") }
    pub fn entries(&self, menu: u16) -> usize { self.menu.range((menu, 0)..=(menu, u16::MAX)).count() }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind { Check, Popup(u16) }

/// One row of a page: the dialog item that takes the hit, the label (item of the appended list) and what it is.
#[derive(Clone, Copy, Debug)]
struct Row { item: i16, label: u16, kind: Kind }

fn rows(page: i16) -> &'static [Row] {
    use Kind::*;
    const P1: [Row; 6] = [
        Row { item: item::RESIZE, label: 1, kind: Check },
        Row { item: item::ESCAPE_DELAY, label: 2, kind: Check },
        Row { item: item::LINE_SKIPPING, label: 3, kind: Check },
        Row { item: item::TRANSLUCENCY, label: 4, kind: Check },
        Row { item: item::EXTRA_DETAIL, label: 5, kind: Check },
        Row { item: item::SHADOWS, label: 6, kind: Check },
    ];
    const P2: [Row; 8] = [
        Row { item: item::VOLUME, label: 1, kind: Popup(1001) },
        Row { item: item::SYSTEM_VOLUME, label: 3, kind: Check },
        Row { item: item::MUSIC_TYPE, label: 4, kind: Popup(1002) },
        Row { item: item::MUSIC_QUALITY, label: 6, kind: Popup(1003) },
        Row { item: item::MUSIC_MODE, label: 8, kind: Popup(1004) },
        Row { item: item::SURROUND, label: 10, kind: Check },
        Row { item: item::FILTERING, label: 11, kind: Check },
        Row { item: item::INTERFACE_MUSIC, label: 12, kind: Check },
    ];
    if page == 2 { &P2 } else { &P1 }
}

/// What this port cannot use.
fn unused(it: i16, page: i16) -> bool {
    match page {
        1 => it == item::RESIZE || it == item::LINE_SKIPPING,
        _ => matches!(it, item::MUSIC_QUALITY | item::MUSIC_MODE | item::SURROUND | item::FILTERING),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiKey { Up, Down, Left, Right, Activate, Escape, NextPage, PrevPage }

/// A thing the pointer or the keyboard can reach.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target { Tab(i16), Row(i16), Button(i16) }

const BUTTONS: [i16; 4] = [item::FACTORY, item::REVERT, item::CANCEL, item::SAVE];

#[derive(Debug, Clone)]
pub struct PrefsUi {
    dialog: Dialog,
    text: DialogText,
    values: BTreeMap<i16, i16>,
    disabled: BTreeSet<i16>,
    menu_off: BTreeSet<i16>,
    focus: usize,
    /// the help string shown (STR# id, index): set by the pointer (`hover_help`), kept when it is over nothing, as the filter proc `0x7d24` does
    help: Option<(u16, u16)>,
    pub x: i32,
    pub y: i32,
}

impl PrefsUi {
    /// `view_w` x `view_h`: the picture the panel is centred in. `mod_ok`: the music can be played; there is no CD.
    pub fn new(prefs: Prefs, mod_ok: bool, text: DialogText, view_w: i32, view_h: i32) -> (PrefsUi, Vec<Op>) {
        let (dialog, ops) = Dialog::open(prefs, mod_ok, false);
        let mut ui = PrefsUi {
            dialog, text, values: BTreeMap::new(), disabled: BTreeSet::new(), menu_off: BTreeSet::new(), focus: 0, help: None,
            x: (view_w - PANEL_W) / 2, y: (view_h - PANEL_H) / 2,
        };
        ui.follow(&ops);
        (ui, ops)
    }

    pub fn prefs(&self) -> &Prefs { self.dialog.prefs() }
    pub fn closed(&self) -> Option<Closed> { self.dialog.closed() }
    pub fn page(&self) -> i16 { self.dialog.page() }

    fn follow(&mut self, ops: &[Op]) {
        for op in ops {
            match *op {
                Op::Clear => { self.values.clear(); self.disabled.clear(); self.focus = 0; }
                Op::Set(i, v) => { self.values.insert(i, v); }
                Op::Enable(i) => { self.disabled.remove(&i); }
                Op::Disable(i) => { self.disabled.insert(i); }
                Op::MenuEnable(i) => { self.menu_off.remove(&i); }
                Op::MenuDisable(i) => { self.menu_off.insert(i); }
                _ => {}
            }
        }
    }

    fn usable(&self, it: i16) -> bool { !unused(it, self.page()) && !self.disabled.contains(&it) }

    fn row_rect(&self, n: usize) -> [i32; 4] {
        let y = self.y + ROW_Y + n as i32 * ROW_H;
        [self.x + 16, y, self.x + PANEL_W - 16, y + ROW_H - 4]
    }

    fn tab_rect(&self, page: i16) -> [i32; 4] {
        let x = self.x + 16 + (page as i32 - 1) * (TAB_W + 8);
        [x, self.y + TAB_Y, x + TAB_W, self.y + TAB_Y + TAB_H]
    }

    fn button_rect(&self, it: i16) -> [i32; 4] {
        // Factory Settings and Revert on the left, Cancel and Save on the right (BUTTONS order)
        const WIDTHS: [i32; 4] = [190, 100, 110, 100];
        let k = BUTTONS.iter().position(|&b| b == it).unwrap_or(0);
        let x = match k {
            0 => self.x + 16,
            1 => self.x + 16 + WIDTHS[0] + 8,
            2 => self.x + PANEL_W - 16 - WIDTHS[3] - 8 - WIDTHS[2],
            _ => self.x + PANEL_W - 16 - WIDTHS[3],
        };
        [x, self.y + BTN_Y, x + WIDTHS[k], self.y + BTN_Y + BTN_H]
    }

    /// The popup's box (right part of the row).
    fn popup_rect(&self, n: usize) -> [i32; 4] {
        let r = self.row_rect(n);
        [r[0] + 250, r[1], r[2], r[3]]
    }

    pub fn target_at(&self, px: i32, py: i32) -> Option<Target> {
        let inside = |r: [i32; 4]| px >= r[0] && px < r[2] && py >= r[1] && py < r[3];
        for p in [1, 2] { if inside(self.tab_rect(p)) { return Some(Target::Tab(p)); } }
        for b in BUTTONS { if inside(self.button_rect(b)) { return Some(Target::Button(b)); } }
        for (n, r) in rows(self.page()).iter().enumerate() {
            if inside(self.row_rect(n)) && self.usable(r.item) {
                return Some(Target::Row(r.item));
            }
        }
        None
    }

    /// The dialog item under the pointer for the help text (`0x7c6c`: the first item whose rectangle holds the mouse; greyed items count): the help string it
    /// selects is STR# 135 index `item` for the dialog's own items and STR# `131 + page` index `item - 17` for the page's items (`0x7d24`).
    fn help_at(&self, px: i32, py: i32) -> Option<(u16, u16)> {
        let inside = |r: [i32; 4]| px >= r[0] && px < r[2] && py >= r[1] && py < r[3];
        for p in [1, 2] { if inside(self.tab_rect(p)) { return Some((135, item::PAGE_MENU as u16)); } }
        for b in BUTTONS { if inside(self.button_rect(b)) { return Some((135, b as u16)); } }
        for (n, r) in rows(self.page()).iter().enumerate() {
            if inside(self.row_rect(n)) { return Some((131 + self.page() as u16, (r.item - 17) as u16)); }
        }
        None
    }

    /// The pointer moved to (`px`, `py`): the help text changes when it is over an item.
    /// Returns true when the shown text changed.
    pub fn hover_help(&mut self, px: i32, py: i32) -> bool {
        match self.help_at(px, py) {
            Some(h) if self.help != Some(h) => { self.help = Some(h); true }
            _ => false,
        }
    }

    /// The help text shown now.
    pub fn help_text(&self) -> &[u8] { self.help.map(|(r, i)| self.text.help(r, i)).unwrap_or(b"") }

    fn popup_entries(&self, menu: u16) -> i16 { self.text.entries(menu) as i16 }

    /// The next (or previous) value of a pop-up: separators ("-") and menu items the original disables (the CD without a CD) are skipped.
    fn cycle(&self, it: i16, menu: u16, dir: i16) -> i16 {
        let n = self.popup_entries(menu).max(1);
        let cur = self.values.get(&it).copied().unwrap_or(1).clamp(1, n);
        let mut v = cur;
        for _ in 0..n {
            v = (v - 1 + dir).rem_euclid(n) + 1;
            let sep = self.text.entry(menu, v as u16) == b"-";
            let off = menu == 1002 && self.menu_off.contains(&v);
            if !sep && !off { return v; }
        }
        cur
    }

    /// Activate an item (a click or Return / Space on it); `dir` is +1 / -1 for a pop-up.
    fn activate(&mut self, t: Target, dir: i16) -> Vec<Op> {
        let ops = match t {
            Target::Tab(p) => self.dialog.hit(item::PAGE_MENU, p),
            Target::Button(b) => self.dialog.hit(b, 0),
            Target::Row(it) => {
                let Some(row) = rows(self.page()).iter().find(|r| r.item == it).copied() else { return Vec::new(); };
                match row.kind {
                    Kind::Check => self.dialog.hit(it, 0),
                    Kind::Popup(m) => {
                        // the control itself holds the choice (the original reads it back with GetControlValue)
                        let v = self.cycle(it, m, dir);
                        self.values.insert(it, v);
                        self.dialog.hit(it, v)
                    }
                }
            }
        };
        self.follow(&ops);
        ops
    }

    /// A click on the panel. `back`: the right button (pop-ups go backwards).
    pub fn click(&mut self, px: i32, py: i32, back: bool) -> Vec<Op> {
        match self.target_at(px, py) { Some(t) => self.activate(t, if back { -1 } else { 1 }), None => Vec::new() }
    }

    fn focusables(&self) -> Vec<Target> {
        let mut v: Vec<Target> = rows(self.page()).iter().filter(|r| self.usable(r.item)).map(|r| Target::Row(r.item)).collect();
        v.extend(BUTTONS.iter().map(|&b| Target::Button(b)));
        v
    }

    pub fn focused(&self) -> Option<Target> { self.focusables().get(self.focus).copied() }

    pub fn key(&mut self, k: UiKey) -> Vec<Op> {
        let list = self.focusables();
        let n = list.len().max(1);
        self.focus = self.focus.min(n - 1);
        match k {
            UiKey::Down => { self.focus = (self.focus + 1) % n; Vec::new() }
            UiKey::Up => { self.focus = (self.focus + n - 1) % n; Vec::new() }
            UiKey::Escape => self.dialog.hit(item::CANCEL, 0),
            UiKey::NextPage | UiKey::PrevPage => {
                let p = if self.page() == 1 { 2 } else { 1 };
                let ops = self.dialog.hit(item::PAGE_MENU, p);
                self.follow(&ops);
                ops
            }
            UiKey::Activate | UiKey::Right | UiKey::Left => {
                let Some(t) = list.get(self.focus).copied() else { return Vec::new(); };
                let is_popup = matches!(t, Target::Row(it) if rows(self.page()).iter().any(|r| r.item == it && matches!(r.kind, Kind::Popup(_))));
                if matches!(k, UiKey::Left | UiKey::Right) && !is_popup { return Vec::new(); }
                self.activate(t, if k == UiKey::Left { -1 } else { 1 })
            }
        }
    }

    // --- drawing

    fn label(&self, n: usize) -> &[u8] {
        let r = rows(self.page())[n];
        self.text.label(190 + self.page() as u16, r.label as u16)
    }

    /// Draws the panel into `buf` (the 640 x 480 picture, over what is in it). `hover` is the target under the pointer.
    pub fn draw(&self, buf: &mut Vec<u32>, art: &CreditsArt, data: &CreditsData, hover: Option<Target>) {
        const BG: u32 = 0xFF14_1824;
        const EDGE: u32 = 0xFFB8_C0D8;
        const DIM: u32 = 0xFF48_4E64;
        fill(buf, [self.x, self.y, self.x + PANEL_W, self.y + PANEL_H], BG);
        frame(buf, [self.x, self.y, self.x + PANEL_W, self.y + PANEL_H], EDGE, 2);
        let focus = self.focused();
        // the two section tabs (menu 20000)
        for p in [1i16, 2] {
            let r = self.tab_rect(p);
            let on = self.page() == p;
            fill(buf, r, if on { 0xFF38_4470 } else { 0xFF1C_2238 });
            frame(buf, r, if on { EDGE } else { 0xFF60_6C90 }, if on { 2 } else { 1 });
            let s = self.text.entry(20000, p as u16);
            let w = text_width(data, s);
            put_text(buf, art, data, s, r[0] + (r[2] - r[0] - w) / 2, r[1] + 9, on || hover == Some(Target::Tab(p)));
        }
        // the page frame
        frame(buf, [self.x + 8, self.y + TAB_Y + TAB_H + 6, self.x + PANEL_W - 8, self.y + NOTE_Y + 28], 0xFF60_6C90, 1);
        for (n, row) in rows(self.page()).iter().enumerate() {
            let r = self.row_rect(n);
            let off = !self.usable(row.item);
            let lit = !off && (hover == Some(Target::Row(row.item)) || focus == Some(Target::Row(row.item)));
            let ink = |hi: bool| hi;
            let label = self.label(n);
            let ty = r[1] + 5;
            match row.kind {
                Kind::Check => {
                    let b = [r[0] + 6, r[1] + 4, r[0] + 24, r[1] + 22];
                    fill(buf, b, 0xFF00_0000);
                    frame(buf, b, if off { DIM } else { EDGE }, 1);
                    if self.values.get(&row.item).copied().unwrap_or(0) != 0 {
                        fill(buf, [b[0] + 4, b[1] + 4, b[2] - 4, b[3] - 4], if off { DIM } else { 0xFF70_E0A0 });
                    }
                    if off { dim_text(buf, art, data, label, r[0] + 36, ty, DIM); } else { put_text(buf, art, data, label, r[0] + 36, ty, ink(lit)); }
                }
                Kind::Popup(m) => {
                    if off { dim_text(buf, art, data, label, r[0] + 6, ty, DIM); } else { put_text(buf, art, data, label, r[0] + 6, ty, ink(lit)); }
                    let b = self.popup_rect(n);
                    fill(buf, b, if lit { 0xFF38_4470 } else { 0xFF24_2C48 });
                    frame(buf, b, if off { DIM } else { EDGE }, 1);
                    let v = self.values.get(&row.item).copied().unwrap_or(1);
                    let s = self.text.entry(m, v.max(1) as u16);
                    if off || unused(row.item, self.page()) { dim_text(buf, art, data, s, b[0] + 10, ty, DIM); } else { put_text(buf, art, data, s, b[0] + 10, ty, ink(lit)); }
                    // arrows of a pop-up
                    if !off { put_text(buf, art, data, b"<>", b[2] - 26, ty, false); }
                }
            }
            if focus == Some(Target::Row(row.item)) { frame(buf, [r[0] - 2, r[1] - 1, r[2] + 2, r[3] + 1], 0xFF70_E0A0, 1); }
        }
        // the note about greyed options
        let any_unused = rows(self.page()).iter().any(|r| unused(r.item, self.page()));
        if any_unused { dim_text(buf, art, data, b"Greyed: stored, not used by this version", self.x + 20, self.y + NOTE_Y, DIM); }
        // the hover help (item 10 of the dialog): the port's small font, wrapped to the box
        {
            let b = [self.x + 16, self.y + HELP_Y, self.x + PANEL_W - 16, self.y + HELP_Y + HELP_H];
            fill(buf, b, 0xFF0C_101A);
            frame(buf, b, 0xFF48_4E64, 1);
            for (i, l) in wrap(self.help_text(), ((b[2] - b[0] - 12) / 6) as usize).iter().take(5).enumerate() {
                crate::font::draw_text(buf, crate::credits::W, b[0] + 6, b[1] + 6 + i as i32 * 9, l, 1, 0xFFD0_D0E0);
            }
        }
        // buttons
        for b in BUTTONS {
            let r = self.button_rect(b);
            let hi = hover == Some(Target::Button(b));
            fill(buf, r, if hi { 0xFF38_4470 } else { 0xFF24_2C48 });
            frame(buf, r, EDGE, if b == item::SAVE { 2 } else { 1 });
            let k = BUTTONS.iter().position(|&x| x == b).unwrap_or(0);
            let s = self.text.label(190, [3u16, 4, 2, 1][k]);
            let w = text_width(data, s);
            put_text(buf, art, data, s, r[0] + (r[2] - r[0] - w) / 2, r[1] + 10, hi);
            if focus == Some(Target::Button(b)) { frame(buf, [r[0] - 3, r[1] - 3, r[2] + 3, r[3] + 3], 0xFF70_E0A0, 1); }
        }
    }
}

/// Word wrap of a MacRoman string into lines of at most `cols` characters (the port font has 6 pixels per character); characters above 127 become '?'.
fn wrap(s: &[u8], cols: usize) -> Vec<String> {
    let text: String = s.iter().map(|&c| if c < 128 && c >= 32 { c as char } else if c == b'\r' || c == b'\n' { ' ' } else { '?' }).collect();
    let mut lines = Vec::new();
    let mut cur = String::new();
    for w in text.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > cols { lines.push(std::mem::take(&mut cur)); }
        if !cur.is_empty() { cur.push(' '); }
        cur.push_str(w);
    }
    if !cur.is_empty() { lines.push(cur); }
    lines
}

/// Text in the dim colour: the game's font is a picture, so the text is drawn normally and the lit pixels pulled towards the panel colour.
fn dim_text(buf: &mut Vec<u32>, art: &CreditsArt, data: &CreditsData, s: &[u8], x: i32, y: i32, _c: u32) {
    let w = (text_width(data, s) + 2).max(1);
    let h = 16;
    let (x0, y0) = (x.max(0), y.max(0));
    let mut before = Vec::new();
    for yy in y0..(y0 + h).min(crate::credits::H as i32) {
        for xx in x0..(x0 + w).min(crate::credits::W as i32) { before.push(buf[yy as usize * crate::credits::W + xx as usize]); }
    }
    put_text(buf, art, data, s, x, y, false);
    let mut k = 0;
    for yy in y0..(y0 + h).min(crate::credits::H as i32) {
        for xx in x0..(x0 + w).min(crate::credits::W as i32) {
            let i = yy as usize * crate::credits::W + xx as usize;
            let (b, a) = (before[k], buf[i]);
            k += 1;
            if a != b {
                // 35 % of the glyph over the old pixel
                let mix = |s: u32, d: u32| (s * 35 + d * 65) / 100;
                let ch = |sh: u32| mix((a >> sh) & 255, (b >> sh) & 255) << sh;
                buf[i] = 0xFF00_0000 | ch(16) | ch(8) | ch(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text() -> DialogText {
        let mut s = String::from("ditl 190 1 4 0 0 0 0 53617665\n");
        for p in [191u16, 192] { for i in 1..=12 { s.push_str(&format!("ditl {} {} 5 0 0 0 0 4c{:02x}\n", p, i, i)); } }
        for (m, n) in [(1001u16, 9u16), (1002, 3), (1003, 7), (1004, 3), (20000, 2)] { for i in 1..=n { s.push_str(&format!("menu {} {} 41\n", m, i)); } }
        DialogText::parse(&s).unwrap()
    }

    fn ui() -> PrefsUi { PrefsUi::new(Prefs::factory(), true, text(), 640, 480).0 }

    #[test]
    fn parse_and_lookup() {
        let t = text();
        assert_eq!(t.label(191, 2), b"L\x02");
        assert_eq!(t.entries(1001), 9);
        assert!(DialogText::parse("junk").is_none());
    }

    #[test]
    fn page_one_clicks_toggle_and_unused_rows_ignore_them() {
        let mut u = ui();
        assert_eq!(u.page(), 1);
        let n = rows(1).iter().position(|r| r.item == item::SHADOWS).unwrap();
        let r = u.row_rect(n);
        let ops = u.click(r[0] + 5, r[1] + 5, false);
        assert!(ops.contains(&Op::Shadows(0)));
        assert!(!u.prefs().shadows());
        let n = rows(1).iter().position(|r| r.item == item::LINE_SKIPPING).unwrap();
        let r = u.row_rect(n);
        assert!(u.click(r[0] + 5, r[1] + 5, false).is_empty(), "line skipping is not used by the port");
        assert!(!u.prefs().line_skipping());
    }

    #[test]
    fn audio_page_volume_follows_system_volume_and_the_popup_cycles() {
        let mut u = ui();
        let tab = u.tab_rect(2);
        u.click(tab[0] + 5, tab[1] + 5, false);
        assert_eq!(u.page(), 2);
        // the factory settings use the system volume: the volume pop-up is not reachable
        let n = rows(2).iter().position(|r| r.item == item::VOLUME).unwrap();
        let r = u.row_rect(n);
        assert!(u.click(r[0] + 5, r[1] + 5, false).is_empty());
        let n2 = rows(2).iter().position(|r| r.item == item::SYSTEM_VOLUME).unwrap();
        let r2 = u.row_rect(n2);
        u.click(r2[0] + 5, r2[1] + 5, false);
        assert!(!u.prefs().system_volume());
        let r = u.row_rect(n);
        let ops = u.click(r[0] + 5, r[1] + 5, false);
        assert!(ops.contains(&Op::Volume));
        assert_eq!(u.prefs().volume(), 4 + 1, "the pop-up went from entry 5 (volume 4) to entry 6");
        let ops = u.click(r[0] + 5, r[1] + 5, true);
        assert!(ops.contains(&Op::Volume));
        assert_eq!(u.prefs().volume(), 4);
    }

    #[test]
    fn music_type_skips_the_cd() {
        let mut u = ui();
        u.key(UiKey::NextPage);
        let it = item::MUSIC_TYPE;
        assert_eq!(u.cycle(it, 1002, 1), 3, "MOD (1) -> None (3): no CD");
        assert_eq!(u.cycle(it, 1002, -1), 3);
    }

    #[test]
    fn keyboard_cancel_restores_and_save_keeps() {
        let mut u = ui();
        // find the escape delay row and flip it
        while u.focused() != Some(Target::Row(item::ESCAPE_DELAY)) { u.key(UiKey::Down); }
        u.key(UiKey::Activate);
        assert!(u.prefs().escape_delay());
        u.key(UiKey::Escape);
        assert_eq!(u.closed(), Some(Closed::Cancel));
        assert!(!u.prefs().escape_delay(), "Cancel puts the opening settings back");
        let mut u = ui();
        while u.focused() != Some(Target::Row(item::ESCAPE_DELAY)) { u.key(UiKey::Down); }
        u.key(UiKey::Activate);
        while u.focused() != Some(Target::Button(item::SAVE)) { u.key(UiKey::Down); }
        u.key(UiKey::Activate);
        assert_eq!(u.closed(), Some(Closed::Save));
        assert!(u.prefs().escape_delay());
    }

    #[test]
    fn factory_and_revert_buttons() {
        let mut u = ui();
        let n = rows(1).iter().position(|r| r.item == item::EXTRA_DETAIL).unwrap();
        let r = u.row_rect(n);
        u.click(r[0] + 5, r[1] + 5, false);
        assert!(!u.prefs().extra_detail());
        let b = u.button_rect(item::FACTORY);
        u.click(b[0] + 3, b[1] + 3, false);
        assert!(u.prefs().extra_detail());
        u.click(r[0] + 5, r[1] + 5, false);
        let b = u.button_rect(item::REVERT);
        u.click(b[0] + 3, b[1] + 3, false);
        assert!(u.prefs().extra_detail(), "Revert returns to the settings the dialog opened with");
    }

    #[test]
    fn buttons_do_not_overlap() {
        let u = ui();
        let rs: Vec<[i32; 4]> = BUTTONS.iter().map(|&b| u.button_rect(b)).collect();
        for i in 0..4 {
            assert!(rs[i][0] >= u.x && rs[i][2] <= u.x + PANEL_W);
            for j in i + 1..4 { assert!(rs[i][2] <= rs[j][0] || rs[j][2] <= rs[i][0], "{:?} {:?}", rs[i], rs[j]); }
        }
    }

    #[test]
    fn hover_help_picks_the_original_strings_and_keeps_them_over_empty_space() {
        let mut s = String::from("ditl 190 1 4 0 0 0 0 53617665\n");
        for p in [191u16, 192] { for i in 1..=12 { s.push_str(&format!("ditl {} {} 5 0 0 0 0 4c{:02x}\n", p, i, i)); } }
        for (m, n) in [(1001u16, 9u16), (1002, 3), (1003, 7), (1004, 3), (20000, 2)] { for i in 1..=n { s.push_str(&format!("menu {} {} 41\n", m, i)); } }
        // help <STR#> <index>: the text is "<res>/<index>" in hex
        for (res, n) in [(132u16, 6u16), (133, 12), (135, 25)] { for i in 1..=n { s.push_str(&format!("help {} {} {}\n", res, i, format!("{}/{}", res, i).bytes().map(|b| format!("{:02x}", b)).collect::<String>())); } }
        let mut u = PrefsUi::new(Prefs::factory(), true, DialogText::parse(&s).unwrap(), 640, 480).0;
        assert_eq!(u.help_text(), b"");
        // a row of page 1: STR# 132, index = item - 17 (shadows = item 23 -> 6; line skipping, greyed, counts too)
        let n = rows(1).iter().position(|r| r.item == item::SHADOWS).unwrap();
        let r = u.row_rect(n);
        assert!(u.hover_help(r[0] + 5, r[1] + 5));
        assert_eq!(u.help_text(), b"132/6");
        let n = rows(1).iter().position(|r| r.item == item::LINE_SKIPPING).unwrap();
        let r = u.row_rect(n);
        assert!(u.hover_help(r[0] + 5, r[1] + 5));
        assert_eq!(u.help_text(), b"132/3");
        // a button: STR# 135, index = item; the tabs: item 11
        let b = u.button_rect(item::REVERT);
        u.hover_help(b[0] + 3, b[1] + 3);
        assert_eq!(u.help_text(), b"135/4");
        let t = u.tab_rect(2);
        u.hover_help(t[0] + 3, t[1] + 3);
        assert_eq!(u.help_text(), b"135/11");
        // over nothing: unchanged
        assert!(!u.hover_help(u.x + 2, u.y + 2));
        assert_eq!(u.help_text(), b"135/11");
        // page 2: STR# 133
        u.click(t[0] + 3, t[1] + 3, false);
        let n = rows(2).iter().position(|r| r.item == item::INTERFACE_MUSIC).unwrap();
        let r = u.row_rect(n);
        u.hover_help(r[0] + 5, r[1] + 5);
        assert_eq!(u.help_text(), b"133/12");
    }

    #[test]
    fn wrap_breaks_at_words() {
        assert_eq!(wrap(b"aaa bbb ccc", 7), vec!["aaa bbb", "ccc"]);
        assert_eq!(wrap(b"abcdefghij", 4), vec!["abcdefghij"]);
        assert_eq!(wrap(b"", 4), Vec::<String>::new());
    }
}
