//! Enemy shots: the 30-record pool (`D.1050` -> 80-byte records, count `D.1a04`, "no new shot before" tick `D.1a00`) and its routines:
//! the three spawn routines (0x383b8 aimed shot, 0x38754 shot with parameters which dispatches to the nine kind routines 0x39650 0x3996c
//! 0x39adc 0x39dfc 0x39f68 0x3a0bc 0x3a3c4 0x3a51c 0x3a670, 0x37fd4 beam), the per-frame update 0x38b0c (animation, muzzle flash, movement, cull,
//! ground-object hit, ship hits), "clear all" 0x38a1c, init 0x37f60 and the two draw passes 0x39020 / 0x39338.
//!
//! Verified against the original executable by `tests/enemy_shot_golden.rs` (vectors made by `tools/re/gen_shot_vectors.py`, which runs the
//! original PowerPC code in the emulator): the whole pool, the counters and every call to the surrounding game must be identical.
//!
//! Record layout (offsets):  +0 kind (u32)  +4 active (u8)  +5 muzzle flash flag (u8)  +8 sprite animation  +12 frame  +16 number of frames
//! +20 tick of the last frame change  +24 frame period  +28 shadow flag (u8)  +29 "draw on top" flag (u8)  +32/+36 position (f32)
//! +40/+44 velocity (f32)  +48/+52 size  +56/+60 half size  +64/+68 muzzle flash position (f32)  +72 flash start tick  +76 damage to the ship.
//! Freeing a record only clears +4 (the other bytes stay, and some spawn routines do not write +5), so the pool is kept as raw bytes.
use crate::draw::{DrawOpts, Prim};
use crate::enemy::{f2i, ShipView};

pub const SHOTS: usize = 30;
pub const SHOT_SIZE: usize = 80;

/// What the shot code takes from, or hands to, the rest of the game.
pub trait ShotWorld {
    /// 0xecf0: the frame tick.
    fn tick(&self) -> u32;
    fn ship(&self, n: u8) -> ShipView;
    /// 0xc098 (`D.13c4`): vertical scroll speed in px per frame.
    fn scroll_speed(&self) -> i32;
    /// 0x2ca38: does the box (left, top, right, bottom) hit a destructible ground object? (the hit side effects belong to the callee)
    fn object_hit(&mut self, left: i32, top: i32, right: i32, bottom: i32) -> bool;
    /// 0xe6f4 / 0xe720: damage to ship 1 / 2.
    fn ship_damage(&mut self, n: u8, amount: i32);
    /// 0x22670: effect.
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32);
    /// 0x45bac: sound (id, priority, delay).
    fn sound(&mut self, id: i32, prio: i32, delay: i32);
}

/// The two draw passes: display options and blit requests (`group` is always 0).
pub trait ShotDrawWorld {
    fn opts(&self) -> DrawOpts;
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32);
}

#[derive(Debug, Clone)]
pub struct ShotPool {
    pub rec: Vec<u8>,
    /// `D.1a04`
    pub live: i32,
    /// `D.1a00`
    pub gate: u32,
}

impl Default for ShotPool {
    fn default() -> Self { Self::new() }
}

/// 0x383b8 / 0x39650..: the direction to the target ship (the ship's centre minus the shot's origin, plus the shot's half size - literally),
/// normalised. Returns the unit vector.
fn aim<W: ShotWorld>(w: &W, target: u8, fx: f32, fy: f32, hw: i32, hh: i32) -> (f32, f32) {
    let s = w.ship(if target == 1 { 1 } else { 2 });
    let dx = (hw as f32) + ((s.x + s.hw as f32) - fx);
    let dy = (hh as f32) + ((s.y + s.hh as f32) - fy);
    let len = (dx.mul_add(dx, dy * dy) as f64).sqrt() as f32; // fmuls, fmadds, fsqrt (double), frsp
    (dx / len, dy / len)
}

impl ShotPool {
    pub fn new() -> Self { ShotPool { rec: vec![0; SHOTS * SHOT_SIZE], live: 0, gate: 0 } }

    #[inline] fn r8(&self, i: usize, o: usize) -> u8 { self.rec[i * SHOT_SIZE + o] }
    #[inline] fn w8(&mut self, i: usize, o: usize, v: u8) { self.rec[i * SHOT_SIZE + o] = v }
    #[inline] fn r32(&self, i: usize, o: usize) -> u32 {
        let a = i * SHOT_SIZE + o;
        u32::from_be_bytes([self.rec[a], self.rec[a + 1], self.rec[a + 2], self.rec[a + 3]])
    }
    #[inline] fn ri32(&self, i: usize, o: usize) -> i32 { self.r32(i, o) as i32 }
    #[inline] fn w32(&mut self, i: usize, o: usize, v: u32) {
        let a = i * SHOT_SIZE + o;
        self.rec[a..a + 4].copy_from_slice(&v.to_be_bytes());
    }
    #[inline] fn rf(&self, i: usize, o: usize) -> f32 { f32::from_bits(self.r32(i, o)) }
    #[inline] fn wf(&mut self, i: usize, o: usize, v: f32) { self.w32(i, o, v.to_bits()) }

    /// 0x37fb0: release a record (only the active byte and the counter).
    pub fn free(&mut self, i: usize) {
        self.w8(i, 4, 0);
        self.live = self.live.wrapping_sub(1);
    }

    /// 0x37f60: all records free, counter 0, the gate at the current tick.
    pub fn init<W: ShotWorld>(&mut self, w: &W) {
        for i in 0..SHOTS {
            self.free(i);
        }
        self.live = 0;
        self.gate = w.tick();
    }

    fn alive<W: ShotWorld>(w: &W, n: u8) -> bool { w.ship(n).alive }

    /// The checks every spawn routine starts with (tick gate, target ship alive, pool not full, position inside the playfield).
    /// `bounds` = (min x, max x) of the position check; y is always 15..400.
    fn can_spawn<W: ShotWorld>(&self, w: &W, mode_ok: bool, x: f32, y: f32, xmin: f32, xmax: f32) -> bool {
        if w.tick() < self.gate || !mode_ok || self.live >= 30 {
            return false;
        }
        !(x < xmin || x > xmax || y < 15.0 || y > 400.0)
    }

    /// The fields every kind routine writes the same way; `kind`, `anim`, `frame`, `nframes`, `period`, `shadow`, `flag`.
    fn begin(&mut self, i: usize, kind: u32) {
        self.w8(i, 4, 1);
        self.live = self.live.wrapping_add(1);
        self.w32(i, 0, kind);
    }

    #[allow(clippy::too_many_arguments)]
    fn fill(&mut self, i: usize, anim: u32, frame: u32, nframes: u32, tick: u32, period: u32, shadow: u8, flag: u8) {
        self.w32(i, 8, anim);
        self.w32(i, 12, frame);
        self.w32(i, 16, nframes);
        self.w32(i, 20, tick);
        self.w32(i, 24, period);
        self.w8(i, 28, shadow);
        self.w8(i, 29, flag);
    }

    fn size(&mut self, i: usize, wd: i32, ht: i32, hw: i32, hh: i32) {
        self.w32(i, 48, wd as u32);
        self.w32(i, 52, ht as u32);
        self.w32(i, 56, hw as u32);
        self.w32(i, 60, hh as u32);
    }

    /// 0x383b8: one shot of `kind` aimed at ship `target` (1 or 2) from (x, y), speed 6; returns whether the call got past the checks.
    pub fn spawn_aimed<W: ShotWorld>(&mut self, w: &W, kind: i32, target: i32, flag: i32, x: f32, y: f32) -> bool {
        let tgt = target as u8;
        let ok = if tgt == 1 { Self::alive(w, 1) } else { Self::alive(w, 2) };
        if !self.can_spawn(w, ok, x, y, 0.0, 416.0) {
            return false;
        }
        let tick = w.tick();
        for i in 0..SHOTS {
            if self.r8(i, 4) != 0 {
                continue;
            }
            self.begin(i, kind as u32);
            self.w8(i, 5, 0);
            self.w8(i, 29, flag as u8);
            self.w32(i, 8, 76);
            self.w32(i, 12, 1);
            self.w32(i, 16, 0);
            self.w32(i, 20, tick);
            self.w32(i, 24, 0);
            self.w8(i, 28, 0);
            self.wf(i, 32, x);
            self.wf(i, 36, y);
            self.size(i, 9, 9, 4, 4);
            self.w32(i, 76, 1);
            let (ux, uy) = aim(w, tgt, x, y, 4, 4);
            self.wf(i, 40, ux * 6.0);
            self.wf(i, 44, uy * 6.0);
            break;
        }
        true
    }

    /// 0x37fd4: beam shot (kind `kind`) with a muzzle flash at (mx - 10, my - 10) aimed at ship `target`; plays `snd` (priority 2) when >= 0.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_beam<W: ShotWorld>(&mut self, w: &mut W, kind: i32, x: f32, y: f32, mx: f32, my: f32, snd: i32, target: i32, flag: i32) {
        let tgt = target as u8;
        let ok = if tgt == 1 { Self::alive(w, 1) } else { Self::alive(w, 2) };
        if !self.can_spawn(w, ok, x, y, 32.0, 480.0) {
            return;
        }
        let tick = w.tick();
        for i in 0..SHOTS {
            if self.r8(i, 4) != 0 {
                continue;
            }
            self.begin(i, kind as u32);
            self.w8(i, 29, flag as u8);
            self.w8(i, 5, 1);
            self.wf(i, 64, -10.0 + mx);
            self.wf(i, 68, -10.0 + my);
            self.w32(i, 72, tick);
            self.w32(i, 8, 76);
            self.w32(i, 12, 1);
            self.w32(i, 16, 5);
            self.w32(i, 20, tick);
            self.w32(i, 24, 2);
            self.w8(i, 28, 0);
            self.wf(i, 32, x);
            self.wf(i, 36, y);
            self.size(i, 9, 9, 4, 4);
            self.w32(i, 76, 1);
            let (ux, uy) = aim(w, tgt, x, y, 4, 4);
            self.wf(i, 40, ux * 6.0);
            self.wf(i, 44, uy * 6.0);
            if (snd as i8) > -1 {
                w.sound(snd, 2, 0);
            }
            return;
        }
    }

    /// 0x38754: shot with parameters. `a` = [kind, count, sound, target mode, p, flag] (r5..r10), (x, y) at the muzzle.
    pub fn spawn_param<W: ShotWorld>(&mut self, w: &mut W, a: [i32; 6], x: f32, y: f32) -> bool {
        let [kind, count, snd, mode, p, flag] = a;
        let mode8 = mode as u8;
        let ok = match mode8 {
            1 => Self::alive(w, 1),
            2 => Self::alive(w, 2),
            3 => Self::alive(w, 1) || Self::alive(w, 2),
            _ => false,
        };
        if !self.can_spawn(w, ok, x, y, 0.0, 416.0) {
            return false;
        }
        if (kind as u32) > 9 {
            return false;
        }
        let tick = w.tick();
        // mode 3 ("either ship") is turned into ship 1 (`li r28,1` at 0x38828), even when only ship 2 is alive
        let mode8 = if mode8 == 3 { 1 } else { mode8 };
        let flag8 = flag as u8;
        let mut prio = 2;
        match kind {
            0 => self.k_aimed(w, 0, 9, 4, 6.0, 76, 1, 5, 2, 9, 4, count, mode8, flag8, x, y, tick),
            1 => {
                self.k_down(w, 1, 80, 1, 0, 0, 0, 8, 16, 4, 8, 3, -4.0, -8.0, 14.0, count, flag8, x, y);
                prio = 3;
            }
            2 => {
                self.k_aimed(w, 0, 9, 4, 6.0, 76, 1, 5, 2, 9, 4, count, mode8, flag8, x, y, tick);
                prio = 3;
            }
            3 => self.k_aimed(w, 3, 11, 5, 9.0, 79, p as u32, 0, 0, 11, 5, count, mode8, flag8, x, y, tick),
            4 => self.k_aimed(w, 4, 8, 4, 8.0, 77, 1, 5, 2, 8, 4, count, mode8, flag8, x, y, tick),
            5 => {
                self.k_down(w, 5, 156, 1, 0, 0, 0, 8, 56, 4, 28, 4, -4.0, 0.0, 16.0, count, flag8, x, y);
                prio = 3;
            }
            6 => self.k_down(w, 6, 78, 1, 24, 1, 1, 16, 16, 8, 8, 2, -8.0, 0.0, 2.2, count, flag8, x, y),
            7 => {
                self.k_down(w, 7, 126, 1, 0, 0, 0, 8, 64, 4, 32, 1, -4.0, 0.0, 16.0, count, flag8, x, y);
                prio = 3;
            }
            8 => {
                self.k_down(w, 8, 127, 1, 0, 0, 0, 8, 64, 4, 32, 1, -4.0, 0.0, 16.0, count, flag8, x, y);
                prio = 3;
            }
            _ => self.k_aimed(w, 9, 8, 4, 7.0, 76, 1, 5, 2, 8, 4, count, mode8, flag8, x, y, tick),
        }
        if (snd as i8) > -1 {
            w.sound(snd, prio, 0);
        }
        true
    }

    /// The kind routines that fire `count` shots aimed at ship `mode` with speeds base + 0.5, base + 1.0, ...
    /// (0x39650 kinds 0 / 2, 0x39adc kind 4, 0x3a0bc kind 3, 0x3a670 kind 9).
    #[allow(clippy::too_many_arguments)]
    fn k_aimed<W: ShotWorld>(&mut self, w: &mut W, kind: u32, wd: i32, hw: i32, base: f32, anim: u32, frame: u32, nframes: u32, period: u32, ht: i32, hh: i32,
                             count: i32, mode: u8, flag: u8, x: f32, y: f32, tick: u32) {
        let (fx, fy) = (x, y);
        let mut speed = base;
        let mut n = 0i32;
        for i in 0..SHOTS {
            if self.r8(i, 4) != 0 {
                continue;
            }
            self.begin(i, kind);
            self.w8(i, 29, flag);
            self.fill_aimed(i, anim, frame, nframes, tick, period);
            self.wf(i, 32, fx);
            self.wf(i, 36, fy);
            self.size(i, wd, ht, hw, hh);
            self.w32(i, 76, 1);
            let (ux, uy) = aim(w, mode, fx, fy, hw, hh);
            speed += 0.5;
            self.wf(i, 40, ux * speed);
            self.wf(i, 44, uy * speed);
            n += 1;
            if n == count || self.live >= 30 {
                break;
            }
        }
    }

    fn fill_aimed(&mut self, i: usize, anim: u32, frame: u32, nframes: u32, tick: u32, period: u32) {
        self.w32(i, 8, anim);
        self.w32(i, 12, frame);
        self.w32(i, 16, nframes);
        self.w32(i, 20, tick);
        self.w32(i, 24, period);
        self.w8(i, 28, 0);
    }

    /// The kind routines that fire `count` shots straight down (kinds 1, 5, 6, 7, 8).
    #[allow(clippy::too_many_arguments)]
    fn k_down<W: ShotWorld>(&mut self, w: &mut W, kind: u32, anim: u32, frame: u32, nframes: u32, period: u32, shadow: u8, wd: i32, ht: i32, hw: i32, hh: i32, dmg: u32,
                            dx: f32, dy: f32, vy: f32, count: i32, flag: u8, x: f32, y: f32) {
        let tick = w.tick();
        let (fx, fy) = (x, y);
        let mut n = 0i32;
        for i in 0..SHOTS {
            if self.r8(i, 4) != 0 {
                continue;
            }
            self.begin(i, kind);
            self.fill(i, anim, frame, nframes, tick, period, shadow, flag);
            self.wf(i, 32, dx + fx);
            self.wf(i, 36, if dy == 0.0 { fy } else { dy + fy });
            self.size(i, wd, ht, hw, hh);
            self.w32(i, 76, dmg);
            self.wf(i, 40, 0.0);
            self.wf(i, 44, vy);
            n += 1;
            if n == count || self.live >= 30 {
                break;
            }
        }
    }

    /// 0x38a1c: every shot explodes where it is and goes; no new shots for 90 ticks (only when there was at least one).
    pub fn clear_all<W: ShotWorld>(&mut self, w: &mut W) {
        if self.live < 1 {
            return;
        }
        for i in 0..SHOTS {
            if self.r8(i, 4) != 0 {
                let x = f2i(self.rf(i, 32)).wrapping_add(self.ri32(i, 56));
                let y = f2i(self.rf(i, 36)).wrapping_add(self.ri32(i, 60));
                w.effect(x, y, 5, 0, -1, 0, 0, self.rf(i, 40), self.rf(i, 44));
                self.free(i);
            }
        }
        self.gate = w.tick().wrapping_add(90);
    }

    /// The hit rectangle of a ship (0x38b0c): its box inset by 6 on every side, as 16-bit values like the Toolbox rect.
    fn ship_rect(s: &ShipView) -> Option<(i32, i32, i32, i32)> {
        if !s.alive || !s.vulnerable {
            return None;
        }
        let (x, y) = (f2i(s.x), f2i(s.y));
        let top = y as i16;
        let left = x as i16;
        let bottom = y.wrapping_add(s.h) as i16;
        let right = x.wrapping_add(s.w) as i16;
        // InsetRect(&r, 6, 6)
        Some(((top.wrapping_add(6)) as i32, (left.wrapping_add(6)) as i32, (bottom.wrapping_sub(6)) as i32, (right.wrapping_sub(6)) as i32))
    }

    /// 0x38b0c: one frame of every shot.
    pub fn update<W: ShotWorld>(&mut self, w: &mut W) {
        if self.live < 1 {
            return;
        }
        let r1 = Self::ship_rect(&w.ship(1));
        let r2 = Self::ship_rect(&w.ship(2));
        for i in 0..SHOTS {
            if self.r8(i, 4) == 0 {
                continue;
            }
            let tick = w.tick();
            // frame animation
            let nf = self.ri32(i, 16);
            if nf > 0 && tick > self.r32(i, 20).wrapping_add(self.r32(i, 24)) {
                self.w32(i, 20, tick);
                let fr = self.r32(i, 12).wrapping_add(1);
                self.w32(i, 12, fr);
                if (fr as i32) > nf {
                    self.w32(i, 12, 1);
                }
            }
            // muzzle flash: lives two ticks, follows the scrolling
            if self.r8(i, 5) != 0 {
                if tick > self.r32(i, 72).wrapping_add(1) {
                    self.w8(i, 5, 0);
                } else {
                    let v = self.rf(i, 68) + w.scroll_speed() as f32;
                    self.wf(i, 68, v);
                }
            }
            let x = self.rf(i, 32) + self.rf(i, 40);
            self.wf(i, 32, x);
            let y = self.rf(i, 36) + self.rf(i, 44);
            self.wf(i, 36, y);
            let (wd, ht) = (self.ri32(i, 48), self.ri32(i, 52));
            if (x + wd as f32) < 0.0 || x > 416.0 || (y + ht as f32) < 0.0 || y > 480.0 {
                self.free(i);
                continue;
            }
            let left = f2i(x);
            let top = f2i(y);
            let right = left.wrapping_add(wd);
            let bottom = top.wrapping_add(ht);
            let (hw, hh) = (self.ri32(i, 56), self.ri32(i, 60));
            if w.object_hit(left, top, right, bottom) {
                self.free(i);
                w.effect(left.wrapping_add(hw), top.wrapping_add(hh), 5, 0, 15, 0, 0, 0.0, 0.0);
                continue;
            }
            if let Some((st, sl, sb, sr)) = r1 {
                if w.object_hit(left, top, right, bottom) {
                    self.free(i);
                    w.effect(left.wrapping_add(hw), top.wrapping_add(hh), 5, 0, 15, 0, 0, 0.0, 0.0);
                    continue;
                }
                if !(left >= sr) && !(sl >= right) && !(top >= sb) && !(st >= bottom) {
                    self.free(i);
                    let dmg = self.ri32(i, 76);
                    w.ship_damage(1, dmg);
                    w.effect(left.wrapping_add(hw), top.wrapping_add(hh), 5, 0, -1, 0, 0, 0.0, 0.0);
                }
            }
            if self.r8(i, 4) == 0 {
                continue;
            }
            if let Some((st, sl, sb, sr)) = r2 {
                if !(left >= sr) && !(sl >= right) && !(top >= sb) && !(st >= bottom) {
                    self.free(i);
                    let dmg = self.ri32(i, 76);
                    w.ship_damage(2, dmg);
                    w.effect(left.wrapping_add(hw), top.wrapping_add(hh), 5, 0, -1, 0, 0, 0.0, 0.0);
                }
            }
        }
    }

    /// 0x39020 (`top` = false: the shots with +29 == 0, drawn below the enemies) and 0x39338 (`top` = true: the others, drawn above).
    pub fn draw<W: ShotDrawWorld>(&self, w: &mut W, top: bool) {
        if self.live < 1 {
            return;
        }
        let opts = w.opts();
        for i in 0..SHOTS {
            if self.r8(i, 4) == 0 || (self.r8(i, 29) != 0) != top {
                continue;
            }
            let (x, y) = (self.rf(i, 32), self.rf(i, 36));
            let (wd, ht) = (self.ri32(i, 48), self.ri32(i, 52));
            let (anim, frame) = (self.ri32(i, 8), self.ri32(i, 12));
            if opts.shadows && self.r8(i, 28) != 0 {
                w.blit(Prim::Shadow, f2i(-24.0 + x), f2i(44.0 + y), wd, ht, anim, frame);
            }
            let kind = self.r32(i, 0);
            if kind == 4 || kind == 9 {
                let prim = if opts.translucent { Prim::Trans } else { Prim::Anim };
                w.blit(prim, f2i(-2.0 + x), f2i(-2.0 + y), 13, 13, if kind == 4 { 115 } else { 116 }, 1);
            }
            w.blit(Prim::Anim, f2i(x), f2i(y), wd, ht, anim, frame);
            if self.r8(i, 5) != 0 {
                w.blit(Prim::Anim, f2i(self.rf(i, 64)), f2i(self.rf(i, 68)), 20, 20, 50, 1);
            }
        }
    }
}
