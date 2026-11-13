//! The title screen's buttons (the original's main loop `0x1a500` and its helpers `0x1c098`, `0x1c5cc`, `0x1c6f8`, `0x1c998`, see RE_NOTES.md).
//!
//! What the original does, in the terms of this port:
//! * eleven screen rectangles (`'Rect'` resources 1000..1010 of the application) and eleven art rectangles on an off-screen sheet
//!   (`0x1b054`: the normal button pictures stacked at x = 0, each hilited picture 200 pixels to the right, `SHEET` below);
//! * `hit` (0x1c098) names the button under the pointer, in the order 1 Player (1), 2 Players (2), [Register (6) unless the game is registered],
//!   Controls (7), Preferences (8), High Scores (9), Credits (10), Quit (11); the Host / Join / Editor rectangles (ids 3..5) are never tested;
//! * `hover_to` is the pointer-tracking block of the main loop: entering a button un-hilites everything, hilites that button and plays sound 20
//!   (priority 3); leaving all buttons un-hilites; both restart the idle timer;
//! * `press` / `Press::step` are the click tracking `0x1c998` (sound 29, priority 3; the button is hilited while the pointer is inside it and
//!   everything is drawn normal when it leaves; a click counts when the button is released inside the rectangle).
//!
//! Everything the original *draws* is returned as `Op`s (a copy of an art rectangle to a screen rectangle with transfer mode 36, the screen
//! refresh, sounds), which `tests/menu_golden.rs` compares with the original's own calls. The renderer of this port turns a `Blit` into
//! "show button k normal / hilited" (`Op::button_state`).

/// A QuickDraw rectangle (signed 16 bit, `top, left, bottom, right`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub top: i16,
    pub left: i16,
    pub bottom: i16,
    pub right: i16,
}

impl Rect {
    pub const fn new(top: i16, left: i16, bottom: i16, right: i16) -> Rect {
        Rect { top, left, bottom, right }
    }
    /// `PtInRect`: the top and left edges are inside, the bottom and right edges are not.
    pub fn contains(&self, v: i16, h: i16) -> bool {
        self.top <= v && v < self.bottom && self.left <= h && h < self.right
    }
    pub fn offset(self, dh: i16, dv: i16) -> Rect {
        Rect { top: self.top.wrapping_add(dv), left: self.left.wrapping_add(dh), bottom: self.bottom.wrapping_add(dv), right: self.right.wrapping_add(dh) }
    }
    pub fn width(&self) -> i32 { self.right as i32 - self.left as i32 }
    pub fn height(&self) -> i32 { self.bottom as i32 - self.top as i32 }
}

/// Art rectangles of the normal pictures on the off-screen sheet (set by `0x1b054`; k = resource 1000 + k).
pub const SHEET: [Rect; 11] = [
    Rect::new(0, 0, 32, 79),
    Rect::new(32, 0, 64, 79),
    Rect::new(64, 0, 96, 126),
    Rect::new(96, 0, 128, 123),
    Rect::new(128, 0, 160, 57),
    Rect::new(160, 0, 192, 77),
    Rect::new(192, 0, 224, 82),
    Rect::new(224, 0, 256, 112),
    Rect::new(256, 0, 288, 105),
    Rect::new(288, 0, 320, 66),
    Rect::new(320, 0, 352, 36),
];
/// The hilited picture of a button lies this many pixels to the right of the normal one on the sheet.
pub const HILITE_DX: i16 = 200;
/// QuickDraw "transparent" transfer mode (the copies use mode 36).
pub const BLIT_MODE: i32 = 36;

/// Sounds of the title screen (ids of the sound engine, priority 3).
pub const SOUND_HOVER: i32 = 20;
pub const SOUND_PRESS: i32 = 29;
pub const SOUND_PRIORITY: i32 = 3;

/// Screen rectangle index k of button number `idx` (the numbers `hit` returns); `None` for 0 and the unused 3..5.
pub fn rect_index(idx: u32) -> Option<usize> {
    match idx {
        1 | 2 | 6..=11 => Some(idx as usize - 1),
        _ => None,
    }
}

/// The handler addresses of the original's click dispatch table (`D.5bb8`, indexed by the button number).
pub const HANDLER_TABLE: [u32; 12] = [0x1be48, 0x1bdb4, 0x1bdc0, 0x1be48, 0x1be48, 0x1bdcc, 0x1bdd4, 0x1bde8, 0x1bdf0, 0x1bdf8, 0x1be3c, 0x1be44];

/// What a completed click does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `0x1cb80`: start a one-player game at level 1 (with the option key held the original first asks for a level).
    OnePlayer,
    /// `0x1cbe8`: start a two-player game at level 1.
    TwoPlayers,
    /// `0x1cc8c`: the registration dialog (only shown by an unregistered copy).
    Register,
    /// `0x1ccbc`: the key / controller set-up dialog.
    Controls,
    /// `0x1cd10`: the preferences dialog.
    Preferences,
    /// `0x1cd5c` (through `0x1bdf8`): the high-score screen.
    HighScores,
    /// `0x1ce24`: the credits.
    Credits,
    /// `0x1cee0`: leave the program.
    Quit,
}

pub fn action(idx: u32) -> Option<Action> {
    Some(match idx {
        1 => Action::OnePlayer,
        2 => Action::TwoPlayers,
        6 => Action::Register,
        7 => Action::Controls,
        8 => Action::Preferences,
        9 => Action::HighScores,
        10 => Action::Credits,
        11 => Action::Quit,
        _ => return None,
    })
}

/// Everything the original does on screen or with sound while the buttons work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `CopyBits` from the sheet to the window (`0x170`): source rectangle, destination rectangle, transfer mode.
    Blit { src: Rect, dst: Rect, mode: i32 },
    /// The window refresh (`0x1b958`) after drawing.
    Refresh,
    Sound { id: i32, prio: i32, delay: i32 },
    /// The idle timer of the main loop was restarted (`TickCount` stored).
    ResetIdle,
}

impl Op {
    /// For a `Blit` of a button picture: (k, hilited).
    pub fn button_state(&self) -> Option<(usize, bool)> {
        if let Op::Blit { src, .. } = self {
            for (k, s) in SHEET.iter().enumerate() {
                if *src == *s { return Some((k, false)); }
                if *src == s.offset(HILITE_DX, 0) { return Some((k, true)); }
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct Menu {
    /// The eleven screen rectangles (resources 1000..1010).
    pub screen: [Rect; 11],
    /// `D.153a`: nonzero in a registered copy, which has no Register button.
    pub registered: bool,
    /// Window origin in global coordinates (`D.9368`, `D.936c`): points are made window-local by subtracting it.
    pub origin: (i32, i32),
    /// The hovered button (0 = none): `r25` of the main loop.
    pub hover: u32,
}

/// A click in progress (`0x1c998`).
#[derive(Debug, Clone, Copy)]
pub struct Press {
    pub idx: u32,
    pub rect: Rect,
}

impl Menu {
    pub fn new(screen: [Rect; 11], registered: bool, origin: (i32, i32)) -> Menu {
        Menu { screen, registered, origin, hover: 0 }
    }

    fn local(&self, h: i32, v: i32) -> (i16, i16) {
        // lha / subf / sth: the 16 bit coordinate minus the 32 bit origin, truncated to 16 bits
        let lh = (h as i16 as i32).wrapping_sub(self.origin.0) as i16;
        let lv = (v as i16 as i32).wrapping_sub(self.origin.1) as i16;
        (lh, lv)
    }

    /// `0x1c098`: the button under the global point (h, v), 0 when there is none.
    pub fn hit(&self, h: i32, v: i32) -> u32 {
        let (lh, lv) = self.local(h, v);
        let order: [(usize, u32); 8] = [(0, 1), (1, 2), (5, 6), (6, 7), (7, 8), (8, 9), (9, 10), (10, 11)];
        for (k, idx) in order {
            if idx == 6 && self.registered {
                continue;
            }
            if self.screen[k].contains(lv, lh) {
                return idx;
            }
        }
        0
    }

    /// `0x1c6f8`: draw one button hilited.
    pub fn hilite(&self, idx: u32) -> Vec<Op> {
        let Some(k) = rect_index(idx) else { return vec![] };
        if idx == 6 && self.registered {
            return vec![];
        }
        vec![Op::Blit { src: SHEET[k].offset(HILITE_DX, 0), dst: self.screen[k], mode: BLIT_MODE }, Op::Refresh]
    }

    /// `0x1c5cc`: draw every button normal.
    pub fn unhilite_all(&self) -> Vec<Op> {
        let mut ops = Vec::new();
        for k in [0usize, 1, 5, 6, 7, 8, 9, 10] {
            if k == 5 && self.registered {
                continue;
            }
            ops.push(Op::Blit { src: SHEET[k], dst: self.screen[k], mode: BLIT_MODE });
        }
        ops.push(Op::Refresh);
        ops
    }

    /// The pointer-tracking block of the main loop (`0x1a6f0..0x1a770`) for a pointer at the global point (h, v).
    pub fn hover_to(&mut self, h: i32, v: i32) -> Vec<Op> {
        let idx = self.hit(h, v);
        let mut ops = Vec::new();
        if idx != 0 {
            if idx != self.hover {
                ops.extend(self.unhilite_all());
                ops.extend(self.hilite(idx));
                ops.push(Op::Sound { id: SOUND_HOVER, prio: SOUND_PRIORITY, delay: 0 });
                ops.push(Op::ResetIdle);
            }
            self.hover = idx;
        } else {
            if self.hover != 0 {
                ops.extend(self.unhilite_all());
                ops.push(Op::ResetIdle);
            }
            self.hover = 0;
        }
        ops
    }

    /// `0x1c998` up to the first `StillDown`: the press sound, and the tracking state when the button can be pressed at all.
    pub fn press(&self, idx: u32) -> (Vec<Op>, Option<Press>) {
        let ops = vec![Op::Sound { id: SOUND_PRESS, prio: SOUND_PRIORITY, delay: 0 }];
        match rect_index(idx) {
            Some(k) => (ops, Some(Press { idx, rect: self.screen[k] })),
            None => (ops, None),
        }
    }

    /// The buttons that exist in this copy, in the order `hit` tests them (used for keyboard and gamepad navigation, an addition of the port).
    pub fn visible(&self) -> Vec<u32> {
        [1u32, 2, 6, 7, 8, 9, 10, 11].into_iter().filter(|&i| !(i == 6 && self.registered)).collect()
    }

    /// Move the keyboard / pad focus by `delta` buttons (wrapping) and return the same operations a pointer entering that button gives.
    /// An addition of the port (the original is driven by the mouse; its menu items and key shortcuts are not ported yet).
    pub fn focus_step(&mut self, delta: i32) -> Vec<Op> {
        let list = self.visible();
        let pos = list.iter().position(|&i| i == self.hover);
        let next = match pos {
            Some(p) => list[(p as i32 + delta).rem_euclid(list.len() as i32) as usize],
            None => if delta >= 0 { list[0] } else { list[list.len() - 1] },
        };
        let mut ops = Vec::new();
        if next != self.hover {
            ops.extend(self.unhilite_all());
            ops.extend(self.hilite(next));
            ops.push(Op::Sound { id: SOUND_HOVER, prio: SOUND_PRIORITY, delay: 0 });
            ops.push(Op::ResetIdle);
        }
        self.hover = next;
        ops
    }
}

impl Press {
    /// One round of the tracking loop with the pointer at the global point (h, v): while the mouse button is down the button is hilited
    /// when the pointer is inside its rectangle and everything is drawn normal otherwise; when it is released everything is drawn normal and
    /// the result says whether the pointer was inside.
    pub fn step(&self, menu: &Menu, h: i32, v: i32, down: bool) -> (Vec<Op>, Option<bool>) {
        let (lh, lv) = menu.local(h, v);
        let inside = self.rect.contains(lv, lh);
        if down {
            (if inside { menu.hilite(self.idx) } else { menu.unhilite_all() }, None)
        } else {
            (menu.unhilite_all(), Some(inside))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> Menu {
        let l = |t: i16, l: i16, b: i16, r: i16| Rect::new(t, l, b, r);
        Menu::new(
            [l(71, 428, 100, 504), l(115, 428, 144, 507), l(214, 428, 262, 589), l(287, 428, 335, 512), l(159, 428, 188, 484), l(159, 428, 188, 505),
             l(203, 428, 232, 505), l(247, 428, 276, 540), l(291, 428, 320, 533), l(335, 428, 364, 494), l(379, 428, 411, 464)],
            false,
            (0, 0),
        )
    }

    #[test]
    fn the_shipped_layout_is_hit_where_the_pictures_are() {
        let m = real();
        assert_eq!(m.hit(450, 80), 1);
        assert_eq!(m.hit(450, 120), 2);
        assert_eq!(m.hit(450, 170), 6);
        assert_eq!(m.hit(450, 215), 7);
        assert_eq!(m.hit(450, 260), 8);
        assert_eq!(m.hit(450, 300), 9);
        assert_eq!(m.hit(450, 350), 10);
        assert_eq!(m.hit(440, 395), 11);
        assert_eq!(m.hit(450, 230), 7, "Controls rectangle 203..232");
        assert_eq!(m.hit(100, 100), 0);
        // the Host / Join rectangles are not tested
        assert_eq!(m.hit(450, 240), 0);
        assert_eq!(m.hit(450, 300), 9);
        let mut reg = real();
        reg.registered = true;
        assert_eq!(reg.hit(450, 170), 0);
        assert_eq!(reg.visible(), vec![1, 2, 7, 8, 9, 10, 11]);
    }

    #[test]
    fn hovering_hilites_plays_the_sound_once_and_leaving_clears() {
        let mut m = real();
        let ops = m.hover_to(450, 80);
        assert_eq!(ops.iter().filter(|o| matches!(o, Op::Sound { id: 20, prio: 3, delay: 0 })).count(), 1);
        assert!(ops.contains(&Op::ResetIdle));
        assert_eq!(m.hover, 1);
        assert!(m.hover_to(451, 81).is_empty(), "moving inside the same button does nothing");
        let ops = m.hover_to(10, 10);
        assert_eq!(m.hover, 0);
        assert!(ops.iter().all(|o| !matches!(o, Op::Sound { .. })));
        assert!(m.hover_to(10, 11).is_empty());
    }

    #[test]
    fn click_tracking_follows_the_pointer_and_reports_where_it_was_released() {
        let m = real();
        let (ops, p) = m.press(2);
        assert_eq!(ops, vec![Op::Sound { id: 29, prio: 3, delay: 0 }]);
        let p = p.unwrap();
        let (o, r) = p.step(&m, 450, 120, true);
        assert!(r.is_none() && o.iter().any(|x| x.button_state() == Some((1, true))));
        let (o, r) = p.step(&m, 10, 10, true);
        assert!(r.is_none() && o.iter().all(|x| x.button_state().map_or(true, |(_, h)| !h)));
        assert_eq!(p.step(&m, 450, 120, false).1, Some(true));
        assert_eq!(p.step(&m, 10, 10, false).1, Some(false));
        assert!(m.press(4).1.is_none() && m.press(0).1.is_none() && m.press(99).1.is_none());
    }

    #[test]
    fn actions() {
        assert_eq!(action(1), Some(Action::OnePlayer));
        assert_eq!(action(2), Some(Action::TwoPlayers));
        assert_eq!(action(11), Some(Action::Quit));
        assert_eq!(action(3), None);
    }

    #[test]
    fn keyboard_focus_walks_the_visible_buttons() {
        let mut m = real();
        m.focus_step(1);
        assert_eq!(m.hover, 1);
        m.focus_step(1);
        assert_eq!(m.hover, 2);
        m.focus_step(1);
        assert_eq!(m.hover, 6);
        m.focus_step(-1);
        assert_eq!(m.hover, 2);
        m.hover = 0;
        m.focus_step(-1);
        assert_eq!(m.hover, 11);
        m.focus_step(1);
        assert_eq!(m.hover, 1, "wraps around");
        m.registered = true;
        m.hover = 2;
        m.focus_step(1);
        assert_eq!(m.hover, 7, "no Register button in a registered copy");
    }
}
