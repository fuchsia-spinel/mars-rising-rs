//! Per-type behaviour of the enemies: the init routine of each type (0x33a64.. via the group's behaviour vector) and its per-frame update
//! routine (jump table 0x6c44 used by 0x2dc20). See `enemy.rs` for the framework and for how these are verified.

use crate::enemy::{angle_between, f, g, f2i, EnemyWorld, Enemies};

/// Constants every init routine stores into the slots of its enemies.
struct Spec {
    ty: u32,
    anim: u32,
    w: i32,
    h: i32,
    state: u32,
    fx: i32,   // +116 death effect
    part: i32, // +120 death particles
    aux: u8,   // +124
    snd_hit: u8,
    snd_death: u8,
    score: i32,
}

impl Enemies {
    /// The part of the init routines that is the same for every type (everything except the type-specific start values).
    /// `member` is the slot; `gi` the group; `tick` the creation tick; `hp` the group's converted hit points.
    fn init_common(&mut self, i: usize, gi: usize, tick: u32, hp: u32, s: &Spec) {
        self.w8(i, f::ACTIVE, 1);
        self.w32(i, f::SPAWN_TICK, tick);
        self.w32(i, f::TIMER, tick);
        self.w32(i, f::DMG, 0);
        self.w32(i, f::HIT_TICK, tick);
        self.w8(i, f::HIT, 0);
        self.w32(i, f::GROUP, gi as u32);
        self.w8(i, f::B104, 0);
        self.w32(i, f::T88, tick);
        self.w32(i, f::F136, 0);
        self.w8(i, f::BOSS, 0);
        self.w32(i, f::BOSS_IDX, 0xffff_ffff);
        self.w8(i, f::B169, 0);
        self.w32(i, f::TYPE, s.ty);
        self.w32(i, f::F16, 1);
        self.w32(i, f::ANIM, s.anim);
        self.w32(i, f::STATE, s.state);
        self.wi32(i, f::W, s.w);
        self.wi32(i, f::H, s.h);
        self.wi32(i, f::HW, s.w / 2);
        self.wi32(i, f::HH, s.h / 2);
        self.wi32(i, f::SCORE, s.score);
        self.wi32(i, f::PART_DEATH, s.part);
        self.wi32(i, f::FX_DEATH, s.fx);
        self.w8(i, f::AUX_DEATH, s.aux);
        self.w8(i, f::SND_DEATH, s.snd_death);
        self.w8(i, f::SND_HIT, s.snd_hit);
        self.w32(i, f::HP, hp);
    }

    /// Calls `body(slot_index)` for every member of group `gi` (the init routines' common loop).
    fn members(&self, gi: usize) -> Vec<usize> {
        let n = self.gri32(gi, g::COUNT);
        let mut v = Vec::new();
        let mut k = 0;
        while k < n {
            let m = self.gri32(gi, g::MEMBERS + 4 * k as usize);
            if m != -1 {
                v.push(m as usize);
            }
            k += 1;
        }
        v
    }

    /// Dispatch of the group's behaviour routine (the original stores a code vector per group; see `ENEMY_TYPES`).
    pub(crate) fn init_type<W: EnemyWorld>(&mut self, ty: u32, gi: usize, w: &mut W) {
        match ty {
            1 => self.init1(gi, w),
            2 => self.init2(gi, w),
            3 => self.init3(gi, w),
            4 => self.init4(gi, w),
            5 => self.init5(gi, w),
            6 => self.init6(gi, w),
            7 => self.init7(gi, w),
            8 => self.init8(gi, w),
            9 => self.init9(gi, w),
            10 => self.init10(gi, w),
            12 => self.init12(gi, w),
            13 => self.init13(gi, w),
            14 => self.init14(gi, w),
            15 => self.init15(gi, w),
            16 => self.init16(gi, w),
            17 => self.init17(gi, w),
            18 => self.init18(gi, w),
            19 => self.init19(gi, w),
            20 => self.init20(gi, w),
            21 => self.init21(gi, w),
            23 => self.init23(gi, w),
            11 => self.init11(gi, w),
            _ => {}
        }
    }

    /// Dispatch of 0x2dc20's jump table. Types 0 and 22 have no routine.
    pub(crate) fn update_type<W: EnemyWorld>(&mut self, ty: u32, i: usize, w: &mut W) {
        match ty {
            1 => self.upd1(i, w),
            2 => self.upd2(i, w),
            3 => self.upd3(i, w),
            4 => self.upd4(i, w),
            5 => self.upd5(i, w),
            6 => self.upd6(i, w),
            7 => self.upd7(i, w),
            8 => self.upd8(i, w),
            9 => self.upd9(i, w),
            10 => self.upd10(i, w),
            12 => self.upd12(i, w),
            13 => self.upd13(i, w),
            15 => self.upd15(i, w),
            16 => self.upd16(i, w),
            17 => self.upd17(i, w),
            18 => self.upd18(i, w),
            19 => self.upd19(i, w),
            20 => self.upd20(i, w),
            21 => self.upd21(i, w),
            23 => self.upd23(i, w),
            _ => {}
        }
    }

    /// Centre of a ship as the enemy code computes it (getter x + half size, in single precision).
    fn ship_centre<W: EnemyWorld>(w: &W, n: u8) -> (f32, f32) {
        let s = w.ship(n);
        (s.x + s.hw as f32, s.y + s.hh as f32)
    }

    /// Centre of the enemy in slot `i` (position + half size).
    fn centre(&self, i: usize) -> (f32, f32) {
        (self.rf(i, f::X) + self.ri32(i, f::HW) as f32, self.rf(i, f::Y) + self.ri32(i, f::HH) as f32)
    }

    /// 0x2e7a8: which ship (1 or 2) is the target of an enemy at (x, y). Only ship 2 is considered when it is alive and ship 1 is not,
    /// etc.; with two live ships the one with the smaller "distance" wins (the original's `|dx|` quirks are kept, see the code).
    pub fn pick_target<W: EnemyWorld>(w: &W, x: f32, y: f32) -> u8 {
        if w.players() == 1 {
            return 1;
        }
        let s2 = w.ship(2);
        if !s2.alive {
            return 1;
        }
        let s1 = w.ship(1);
        if !s1.alive {
            return 2;
        }
        // distance measure of ship 2 (f3) and ship 1 (f0)
        let measure = |sx: f32, sy: f32| -> f32 {
            let dx = sx - x;
            if dx < 0.0 {
                -dx
            } else {
                let dy = sy - y;
                if dx + dy < 0.0 { -dy } else { dy }
            }
        };
        let m2 = measure(s2.x, s2.y);
        let m1 = measure(s1.x, s1.y);
        if m1 > m2 { 2 } else { 1 }
    }

    // ---- type 1: 32x32 ships that drop in beside the player and fall towards him -----------------------------------------------------
    fn init1<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        for i in self.members(gi) {
            let s = Spec { ty: 1, anim: 142, w: 32, h: 32, state: 1, fx: 14, part: -1, aux: 0, snd_hit: 46, snd_death: 27, score: 30 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            let d = w.random(0, 20) as u16 as u32;
            self.w32(i, f::DELAY, d);
            let p1 = w.ship(1);
            let x = if p1.x < 208.0 { w.random(308, 416) as u16 } else { w.random(64, 108) as u16 };
            self.wf(i, f::X, x as f32);
            self.wf(i, f::Y, -128.0);
            let x = self.rf(i, f::X);
            self.wf(i, f::BX0, x);
            let y = self.rf(i, f::Y);
            self.wf(i, f::BY0, y);
            self.wf(i, f::BX1, x + self.ri32(i, f::W) as f32);
            self.wf(i, f::BY1, y + self.ri32(i, f::H) as f32);
            // aim at ship 1
            let (cx, cy) = Self::ship_centre(w, 1);
            let (ex, ey) = self.centre(i);
            let dx = cx - ex;
            let dy = cy - ey;
            let sq = dx.mul_add(dx, dy * dy);
            let l = (sq as f64).sqrt() as f32;
            let nx = dx / l;
            let ny = dy / l;
            let sp = (w.random(6, 7) as u16) as f32;
            self.wf(i, f::VX, nx * sp);
            self.wf(i, f::VY, ny * sp);
        }
    }

    fn upd1<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        let (ex, ey) = self.centre(i);
        let n = Self::pick_target(w, ex, ey);
        let (f31, f30) = Self::ship_centre(w, n);
        if !(t < self.r32(i, f::TIMER)) {
            let (cx, cy) = self.centre(i);
            w.shot_aimed(0, n as i32, 0, cx, cy);
            let gi = self.r32(i, f::GROUP) as usize;
            let a = self.gr32(gi, g::A64);
            self.w32(i, f::TIMER, t.wrapping_add(a));
        }
        if self.r8(i, f::B104) == 0 {
            let y = self.rf(i, f::Y);
            if !(y > 50.0) {
                return;
            }
            if !((y as f64) < 60.0f64 + f30 as f64) {
                return;
            }
            if self.rf(i, f::VX) > 0.0 {
                if self.rf(i, f::X) + self.ri32(i, f::W) as f32 > f31 {
                    self.wf(i, f::AX, -0.9);
                    self.wf(i, f::AY, 0.9);
                    self.w8(i, f::B104, 1);
                }
            } else if self.rf(i, f::X) < f31 {
                self.wf(i, f::AX, 0.9);
                self.wf(i, f::AY, 0.9);
                self.w8(i, f::B104, 1);
            }
        } else {
            let v = self.rf(i, f::VX) + self.rf(i, f::AX);
            self.wf(i, f::VX, v);
            let v = self.rf(i, f::VY) + self.rf(i, f::AY);
            self.wf(i, f::VY, v);
            let vx = self.rf(i, f::VX);
            if vx > 7.0 {
                self.wf(i, f::VX, 7.0);
            } else if vx < -7.0 {
                self.wf(i, f::VX, -7.0);
            }
            let vy = self.rf(i, f::VY);
            if vy > 6.0 {
                self.wf(i, f::VY, 6.0);
            } else if vy < -6.0 {
                self.wf(i, f::VY, -6.0);
            }
        }
        // animation frame counter 1..10 (runs backwards while the ship drifts left)
        if !(self.rf(i, f::VX) < 0.0) {
            let s = self.ri32(i, f::STATE).wrapping_add(1);
            self.wi32(i, f::STATE, s);
            if s > 10 {
                self.wi32(i, f::STATE, 1);
            }
        } else {
            let s = self.ri32(i, f::STATE).wrapping_sub(1);
            self.wi32(i, f::STATE, s);
            if s < 1 {
                self.wi32(i, f::STATE, 10);
            }
        }
    }

    /// Hit box = the sprite rectangle at the current position.
    fn box_from_pos(&mut self, i: usize) {
        let x = self.rf(i, f::X);
        self.wf(i, f::BX0, x);
        let y = self.rf(i, f::Y);
        self.wf(i, f::BY0, y);
        self.wf(i, f::BX1, x + self.ri32(i, f::W) as f32);
        self.wf(i, f::BY1, y + self.ri32(i, f::H) as f32);
    }

    /// The "unit vector times speed" computation (`sqrt(fmadds(dx, dx, dy*dy))`, then fdivs, fmuls) that many init routines use.
    fn aim(dx: f32, dy: f32, speed: f32) -> (f32, f32) {
        let l = ((dx.mul_add(dx, dy * dy)) as f64).sqrt() as f32;
        ((dx / l) * speed, (dy / l) * speed)
    }

    // ---- type 2: 32x32 ships that fly to a point near the top centre, stop, and fire at the nearest ship -------------------------------
    fn init2<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let hp = self.gr32(gi, g::B68);
        let mut delay = 0u32;
        for i in self.members(gi) {
            let s = Spec { ty: 2, anim: 142, w: 32, h: 32, state: 1, fx: 14, part: 10, aux: 2, snd_hit: 48, snd_death: 3, score: 50 };
            self.init_common(i, gi, tick, hp, &s);
            self.w8(i, 92, 0);
            let d = w.random(25, 65) as u16 as u32;
            self.w32(i, f::DELAY, delay.wrapping_add(d));
            let x = w.random(0, 416) as u16 as f32;
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -128.0);
            self.box_from_pos(i);
            let tx = w.random(200, 216) as u16 as f32;
            let ty = w.random(100, 240) as u16 as f32;
            let (ex, ey) = self.centre(i);
            let sp = w.random(6, 9) as u16 as f32;
            let (vx, vy) = Self::aim(tx - ex, ty - ey, sp);
            self.wf(i, f::VX, vx);
            self.wf(i, f::VY, vy);
            delay = delay.wrapping_add(d);
        }
    }

    fn upd2<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        if !(t < self.r32(i, f::T88).wrapping_add(2)) {
            if self.r8(i, 92) != 0 {
                let s = self.ri32(i, f::STATE).wrapping_add(1);
                self.wi32(i, f::STATE, s);
                if s > 6 {
                    self.wi32(i, f::STATE, 5);
                    self.w8(i, 92, 0);
                }
            } else {
                let s = self.ri32(i, f::STATE).wrapping_sub(1);
                self.wi32(i, f::STATE, s);
                if s < 1 {
                    self.wi32(i, f::STATE, 2);
                    self.w8(i, 92, 1);
                }
            }
            self.w32(i, f::T88, t);
        }
        if self.r8(i, f::B104) == 0 {
            if !(self.rf(i, f::Y) > 190.0) {
                return;
            }
            // arrived: remember the velocity (negated) as the later "drift" velocity, stop and fire once
            let vx = self.rf(i, f::VX);
            self.wf(i, f::AX, -vx);
            let vy = self.rf(i, f::VY);
            self.wf(i, f::AY, -vy);
            self.wf(i, f::VX, 0.0);
            self.wf(i, f::VY, 0.0);
            self.w8(i, f::B104, 1);
            let (ex, ey) = self.centre(i);
            let n = Self::pick_target(w, ex, ey);
            let (cx, cy) = self.centre(i);
            w.shot_aimed(0, n as i32, 0, cx, cy);
            self.w32(i, f::TIMER, t);
        } else if t > self.r32(i, f::TIMER).wrapping_add(10) {
            let ax = self.rf(i, f::AX);
            self.wf(i, f::VX, ax);
            let ay = self.rf(i, f::AY);
            self.wf(i, f::VY, ay);
        }
    }

    /// The ship an enemy picks as its target (0x2e7a8) and that ship's centre.
    fn target<W: EnemyWorld>(&self, w: &W, i: usize) -> (u8, f32, f32) {
        let (ex, ey) = self.centre(i);
        let n = Self::pick_target(w, ex, ey);
        let (cx, cy) = Self::ship_centre(w, n);
        (n, cx, cy)
    }

    /// An enemy that goes away on its own (not killed): group bookkeeping and slot release, without score or effects.
    fn leave(&mut self, i: usize) {
        let gi = self.r32(i, f::GROUP) as usize;
        let alive = self.gri32(gi, g::ALIVE).wrapping_sub(1);
        self.gw32(gi, g::ALIVE, alive as u32);
        if alive == 0 {
            self.gw8(gi, g::ACTIVE, 0);
            self.live_groups -= 1;
        }
        self.clear_slot(i);
    }

    // ---- type 4: 16x16 ships that appear at random places, flash in, fire once and flash out -----------------------------------------
    fn init4<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let hp = self.gr32(gi, g::B68);
        let mut delay = 0u32;
        for i in self.members(gi) {
            let s = Spec { ty: 4, anim: 148, w: 16, h: 16, state: 4, fx: 14, part: -1, aux: 6, snd_hit: 48, snd_death: 27, score: 100 };
            self.init_common(i, gi, tick, hp, &s);
            let d = w.random(45, 80) as u16 as u32;
            self.w32(i, f::DELAY, delay.wrapping_add(d));
            self.w32(i, f::TIMER, tick.wrapping_add(delay.wrapping_add(d)));
            let x = w.random(64, 352) as u16 as f32;
            self.wf(i, f::X, x);
            let y = w.random(200, 380) as u16 as f32;
            self.wf(i, f::Y, y);
            self.wf(i, f::VX, 0.0);
            self.wf(i, f::VY, 0.0);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
            self.box_from_pos(i);
            delay = delay.wrapping_add(d);
        }
    }

    fn upd4<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        let (n, f30, f4) = self.target(w, i);
        if self.r8(i, f::B104) == 0 {
            if self.ri32(i, f::DELAY) as u32 == 0 {
                let x = self.rf(i, f::X);
                let y = self.rf(i, f::Y);
                if (50.0 + x) > f30 && (-50.0 + x) < f30 && (50.0 + y) > f4 && (-50.0 + y) < (41.0 + f4) {
                    // the player is close: it flashes in right above him
                    self.wf(i, f::X, f30);
                    self.wf(i, f::Y, -100.0 + f4);
                    self.box_from_pos(i);
                }
            }
            if self.r32(i, f::STATE) != 1 {
                if t > self.r32(i, f::TIMER).wrapping_add(1) {
                    let s = self.r32(i, f::STATE).wrapping_sub(1);
                    self.w32(i, f::STATE, s);
                    self.w32(i, f::TIMER, t);
                }
            } else if t > self.r32(i, f::TIMER).wrapping_add(30) {
                let (cx, cy) = self.centre(i);
                if w.shot_param([4, 1, -1, n as i32, 1, 0], cx, cy) as u8 != 0 {
                    self.w32(i, f::TIMER, t);
                    self.w8(i, f::B104, 1);
                }
            }
        } else if t > self.r32(i, f::TIMER).wrapping_add(1) {
            let s = self.r32(i, f::STATE).wrapping_add(1);
            self.w32(i, f::STATE, s);
            self.w32(i, f::TIMER, t);
            if (self.r32(i, f::STATE) as i32) > 4 {
                self.leave(i);
            }
        }
    }

    /// Clamp the velocity the way several types do (|vx| <= 7, |vy| <= 6).
    fn clamp_vel(&mut self, i: usize) {
        let vx = self.rf(i, f::VX);
        if vx > 7.0 {
            self.wf(i, f::VX, 7.0);
        } else if vx < -7.0 {
            self.wf(i, f::VX, -7.0);
        }
        let vy = self.rf(i, f::VY);
        if vy > 6.0 {
            self.wf(i, f::VY, 6.0);
        } else if vy < -6.0 {
            self.wf(i, f::VY, -6.0);
        }
    }

    /// vel += acc
    fn accelerate(&mut self, i: usize) {
        let v = self.rf(i, f::VX) + self.rf(i, f::AX);
        self.wf(i, f::VX, v);
        let v = self.rf(i, f::VY) + self.rf(i, f::AY);
        self.wf(i, f::VY, v);
    }

    // ---- type 5: 40x40 ships like type 1 (fixed speed 5); from difficulty 3 on they also turn back towards the player --------------------
    fn init5<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let hard = (w.difficulty() as i32) > 2;
        let mut delay = 1u32; // (the original's accumulator starts out as the register holding the constant 1)
        for i in self.members(gi) {
            self.w32(i, 12, hard as u32);
            let s = Spec { ty: 5, anim: 142, w: 40, h: 40, state: 1, fx: -1, part: 10, aux: 2, snd_hit: 55, snd_death: 27, score: 50 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            let d = w.random(5, 25) as u16 as u32;
            self.w32(i, f::DELAY, delay.wrapping_add(d));
            delay = delay.wrapping_add(d);
            let x = if w.ship(1).x < 208.0 { w.random(308, 416) as u16 } else { w.random(64, 108) as u16 };
            self.wf(i, f::X, x as f32);
            self.wf(i, f::Y, -128.0);
            self.box_from_pos(i);
            let (cx, cy) = Self::ship_centre(w, 1);
            let (ex, ey) = self.centre(i);
            let (vx, vy) = Self::aim(cx - ex, cy - ey, 5.0);
            self.wf(i, f::VX, vx);
            self.wf(i, f::VY, vy);
        }
    }

    fn upd5<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        let (_n, f31, f2) = self.target(w, i);
        if t > self.r32(i, f::T88) {
            self.w32(i, f::T88, t);
            let step = if self.r8(i, f::B104) == 0 { 1 } else { 3 };
            if self.rf(i, f::VX) < 0.0 {
                let s = self.ri32(i, f::STATE).wrapping_add(step);
                self.wi32(i, f::STATE, s);
                if s > 9 {
                    self.wi32(i, f::STATE, 1);
                }
            } else {
                let s = self.ri32(i, f::STATE).wrapping_sub(step);
                self.wi32(i, f::STATE, s);
                if s < 1 {
                    self.wi32(i, f::STATE, 9);
                }
            }
        }
        if self.r8(i, f::B104) == 0 {
            let mut r5 = false;
            let mut r6 = false;
            let y = self.rf(i, f::Y);
            if y > 200.0 && (y as f64) < 60.0f64 + f2 as f64 {
                r5 = true;
            }
            if self.rf(i, f::Y) > 320.0 {
                r5 = true;
                r6 = true;
            }
            if r5 {
                let x = self.rf(i, f::X);
                if self.rf(i, f::VX) > 0.0 {
                    if x + self.ri32(i, f::W) as f32 > f31 || r6 {
                        self.wf(i, f::AX, -0.3);
                        self.wf(i, f::AY, -0.3);
                        self.w8(i, f::B104, 1);
                    }
                } else if x < f31 || r6 {
                    self.wf(i, f::AX, 0.3);
                    self.wf(i, f::AY, -0.3);
                    self.w8(i, f::B104, 1);
                }
            }
        } else {
            self.accelerate(i);
            self.clamp_vel(i);
            if self.r32(i, 12) == 1 && self.rf(i, f::Y) < 180.0 {
                let x = self.rf(i, f::X);
                if self.rf(i, f::VX) > 0.0 {
                    if x + self.ri32(i, f::W) as f32 > f31 {
                        self.wf(i, f::AX, -0.3);
                        self.wf(i, f::AY, 0.9);
                        self.w8(i, f::B104, 1);
                    }
                } else if x < f31 {
                    self.wf(i, f::AX, 0.3);
                    self.wf(i, f::AY, 0.9);
                    self.w8(i, f::B104, 1);
                }
            }
        }
    }

    // ---- type 6 (and 11, which creates type-6 enemies): 26x26 ships that fall straight down, steer towards the player's column and shoot ---
    fn init6<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        for i in self.members(gi) {
            let s = Spec { ty: 6, anim: 150, w: 26, h: 26, state: 1, fx: 14, part: -1, aux: 9, snd_hit: 48, snd_death: 47, score: 100 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            let d = w.random(0, 30) as u16 as u32;
            self.w32(i, f::DELAY, d);
            let x = w.random(64, 352) as u16 as f32;
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -128.0);
            self.box_from_pos(i);
            self.wf(i, f::VX, 0.0);
            let vy = w.random(6, 9) as u16 as f32;
            self.wf(i, f::VY, vy);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
        }
    }

    fn init11<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let side = w.random(0, 2) as u16;
        let (mut x, step) = if side == 1 {
            let x = w.random(32, 384) as u16 as f32;
            (x, if x > 208.0 { -25i32 } else { 25 })
        } else {
            (w.random(64, 384) as u16 as f32, 0)
        };
        let mut delay = 1u32;
        for i in self.members(gi) {
            let s = Spec { ty: 6, anim: 150, w: 26, h: 26, state: 1, fx: 14, part: -1, aux: 9, snd_hit: 48, snd_death: 3, score: 50 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            self.w32(i, f::DELAY, delay);
            self.wf(i, f::X, x);
            x += step as f32;
            delay = delay.wrapping_add(if side == 1 { 5 } else if side == 0 { 10 } else { 20 });
            self.wf(i, f::Y, -128.0);
            self.box_from_pos(i);
            self.wf(i, f::VX, 0.0);
            self.wf(i, f::VY, 7.0);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
        }
    }

    fn upd6<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        if !(self.rf(i, f::Y) > 100.0) {
            return;
        }
        let t = w.tick();
        let (n, f31, _) = self.target(w, i);
        if !(t < self.r32(i, f::TIMER)) {
            let (cx, cy) = self.centre(i);
            w.shot_aimed(0, n as i32, 0, cx, cy);
            let gi = self.r32(i, f::GROUP) as usize;
            let a = self.gr32(gi, g::A64);
            self.w32(i, f::TIMER, t.wrapping_add(a));
        }
        if self.rf(i, f::X) + (self.ri32(i, f::W) as f32) < f31 {
            self.wf(i, f::AX, 0.2);
        } else {
            self.wf(i, f::AX, -0.2);
        }
        let v = self.rf(i, f::VX) + self.rf(i, f::AX);
        self.wf(i, f::VX, v);
        let vx = self.rf(i, f::VX);
        if vx > 7.0 {
            self.wf(i, f::VX, 7.0);
        } else if vx < -7.0 {
            self.wf(i, f::VX, -7.0);
        }
    }

    // ---- type 7: 37x61 gunship that slides down, brakes and fires a burst of four shots ------------------------------------------------
    fn init7<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let mut delay = 1u32;
        for i in self.members(gi) {
            let s = Spec { ty: 7, anim: 151, w: 37, h: 61, state: 1, fx: 14, part: -1, aux: 5, snd_hit: 46, snd_death: 0, score: 500 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            let d = w.random(30, 90) as u16 as u32;
            self.w32(i, f::DELAY, delay.wrapping_add(d));
            let x = w.random(64, 352) as u16 as f32;
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -128.0);
            self.box_from_pos(i);
            self.w8(i, f::B148, 0);
            self.w32(i, 152, 118);
            self.w32(i, 156, 1);
            self.wf(i, 160, 0.0);
            self.wf(i, 164, 42.0);
            self.wf(i, f::VX, 0.0);
            delay = delay.wrapping_add(d);
            let r = w.random(0, 3) as u16;
            let vy = match r {
                0 => 3.0f32,
                1 => 3.2,
                2 => 3.4,
                3 => 3.6,
                _ => self.rf(i, f::VY),
            };
            if r <= 3 {
                self.wf(i, f::VY, vy);
            }
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
        }
    }

    fn upd7<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        let any = w.ship(1).alive || w.ship(2).alive;
        let mut moving = true; // r27
        if self.r8(i, f::B104) == 0 {
            let y = self.rf(i, f::Y);
            if y > 130.0 && y < 300.0 && any {
                self.w8(i, f::B104, 1);
                self.wf(i, f::AY, -0.2);
                self.w32(i, f::TIMER, t);
            }
            let v = self.rf(i, f::VY) + self.rf(i, f::AY);
            self.wf(i, f::VY, v);
        } else {
            let v = self.rf(i, f::VY) + self.rf(i, f::AY);
            self.wf(i, f::VY, v);
            if self.rf(i, f::VY) > 12.0 {
                self.wf(i, f::VY, 12.0);
            }
            if self.rf(i, f::VY) < -12.0 {
                self.wf(i, f::VY, -12.0);
            }
            if self.rf(i, f::VY) < -2.0 {
                moving = false;
            }
            if !any {
                self.w8(i, f::B148, 0);
            } else {
                let c = self.ri32(i, 156).wrapping_add(1);
                self.wi32(i, 156, c);
                if c > 5 {
                    self.wi32(i, 156, 5);
                }
                if t == self.r32(i, f::TIMER).wrapping_add(1) {
                    self.w8(i, f::B148, 1);
                    self.wi32(i, 156, 1);
                    w.sound_rate(9, 3, 0x11000);
                    w.sound_rate(9, 3, 0x11000);
                }
                let tt = self.r32(i, f::TIMER);
                let fire = |s: &mut Self, w: &mut W| -> bool {
                    let x = s.rf(i, f::X) + s.ri32(i, f::HW) as f32;
                    let y = 10.0f32 + s.rf(i, f::Y);
                    w.shot_param([8, 1, 56, 3, 1, 0], x, y) as u8 != 0
                };
                if t == tt.wrapping_add(16) || t == tt.wrapping_add(18) || t == tt.wrapping_add(20) {
                    if fire(self, w) {
                        self.w8(i, f::B148, 1);
                    }
                } else if t == tt.wrapping_add(22) && fire(self, w) {
                    self.w32(i, f::TIMER, t.wrapping_add(9999));
                    self.w8(i, f::B148, 0);
                }
            }
        }
        let r5 = moving as u32;
        if t > r5.wrapping_add(self.r32(i, f::T88)) {
            let s = self.ri32(i, f::STATE).wrapping_add(1);
            self.wi32(i, f::STATE, s);
            self.w32(i, f::T88, t);
            if s > 7 {
                self.wi32(i, f::STATE, 1);
            }
        }
    }

    // ---- type 8 (and 14, which creates type-8 enemies): 45x35 saucers that fall, and after a while pull up; some of them do ------------
    fn init8<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let mut delay = 1u32;
        for i in self.members(gi) {
            let s = Spec { ty: 8, anim: 128, w: 45, h: 35, state: 1, fx: 14, part: -1, aux: 1, snd_hit: 48, snd_death: 3, score: 100 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            let pull = w.random(0, 2) as u16 != 0;
            self.w32(i, 12, pull as u32);
            self.w32(i, f::DELAY, delay);
            let x = w.random(64, 352) as u16 as f32;
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -128.0);
            self.box_from_pos(i);
            self.wf(i, f::VX, 0.0);
            let vy = w.random(6, 7) as u16 as f32;
            self.wf(i, f::VY, vy);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
            delay = delay.wrapping_add(10);
        }
    }

    fn init14<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let vy = w.random(8, 9) as u16 as f32;
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let (dstep, mut xstep) = if w.random(0, 1) as u16 == 0 { (7u32, 20i32) } else { (2, 30) };
        let mut x = w.random(32, 384) as u16 as f32;
        if x > 208.0 {
            xstep = -xstep;
        }
        let pull = w.random(0, 4) as u16 != 0;
        let mut delay = 1u32;
        for i in self.members(gi) {
            let s = Spec { ty: 8, anim: 128, w: 45, h: 35, state: 1, fx: 14, part: -1, aux: 1, snd_hit: 48, snd_death: 3, score: 50 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            self.w32(i, 12, pull as u32);
            self.w32(i, f::DELAY, delay);
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -128.0);
            self.box_from_pos(i);
            self.wf(i, f::VX, 0.0);
            self.wf(i, f::VY, vy);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
            x += xstep as f32;
            delay = delay.wrapping_add(dstep);
        }
    }

    fn upd8<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        if self.r8(i, f::B104) == 0 {
            if t > self.r32(i, f::T88).wrapping_add(1) {
                let s = self.ri32(i, f::STATE).wrapping_add(1);
                self.wi32(i, f::STATE, s);
                self.w32(i, f::T88, t);
                if s > 8 {
                    self.wi32(i, f::STATE, 1);
                }
            }
            if self.r32(i, 12) == 1 && self.rf(i, f::Y) > 230.0 && self.r32(i, f::STATE) == 1 {
                self.w8(i, f::B104, 1);
                self.wf(i, f::AY, -0.4);
                self.w32(i, f::ANIM, 129);
                self.w32(i, f::STATE, 1);
                self.w32(i, f::T88, t);
            }
        } else {
            if t > self.r32(i, f::T88).wrapping_add(2) {
                let s = self.ri32(i, f::STATE).wrapping_add(1);
                self.wi32(i, f::STATE, s);
                self.w32(i, f::T88, t);
                if s > 8 {
                    self.wi32(i, f::STATE, 8);
                }
            }
            let v = self.rf(i, f::VY) + self.rf(i, f::AY);
            self.wf(i, f::VY, v);
            if self.rf(i, f::VY) > 12.0 {
                self.wf(i, f::VY, 12.0);
            }
            if self.rf(i, f::VY) < -12.0 {
                self.wf(i, f::VY, -12.0);
            }
        }
    }

    // ---- type 9: 45x35 saucers that rise from the bottom -----------------------------------------------------------------------------
    fn init9<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        for i in self.members(gi) {
            let s = Spec { ty: 9, anim: 129, w: 45, h: 35, state: 8, fx: 14, part: -1, aux: 1, snd_hit: 48, snd_death: 3, score: 100 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            let pull = w.random(0, 2) as u16 != 0;
            self.w32(i, 12, pull as u32);
            let d = w.random(1, 30) as u16 as u32;
            self.w32(i, f::DELAY, d);
            let x = w.random(64, 352) as u16 as f32;
            self.wf(i, f::X, x);
            self.wf(i, f::Y, 480.0);
            self.box_from_pos(i);
            self.wf(i, f::VX, 0.0);
            let vy = -((w.random(9, 10) as u16) as i32) as f32;
            self.wf(i, f::VY, vy);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
        }
    }

    fn upd9<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        let (_n, px, _) = self.target(w, i);
        if self.r32(i, f::DELAY) == 0 {
            // first active frame: pick the column, on the side away from the player
            let x = if px < 208.0 {
                let hi = 416i32.wrapping_sub(self.ri32(i, f::W));
                w.random(316, hi) as u16
            } else {
                w.random(0, 100) as u16
            } as f32;
            self.wf(i, f::X, x);
            let v = self.rf(i, f::X);
            self.wf(i, f::BX0, v);
            let v = self.rf(i, f::X) + self.ri32(i, f::W) as f32;
            self.wf(i, f::BX1, v);
        }
        if self.r8(i, f::B104) == 0 {
            if self.r32(i, 12) == 1 && self.rf(i, f::Y) < 120.0 {
                self.w8(i, f::B104, 1);
                self.wf(i, f::AY, 0.8);
                self.w32(i, f::T88, t);
            }
        } else {
            if t > self.r32(i, f::T88).wrapping_add(2) {
                let s = self.ri32(i, f::STATE).wrapping_sub(1);
                self.wi32(i, f::STATE, s);
                self.w32(i, f::T88, t);
                if s == 0 {
                    self.wi32(i, f::STATE, 1);
                    self.w32(i, f::ANIM, 128);
                }
            }
            let v = self.rf(i, f::VY) + self.rf(i, f::AY);
            self.wf(i, f::VY, v);
            if self.rf(i, f::VY) > 12.0 {
                self.wf(i, f::VY, 12.0);
            }
            if self.rf(i, f::VY) < -12.0 {
                self.wf(i, f::VY, -12.0);
            }
            if self.r32(i, f::ANIM) == 128 && t > self.r32(i, f::T88) {
                let s = self.ri32(i, f::STATE).wrapping_add(1);
                self.wi32(i, f::STATE, s);
                self.w32(i, f::T88, t);
                if s > 8 {
                    self.wi32(i, f::STATE, 1);
                }
            }
        }
    }

    // ---- type 10: 58x62 twin-gun ships: two alternating barrels ---------------------------------------------------------------------
    fn init10<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let mut delay = 0u32;
        for i in self.members(gi) {
            let s = Spec { ty: 10, anim: 119, w: 58, h: 62, state: 1, fx: 14, part: 9, aux: 8, snd_hit: 48, snd_death: 45, score: 400 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            self.w8(i, 92, 0);
            let d = w.random(40, 120) as u16 as u32;
            self.w32(i, f::DELAY, delay.wrapping_add(d));
            let x = w.random(64, 352) as u16 as f32;
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -128.0);
            self.box_from_pos(i);
            self.wf(i, f::VX, 0.0);
            let vy = w.random(2, 3) as u16 as f32;
            self.wf(i, f::VY, vy);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
            delay = delay.wrapping_add(d);
        }
    }

    fn upd10<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        if t > self.r32(i, f::T88).wrapping_add(2) {
            if self.r8(i, 92) != 0 {
                let s = self.ri32(i, f::STATE).wrapping_add(1);
                self.wi32(i, f::STATE, s);
                self.w32(i, f::T88, t);
                if s > 9 {
                    self.wi32(i, f::STATE, 8);
                    self.w8(i, 92, 0);
                }
            } else {
                let s = self.ri32(i, f::STATE).wrapping_sub(1);
                self.wi32(i, f::STATE, s);
                self.w32(i, f::T88, t);
                if s < 1 {
                    self.wi32(i, f::STATE, 2);
                    self.w8(i, 92, 1);
                }
            }
        }
        let y = self.rf(i, f::Y);
        if !(y < 250.0) || !(y > 70.0) {
            return;
        }
        if !(w.ship(1).alive || w.ship(2).alive) {
            return;
        }
        let x = self.rf(i, f::X);
        let y = self.rf(i, f::Y);
        if self.r8(i, f::B104) == 0 {
            if t > self.r32(i, f::TIMER).wrapping_add(30) {
                if w.shot_param([1, 1, 20, 3, 1, 0], 8.0 + x, 20.0 + y) as u8 != 0 {
                    self.w8(i, f::B104, 1);
                    self.wf(i, 140, -2.0 + x);
                    self.wf(i, 144, 20.0 + y);
                    self.w32(i, f::TIMER, t);
                    self.w32(i, f::F136, 1);
                }
            }
        } else if t > self.r32(i, f::TIMER).wrapping_add(5) {
            if w.shot_param([1, 1, 20, 3, 1, 0], 52.0 + x, 20.0 + y) as u8 != 0 {
                self.w8(i, f::B104, 0);
                self.wf(i, 140, -16.0 + (x + self.ri32(i, f::W) as f32));
                self.wf(i, 144, 20.0 + y);
                self.w32(i, f::TIMER, t);
                self.w32(i, f::F136, 1);
            }
        }
    }

    // ---- types 12 and 13: 26x26 ships that cross the screen sideways (12) or drop from the top (13), bouncing ----------------------------
    fn init12<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let (x, vx, state) = if w.random(0, 1) as u16 != 0 { (-32.0f32, 7.0f32, 2u32) } else { (512.0, -7.0, 3) };
        let y = w.random(0, 90) as u16 as f32;
        let mut ay = 0.5f32;
        let mode = w.random(0, 2) as u16 as u32;
        let mut act = 1u8;
        if mode != 0 {
            if w.random(0, 1) as u16 == 0 {
                act = 0;
            }
        } else {
            ay = 0.2;
        }
        let mut delay = 1u32;
        for i in self.members(gi) {
            let s = Spec { ty: 12, anim: 150, w: 26, h: 26, state, fx: 14, part: -1, aux: 9, snd_hit: 48, snd_death: 3, score: 50 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            self.w32(i, 12, mode);
            self.w8(i, f::B104, act);
            self.w32(i, f::DELAY, delay);
            self.wf(i, f::X, x);
            self.wf(i, f::Y, y);
            self.wf(i, f::VX, vx);
            self.wf(i, f::VY, 0.0);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, ay);
            self.box_from_pos(i);
            delay = delay.wrapping_add(6);
        }
    }

    fn init13<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let step = w.random(6, 8) as u16 as u32;
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let x = w.random(64, 384) as u16 as f32;
        let ax = if w.random(0, 1) as u16 != 0 { 0.5f32 } else { -0.5 };
        let mode = w.random(0, 1) as u16 as u32;
        let act = if w.random(0, 2) as u16 != 0 { 1u8 } else { 0 };
        let mut delay = 1u32;
        for i in self.members(gi) {
            let s = Spec { ty: 13, anim: 150, w: 26, h: 26, state: 1, fx: 14, part: -1, aux: 9, snd_hit: 48, snd_death: 3, score: 50 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            self.w32(i, 12, mode);
            self.w8(i, f::B104, act);
            self.w32(i, f::DELAY, delay);
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -64.0);
            self.wf(i, f::VX, 0.0);
            self.wf(i, f::VY, 7.0);
            self.wf(i, f::AX, ax);
            self.wf(i, f::AY, 0.0);
            self.box_from_pos(i);
            delay = delay.wrapping_add(step);
        }
    }

    /// The aimed shot that types 6, 12 and 13 (and others) fire when their timer runs out; reschedules by the group's interval.
    fn timed_shot<W: EnemyWorld>(&mut self, i: usize, t: u32, w: &mut W) {
        if !(t < self.r32(i, f::TIMER)) {
            let (ex, ey) = self.centre(i);
            let n = Self::pick_target(w, ex, ey);
            let (cx, cy) = self.centre(i);
            w.shot_aimed(0, n as i32, 0, cx, cy);
            let gi = self.r32(i, f::GROUP) as usize;
            let a = self.gr32(gi, g::A64);
            self.w32(i, f::TIMER, t.wrapping_add(a));
        }
    }

    fn upd12<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        self.timed_shot(i, t, w);
        if self.r8(i, f::B104) == 0 {
            if self.rf(i, f::VX) > 0.0 {
                if self.rf(i, f::X) > 336.0 {
                    self.wf(i, f::AX, -0.8);
                    self.w8(i, f::B104, 1);
                }
            } else if self.rf(i, f::X) < 80.0 {
                self.wf(i, f::AX, 0.8);
                self.w8(i, f::B104, 1);
            }
        }
        let v = self.rf(i, f::VX) + self.rf(i, f::AX);
        self.wf(i, f::VX, v);
        let vx = self.rf(i, f::VX);
        if vx > 7.0 {
            self.wf(i, f::VX, 7.0);
        } else if vx < -7.0 {
            self.wf(i, f::VX, -7.0);
        }
        let v = self.rf(i, f::VY) + self.rf(i, f::AY);
        self.wf(i, f::VY, v);
        let vy = self.rf(i, f::VY);
        match self.ri32(i, 12) {
            0 => {
                if vy > 9.0 {
                    self.wf(i, f::VY, 9.0);
                } else if vy < -9.0 {
                    self.wf(i, f::VY, -9.0);
                }
            }
            1 => {
                if vy > 10.0 {
                    self.wf(i, f::VY, 10.0);
                    let a = self.rf(i, f::AY);
                    self.wf(i, f::AY, -a);
                } else if vy < -10.0 {
                    self.wf(i, f::VY, -10.0);
                    let a = self.rf(i, f::AY);
                    self.wf(i, f::AY, -a);
                }
            }
            2 => {
                if vy > 12.0 {
                    self.wf(i, f::VY, 10.0);
                    let a = self.rf(i, f::AY);
                    self.wf(i, f::AY, -a);
                } else if vy < -12.0 {
                    self.wf(i, f::VY, -12.0);
                    let a = self.rf(i, f::AY);
                    self.wf(i, f::AY, -a);
                }
            }
            _ => {}
        }
    }

    fn upd13<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        self.timed_shot(i, t, w);
        if self.r8(i, f::B104) == 0 && self.rf(i, f::Y) > 300.0 {
            self.wf(i, f::AY, -0.8);
            self.w8(i, f::B104, 1);
        }
        let v = self.rf(i, f::VY) + self.rf(i, f::AY);
        self.wf(i, f::VY, v);
        let vy = self.rf(i, f::VY);
        if vy > 7.0 {
            self.wf(i, f::VY, 7.0);
        } else if vy < -7.0 {
            self.wf(i, f::VY, -7.0);
        }
        let v = self.rf(i, f::VX) + self.rf(i, f::AX);
        self.wf(i, f::VX, v);
        let vx = self.rf(i, f::VX);
        let lim = match self.ri32(i, 12) {
            0 => 7.0f32,
            1 => 10.0,
            _ => return,
        };
        if vx > lim {
            self.wf(i, f::VX, lim);
            let a = self.rf(i, f::AX);
            self.wf(i, f::AX, -a);
        } else if vx < -lim {
            self.wf(i, f::VX, -lim);
            let a = self.rf(i, f::AX);
            self.wf(i, f::AX, -a);
        }
    }

    // ---- type 15: 45x48 heavy ships that glide sideways towards the player's column while firing ---------------------------------------
    fn init15<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let mut delay = 1u32;
        for i in self.members(gi) {
            let s = Spec { ty: 15, anim: 138, w: 45, h: 48, state: 1, fx: 14, part: -1, aux: 5, snd_hit: 68, snd_death: 62, score: 200 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            let d = w.random(15, 30) as u16 as u32;
            self.w32(i, f::DELAY, delay.wrapping_add(d));
            let x = w.random(64, 416) as u16 as f32;
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -128.0);
            self.box_from_pos(i);
            self.wf(i, f::VX, 0.0);
            let vy = w.random(3, 4) as u16 as f32;
            self.wf(i, f::VY, vy);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
            delay = delay.wrapping_add(d);
        }
    }

    fn upd15<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        let (n, f31, _) = self.target(w, i);
        let y = self.rf(i, f::Y);
        if !(y > 0.0) {
            return;
        }
        if !(t < self.r32(i, f::TIMER)) {
            let gi = self.r32(i, f::GROUP) as usize;
            let (cx, _) = self.centre(i);
            if w.shot_param([6, 1, 37, n as i32, 1, 0], cx, y) as u8 != 0 {
                self.w32(i, f::F136, 1);
                let x = self.rf(i, f::X) + self.ri32(i, f::HW) as f32;
                self.wf(i, 140, -10.0 + x);
                let y = self.rf(i, f::Y);
                self.wf(i, 144, -10.0 + y);
                let a = self.gr32(gi, g::A64);
                self.w32(i, f::TIMER, t.wrapping_add(a));
            }
        }
        if self.rf(i, f::X) + (self.ri32(i, f::W) as f32) < f31 {
            self.wf(i, f::AX, 0.2);
            if t > self.r32(i, f::T88).wrapping_add(1) {
                let s = self.ri32(i, f::STATE).wrapping_add(1);
                self.wi32(i, f::STATE, s);
                self.w32(i, f::T88, t);
                if s > 6 {
                    if s > 28 {
                        self.wi32(i, f::STATE, 1);
                    }
                } else if s > 5 {
                    self.wi32(i, f::STATE, 5);
                }
            }
        } else {
            self.wf(i, f::AX, -0.2);
            if t > self.r32(i, f::T88).wrapping_add(1) {
                let s = self.ri32(i, f::STATE).wrapping_sub(1);
                self.wi32(i, f::STATE, s);
                self.w32(i, f::T88, t);
                if s >= 23 {
                    if s < 24 {
                        self.wi32(i, f::STATE, 24);
                    }
                } else if s < 1 {
                    self.wi32(i, f::STATE, 28);
                }
            }
        }
        let v = self.rf(i, f::VX) + self.rf(i, f::AX);
        self.wf(i, f::VX, v);
        let vx = self.rf(i, f::VX);
        if vx > 5.0 {
            self.wf(i, f::VX, 5.0);
        } else if vx < -5.0 {
            self.wf(i, f::VX, -5.0);
        }
    }

    // ---- type 16: 32x32 drifters that home in on the player (and flee upwards when nobody is alive) ----------------------------------
    fn init16<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let mut delay = 1u32;
        for i in self.members(gi) {
            let s = Spec { ty: 16, anim: 133, w: 32, h: 32, state: 1, fx: 14, part: -1, aux: 2, snd_hit: 46, snd_death: 47, score: 300 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            let d = w.random(20, 40) as u16 as u32;
            self.w32(i, f::DELAY, delay.wrapping_add(d));
            let x = w.random(64, 480) as u16 as f32;
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -128.0);
            self.box_from_pos(i);
            self.wf(i, f::VX, 0.0);
            self.wf(i, f::VY, 3.0);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
            delay = delay.wrapping_add(d);
        }
    }

    fn upd16<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let _ = w.tick();
        let flee = !(w.ship(1).alive || w.ship(2).alive);
        let (tx, ty) = if flee {
            (w.random(0, 416) as u16 as f32, -200.0f32)
        } else {
            let (_, cx, cy) = self.target(w, i);
            (cx, cy)
        };
        let s = self.ri32(i, f::STATE).wrapping_add(if flee { 2 } else { 1 });
        self.wi32(i, f::STATE, s);
        if s > 40 {
            self.wi32(i, f::STATE, 1);
        }
        self.wf(i, f::AX, if self.rf(i, f::X) + (self.ri32(i, f::W) as f32) < tx { 0.1 } else { -0.1 });
        self.wf(i, f::AY, if self.rf(i, f::Y) + (self.ri32(i, f::H) as f32) < ty { 0.2 } else { -0.2 });
        let v = self.rf(i, f::VX) + self.rf(i, f::AX);
        self.wf(i, f::VX, v);
        let vx = self.rf(i, f::VX);
        if vx > 2.0 {
            self.wf(i, f::VX, 2.0);
        } else if vx < -2.0 {
            self.wf(i, f::VX, -2.0);
        }
        let v = self.rf(i, f::VY) + self.rf(i, f::AY);
        self.wf(i, f::VY, v);
        let lim = if flee { 5.0f32 } else { 2.8 };
        let v = self.rf(i, f::VY) + self.rf(i, f::AY); // (sic: the acceleration is applied twice)
        self.wf(i, f::VY, v);
        if v > lim {
            self.wf(i, f::VY, lim);
        } else if v < -lim {
            self.wf(i, f::VY, -lim);
        }
    }

    // ---- type 17: 32x32 ships that drop in from the top, bounce about, then fly off upwards ---------------------------------------------
    fn init17<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let step = w.random(4, 8) as u16 as u32;
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let x = w.random(90, 326) as u16 as f32;
        let vy = w.random(5, 7) as u16 as f32;
        let ax = if x >= 240.0 { -0.5f32 } else { 0.5 };
        let mode = w.random(0, 1) as u16 as u32;
        let act = if w.random(0, 1) as u16 != 0 { 1u8 } else { 0 };
        let mut delay = 1u32;
        for i in self.members(gi) {
            let s = Spec { ty: 17, anim: 143, w: 32, h: 32, state: 1, fx: 14, part: -1, aux: 0, snd_hit: 46, snd_death: 3, score: 100 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            self.w32(i, 12, mode);
            self.w8(i, f::B104, act);
            self.w32(i, f::DELAY, delay);
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -64.0);
            self.wf(i, f::VX, 0.0);
            self.wf(i, f::VY, vy);
            self.wf(i, f::AX, ax);
            self.wf(i, f::AY, 0.0);
            self.box_from_pos(i);
            delay = delay.wrapping_add(step);
        }
    }

    fn upd17<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        if t > self.r32(i, f::T88) {
            let s = self.ri32(i, f::STATE).wrapping_add(1);
            self.wi32(i, f::STATE, s);
            self.w32(i, f::T88, t);
            if s > 18 {
                self.wi32(i, f::STATE, 1);
            }
        }
        if !(t < self.r32(i, f::TIMER)) {
            let (ex, ey) = self.centre(i);
            let n = Self::pick_target(w, ex, ey);
            let gi = self.r32(i, f::GROUP) as usize;
            let (cx, cy) = self.centre(i);
            w.shot_aimed(4, n as i32, 0, cx, cy);
            let a = self.gr32(gi, g::A64);
            self.w32(i, f::TIMER, t.wrapping_add(a));
        }
        let act = self.r8(i, f::B104) != 0;
        if !act && self.rf(i, f::Y) > 350.0 {
            self.wf(i, f::AY, -0.8);
            self.w8(i, f::B104, 1);
        }
        let v = self.rf(i, f::VY) + self.rf(i, f::AY);
        self.wf(i, f::VY, v);
        let vy = self.rf(i, f::VY);
        if vy > 7.0 {
            self.wf(i, f::VY, 7.0);
        } else if self.r8(i, f::B104) == 0 {
            if vy < -7.0 {
                self.wf(i, f::VY, -7.0);
            }
        } else if vy < -5.0 {
            self.wf(i, f::VY, -5.0);
        }
        let v = self.rf(i, f::VX) + self.rf(i, f::AX);
        self.wf(i, f::VX, v);
        let lim = match (self.r8(i, f::B104) != 0, self.ri32(i, 12)) {
            (false, 0) => 5.0f32,
            (false, 1) => 8.0,
            (true, 0) => 9.0,
            (true, 1) => 12.0,
            _ => return,
        };
        let vx = self.rf(i, f::VX);
        if vx > lim {
            self.wf(i, f::VX, lim);
            let a = self.rf(i, f::AX);
            self.wf(i, f::AX, -a);
        } else if vx < -lim {
            self.wf(i, f::VX, -lim);
            let a = self.rf(i, f::AX);
            self.wf(i, f::AX, -a);
        }
    }

    /// 0x2e8e0: the nearest live ship to (x, y): (ship number, distance, its centre x, its centre y). With no live ship the original leaves
    /// stale registers behind, so callers must check for that first.
    fn nearest<W: EnemyWorld>(w: &W, x: f32, y: f32) -> (u8, f32, f32, f32) {
        let dist = |c: (f32, f32)| -> f32 {
            let dx = c.0 - x;
            let dy = c.1 - y;
            let sq = dx.mul_add(dx, dy * dy);
            (sq as f64).sqrt() as f32
        };
        let mut d1 = 10000.0f32;
        let mut d2 = 10000.0f32;
        let mut c1 = (0.0f32, 0.0f32);
        let mut c2 = (0.0f32, 0.0f32);
        if w.ship(1).alive {
            c1 = Self::ship_centre(w, 1);
            d1 = dist(c1);
        }
        if w.ship(2).alive {
            c2 = Self::ship_centre(w, 2);
            d2 = dist(c2);
        }
        if d1 > d2 { (2, d2, c2.0, c2.1) } else { (1, d1, c1.0, c1.1) }
    }

    // ---- type 18: like 16 (32x32 drifters that home in on the player), a little slower, with a shorter animation cycle ----------------
    fn init18<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let mut delay = 1u32;
        for i in self.members(gi) {
            let s = Spec { ty: 18, anim: 144, w: 32, h: 32, state: 1, fx: 14, part: -1, aux: 2, snd_hit: 48, snd_death: 47, score: 300 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            let d = w.random(20, 40) as u16 as u32;
            self.w32(i, f::DELAY, delay.wrapping_add(d));
            let x = w.random(64, 480) as u16 as f32;
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -128.0);
            self.box_from_pos(i);
            self.wf(i, f::VX, 0.0);
            self.wf(i, f::VY, 2.5);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
            delay = delay.wrapping_add(d);
        }
    }

    fn upd18<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let _ = w.tick();
        let flee = !(w.ship(1).alive || w.ship(2).alive);
        let (tx, ty) = if flee {
            (w.random(0, 416) as u16 as f32, -200.0f32)
        } else {
            let (_, cx, cy) = self.target(w, i);
            (cx, cy)
        };
        let s = self.ri32(i, f::STATE).wrapping_add(if flee { 2 } else { 1 });
        self.wi32(i, f::STATE, s);
        if s > 18 {
            self.wi32(i, f::STATE, 1);
        }
        self.wf(i, f::AX, if self.rf(i, f::X) + (self.ri32(i, f::W) as f32) < tx { 0.1 } else { -0.1 });
        self.wf(i, f::AY, if self.rf(i, f::Y) + (self.ri32(i, f::H) as f32) < ty { 0.2 } else { -0.2 });
        let v = self.rf(i, f::VX) + self.rf(i, f::AX);
        self.wf(i, f::VX, v);
        let vx = self.rf(i, f::VX);
        if vx > 2.0 {
            self.wf(i, f::VX, 2.0);
        } else if vx < -2.0 {
            self.wf(i, f::VX, -2.0);
        }
        let v = self.rf(i, f::VY) + self.rf(i, f::AY);
        self.wf(i, f::VY, v);
        let lim = if flee { 5.0f32 } else { 2.6 };
        if v > lim {
            self.wf(i, f::VY, lim);
        } else if v < -lim {
            self.wf(i, f::VY, -lim);
        }
    }

    // ---- type 19: 64x64 gunships that chase the nearest player, turn their turret towards him and shoot --------------------------------
    fn init19<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let mut delay = 1u32;
        for i in self.members(gi) {
            let s = Spec { ty: 19, anim: 130, w: 64, h: 64, state: 1, fx: 14, part: -1, aux: 5, snd_hit: 48, snd_death: 3, score: 400 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            let d = w.random(20, 40) as u16 as u32;
            self.w32(i, f::DELAY, delay.wrapping_add(d));
            let x = w.random(64, 352) as u16 as f32;
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -128.0);
            let (x, y) = (self.rf(i, f::X), self.rf(i, f::Y));
            self.wf(i, f::BX0, 6.0 + x);
            self.wf(i, f::BY0, 6.0 + y);
            self.wf(i, f::BX1, -6.0 + (x + self.ri32(i, f::W) as f32));
            self.wf(i, f::BY1, -6.0 + (y + self.ri32(i, f::H) as f32));
            self.wf(i, f::VX, 0.0);
            let vy = w.random(3, 4) as u16 as f32;
            self.wf(i, f::VY, vy);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
            delay = delay.wrapping_add(d);
        }
    }

    fn upd19<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        let (cx, cy) = self.centre(i);
        let (n, dist, tx, ty, flee);
        if !(w.ship(1).alive || w.ship(2).alive) {
            n = 1u8;
            dist = 0.0f32;
            tx = 208.0f32;
            ty = 600.0f32;
            flee = true;
        } else {
            let r = Self::nearest(w, cx, cy);
            n = r.0;
            dist = r.1;
            tx = r.2;
            ty = r.3;
            flee = false;
        }
        let wd = self.ri32(i, f::W) as f32;
        let ht = self.ri32(i, f::H) as f32;
        if dist > 140.0 || flee {
            let a = if flee { 0.3f32 } else { 0.1 };
            self.wf(i, f::AX, if self.rf(i, f::X) + wd < tx { a } else { -a });
            self.wf(i, f::AY, if self.rf(i, f::Y) + ht < ty { a } else { -a });
        } else {
            self.wf(i, f::AX, if self.rf(i, f::X) + wd < tx { -0.3 } else { 0.3 });
            self.wf(i, f::AY, if self.rf(i, f::Y) + ht < ty { -0.3 } else { 0.3 });
        }
        let v = self.rf(i, f::VX) + self.rf(i, f::AX);
        self.wf(i, f::VX, v);
        if v > 4.0 {
            self.wf(i, f::VX, 4.0);
        } else if v < -4.0 {
            self.wf(i, f::VX, -4.0);
        }
        let lim = if flee { 6.0f32 } else { 4.0 };
        let v = self.rf(i, f::VY) + self.rf(i, f::AY);
        self.wf(i, f::VY, v);
        if v > lim {
            self.wf(i, f::VY, lim);
        } else if v < -lim {
            self.wf(i, f::VY, -lim);
        }
        let step = match self.turret_turn(i, cx, cy, tx, ty) {
            Some(st) => st,
            None => return,
        };
        if t < self.r32(i, f::TIMER) {
            return;
        }
        if !self.turret_in_range(cx, cy) {
            return;
        }
        let (ox, oy) = ENEMY_TURRET[(step - 1) as usize];
        let sx = oy as f32 + self.rf(i, f::X);
        let sy = ox as f32 + self.rf(i, f::Y);
        if w.shot_param([3, 1, 51, n as i32, Self::turret_arg(step), 0], 10.0 + sx, 10.0 + sy) as u8 != 0 {
            self.w32(i, f::F136, 1);
            self.wf(i, 140, sx);
            self.wf(i, 144, sy);
            let gi = self.r32(i, f::GROUP) as usize;
            let a = self.gr32(gi, g::A64);
            self.w32(i, f::TIMER, t.wrapping_add(a));
        }
    }

    /// The turret logic shared by the types 19, 20 and 23: the aim step (1..24) towards (tx, ty) as seen from the centre (cx, cy); the
    /// turret (+84) turns one step the short way round; returns the step when the turret already points that way.
    fn turret_turn(&mut self, i: usize, cx: f32, cy: f32, tx: f32, ty: f32) -> Option<i32> {
        let ang = angle_between(&self.arctan, f2i(cx), f2i(cy), f2i(tx), f2i(ty));
        let mut step = ((ang as i16) / 15 + 1) as i32;
        if step < 1 {
            step = 24;
        } else if step > 24 {
            step = 1;
        }
        let st = self.ri32(i, f::STATE);
        if st == step {
            return Some(step);
        }
        if st < step {
            let d = (st - step).abs();
            let other = (d - 24).abs();
            let d2 = (step - st).abs();
            if d2 < other {
                let s = st.wrapping_add(1);
                self.wi32(i, f::STATE, s);
                if s > 24 {
                    self.wi32(i, f::STATE, 1);
                }
            } else {
                let s = st.wrapping_sub(1);
                self.wi32(i, f::STATE, s);
                if s < 1 {
                    self.wi32(i, f::STATE, 24);
                }
            }
        } else {
            self.wi32(i, f::STATE, st.wrapping_sub(1));
        }
        None
    }

    fn turret_in_range(&self, cx: f32, cy: f32) -> bool {
        cx > 64.0 && cx < 352.0 && cy > 64.0 && cy < 416.0
    }

    fn turret_arg(step: i32) -> i32 {
        if step > 12 { step - 12 } else { step }
    }

    // ---- type 20: 64x64 gunships (slower, burst fire of three shots) ----------------------------------------------------------------------
    fn init20<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let mut delay = 1u32;
        for i in self.members(gi) {
            let s = Spec { ty: 20, anim: 153, w: 64, h: 64, state: 1, fx: 14, part: 11, aux: 2, snd_hit: 61, snd_death: 62, score: 400 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a.wrapping_add(60)));
            let d = w.random(30, 50) as u16 as u32;
            self.w32(i, f::DELAY, delay.wrapping_add(d));
            let x = w.random(64, 352) as u16 as f32;
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -128.0);
            let (x, y) = (self.rf(i, f::X), self.rf(i, f::Y));
            self.wf(i, f::BX0, 8.0 + x);
            self.wf(i, f::BY0, 8.0 + y);
            self.wf(i, f::BX1, -8.0 + (x + self.ri32(i, f::W) as f32));
            self.wf(i, f::BY1, -8.0 + (y + self.ri32(i, f::H) as f32));
            self.wf(i, f::VX, 0.0);
            let vy = w.random(2, 3) as u16 as f32;
            self.wf(i, f::VY, vy);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
            delay = delay.wrapping_add(d);
        }
    }

    /// The ship the gunship types 19/20/23 head for: (flee, ship number, distance, target x, target y); `flee_y` is the y they run to when no
    /// ship is alive.
    fn gunship_target<W: EnemyWorld>(w: &W, cx: f32, cy: f32, flee_y: f32) -> (bool, u8, f32, f32, f32) {
        if !(w.ship(1).alive || w.ship(2).alive) {
            (true, 1, 0.0, 208.0, flee_y)
        } else {
            let r = Self::nearest(w, cx, cy);
            (false, r.0, r.1, r.2, r.3)
        }
    }

    fn upd20<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        let (cx, cy) = self.centre(i);
        let (flee, n, dist, tx, ty) = Self::gunship_target(w, cx, cy, -200.0);
        let wd = self.ri32(i, f::W) as f32;
        let ht = self.ri32(i, f::H) as f32;
        if dist > 160.0 || flee {
            let a = if flee { 0.3f32 } else { 0.1 };
            self.wf(i, f::AX, if self.rf(i, f::X) + wd < tx { a } else { -a });
            self.wf(i, f::AY, if self.rf(i, f::Y) + ht < ty { a } else { -a });
        } else {
            self.wf(i, f::AX, if self.rf(i, f::X) + wd < tx { -0.2 } else { 0.2 });
            self.wf(i, f::AY, if self.rf(i, f::Y) + ht < ty { -0.2 } else { 0.2 });
        }
        let v = self.rf(i, f::VX) + self.rf(i, f::AX);
        self.wf(i, f::VX, v);
        if v > 2.0 {
            self.wf(i, f::VX, 2.0);
        } else if v < -2.0 {
            self.wf(i, f::VX, -2.0);
        }
        let lim = if flee { 6.0f32 } else { 2.0 };
        let v = self.rf(i, f::VY) + self.rf(i, f::AY);
        self.wf(i, f::VY, v);
        if v > lim {
            self.wf(i, f::VY, lim);
        } else if v < -lim {
            self.wf(i, f::VY, -lim);
        }
        let step = match self.turret_turn(i, cx, cy, tx, ty) {
            Some(st) => st,
            None => return,
        };
        if t < self.r32(i, f::TIMER) || !self.turret_in_range(cx, cy) {
            return;
        }
        let tm = self.r32(i, f::TIMER);
        let arg = [4, 1, 60, n as i32, Self::turret_arg(step), 0];
        if t == tm {
            w.shot_param(arg, cx, cy);
        } else if t == tm.wrapping_add(4) {
            w.shot_param(arg, cx, cy);
        } else if !(t < tm.wrapping_add(8)) {
            w.shot_param(arg, cx, cy);
            let gi = self.r32(i, f::GROUP) as usize;
            let a = self.gr32(gi, g::A64);
            self.w32(i, f::TIMER, t.wrapping_add(a));
        }
    }

    // ---- type 23: 58x58 gunships that circle in on the nearest player and fire bursts -------------------------------------------------------
    fn init23<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let mut delay = 1u32;
        for i in self.members(gi) {
            let s = Spec { ty: 23, anim: 157, w: 58, h: 58, state: 1, fx: 14, part: 10, aux: 2, snd_hit: 58, snd_death: 0, score: 500 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a.wrapping_add(60)));
            let d = w.random(30, 50) as u16 as u32;
            self.w32(i, f::DELAY, delay.wrapping_add(d));
            let x = w.random(64, 352) as u16 as f32;
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -128.0);
            let (x, y) = (self.rf(i, f::X), self.rf(i, f::Y));
            self.wf(i, f::BX0, 2.0 + x);
            self.wf(i, f::BY0, 2.0 + y);
            self.wf(i, f::BX1, -2.0 + (x + self.ri32(i, f::W) as f32));
            self.wf(i, f::BY1, -2.0 + (y + self.ri32(i, f::H) as f32));
            self.wf(i, f::VX, 0.0);
            let vy = w.random(2, 3) as u16 as f32;
            self.wf(i, f::VY, vy);
            self.wf(i, f::AX, 0.0);
            self.wf(i, f::AY, 0.0);
            delay = delay.wrapping_add(d);
        }
    }

    fn upd23<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        let (cx, cy) = self.centre(i);
        let (flee, n, dist, tx, ty) = Self::gunship_target(w, cx, cy, 600.0);
        let mut a = 0.1f32;
        let mut late = false;
        if !flee && t > self.r32(i, f::SPAWN_TICK).wrapping_add(300) {
            a = 0.3;
            late = true;
        }
        let range = if late { 120.0f32 } else { 240.0 };
        // (the original's "inside the arena" flag: its two comparisons can never both hold, so it is only ever set for NaN; kept literally)
        let y = self.rf(i, f::Y);
        let mut r4 = true;
        if y > 80.0 {
            r4 = false;
        } else if y < 380.0 {
            r4 = false;
        }
        let wd = self.ri32(i, f::W) as f32;
        let ht = self.ri32(i, f::H) as f32;
        if dist > range || r4 || flee {
            if flee {
                a = 0.4;
            }
            self.wf(i, f::AX, if self.rf(i, f::X) + wd < tx { a } else { -a });
            self.wf(i, f::AY, if self.rf(i, f::Y) + ht < ty { a } else { -a });
        } else if r4 {
            self.wf(i, f::AX, if self.rf(i, f::X) + wd < tx { 0.2 } else { -0.2 });
            self.wf(i, f::AY, if self.rf(i, f::Y) + ht < ty { 0.2 } else { -0.2 });
        }
        let lim = if flee { 6.0f32 } else if late { 4.0 } else { 2.0 };
        let v = self.rf(i, f::VX) + self.rf(i, f::AX);
        self.wf(i, f::VX, v);
        if v > lim {
            self.wf(i, f::VX, lim);
        } else if v < -lim {
            self.wf(i, f::VX, -lim);
        }
        let v = self.rf(i, f::VY) + self.rf(i, f::AY);
        self.wf(i, f::VY, v);
        if v > lim {
            self.wf(i, f::VY, lim);
        } else if v < -lim {
            self.wf(i, f::VY, -lim);
        }
        let step = match self.turret_turn(i, cx, cy, tx, ty) {
            Some(st) => st,
            None => return,
        };
        if t < self.r32(i, f::TIMER) || !self.turret_in_range(cx, cy) {
            return;
        }
        let tm = self.r32(i, f::TIMER);
        let arg = [4, 1, 70, n as i32, Self::turret_arg(step), 0];
        if t == tm {
            w.shot_param(arg, cx, cy);
        } else if t == tm.wrapping_add(5) {
            w.shot_param(arg, cx, cy);
        } else if !(t < tm.wrapping_add(10)) {
            w.shot_param(arg, cx, cy);
            let gi = self.r32(i, f::GROUP) as usize;
            let a = self.gr32(gi, g::A64);
            self.w32(i, f::TIMER, t.wrapping_add(a));
        }
    }

    // ---- type 21: like type 5 (40x40 ships that drop in beside the player, steer in and loop out) but faster -------------------------------
    fn init21<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let a = self.gr32(gi, g::A64);
        let hp = self.gr32(gi, g::B68);
        let hard = (w.difficulty() as i32) > 2;
        let mut delay = 1u32;
        for i in self.members(gi) {
            self.w32(i, 12, hard as u32);
            let s = Spec { ty: 21, anim: 142, w: 40, h: 40, state: 1, fx: -1, part: 10, aux: 2, snd_hit: 55, snd_death: 27, score: 50 };
            self.init_common(i, gi, tick, hp, &s);
            self.w32(i, f::TIMER, tick.wrapping_add(a));
            let d = w.random(5, 20) as u16 as u32;
            self.w32(i, f::DELAY, delay.wrapping_add(d));
            delay = delay.wrapping_add(d);
            let x = if w.ship(1).x < 208.0 { w.random(308, 416) as u16 } else { w.random(64, 108) as u16 };
            self.wf(i, f::X, x as f32);
            self.wf(i, f::Y, -128.0);
            self.box_from_pos(i);
            let (cx, cy) = Self::ship_centre(w, 1);
            let (ex, ey) = self.centre(i);
            let (vx, vy) = Self::aim(cx - ex, cy - ey, 6.0);
            self.wf(i, f::VX, vx);
            self.wf(i, f::VY, vy);
        }
    }

    fn upd21<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        let (_n, f31, f2) = self.target(w, i);
        if t > self.r32(i, f::T88) {
            self.w32(i, f::T88, t);
            if self.rf(i, f::VX) < 0.0 {
                let s = self.ri32(i, f::STATE).wrapping_add(3);
                self.wi32(i, f::STATE, s);
                if s > 9 {
                    self.wi32(i, f::STATE, 1);
                }
            } else {
                let s = self.ri32(i, f::STATE).wrapping_sub(3);
                self.wi32(i, f::STATE, s);
                if s < 1 {
                    self.wi32(i, f::STATE, 9);
                }
            }
        }
        if self.r8(i, f::B104) == 0 {
            let mut r5 = false;
            let mut r6 = false;
            let y = self.rf(i, f::Y);
            if y > 200.0 && (y as f64) < 60.0f64 + f2 as f64 {
                r5 = true;
            }
            if self.rf(i, f::Y) > 320.0 {
                r5 = true;
                r6 = true;
            }
            if r5 {
                let x = self.rf(i, f::X);
                if self.rf(i, f::VX) > 0.0 {
                    if x + self.ri32(i, f::W) as f32 > f31 || r6 {
                        self.wf(i, f::AX, -0.4);
                        self.wf(i, f::AY, -0.4);
                        self.w8(i, f::B104, 1);
                    }
                } else if x < f31 || r6 {
                    self.wf(i, f::AX, 0.4);
                    self.wf(i, f::AY, -0.4);
                    self.w8(i, f::B104, 1);
                }
            }
        } else {
            self.accelerate(i);
            self.clamp_vel(i);
            if self.r32(i, 12) == 1 && self.rf(i, f::Y) < 180.0 {
                let x = self.rf(i, f::X);
                if self.rf(i, f::VX) > 0.0 {
                    if x + self.ri32(i, f::W) as f32 > f31 {
                        self.wf(i, f::AX, -0.4);
                        self.wf(i, f::AY, 0.9);
                        self.w8(i, f::B104, 1);
                    }
                } else if x < f31 {
                    self.wf(i, f::AX, 0.4);
                    self.wf(i, f::AY, 0.9);
                    self.w8(i, f::B104, 1);
                }
            }
        }
    }

    // ---- type 3: 56x56 drop-in blocks falling from above, hit box pulses -------------------------------------------------------------
    fn init3<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        let tick = w.tick();
        let hp = self.gr32(gi, g::B68);
        let mut delay = 0u32;
        for i in self.members(gi) {
            let s = Spec { ty: 3, anim: 141, w: 56, h: 56, state: 1, fx: 14, part: 12, aux: 5, snd_hit: 48, snd_death: 47, score: 1 };
            self.init_common(i, gi, tick, hp, &s);
            let d = w.random(20, 30) as u16 as u32;
            self.w32(i, f::DELAY, delay.wrapping_add(d));
            let x = (w.random(0, 416) as u16) as f32;
            self.wf(i, f::X, x);
            self.wf(i, f::Y, -128.0);
            let x = self.rf(i, f::X);
            self.wf(i, f::BX0, 2.0 + x);
            let y = self.rf(i, f::Y);
            self.wf(i, f::BY0, y);
            let x = self.rf(i, f::X);
            self.wf(i, f::BX1, -2.0 + (x + (self.ri32(i, f::W) as f32)));
            let y = self.rf(i, f::Y);
            self.wf(i, f::BY1, y + (self.ri32(i, f::H) as f32));
            self.wf(i, f::VX, 0.0);
            self.wf(i, f::VY, 2.0);
            delay = delay.wrapping_add(d);
        }
    }

    fn upd3<W: EnemyWorld>(&mut self, i: usize, w: &mut W) {
        let t = w.tick();
        if t < self.r32(i, f::TIMER).wrapping_add(4) {
            return;
        }
        let s = self.r32(i, f::STATE).wrapping_add(1);
        self.w32(i, f::STATE, s);
        if (s as i32) > 16 {
            self.w32(i, f::STATE, 1);
        }
        self.w32(i, f::TIMER, t);
        let (a, b) = match self.r32(i, f::STATE) {
            1 | 9 => (2.0f32, -2.0f32),
            2 | 8 | 10 | 16 => (3.0, -3.0),
            3 | 7 | 11 | 15 => (7.0, -7.0),
            4 | 6 | 12 | 14 => (15.0, -15.0),
            5 | 13 => (24.0, -24.0),
            _ => return,
        };
        let y = self.rf(i, f::Y);
        self.wf(i, f::BY0, a + y);
        let h = self.ri32(i, f::H) as f32;
        self.wf(i, f::BY1, b + (y + h));
    }
}

#[allow(dead_code)]
fn _unused(x: f32) -> i32 { f2i(x) }

/// D.1044 (filled in by the level set-up code at 0x2d580): per turret step (0..24) the (a, b) offsets used for the shot position of type 19/20/23.
pub const ENEMY_TURRET: [(i16, i16); 24] = [
    (-4, 23), (-5, 30), (-6, 39), (1, 45), (8, 50), (15, 56), (23, 54), (33, 55), (43, 53), (49, 49), (54, 41), (59, 34),
    (58, 23), (58, 14), (54, 5), (50, -2), (42, -10), (35, -12), (23, -15), (15, -12), (7, -10), (-1, -3), (-5, 3), (-9, 14),
];
