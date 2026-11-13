//! The effect pool: explosions, smoke, sparks and the other short animations the game plays, with their sounds. This is the original's effect spawner
//! `0x22670` (15 kinds), the particle burst `0x23440`, the per-frame move / animate / draw pass `0x236b4`, the pool initialisation `0x22618` and the table of
//! 50 random directions it builds first (`0x224e8`).
//!
//! The pool is 60 records of 180 bytes (`0xdd6c`), the direction table 50 pairs of f32 (`0xdbdc`), `D.165c` the next table entry.
//! Verified against the original executable by `tests/effects_golden.rs` (vectors from `tools/re/gen_effects_vectors.py`, which runs the original
//! routines): the pool, the table and the exact sequence of calls to the surroundings (random numbers, sounds, blits) must be identical after every operation.
//!
//! **The effect plays its own sound:** the spawner calls the sound routine `0x45bac(id, 4, 0)` itself when the effect has no delay, and the update pass calls it
//! on the frame a delayed effect starts (delay reaches 1) unless the sound byte is -1. Callers pass the sound id as a parameter of the effect; nothing
//! else plays the explosion sounds.

use crate::draw::Prim;
use crate::enemy::f2i;

pub const POOL: usize = 60;
pub const REC: usize = 180;
pub const STARS: usize = 50;
pub const TRAIL: usize = 6;
const TRAIL_REC: usize = 20;

/// Record offsets.
pub mod o {
    pub const ACTIVE: usize = 0;
    pub const SCROLLS: usize = 1; // u8: moves with the ground
    pub const LOOPS: usize = 2; // u8: restarts after the last frame instead of ending
    pub const TRANS: usize = 3; // u8: drawn translucent (never set by the spawners)
    pub const DELAY: usize = 4; // u32 ticks before it starts
    pub const ANIM: usize = 8; // i32 animation index
    pub const FRAME: usize = 12; // i32, drawn as frame + 1
    pub const FRAMES: usize = 16; // i32 number of frames
    pub const STEP: usize = 20; // u32 ticks per frame
    pub const X: usize = 24; // f32
    pub const Y: usize = 28;
    pub const VX: usize = 32;
    pub const VY: usize = 36;
    pub const W: usize = 40; // i32
    pub const H: usize = 44;
    pub const LAST: usize = 48; // u32 tick of the last frame change
    pub const SOUND: usize = 52; // u8 sound id (0xff = none)
    pub const TRAIL_ON: usize = 53; // u8: has a trail of six dots
    pub const TRAIL: usize = 56; // 6 x { u32 frame, f32 x, f32 y, u8 active, u32 start tick }
    pub const SHADOW: usize = 176; // u8: casts a shadow
}

/// What the effects need from the game. Every method is one routine of the original.
pub trait EffectWorld {
    /// 0xecf0: the frame tick.
    fn tick(&self) -> u32;
    /// 0xc098: scroll step per frame.
    fn scroll_step(&self) -> i32;
    /// 0x29a4 (`D.12be`).
    fn translucent(&self) -> bool;
    /// 0xe364 (`D.13c8`): effect shadows and trails are drawn only when on.
    fn fancy(&self) -> bool;
    /// 0xa0d4: random number in `lo..=hi`.
    fn random(&mut self, lo: i32, hi: i32) -> i32;
    /// 0x45bac: sound effect (id, priority, argument).
    fn sound(&mut self, id: i32, prio: i32, arg: i32);
    /// The draw primitives: 0x2f88 plain, 0x3170 translucent, 0x3264 dithered, 0x344c shadow; called with `(0, x, y, w, h, anim, frame)`.
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32);
}

#[derive(Clone)]
pub struct Effects {
    pub rec: Vec<u8>,
    /// `0xdbdc`: 50 x (vx, vy), random directions with a speed of 6..8.
    pub stars: Vec<f32>,
    /// `D.165c`
    pub next_star: u32,
}

impl Default for Effects {
    fn default() -> Self { Self::new() }
}

impl Effects {
    pub fn new() -> Self {
        Effects { rec: vec![0; POOL * REC], stars: vec![0.0; STARS * 2], next_star: 0 }
    }

    // ---- raw accessors ------------------------------------------------------------------------------------------------------------
    #[inline] pub fn r8(&self, i: usize, off: usize) -> u8 { self.rec[i * REC + off] }
    #[inline] pub fn w8(&mut self, i: usize, off: usize, v: u8) { self.rec[i * REC + off] = v }
    #[inline] pub fn r32(&self, i: usize, off: usize) -> u32 {
        let a = i * REC + off;
        u32::from_be_bytes([self.rec[a], self.rec[a + 1], self.rec[a + 2], self.rec[a + 3]])
    }
    #[inline] pub fn ri32(&self, i: usize, off: usize) -> i32 { self.r32(i, off) as i32 }
    #[inline] pub fn w32(&mut self, i: usize, off: usize, v: u32) {
        let a = i * REC + off;
        self.rec[a..a + 4].copy_from_slice(&v.to_be_bytes());
    }
    #[inline] pub fn rf(&self, i: usize, off: usize) -> f32 { f32::from_bits(self.r32(i, off)) }
    #[inline] pub fn wf(&mut self, i: usize, off: usize, v: f32) { self.w32(i, off, v.to_bits()) }

    /// The direction table as the original keeps it in memory (big-endian f32 pairs).
    pub fn star_bytes(&self) -> Vec<u8> {
        self.stars.iter().flat_map(|v| v.to_bits().to_be_bytes()).collect()
    }

    /// Number of records in use.
    pub fn active(&self) -> usize { (0..POOL).filter(|&i| self.r8(i, o::ACTIVE) != 0).count() }

    /// 0x2265c: free one record.
    pub fn free(&mut self, i: usize) { self.w8(i, o::ACTIVE, 0) }

    /// 0x22618: build the direction table (0x224e8), reset the table index and free every record.
    pub fn init<W: EffectWorld>(&mut self, w: &mut W) {
        let (cx, cy) = (208.0f32, 240.0f32);
        for k in 0..STARS {
            let speed = (w.random(6, 8) as u16) as f32;
            let rx = (w.random(0, 416) as u16) as f32;
            let ry = (w.random(0, 480) as u16) as f32;
            let (dx, dy) = (cx - rx, cy - ry);
            let len = (dx.mul_add(dx, dy * dy) as f64).sqrt() as f32; // fmuls, fmadds, sqrt (double), frsp
            self.stars[k * 2] = (dx / len) * speed;
            self.stars[k * 2 + 1] = (dy / len) * speed;
        }
        self.next_star = 0;
        for i in 0..POOL { self.free(i); }
    }

    fn take_star(&mut self, i: usize) {
        let k = self.next_star as usize;
        self.wf(i, o::VX, self.stars[(k * 2).min(STARS * 2 - 2)]);
        self.wf(i, o::VY, self.stars[(k * 2 + 1).min(STARS * 2 - 1)]);
        self.next_star = self.next_star.wrapping_add(1);
        if (self.next_star as i32) >= STARS as i32 { self.next_star = 0; }
    }

    fn first_free(&self) -> Option<usize> { (0..POOL).find(|&i| self.r8(i, o::ACTIVE) == 0) }

    /// Start the six trail dots of record `i` (spawner: 4 pixels up and left of the record's position).
    fn start_trail(&mut self, i: usize, now: u32, dot: f32) {
        let (x, y) = (self.rf(i, o::X), self.rf(i, o::Y));
        for k in 0..TRAIL {
            let b = o::TRAIL + k * TRAIL_REC;
            self.wf(i, b + 4, dot + x);
            self.wf(i, b + 8, dot + y);
            self.w32(i, b, k as u32 + 1);
            self.w32(i, b + 16, now.wrapping_add(k as u32).wrapping_add(2));
            self.w8(i, b + 12, 0);
        }
    }

    /// 0x22670: start an effect of `kind` (0..=14) centred on the original's reference point (x, y). `delay` ticks later it starts (0 = now, with its sound
    /// `snd`), `scrolls` makes it move with the ground, `looping` makes it repeat, (vx, vy) is its velocity (the kinds with sparks take one from the table).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn<W: EffectWorld>(&mut self, w: &mut W, x: i32, y: i32, kind: i32, delay: i32, snd: i32, scrolls: i32, looping: i32, vx: f32, vy: f32) {
        if !(0..=14).contains(&kind) { return; }
        let now = w.tick();
        let Some(i) = self.first_free() else { return };
        self.w8(i, o::ACTIVE, 1);
        self.w32(i, o::DELAY, delay as u32);
        self.w32(i, o::FRAME, 0);
        self.w32(i, o::LAST, now);
        self.w8(i, o::SOUND, snd as u8);
        self.w8(i, o::SCROLLS, scrolls as u8);
        self.w8(i, o::LOOPS, looping as u8);
        self.wf(i, o::VX, vx);
        self.wf(i, o::VY, vy);
        self.w8(i, o::TRANS, 0);
        self.w8(i, o::TRAIL_ON, 0);
        self.w8(i, o::SHADOW, 0);
        if delay == 0 { w.sound(snd, 4, 0); }
        let (xf, yf) = (x as f32, y as f32);
        // (x offset, y offset, w, h, animation, frames, ticks per frame)
        let plain = |s: &mut Self, ox: f32, oy: f32, wd: u32, ht: u32, anim: u32, frames: u32, step: u32| {
            s.wf(i, o::X, ox + xf);
            s.wf(i, o::Y, oy + yf);
            s.w32(i, o::W, wd);
            s.w32(i, o::H, ht);
            s.w32(i, o::ANIM, anim);
            s.w32(i, o::FRAMES, frames);
            s.w32(i, o::STEP, step);
        };
        match kind {
            0 => plain(self, -32.0, -32.0, 64, 64, 1, 8, 1),
            1 => plain(self, -8.0, -8.0, 16, 16, 2, 8, 1),
            2 => plain(self, -8.0, -8.0, 16, 16, 2, 11, 1),
            3 => {
                self.wf(i, o::X, xf);
                self.wf(i, o::Y, yf);
                self.w32(i, o::W, 48);
                self.w32(i, o::H, 49);
                self.w32(i, o::ANIM, 17);
                self.w32(i, o::FRAMES, 4);
                self.w32(i, o::STEP, 3);
            }
            4 => plain(self, -32.0, -32.0, 64, 64, 3, 12, 3),
            5 => plain(self, -14.0, -14.0, 28, 28, 5, 1, 2),
            6 => {
                // sparks with a trail and a shadow, starting at a random frame
                self.wf(i, o::X, -5.0 + xf);
                self.wf(i, o::Y, -5.0 + yf);
                self.take_star(i);
                self.w32(i, o::W, 10);
                self.w32(i, o::H, 10);
                self.w32(i, o::ANIM, 6);
                self.w32(i, o::FRAMES, 15);
                let step = w.random(0, 1) as u16 as u32;
                self.w32(i, o::STEP, step);
                self.w8(i, o::TRAIL_ON, 1);
                self.w8(i, o::SHADOW, 1);
                let f = w.random(0, 15 - 1) as u16 as u32;
                self.w32(i, o::FRAME, f);
            }
            7 | 8 => {
                self.wf(i, o::X, -4.0 + xf);
                self.wf(i, o::Y, -4.0 + yf);
                self.take_star(i);
                self.w32(i, o::W, 10);
                self.w32(i, o::H, 10);
                self.w32(i, o::ANIM, if kind == 7 { 7 } else { 8 });
                self.w32(i, o::FRAMES, 7);
                let step = w.random(1, 2) as u16 as u32;
                self.w32(i, o::STEP, step);
                self.w8(i, o::TRAIL_ON, 1);
                self.w8(i, o::SHADOW, 1);
            }
            9..=12 => {
                self.wf(i, o::X, -4.0 + xf);
                self.wf(i, o::Y, -4.0 + yf);
                self.take_star(i);
                self.w32(i, o::W, 10);
                self.w32(i, o::H, 10);
                self.w32(i, o::ANIM, kind as u32);
                self.w32(i, o::FRAMES, 7);
                let step = w.random(1, 2) as u16 as u32;
                self.w32(i, o::STEP, step);
                self.w8(i, o::TRAIL_ON, 1);
            }
            13 => plain(self, -10.0, -9.0, 20, 18, 4, 8, 1),
            _ => plain(self, -20.0, -17.0, 40, 37, 13, 8, 2), // 14
        }
        if scrolls as u8 == 0 { self.w8(i, o::TRAIL_ON, 0); }
        if self.r8(i, o::TRAIL_ON) != 0 { self.start_trail(i, now, -4.0); }
    }

    /// 0x23440: a burst of `count` sparks (animation `anim`, 7 frames, 10-pixel hit-free dots) at (x, y), each with a direction from the table and a trail when the burst
    /// scrolls with the ground. They start `delay` ticks from now.
    pub fn particles<W: EffectWorld>(&mut self, w: &mut W, x: i32, y: i32, anim: i32, count: i32, delay: i32, scrolls: i32) {
        let now = w.tick();
        let mut from = 0usize;
        for _ in 0..count.max(0) {
            let Some(i) = (from..POOL).find(|&i| self.r8(i, o::ACTIVE) == 0) else { return };
            from = i + 1;
            self.w8(i, o::ACTIVE, 1);
            self.w32(i, o::DELAY, delay as u32);
            self.w32(i, o::LAST, now);
            self.w8(i, o::SOUND, 0xff);
            self.w8(i, o::SCROLLS, scrolls as u8);
            self.w8(i, o::LOOPS, 0);
            self.wf(i, o::VX, 0.0);
            self.wf(i, o::VY, 0.0);
            self.w8(i, o::SHADOW, 1);
            self.wf(i, o::X, -4.0 + x as f32);
            self.wf(i, o::Y, -4.0 + y as f32);
            self.take_star(i);
            self.w32(i, o::W, 8);
            self.w32(i, o::H, 8);
            self.w32(i, o::ANIM, anim as u32);
            self.w32(i, o::FRAME, 1);
            self.w32(i, o::FRAMES, 7);
            let step = w.random(1, 2) as u16 as u32;
            self.w32(i, o::STEP, step);
            if scrolls as u8 != 0 {
                self.w8(i, o::TRAIL_ON, 1);
                // the dots start exactly at the position (no offset), one tick apart
                self.start_trail(i, now, 0.0);
                // (the original copies the plain position: -0.0 + x would turn -0.0 into 0.0, so write the bits directly)
                let (px, py) = (self.r32(i, o::X), self.r32(i, o::Y));
                for k in 0..TRAIL {
                    self.w32(i, o::TRAIL + k * TRAIL_REC + 4, px);
                    self.w32(i, o::TRAIL + k * TRAIL_REC + 8, py);
                }
            } else {
                self.w8(i, o::TRAIL_ON, 0);
            }
        }
    }

    /// 0x236b4: move, animate and draw every effect.
    pub fn update<W: EffectWorld>(&mut self, w: &mut W) {
        let now = w.tick();
        let scroll = w.scroll_step() as f32;
        let trans = w.translucent();
        let fancy = w.fancy();
        for i in 0..POOL {
            if self.r8(i, o::ACTIVE) == 0 { continue; }
            if self.r8(i, o::SCROLLS) != 0 {
                let y = self.rf(i, o::Y) + scroll;
                self.wf(i, o::Y, y);
            }
            if self.r32(i, o::DELAY) == 1 {
                let s = self.r8(i, o::SOUND);
                if s as i8 != -1 { w.sound(s as i32, 4, 0); }
            }
            let d = self.r32(i, o::DELAY).wrapping_sub(1);
            self.w32(i, o::DELAY, d);
            if (d as i32) > 0 { continue; }
            let x = self.rf(i, o::X) + self.rf(i, o::VX);
            self.wf(i, o::X, x);
            let y = self.rf(i, o::Y) + self.rf(i, o::VY);
            self.wf(i, o::Y, y);
            let (wd, ht) = (self.ri32(i, o::W), self.ri32(i, o::H));
            if x + (wd as f32) < 0.0 || x > 416.0 || y + (ht as f32) < 0.0 || y > 480.0 {
                self.free(i);
                continue;
            }
            let (anim, frame) = (self.ri32(i, o::ANIM), self.ri32(i, o::FRAME));
            if fancy && self.r8(i, o::SHADOW) != 0 {
                if self.r8(i, o::SCROLLS) != 0 {
                    w.blit(Prim::Shadow, f2i(x).wrapping_sub(12), f2i(y).wrapping_add(22), wd, ht, anim, frame.wrapping_add(1));
                } else {
                    w.blit(Prim::Shadow, f2i(x).wrapping_sub(24), f2i(y).wrapping_add(44), wd, ht, anim, frame.wrapping_add(1));
                }
            }
            if self.r8(i, o::TRAIL_ON) != 0 {
                let (vx, vy) = (self.rf(i, o::VX), self.rf(i, o::VY));
                for k in 0..TRAIL {
                    let b = o::TRAIL + k * TRAIL_REC;
                    if self.r8(i, o::SCROLLS) != 0 {
                        let ty = self.rf(i, b + 8) + scroll;
                        self.wf(i, b + 8, ty);
                    }
                    if self.r8(i, b + 12) == 0 && now >= self.r32(i, b + 16) { self.w8(i, b + 12, 1); }
                    if self.r8(i, b + 12) != 0 {
                        let tx = self.rf(i, b + 4) + vx;
                        self.wf(i, b + 4, tx);
                        let ty = self.rf(i, b + 8) + vy;
                        self.wf(i, b + 8, ty);
                        if fancy {
                            let fr = self.ri32(i, b);
                            let prim = if trans { Prim::Trans } else { Prim::Dither };
                            w.blit(prim, f2i(tx), f2i(ty), 8, 8, 48, fr);
                        }
                    }
                }
            }
            if self.r8(i, o::TRANS) == 0 || !trans {
                w.blit(Prim::Anim, f2i(x), f2i(y), wd, ht, anim, frame.wrapping_add(1));
            } else {
                w.blit(Prim::Trans, f2i(x), f2i(y), wd, ht, anim, frame.wrapping_add(1));
            }
            if now > self.r32(i, o::LAST).wrapping_add(self.r32(i, o::STEP)) {
                self.w32(i, o::FRAME, (frame as u32).wrapping_add(1));
                self.w32(i, o::LAST, now);
            }
            if self.ri32(i, o::FRAME) >= self.ri32(i, o::FRAMES) {
                if self.r8(i, o::LOOPS) != 0 { self.w32(i, o::FRAME, 1); } else { self.free(i); }
            }
        }
    }
}

impl std::fmt::Debug for Effects {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Effects").field("active", &self.active()).field("next_star", &self.next_star).finish()
    }
}
