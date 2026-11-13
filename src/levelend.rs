//! The end-of-level coin bonus: set up by `0x20764` when the level is five rows from its end, stepped every frame by `0x208fc` and drawn by `0x20be0`
//! (with the number drawers `0x21090` for the coin count and `0x21150` for the points total).
//!
//! The tally runs in four states (`D.15d8`): 0 waits 45 ticks, then plays sounds 54 and 21 (delayed 5); 1 shows the panel for another 45 ticks; 2 pays one coin every
//! 3 ticks to every player in the game (`level * 100` points per coin, at most 1000, through the score adder `0xe68c` / `0xe6c0`, sound 34 for every coin) and, when
//! all coins are paid, turns the coins still lying on the screen into explosions (`0x2b73c`) and plays 54 (something was paid) or 53 (nothing); 3 waits 60 ticks and
//! ends the tally. The routines return 1 when the tally is over (or was never started because nobody is in the game) and the level is complete.
//!
//! Verified against the original executable by `tests/levelend_golden.rs` (vectors from `tools/re/gen_levelend_vectors.py`): after every call the state block and
//! the exact sequence of calls to the surroundings (sounds, score, blits) must be identical.

use crate::draw::Prim;

/// What the tally needs from the game. Every method is one routine of the original.
pub trait TallyWorld {
    /// 0xecf0: the game tick (`D.13e4`).
    fn tick(&self) -> u32;
    /// 0xece0: number of players (`D.13e2`).
    fn players(&self) -> u8;
    /// 0xd7a0: the level number (`D.13ec`).
    fn level(&self) -> i32;
    /// 0xe090: coin style (`D.13d8`); non-zero draws the other coin sprite.
    fn coin_style(&self) -> bool;
    /// 0x29a4: translucency option (`D.12be`).
    fn translucent(&self) -> bool;
    /// 0x24060: is player `n` (1 or 2) in the game.
    fn ship_active(&self, n: u8) -> bool;
    /// 0x2414c: lives of player `n`.
    fn ship_lives(&self, n: u8) -> i32;
    /// 0x24378: coins collected by player `n` (0..=99).
    fn ship_coins(&self, n: u8) -> i32;
    /// 0xe68c (player 1) / 0xe6c0 (player 2): add `points` (flash = 0) to the score.
    fn add_score(&mut self, n: u8, points: i32, flash: i32);
    /// 0x2b73c: turn the uncollected coins on the screen into explosions.
    fn convert_coins(&mut self);
    /// 0x45bac: sound (id, priority, argument).
    fn sound(&mut self, id: i32, prio: i32, arg: i32);
    /// 0x2f88 (`Anim`), 0x3170 (`Trans`) and the clipping variant 0x307c (also `Anim`): a sprite of animation `anim`, frame `frame`.
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32);
    /// 0x307c: the clipping variant of `blit(Prim::Anim, ..)` (the digits of the points per coin); it draws the same.
    fn blit_clipped(&mut self, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) { self.blit(Prim::Anim, x, y, w, h, anim, frame); }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    /// `D.1584` points paid to player 2, `D.1588` his coins, `D.158c` coins counted so far, `D.1590` he takes part.
    pub p2_points: u32,
    pub p2_coins: u32,
    pub p2_count: u32,
    pub p2_on: bool,
    /// `D.1594`, `D.1598`, `D.159c`, `D.15a0`: the same for player 1.
    pub p1_points: u32,
    pub p1_coins: u32,
    pub p1_count: u32,
    pub p1_on: bool,
    /// `D.15a4` / `D.15a8`: tick and value (8 down to 1) of the coin sprite's frame counter.
    pub anim_tick: u32,
    pub anim_frame: u32,
    /// `D.15ac`: at least one coin was paid.
    pub paid: bool,
    /// `D.15b0 .. D.15c8`: x positions of the panel's parts (the two first are not used by the drawing).
    pub xs: [u32; 7],
    /// `D.15cc`: y of the first line.
    pub y: u32,
    /// `D.15d0`: the tally is running.
    pub run: bool,
    /// `D.15d4`: tick of the last state change.
    pub tick: u32,
    /// `D.15d8`: state 0..=3.
    pub state: u8,
}

/// The fields in the order and with the widths of the original's variables (for the golden test's hash).
impl Tally {
    pub fn field_bytes(&self) -> Vec<u8> {
        let mut o = Vec::new();
        for x in [self.p2_points, self.p2_coins, self.p2_count] { o.extend(x.to_be_bytes()); }
        o.push(self.p2_on as u8);
        for x in [self.p1_points, self.p1_coins, self.p1_count] { o.extend(x.to_be_bytes()); }
        o.push(self.p1_on as u8);
        for x in [self.anim_tick, self.anim_frame] { o.extend(x.to_be_bytes()); }
        o.push(self.paid as u8);
        for x in self.xs { o.extend(x.to_be_bytes()); }
        o.extend(self.y.to_be_bytes());
        o.push(self.run as u8);
        o.extend(self.tick.to_be_bytes());
        o.push(self.state);
        o
    }

    /// 0x20764: start the tally for the ships `1` and `2`.
    pub fn setup<W: TallyWorld>(&mut self, w: &W) {
        self.state = 0;
        self.tick = w.tick();
        self.run = false;
        self.p1_on = false;
        self.p2_on = false;
        self.paid = false;
        self.anim_frame = 8;
        self.anim_tick = w.tick();
        if w.ship_active(1) && w.ship_lives(1) > 0 {
            self.p1_on = true;
            self.p1_count = 0;
            self.p1_points = 0;
            self.p1_coins = w.ship_coins(1) as u32;
        }
        if w.players() == 2 && w.ship_active(2) && w.ship_lives(2) > 0 {
            self.p2_on = true;
            self.p2_count = 0;
            self.p2_points = 0;
            self.p2_coins = w.ship_coins(2) as u32;
        }
        if self.p1_on || self.p2_on {
            self.run = true;
            self.y = 298;
            let base: i32 = if w.players() == 2 { 0 } else { -23 };
            let pos = |d: i32| base.wrapping_add(d) as u32;
            self.xs = [pos(310), pos(278), pos(231), pos(201), pos(166), pos(111), pos(52)];
        }
    }

    /// 0x208fc: one frame of the tally. Returns true when the tally is over.
    pub fn step<W: TallyWorld>(&mut self, w: &mut W) -> bool {
        if !self.run { return true; }
        let p = (w.level().wrapping_mul(100)).min(1000);
        let now = w.tick();
        match self.state {
            0 => {
                if now >= self.tick.wrapping_add(45) {
                    self.state = 1;
                    self.tick = now;
                    w.sound(54, 4, 0);
                    w.sound(21, 4, 5);
                }
                false
            }
            1 => {
                self.draw(w, p);
                if now >= self.tick.wrapping_add(45) {
                    self.state = 2;
                    self.tick = now;
                }
                false
            }
            2 => {
                let mut paid_now = false;
                if now >= self.tick.wrapping_add(3) {
                    self.tick = now;
                    let (done1, done2);
                    if self.p1_on {
                        if (self.p1_count as i32) < w.ship_coins(1) {
                            self.paid = true;
                            self.p1_count = self.p1_count.wrapping_add(1);
                            self.p1_points = self.p1_points.wrapping_add(p as u32);
                            w.add_score(1, p, 0);
                            paid_now = true;
                            done1 = (self.p1_count as i32) >= w.ship_coins(1);
                        } else {
                            done1 = true;
                        }
                    } else {
                        done1 = true;
                    }
                    if self.p2_on {
                        if (self.p2_count as i32) < w.ship_coins(2) {
                            self.paid = true;
                            self.p2_count = self.p2_count.wrapping_add(1);
                            self.p2_points = self.p2_points.wrapping_add(p as u32);
                            w.add_score(2, p, 0);
                            paid_now = true;
                            done2 = (self.p2_count as i32) >= w.ship_coins(2);
                        } else {
                            done2 = true;
                        }
                    } else {
                        done2 = true;
                    }
                    if done1 && done2 {
                        self.state = 3;
                        self.tick = now;
                        w.convert_coins();
                        w.sound(if self.paid { 54 } else { 53 }, 4, 0);
                    }
                }
                if paid_now { w.sound(34, 4, 0); }
                self.draw(w, p);
                false
            }
            3 => {
                self.draw(w, p);
                if now >= self.tick.wrapping_add(60) {
                    self.run = false;
                    return true;
                }
                false
            }
            _ => true,
        }
    }

    /// 0x21090: the coin count as two digits of the big number font (animation 84, 12 x 28, 15 pixels apart).
    fn draw_count<W: TallyWorld>(&self, w: &mut W, n: u32, x: i32, y: i32) {
        let digits = [(n % 10) as i32, ((n / 10) % 10) as i32];
        let mut x = x;
        for k in [1usize, 0] {
            w.blit(Prim::Anim, x, y, 12, 28, 84, digits[k] + 1);
            x += 15;
        }
    }

    /// 0x21150: the points total, up to six digits; digits 3..5 are left out while they are leading zeros (the first three are always drawn).
    fn draw_total<W: TallyWorld>(&self, w: &mut W, n: u32, x: i32, y: i32) {
        let mut d = [0i32, 0, 0, -1, -1, -1, 0];
        let (mut i, mut v) = (0usize, n);
        loop {
            d[i] = (v % 10) as i32;
            i += 1;
            v /= 10;
            if i > 6 { i -= 1; }
            if v == 0 { break; }
        }
        let mut x = x;
        for k in (0..=5).rev() {
            if d[k] != -1 {
                w.blit(Prim::Anim, x, y, 12, 28, 84, d[k] + 1);
                x += 15;
            }
        }
    }

    /// 0x20be0(p): draw the panel with `p` points per coin.
    pub fn draw<W: TallyWorld>(&mut self, w: &mut W, p: i32) {
        let trans = w.translucent();
        let mut frame = self.anim_frame as i32;
        let now = w.tick();
        if now > self.anim_tick.wrapping_add(2) {
            self.anim_frame = self.anim_frame.wrapping_sub(1);
            if (self.anim_frame as i32) < 1 { self.anim_frame = 8; }
            self.anim_tick = now;
        }
        let y0 = self.y as i32;
        let mut row = 0i32;
        let mut shown1 = false;
        for side in 0..2 {
            let (on, count, total) = if side == 0 { (self.p1_on, self.p1_count, self.p1_points) } else { (self.p2_on, self.p2_count, self.p2_points) };
            if side == 1 && shown1 { row = 70; }
            if !on { continue; }
            if side == 0 { shown1 = true; }
            if w.players() == 2 { w.blit(Prim::Anim, self.xs[6] as i32, y0 + (row - 10), 48, 49, 14 + side as i32, 1); }
            if trans { w.blit(Prim::Trans, self.xs[5] as i32, y0 + (row - 6), 38, 38, 114, 1); }
            let coin = if w.coin_style() { frame = 1; 137 } else { 135 };
            w.blit(Prim::Anim, self.xs[5] as i32 + 6, y0 + ((6 + row) - 6), 26, 26, coin, frame);
            self.draw_count(w, count, self.xs[4] as i32, y0 + row);
            w.blit(Prim::Anim, self.xs[3] as i32 + 6, y0 + (6 + row), 11, 11, 85, 1);
            // the points per coin: up to four digits, the first three always drawn, the fourth only when it is used
            let mut d = [0i32, 0, 0, -1, 0];
            let (mut i, mut v) = (0usize, p);
            loop {
                d[i] = v.wrapping_rem(10);
                i += 1;
                v /= 10;
                if i > 4 { i -= 1; }
                if v == 0 { break; }
            }
            let mut x = self.xs[2] as i32;
            for k in (0..=3).rev() {
                if d[k] != -1 {
                    w.blit_clipped(x, y0 + row, 12, 28, 84, d[k] + 1);
                    x += 15;
                }
            }
            w.blit(Prim::Anim, (x - 3) + 14, y0 + (6 + row), 11, 11, 86, 1);
            self.draw_total(w, total, ((x - 3) + 14) + 23, y0 + row);
        }
    }
}
