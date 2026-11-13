//! The side-panel HUD logic of the original, ported from the disassembly and checked bit for bit by `tests/hud_golden.rs`
//! (vectors from `tools/re/gen_hud_vectors.py`, which run the original routines in the PowerPC emulator).
//!
//! The original keeps three offscreen surfaces: the *panel* (`D.81bc`, the 704 x 608 working buffer every sprite is drawn into, coordinates + 64), the
//! *store* (`D.81b4`, a strip bitmap holding a clean copy of the panel backgrounds) and the *screen* buffer (`D.9340`). Every HUD update restores a
//! rectangle from the store into the panel, draws sprites there and copies the finished rectangle to the screen. This module reproduces the decisions
//! and the exact sequence of those operations; the actual pixel copies are done by whoever implements [`HudWorld`].
//!
//! Routines: init `0x1e950` ([`Hud::init`]), score `0x2633c` ([`Hud::score`]), power bar `0x1ef5c` ([`Hud::power`]), shield `0x1f51c` / `0x1f5c4` / `0x1f69c`
//! ([`Hud::shield_target`], [`Hud::shield_tick`], [`Hud::shield`]), lives `0x1f848` ([`Hud::lives`]), weapon icons `0x1fe28` / `0x1ff94`
//! ([`Hud::weapon`], [`Hud::weapon_level`]), hit flash `0x1fc50` ([`Hud::flash`]).
use crate::ship::Ship;

/// A QuickDraw rectangle (top, left, bottom, right), as stored in the original's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub top: i16,
    pub left: i16,
    pub bottom: i16,
    pub right: i16,
}

impl Rect {
    fn at(top: i16, left: i16, h: i16, w: i16) -> Rect {
        Rect { top, left, bottom: top.wrapping_add(h), right: left.wrapping_add(w) }
    }
    pub fn to_bytes(self) -> [u8; 8] {
        let mut b = [0u8; 8];
        for (i, v) in [self.top, self.left, self.bottom, self.right].iter().enumerate() {
            b[i * 2..i * 2 + 2].copy_from_slice(&v.to_be_bytes());
        }
        b
    }
}

/// Which surface the original's drawing port (`SetGWorld`) is switched to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Port {
    /// `0x15f0`: the panel buffer (`D.8200`).
    Panel,
    /// `0x1590`: the screen buffer (`D.9340`).
    Screen,
    /// `0x15c0`: the store (`D.81fc`).
    Store,
}

/// One operation the HUD asks for, in the order the original performs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudCall {
    /// `0xd8(strip, rect)`: CopyBits store -> panel.
    Restore(Rect, Rect),
    /// `0x30(rect, dst)`: CopyBits panel -> screen.
    Present(Rect, Rect),
    /// `0x7c(rect, strip)`: CopyBits panel -> store (at start-up).
    Store(Rect, Rect),
    /// `0x307c` (clipped `Anim`) or `0x3634` (clipped `Flash`) at (x, y) in panel coordinates - 64.
    Blit { addr: u32, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32 },
    Port(Port),
    /// QuickDraw `OpColor`.
    OpColor([i16; 3]),
    PenMode(i32),
    ForeColor(i32),
    /// QuickDraw `PaintRect`.
    Paint(Rect),
}

pub const ANIM: u32 = 0x307c;
pub const FLASH: u32 = 0x3634;

pub trait HudWorld {
    /// `TickCount` replacement (`D.13e4`).
    fn tick(&self) -> u32;
    /// The four halfwords the original reads from `D.9340`: +64, +66 (the panel's top / left in panel coordinates), +72, +74 (the panel's top / left on the screen).
    fn origin(&self) -> [i16; 4];
    fn call(&mut self, c: HudCall);
}

// indices into the rectangle table (strip, rect pairs, 8 bytes each, starting at D.cab2)
const T: usize = 24;
const CAB2: usize = 0;
const CABA: usize = 1;
const CAC2: usize = 2;
const CACA: usize = 3;
const CAD2: usize = 4;
const CADA: usize = 5;
const CAE2: usize = 6;
const CAEA: usize = 7;
const CAF2: usize = 8;
const CAFA: usize = 9;
const CB02: usize = 10;
const CB0A: usize = 11;
const CB12: usize = 12;
const CB1A: usize = 13;
const CB22: usize = 14;
const CB2A: usize = 15;
const CB32: usize = 16;
const CB3A: usize = 17;
const CB42: usize = 18;
const CB4A: usize = 19;
const CB52: usize = 20;
const CB5A: usize = 21;
const CB62: usize = 22;
const CB6A: usize = 23;

#[derive(Debug, Clone)]
pub struct Hud {
    pub rects: [Rect; T],
    // the variables D.154c..D.157c
    pub p2_bar: i32,   // D.154c shown value of player 2's power bar
    pub p2_drawn: u8,  // D.1550
    pub p1_bar: i32,   // D.1554
    pub p1_drawn: u8,  // D.1558
    pub p2_delay: i32, // D.155c
    pub p2_next: u32,  // D.1560
    pub p2_cur: i32,   // D.1564
    pub p2_target: i32, // D.1568
    pub p1_delay: i32, // D.156c
    pub p1_next: u32,  // D.1570
    pub p1_cur: i32,   // D.1574
    pub p1_target: i32, // D.1578
}

impl Hud {
    pub fn new() -> Hud {
        Hud {
            rects: [Rect::default(); T], p2_bar: 0, p2_drawn: 0, p1_bar: 0, p1_drawn: 0, p2_delay: 0, p2_next: 0, p2_cur: 0, p2_target: 0, p1_delay: 0, p1_next: 0,
            p1_cur: 0, p1_target: 0,
        }
    }

    /// The bytes of `D.154c..D.1580` (52 bytes; the bytes after the two flag bytes are never written).
    pub fn vars_bytes(&self) -> [u8; 52] {
        let mut b = [0u8; 52];
        let w = |b: &mut [u8; 52], off: usize, v: i32| b[off..off + 4].copy_from_slice(&v.to_be_bytes());
        w(&mut b, 0, self.p2_bar);
        b[4] = self.p2_drawn;
        w(&mut b, 8, self.p1_bar);
        b[12] = self.p1_drawn;
        w(&mut b, 16, self.p2_delay);
        w(&mut b, 20, self.p2_next as i32);
        w(&mut b, 24, self.p2_cur);
        w(&mut b, 28, self.p2_target);
        w(&mut b, 32, self.p1_delay);
        w(&mut b, 36, self.p1_next as i32);
        w(&mut b, 40, self.p1_cur);
        w(&mut b, 44, self.p1_target);
        b
    }

    /// Poke one of the words of `D.154c..D.1578` (test hook).
    pub fn set_var(&mut self, addr: u32, v: u32) {
        match addr {
            0x154c => self.p2_bar = v as i32,
            0x1550 => self.p2_drawn = v as u8,
            0x1554 => self.p1_bar = v as i32,
            0x1558 => self.p1_drawn = v as u8,
            0x155c => self.p2_delay = v as i32,
            0x1560 => self.p2_next = v,
            0x1564 => self.p2_cur = v as i32,
            0x1568 => self.p2_target = v as i32,
            0x156c => self.p1_delay = v as i32,
            0x1570 => self.p1_next = v,
            0x1574 => self.p1_cur = v as i32,
            0x1578 => self.p1_target = v as i32,
            _ => panic!("hud var {:x}", addr),
        }
    }

    /// The bytes of the rectangle table `D.cab2..D.cb74`.
    pub fn rect_bytes(&self) -> [u8; 194] {
        let mut b = [0u8; 194];
        for (i, r) in self.rects.iter().enumerate() {
            b[i * 8..i * 8 + 8].copy_from_slice(&r.to_bytes());
        }
        b
    }

    /// 0x1e950: compute the panel rectangles from the panel origin and copy the clean backgrounds into the store.
    pub fn init<W: HudWorld>(&mut self, w: &mut W) {
        let [a, b, _, _] = w.origin();
        self.p1_drawn = 0;
        self.p2_drawn = 0;
        self.p1_bar = 0;
        self.p2_bar = 0;
        let r = &mut self.rects;
        let ad = |v: i16, n: i16| v.wrapping_add(n);
        r[CB6A] = Rect::at(ad(a, 95), ad(b, 28), 28, 104);
        r[CB5A] = Rect::at(ad(a, 328), ad(b, 28), 28, 104);
        r[CB62] = Rect::at(104, 0, 28, 104);
        r[CB4A] = Rect::at(ad(a, 48), ad(b, 29), 28, 104);
        r[CB3A] = Rect::at(ad(a, 281), ad(b, 29), 28, 104);
        r[CB42] = Rect::at(132, 0, 28, 104);
        r[CB2A] = Rect::at(ad(a, 149), ad(b, 27), 15, 108);
        r[CB1A] = Rect::at(ad(a, 382), ad(b, 27), 15, 108);
        r[CB22] = Rect::at(160, 0, 15, 108);
        r[CB0A] = Rect::at(ad(a, 197), ad(b, 27), 15, 108);
        r[CAFA] = Rect::at(ad(a, 430), ad(b, 27), 15, 108);
        r[CAEA] = Rect::at(ad(a, 11), ad(b, 72), 20, 31);
        r[CADA] = Rect::at(ad(a, 244), ad(b, 71), 20, 31);
        r[CACA] = Rect::at(ad(a, 11), ad(b, 106), 20, 31);
        r[CABA] = Rect::at(ad(a, 244), ad(b, 106), 20, 31);
        r[CB02] = Rect::at(175, 0, 15, 108);
        r[CB52] = Rect::at(190, 0, 28, 104);
        r[CB32] = Rect::at(218, 0, 28, 104);
        r[CAF2] = Rect::at(246, 0, 15, 108);
        r[CB12] = Rect::at(261, 0, 15, 108);
        r[CAE2] = Rect::at(276, 0, 20, 31);
        r[CAD2] = Rect::at(296, 0, 20, 31);
        r[CAC2] = Rect::at(296, 0, 20, 31);
        r[CAB2] = Rect::at(316, 0, 20, 31);
        w.call(HudCall::Port(Port::Store));
        let r = self.rects;
        for (s, d) in [(CB6A, CB62), (CB4A, CB42), (CB2A, CB22), (CB0A, CB02), (CAEA, CAE2), (CACA, CAE2), (CB5A, CB52), (CB3A, CB32), (CAFA, CAF2), (CB1A, CB12), (CADA, CAD2), (CABA, CAD2)] {
            w.call(HudCall::Store(r[s], r[d]));
        }
        w.call(HudCall::Port(Port::Screen));
    }

    fn blit<W: HudWorld>(w: &mut W, addr: u32, x: i32, y: i32, ww: i32, h: i32, anim: i32, frame: i32) {
        w.call(HudCall::Blit { addr, x, y, w: ww, h, anim, frame });
    }

    /// 0x2633c: draw the seven score digits of a ship when its score changed (`+160`) or `force` is set; the digits flash for a few ticks after a bonus (`+161`).
    pub fn score<W: HudWorld>(&mut self, ship: &mut Ship, force: u32, w: &mut W) {
        if ship.score_flash != 0 && w.tick() > ship.score_tick.wrapping_add(3) {
            ship.score_flash = 0;
            ship.score_flag = 1;
        }
        if ship.score_flag == 0 && force & 0xff == 0 { return; }
        let now = w.tick();
        ship.score_flag = 0;
        let p1 = ship.input_set == 1;
        let (rect, strip) = if p1 { (self.rects[CB6A], self.rects[CB62]) } else { (self.rects[CB5A], self.rects[CB52]) };
        let mut x = rect.left as i32 - 64;
        let y = rect.top as i32 - 64;
        w.call(HudCall::Restore(strip, rect));
        let mut digits = [0i32; 8];
        let mut n = ship.score;
        let mut k = 0usize;
        loop {
            digits[k] = n.wrapping_sub(n.wrapping_div(10).wrapping_mul(10));
            n = n.wrapping_div(10);
            k += 1;
            if k > 7 { k -= 1; }
            if n == 0 { break; }
        }
        w.call(HudCall::Port(Port::Panel));
        for i in (0..=6).rev() {
            let d = digits[i];
            if d == -1 { continue; }
            Self::blit(w, ANIM, x, y, 12, 28, 84, d.wrapping_add(1));
            if ship.score_flash != 0 {
                Self::blit(w, FLASH, x, y, 12, 28, 84, d.wrapping_add(1));
                if now > ship.score_tick.wrapping_add(1) { ship.score_flash = 0; }
            }
            x += 15;
        }
        w.call(HudCall::Port(Port::Screen));
        let [_, _, d72, d74] = w.origin();
        let top = d72.wrapping_add(if p1 { 95 } else { 328 });
        let left = d74.wrapping_add(28);
        w.call(HudCall::Present(rect, Rect::at(top, left, 28, 104)));
    }

    /// 0x1ef5c(player, value, flag): the wave-motion power bar (the charge of the beam): it shrinks by 3 per call toward `value`, grows at once, one 9 x 15 segment per 3 points above 20.
    pub fn power<W: HudWorld>(&mut self, player: u32, v: i32, flag: u32, w: &mut W) {
        let p1 = player & 0xff == 1;
        let (strip, rect) = if p1 { (self.rects[CB22], self.rects[CB2A]) } else { (self.rects[CB12], self.rects[CB1A]) };
        let [_, _, d72, d74] = w.origin();
        let top = d72.wrapping_add(if p1 { 149 } else { 382 });
        let dst = Rect::at(top, d74.wrapping_add(27), 15, 108);
        let flag = flag & 0xff != 0;
        let (cur, drawn) = if p1 { (&mut self.p1_bar, &mut self.p1_drawn) } else { (&mut self.p2_bar, &mut self.p2_drawn) };
        let segments = |amount: i32| (amount.wrapping_sub(20).wrapping_div(3)).min(12);
        let draw = |w: &mut W, amount: i32, always_flat: bool| {
            let x0 = rect.left as i32 - 64;
            let y0 = rect.top as i32 - 64;
            let n = segments(amount);
            for i in 0..n {
                let frame = if i == 0 { 1 } else if i == n - 1 { 3 } else { 2 };
                let anim = if always_flat || !flag { 81 } else { 82 };
                Self::blit(w, ANIM, x0 + 9 * i, y0, 9, 15, anim, frame);
            }
        };
        if *cur > v || flag {
            if !flag { *cur = cur.wrapping_sub(3); }
            w.call(HudCall::Port(Port::Panel));
            w.call(HudCall::Restore(strip, rect));
            if cur.wrapping_sub(20) > 0 {
                *drawn = 1;
                draw(w, *cur, false);
                w.call(HudCall::Port(Port::Screen));
                w.call(HudCall::Present(rect, dst));
                return;
            }
            // (falls through to the target test below with the bar already cleared)
        } else {
            *cur = v;
        }
        if v < 20 {
            if *drawn == 0 { return; }
            w.call(HudCall::Port(Port::Panel));
            w.call(HudCall::Restore(strip, rect));
            w.call(HudCall::Port(Port::Screen));
            w.call(HudCall::Present(rect, dst));
            *drawn = 0;
            return;
        }
        if v.wrapping_sub(20) <= 0 { return; }
        *drawn = 1;
        draw(w, v, true);
        w.call(HudCall::Port(Port::Screen));
        w.call(HudCall::Present(rect, dst));
    }

    /// 0x1f51c(player, target): start refilling the shield bar of a player toward `target` segments.
    pub fn shield_target<W: HudWorld>(&mut self, player: u32, target: i32, w: &W) {
        let delay = if target > 10 { 1 } else { 2 };
        if player & 0xff == 1 {
            self.p1_target = target;
            self.p1_cur = 0;
            self.p1_next = w.tick().wrapping_add(30);
            self.p1_delay = delay;
        } else {
            self.p2_target = target;
            self.p2_cur = 0;
            self.p2_next = w.tick().wrapping_add(30);
            self.p2_delay = delay;
        }
    }

    /// 0x1f5c4(player): advance the shield refill by one segment when its delay is over.
    pub fn shield_tick<W: HudWorld>(&mut self, player: u32, w: &mut W) {
        if player & 0xff == 1 {
            if w.tick() > (self.p1_next as i32).wrapping_add(self.p1_delay) as u32 && self.p1_cur < self.p1_target {
                self.p1_cur += 1;
                self.p1_next = w.tick();
                let n = self.p1_cur;
                self.shield(player, n, w);
            }
        } else if w.tick() > (self.p2_next as i32).wrapping_add(self.p2_delay) as u32 && self.p2_cur < self.p2_target {
            self.p2_cur += 1;
            self.p2_next = w.tick();
            let n = self.p2_cur;
            self.shield(player, n, w);
        }
    }

    /// 0x1f69c(player, n): redraw the shield bar with `n` (at most 12) segments.
    pub fn shield<W: HudWorld>(&mut self, player: u32, n: i32, w: &mut W) {
        let p1 = player & 0xff == 1;
        w.call(HudCall::Port(Port::Panel));
        let (strip, rect) = if p1 { (self.rects[CB02], self.rects[CB0A]) } else { (self.rects[CAF2], self.rects[CAFA]) };
        w.call(HudCall::Restore(strip, rect));
        if n > 0 {
            let n = n.min(12);
            let (x0, y0) = (rect.left as i32 - 64, rect.top as i32 - 64);
            for i in 0..n {
                let frame = if i == 0 { 1 } else if i == 11 { 3 } else { 2 };
                Self::blit(w, ANIM, x0 + 9 * i, y0, 9, 15, 83, frame);
            }
        }
        w.call(HudCall::Port(Port::Screen));
        let [_, _, d72, d74] = w.origin();
        let top = d72.wrapping_add(if p1 { 197 } else { 430 });
        w.call(HudCall::Present(rect, Rect::at(top, d74.wrapping_add(27), 15, 108)));
    }

    /// 0x1f848(player, lives, flash): the lives counter: up to four ship icons, from five on one icon and the number.
    pub fn lives<W: HudWorld>(&mut self, player: u32, lives: i32, flash: u32, w: &mut W) {
        let p1 = player & 0xff == 1;
        let flash = flash & 0xff != 0;
        w.call(HudCall::Port(Port::Panel));
        let (strip, rect) = if p1 { (self.rects[CB42], self.rects[CB4A]) } else { (self.rects[CB32], self.rects[CB3A]) };
        let mut x = rect.left as i32 - 64;
        let y = rect.top as i32 - 64;
        w.call(HudCall::Restore(strip, rect));
        let lives = lives.min(12);
        let icon = if player & 0xff == 2 { 27 } else { 26 };
        if lives > 4 {
            let n = lives - 1;
            if n > 9 {
                Self::blit(w, ANIM, x, y, 29, 28, icon, 1);
                if flash { Self::blit(w, FLASH, x, y, 29, 28, icon, 1); }
                let (ones, tens) = (n.wrapping_sub(n.wrapping_div(10).wrapping_mul(10)), n.wrapping_div(10));
                Self::blit(w, ANIM, x + 31, y, 12, 28, 84, tens + 1);
                if flash { Self::blit(w, FLASH, x + 31, y, 12, 28, 84, tens + 1); }
                Self::blit(w, ANIM, x + 45, y, 12, 28, 84, ones + 1);
                if flash { Self::blit(w, FLASH, x + 45, y, 12, 28, 84, ones + 1); }
            } else {
                Self::blit(w, ANIM, x, y, 29, 28, icon, 1);
                Self::blit(w, ANIM, x + 31, y, 12, 28, 84, 1);
                Self::blit(w, ANIM, x + 44, y, 12, 28, 84, n + 1);
                if flash {
                    Self::blit(w, FLASH, x, y, 29, 28, icon, 1);
                    Self::blit(w, FLASH, x + 31, y, 12, 28, 84, 1);
                    Self::blit(w, FLASH, x + 44, y, 12, 28, 84, n + 1);
                }
            }
        } else {
            let mut i = 0;
            while i < lives - 1 {
                Self::blit(w, ANIM, x, y, 29, 28, icon, 1);
                if flash { Self::blit(w, FLASH, x, y, 29, 28, icon, 1); }
                x += 35;
                i += 1;
            }
        }
        w.call(HudCall::Port(Port::Screen));
        let [_, _, d72, d74] = w.origin();
        let top = d72.wrapping_add(if p1 { 48 } else { 281 });
        w.call(HudCall::Present(rect, Rect::at(top, d74.wrapping_add(29), 28, 104)));
    }

    /// 0x1fe28(player, flag): the small icon of the "keep weapons" insurance (`flag` = shown).
    pub fn weapon<W: HudWorld>(&mut self, player: u32, flag: u32, w: &mut W) {
        let p1 = player & 0xff == 1;
        w.call(HudCall::Port(Port::Panel));
        let (strip, rect) = if p1 { (self.rects[CAE2], self.rects[CAEA]) } else { (self.rects[CAD2], self.rects[CADA]) };
        let y = rect.top as i32 - 64;
        w.call(HudCall::Restore(strip, rect));
        let x = rect.left as i32 - 64;
        if flag & 0xff != 0 {
            w.call(HudCall::Port(Port::Panel));
            Self::blit(w, ANIM, x, y, 31, 20, 99, 1);
        }
        w.call(HudCall::Port(Port::Screen));
        let [_, _, d72, d74] = w.origin();
        let (top, left) = if p1 { (d72.wrapping_add(11), d74.wrapping_add(72)) } else { (d72.wrapping_add(244), d74.wrapping_add(71)) };
        w.call(HudCall::Present(rect, Rect::at(top, left, 20, 31)));
    }

    /// 0x1ff94(player, level): the armor / weapon level icon (frames 2..5 of anim 99; level 1 shows nothing, 0 and above 5 return before drawing).
    pub fn weapon_level<W: HudWorld>(&mut self, player: u32, level: u32, w: &mut W) {
        let p1 = player & 0xff == 1;
        w.call(HudCall::Port(Port::Panel));
        let (strip, rect) = if p1 { (self.rects[CAC2], self.rects[CACA]) } else { (self.rects[CAC2], self.rects[CABA]) };
        let y = rect.top as i32 - 64;
        w.call(HudCall::Restore(strip, rect));
        let x = rect.left as i32 - 64;
        let frame = match level & 0xff {
            2 => 2,
            3 => 3,
            4 => 4,
            5 => 5,
            1 => -1,
            _ => return,
        };
        if frame != -1 {
            w.call(HudCall::Port(Port::Panel));
            Self::blit(w, ANIM, x, y, 31, 20, 99, frame);
        }
        w.call(HudCall::Port(Port::Screen));
        let [_, _, d72, d74] = w.origin();
        let top = d72.wrapping_add(if p1 { 11 } else { 244 });
        w.call(HudCall::Present(rect, Rect::at(top, d74.wrapping_add(106), 20, 31)));
    }

    /// 0x1fc50(player): paint the translucent "hit" flash over a player's panel (two rectangles) and copy them to the screen.
    pub fn flash<W: HudWorld>(&mut self, player: u32, w: &mut W) {
        let [d64, d66, d72, d74] = w.origin();
        w.call(HudCall::Port(Port::Panel));
        w.call(HudCall::OpColor([0x7fff, 0x7fff, 0x7fff]));
        w.call(HudCall::PenMode(32));
        w.call(HudCall::ForeColor(33));
        let ad = |v: i16, n: i16| v.wrapping_add(n);
        let (r1, r2) = if player & 0xff == 1 {
            (Rect { top: ad(d64, 19), left: ad(d66, 13), bottom: ad(d64, 234), right: ad(d66, 147) },
             Rect { top: ad(d64, 13), left: ad(d66, 24), bottom: ad(d64, 19), right: ad(d66, 61) })
        } else {
            (Rect { top: ad(d64, 252), left: ad(d66, 13), bottom: ad(d64, 467), right: ad(d66, 147) },
             Rect { top: ad(d64, 246), left: ad(d66, 24), bottom: ad(d64, 252), right: ad(d66, 64) })
        };
        w.call(HudCall::Paint(r2));
        w.call(HudCall::Paint(r1));
        w.call(HudCall::PenMode(0));
        w.call(HudCall::ForeColor(33));
        w.call(HudCall::Port(Port::Screen));
        let to_screen = |r: Rect| Rect {
            top: ad(d72, r.top.wrapping_sub(d64)),
            left: ad(d74, r.left.wrapping_sub(d66)),
            bottom: ad(d72, r.bottom.wrapping_sub(d64)),
            right: ad(d74, r.right.wrapping_sub(d66)),
        };
        w.call(HudCall::Present(r2, to_screen(r2)));
        w.call(HudCall::Present(r1, to_screen(r1)));
    }
}

impl Default for Hud {
    fn default() -> Self { Hud::new() }
}
