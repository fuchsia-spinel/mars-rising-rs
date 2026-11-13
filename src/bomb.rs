//! The button-2 weapon: bombs that fly to the bomb-sight and hit the ground objects (`ground.rs`) there.
//!
//! * 0x26b58 button-2 control ([`special_step`]): a burst of `special_max` bombs, one every 3 ticks, then a pause of 25 ticks before the next burst can start,
//! * 0x201c0 launch ([`Bombs::launch`]): a record of the pool of 30 (44 bytes, `D.0xcb74`) at the ship's nose that flies up toward the sight position,
//! * 0x203a4 update ([`Bombs::update`]): 4 px up per tick while the target scrolls down; on arrival the bomb explodes: effect 2, damage 1 to every ground object under
//!   it (0x3fec8), a scorch decal (anim 44) when it hit nothing,
//! * 0x205ec draw ([`Bombs::draw`]): shadow, the target marker (anim 24) and the bomb (anim 43, frames 1..7).
//!
//! Verified against the executable by `tests/bomb_golden.rs` (vectors from `tools/re/gen_bomb_vectors.py`).
use crate::draw::Prim;
use crate::enemy::f2i;
use crate::ship::{Ship, STATE_PLAYING};

pub const SLOTS: usize = 30;
pub const SIZE: usize = 44;

/// Record offsets.
pub mod o {
    pub const ACTIVE: usize = 0;
    pub const FLAG: usize = 1;
    pub const OWNER: usize = 2;
    pub const X: usize = 4;
    pub const Y: usize = 8;
    pub const TX: usize = 12; // sight position at launch (x), fixed
    pub const TY: usize = 16; // sight y: scrolls down with the ground
    pub const W: usize = 20;
    pub const H: usize = 24;
    pub const FRAME: usize = 28;
    pub const TICK: usize = 32;
    pub const SHADOW_DX: usize = 36;
    pub const SHADOW_DY: usize = 40;
}

pub trait BombWorld {
    /// 0xecf0.
    fn tick(&self) -> u32;
    /// 0xc098: scroll step per frame.
    fn scroll_step(&self) -> i32;
    /// 0xa0d4.
    fn random(&mut self, lo: i32, hi: i32) -> i32;
    /// 0x45bac.
    fn sound(&mut self, id: i32, prio: i32, arg: i32);
    /// 0x22670.
    #[allow(clippy::too_many_arguments)]
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32);
    /// 0x3fec8: damage to the ground objects in a box; -1 = nothing there.
    fn ground_hit(&mut self, l: i32, t: i32, r: i32, b: i32, amount: i32, owner: u8) -> i32;
    /// 0x40954: add a decal (`a = [x, y, anim, frame, w, h, r9, r10]`, then the two stack parameters).
    fn add_decal(&mut self, a: [i32; 8], p9: i32, p10: i32);
    /// 0x2994 / 0x29a4 / 0xe364 (`D.12bc`, `D.12be`, `D.13c8`).
    fn shadows(&self) -> bool;
    fn translucent(&self) -> bool;
    fn fancy(&self) -> bool;
    /// A blit request (0x344c shadow, 0x3170 translucent, 0x2f88 plain).
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32);
}

#[derive(Clone)]
pub struct Bombs {
    pub rec: Vec<u8>,
    /// D.157c: bombs in flight.
    pub count: i32,
}

impl Default for Bombs {
    fn default() -> Self { Self::new() }
}

impl Bombs {
    /// 0x2015c: no bomb in flight.
    pub fn new() -> Bombs { Bombs { rec: vec![0; SLOTS * SIZE], count: 0 } }

    pub fn clear(&mut self) {
        for n in 0..SLOTS { self.free(n); }
        self.count = 0;
    }

    #[inline] fn r8(&self, n: usize, off: usize) -> u8 { self.rec[n * SIZE + off] }
    #[inline] fn ri(&self, n: usize, off: usize) -> i32 {
        let a = n * SIZE + off;
        i32::from_be_bytes([self.rec[a], self.rec[a + 1], self.rec[a + 2], self.rec[a + 3]])
    }
    #[inline] fn wi(&mut self, n: usize, off: usize, v: i32) {
        let a = n * SIZE + off;
        self.rec[a..a + 4].copy_from_slice(&v.to_be_bytes());
    }
    #[inline] pub fn w32(&mut self, n: usize, off: usize, v: u32) { self.wi(n, off, v as i32) }
    #[inline] pub fn r32(&self, n: usize, off: usize) -> u32 { self.ri(n, off) as u32 }

    /// 0x201a0: free a slot (the counter goes down even when it was already free, like the original).
    fn free(&mut self, n: usize) {
        self.rec[n * SIZE] = 0;
        self.count = self.count.wrapping_sub(1);
    }

    /// 0x201c0: launch a bomb for `ship` (the ship of the player `owner`). Returns false when all 30 records are in use.
    pub fn launch<W: BombWorld>(&mut self, owner: u8, ship: &Ship, w: &mut W) -> bool {
        let Some(n) = (0..SLOTS).find(|&n| self.r8(n, o::ACTIVE) == 0) else { return false };
        self.rec[n * SIZE + o::ACTIVE] = 1;
        self.rec[n * SIZE + o::OWNER] = owner;
        self.rec[n * SIZE + o::FLAG] = 0;
        self.wi(n, o::W, 8);
        self.wi(n, o::H, 12);
        self.wi(n, o::FRAME, 1);
        let t = w.tick().wrapping_add(3);
        self.w32(n, o::TICK, t);
        self.wi(n, o::SHADOW_DX, 24);
        self.wi(n, o::SHADOW_DY, 44);
        let (x, y, width) = (f2i(ship.x), f2i(ship.y), ship.w);
        self.wi(n, o::X, x.wrapping_add(width.wrapping_sub(8) / 2));
        self.wi(n, o::Y, y.wrapping_sub(12));
        self.wi(n, o::TX, ship.trail_x);
        self.wi(n, o::TY, ship.trail_y);
        if self.ri(n, o::TY) < 0 { self.wi(n, o::TY, 0); }
        w.sound(35, 3, 0);
        self.count = self.count.wrapping_add(1);
        true
    }

    /// 0x203a4: move the bombs, animate them and let the arrived ones explode.
    pub fn update<W: BombWorld>(&mut self, w: &mut W) {
        if self.count < 1 { return; }
        let scroll = w.scroll_step();
        let now = w.tick();
        for n in 0..SLOTS {
            if self.r8(n, o::ACTIVE) == 0 { continue; }
            let y = self.ri(n, o::Y).wrapping_sub(4);
            self.wi(n, o::Y, y);
            let ty = self.ri(n, o::TY).wrapping_add(scroll);
            self.wi(n, o::TY, ty);
            if y > ty.wrapping_add(5) {
                if now > self.r32(n, o::TICK).wrapping_add(2) {
                    self.w32(n, o::TICK, now);
                    let f = self.ri(n, o::FRAME).wrapping_add(1);
                    self.wi(n, o::FRAME, if f > 7 { 7 } else { f });
                    let dx = self.ri(n, o::SHADOW_DX).wrapping_sub(3);
                    self.wi(n, o::SHADOW_DX, dx);
                    let dy = self.ri(n, o::SHADOW_DY).wrapping_sub(5);
                    self.wi(n, o::SHADOW_DY, dy);
                }
                continue;
            }
            // arrived
            self.free(n);
            if self.r8(n, o::FLAG) != 0 { return; }
            let (x, y) = (self.ri(n, o::X), self.ri(n, o::Y));
            let (bw, bh) = (self.ri(n, o::W), self.ri(n, o::H));
            w.effect(x.wrapping_add(bw / 2), y.wrapping_add(bh / 2), 2, 0, -1, 1, 0, 0.0, 0.0);
            let owner = self.r8(n, o::OWNER);
            let hit = w.ground_hit(x.wrapping_sub(1), y.wrapping_sub(1), x.wrapping_sub(1).wrapping_add(bw.wrapping_add(2)), y.wrapping_sub(1).wrapping_add(bh.wrapping_add(2)), 1, owner);
            if hit == -1 {
                let r = w.random(1, 3) as u16 as i32;
                w.add_decal([x.wrapping_add(bw / 2 - 8), y.wrapping_add(bh / 2 - 8), 44, r, 16, 16, 0, 0], 0, 1);
                w.sound(50, 2, 0);
            }
        }
    }

    /// 0x205ec: draw the bombs and their target markers.
    pub fn draw<W: BombWorld>(&self, w: &mut W) {
        if self.count < 1 { return; }
        let (shadows, trans, fancy) = (w.shadows(), w.translucent(), w.fancy());
        for n in 0..SLOTS {
            if self.r8(n, o::ACTIVE) == 0 { continue; }
            let (x, y, bw, bh, f) = (self.ri(n, o::X), self.ri(n, o::Y), self.ri(n, o::W), self.ri(n, o::H), self.ri(n, o::FRAME));
            if shadows {
                w.blit(Prim::Shadow, x.wrapping_sub(self.ri(n, o::SHADOW_DX)), y.wrapping_add(self.ri(n, o::SHADOW_DY)), bw, bh, 43, f);
            }
            let (tx, ty) = (self.ri(n, o::TX), self.ri(n, o::TY));
            if trans && fancy {
                w.blit(Prim::Trans, tx, ty, 20, 20, 24, 1);
            } else {
                w.blit(Prim::Anim, tx, ty, 20, 20, 24, 1);
            }
            w.blit(Prim::Anim, x, y, bw, bh, 43, f);
        }
    }
}

/// 0x26b58: the button-2 control of one ship. Holding the button drops a burst of `special_max` bombs (one every 3 ticks); the next burst may start
/// 25 ticks after the last bomb.
pub fn special_step<W: BombWorld>(ship: &mut Ship, bombs: &mut Bombs, w: &mut W) {
    if ship.state != STATE_PLAYING {
        ship.special_flag = false;
        ship.special_left = 0;
        return;
    }
    let mut refill = false;
    if !ship.input.alt {
        ship.special_flag = false;
        if ship.special_left <= 0 { return; }
    }
    let now = w.tick();
    if ship.special_left <= 0 {
        if ship.special_flag { return; }
        ship.special_flag = true;
        if now > ship.special_tick.wrapping_add(25) {
            ship.special_left = ship.special_max;
            refill = true;
        }
    }
    if !refill && now < ship.special_tick.wrapping_add(3) { return; }
    if ship.special_left <= 0 { return; }
    let owner = ship.number;
    if !bombs.launch(owner, ship, w) { return; }
    ship.special_tick = now;
    ship.special_left -= 1;
}

impl std::fmt::Debug for Bombs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bombs").field("count", &self.count).finish()
    }
}
