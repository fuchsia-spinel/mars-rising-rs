//! Boss structures: the large multi-part enemies ("capital ships", types 2, 4 and 5 of the level data's special groups, whose group record has a
//! non-zero word at +88).
//!
//! A boss is a 228-byte record in a pool of 10 (`D.109c`, count `D.1abc`) that owns 2..5 ordinary enemy slots (its parts, flagged `+168 = 1` and
//! `+172 = boss index`). The record moves as a whole (0x4127c copies its velocity to every part) and the parts are drawn by 0x417a4.
//!
//! * level load (0x2d7a8): every group with word 22 != 0 is *registered* (0x40dc4: copies type / hit points / bonus fields into a free record and
//!   remembers the group index in `+168`); the main loop set-up then frees all records again (0xd9b0 -> 0x40d6c), which only clears the "in use" byte,
//!   so the registrations stay in the records;
//! * the map's spawn trigger (0x2d9bc) calls 0x40e7c with the group index: it finds the registered record, reserves free enemy slots for the parts
//!   (type 1: 4, type 2: 5, types 3/4/5: 2) and calls the type's init routine (0x433e0 / 0x43bcc / 0x44048, types 1 and 3 have none and cancel the spawn);
//! * every frame 0x4127c runs the type routine (0x42344 / 0x4274c / 0x42f48), moves the parts and tests them against the ships (body contact hurts the ship
//!   and the part); 0x417a4 draws and culls; 0x41c88 is the damage routine for parts (the part flagged `+169` carries the boss's hit points);
//! * 0x4123c is the audio callback that asks for the looping alarm to be started again.
//!
//! Verified against the original executable by `tests/boss_golden.rs` (vectors from `tools/re/gen_boss_vectors.py`): the whole boss pool, the enemy pool and
//! group table and every call made to the surroundings must be identical after each routine.
//!
//! Records are raw bytes with the original offsets (named in [`b`]). One value comes from outside the data: type 4's target choice (0x2e7a8) is called
//! with the caller's stale `f31` as the position; this port uses 0.0 (the emulator's value).

use crate::draw::{DrawOpts, Prim};
use crate::enemy::{f, f2i, Enemies, EnemyWorld, SLOTS};
use crate::enemy_draw::EnemyDrawWorld;

pub const BOSSES: usize = 10;
pub const REC: usize = 228;

/// Offsets inside a boss record.
pub mod b {
    pub const ACTIVE: usize = 0; // u8
    pub const TYPE: usize = 4;
    pub const SPAWN: usize = 8; // tick of the spawn
    pub const MEMBERS: usize = 12; // 8 x i32 enemy slot indices (-1: none)
    pub const X: usize = 44; // f32 position (top left of the whole structure)
    pub const Y: usize = 48;
    pub const VX: usize = 52;
    pub const VY: usize = 56;
    pub const AX: usize = 60;
    pub const AY: usize = 64;
    pub const HW: usize = 68; // f32 half size (centre = position + half size)
    pub const HH: usize = 72;
    pub const W: usize = 76; // u32 size
    pub const H: usize = 80;
    pub const STATE: usize = 84;
    pub const PHASE: usize = 96;
    pub const FLAG: usize = 100; // u8
    pub const INTERVAL: usize = 104; // converted `+64` of the group (shot interval)
    pub const HP: usize = 108; // converted `+68` of the group + level * 10
    pub const DMG: usize = 112;
    pub const HIT: usize = 117; // u8: hit flash
    pub const HIT_TICK: usize = 120;
    pub const SHOT_TICK: usize = 124;
    pub const DEATH_AUX: usize = 128; // u8
    pub const PARTICLE: usize = 132; // i32 particle burst id of the death (-1: none)
    pub const PARTS: usize = 136;
    pub const ALIVE: usize = 140;
    pub const KILLED: usize = 144;
    pub const BONUS_KIND: usize = 148; // group `+80`
    pub const BONUS_N: usize = 152; // group `+76`
    pub const ANG: usize = 156; // f32 (type 2: rotation of the satellites)
    pub const RAD: usize = 160;
    pub const DRAD: usize = 164;
    pub const GROUP: usize = 168; // group index it was registered for
    pub const OVERLAY: usize = 172; // u8
    pub const OV_ANIM: usize = 176;
    pub const OV_FRAME: usize = 180;
    pub const OV_X: usize = 184; // f32
    pub const OV_Y: usize = 188;
    pub const SND_HIT: usize = 192; // u8 sound when hit
    pub const SND_LOOP: usize = 193; // u8 looping alarm sound
    pub const LOOP_ON: usize = 194; // u8
    pub const RESTART: usize = 195; // u8 set by the audio callback
    pub const HANDLE: usize = 196;
}

#[derive(Debug, Clone)]
pub struct Bosses {
    pub rec: Vec<u8>,
    /// `D.1abc`
    pub count: i32,
}

impl Default for Bosses {
    fn default() -> Self { Self::new() }
}

impl Bosses {
    pub fn new() -> Self { Bosses { rec: vec![0; BOSSES * REC], count: 0 } }
    #[inline] pub fn r8(&self, i: usize, o: usize) -> u8 { self.rec[i * REC + o] }
    #[inline] pub fn w8(&mut self, i: usize, o: usize, v: u8) { self.rec[i * REC + o] = v }
    #[inline] pub fn r32(&self, i: usize, o: usize) -> u32 {
        let a = i * REC + o;
        u32::from_be_bytes([self.rec[a], self.rec[a + 1], self.rec[a + 2], self.rec[a + 3]])
    }
    #[inline] pub fn ri32(&self, i: usize, o: usize) -> i32 { self.r32(i, o) as i32 }
    #[inline] pub fn w32(&mut self, i: usize, o: usize, v: u32) {
        let a = i * REC + o;
        self.rec[a..a + 4].copy_from_slice(&v.to_be_bytes());
    }
    #[inline] pub fn wi32(&mut self, i: usize, o: usize, v: i32) { self.w32(i, o, v as u32) }
    #[inline] pub fn rf(&self, i: usize, o: usize) -> f32 { f32::from_bits(self.r32(i, o)) }
    #[inline] pub fn wf(&mut self, i: usize, o: usize, v: f32) { self.w32(i, o, v.to_bits()) }

    /// 0x40db0: free a record (only the "in use" byte).
    pub fn free(&mut self, i: usize) { self.w8(i, b::ACTIVE, 0) }

    /// 0x40d6c: free all records and clear the counter.
    pub fn free_all(&mut self) {
        for i in 0..BOSSES { self.free(i); }
        self.count = 0;
    }

    /// 0x4123c: the audio callback of the looping alarm (`sel` 1 = ended, 2 = stopped; `param` = boss index): ask the update to start it again.
    pub fn sound_ended(&mut self, sel: i32, param: i32) {
        if (sel == 1 || sel == 2) && (0..BOSSES as i32).contains(&param) {
            self.w8(param as usize, b::RESTART, 1);
        }
    }

    /// The member slot `k` of boss `i` (`-1` for none; slot numbers outside the pool also count as none).
    fn member(&self, i: usize, k: usize) -> Option<usize> {
        let e = self.ri32(i, b::MEMBERS + 4 * k);
        if (0..SLOTS as i32).contains(&e) { Some(e as usize) } else { None }
    }
}

/// `int` to `float`, as the original's `fsubs (magic) ` conversion does it.
#[inline]
fn i2f(v: i32) -> f32 { v as f32 }

impl Enemies {
    /// 0x40dc4: register group `gi` in the first free boss record; returns its index or -1.
    pub fn boss_register(&mut self, gi: usize) -> i32 {
        for i in 0..BOSSES {
            if self.bosses.r8(i, b::ACTIVE) != 0 {
                continue;
            }
            self.bosses.w8(i, b::ACTIVE, 1);
            let (ty, hp, n, kind, iv) = (self.gr32(gi, 44), self.gr32(gi, 68), self.gr32(gi, 76), self.gr32(gi, 80), self.gr32(gi, 64));
            self.bosses.w32(i, b::TYPE, ty);
            self.bosses.w32(i, b::HP, hp);
            self.bosses.w32(i, b::BONUS_N, n);
            self.bosses.w32(i, b::BONUS_KIND, kind);
            self.bosses.w32(i, b::INTERVAL, iv);
            self.bosses.w32(i, b::GROUP, gi as u32);
            return i as i32;
        }
        -1
    }

    /// 0x40e7c: launch the boss registered for group `gi`.
    pub fn boss_spawn<W: EnemyWorld>(&mut self, gi: i32, w: &mut W) {
        let tick = w.tick();
        let Some(bi) = (0..BOSSES).find(|&i| self.bosses.ri32(i, b::GROUP) == gi) else { return };
        if self.bosses.r8(bi, b::ACTIVE) != 0 {
            return;
        }
        let ty = self.bosses.ri32(bi, b::TYPE);
        let need: usize = match ty {
            1 => 4,
            2 => 5,
            3 | 4 | 5 => 2,
            _ => return,
        };
        let mut found = [-1i32; 8];
        let mut n = 0usize;
        let mut ok = false;
        for s in 0..SLOTS {
            if self.r8(s, f::ACTIVE) == 0 {
                found[n] = s as i32;
                n += 1;
                if n == need {
                    ok = true;
                    break;
                }
            }
        }
        if !ok {
            return;
        }
        let bs = &mut self.bosses;
        bs.w8(bi, b::ACTIVE, 1);
        bs.wi32(bi, b::TYPE, ty);
        bs.wi32(bi, b::PARTS, need as i32);
        bs.wi32(bi, b::ALIVE, need as i32);
        bs.wi32(bi, b::KILLED, 0);
        bs.w8(bi, b::FLAG, 0);
        for k in 0..8 {
            bs.wi32(bi, b::MEMBERS + 4 * k, -1);
        }
        for k in 0..need {
            bs.wi32(bi, b::MEMBERS + 4 * k, found[k]);
        }
        let hp = Enemies::convert_b(bs.r32(bi, b::HP));
        bs.w32(bi, b::HP, hp);
        let hp = bs.ri32(bi, b::HP).wrapping_add((w.difficulty() as i32).wrapping_mul(10));
        bs.wi32(bi, b::HP, hp);
        let iv = Enemies::convert_a(bs.r32(bi, b::INTERVAL));
        bs.w32(bi, b::INTERVAL, iv);
        bs.w8(bi, b::OVERLAY, 0);
        match ty {
            2 => self.boss_init2(bi, tick, w),
            4 => self.boss_init4(bi, tick, w),
            5 => self.boss_init5(bi, tick, w),
            _ => {
                self.bosses.w8(bi, b::ACTIVE, 0);
                return;
            }
        }
        w.sound(52, 4, 0);
        w.sound(52, 4, 40);
        let id = self.bosses.r8(bi, b::SND_LOOP);
        if (id as i8) >= 0 {
            if self.bosses.r8(bi, b::LOOP_ON) != 0 {
                let h = w.sound_loop(id as i32, 4, bi as i32);
                self.bosses.w32(bi, b::HANDLE, h);
            } else {
                w.sound(id as i32, 4, 5);
            }
        }
        self.bosses.count = self.bosses.count.wrapping_add(1);
    }

    /// The fields every type's init sets in the record before the parts (everything except size, position, motion and sounds).
    fn boss_init_common(&mut self, bi: usize, tick: u32) {
        let bs = &mut self.bosses;
        bs.w32(bi, b::SPAWN, tick);
        bs.w32(bi, b::SHOT_TICK, tick);
        bs.w32(bi, b::DMG, 0);
        bs.w32(bi, b::HIT_TICK, tick);
        bs.w8(bi, b::HIT, 0);
        bs.w8(bi, 116, 0);
        bs.w32(bi, b::PHASE, tick);
        bs.w32(bi, b::STATE, 0);
        let parts = bs.r32(bi, b::PARTS);
        bs.w32(bi, b::ALIVE, parts);
        bs.w32(bi, b::KILLED, 0);
    }

    /// The common part of every boss part's init (an enemy slot).
    fn boss_part_common(&mut self, e: usize, bi: usize, tick: u32, main: u8) {
        self.w8(e, f::BOSS, 1);
        self.wi32(e, f::BOSS_IDX, bi as i32);
        self.w8(e, f::B169, main);
        self.w8(e, f::ACTIVE, 1);
        self.w32(e, f::SPAWN_TICK, tick);
        self.w32(e, f::TIMER, tick);
        self.w32(e, f::DMG, 0);
        self.w32(e, f::HIT_TICK, tick);
        self.w8(e, f::HIT, 0);
        self.wi32(e, f::GROUP, -1);
        self.w8(e, f::B104, 0);
        self.w32(e, f::T88, tick);
        self.w32(e, f::F136, 0);
        self.w32(e, f::TYPE, 1);
        self.w32(e, f::F16, 1);
        self.wi32(e, f::PART_DEATH, -1);
        self.w32(e, f::DELAY, 0);
    }

    /// Hit box of slot `e` from its position and size.
    fn boss_part_box(&mut self, e: usize) {
        let (x, y) = (self.rf(e, f::X), self.rf(e, f::Y));
        self.wf(e, f::BX0, x);
        self.wf(e, f::BY0, y);
        let wd = i2f(self.ri32(e, f::W));
        self.wf(e, f::BX1, x + wd);
        let ht = i2f(self.ri32(e, f::H));
        self.wf(e, f::BY1, y + ht);
    }

    /// The boss's velocity (and acceleration) goes into the part (`+32..+44`).
    fn boss_part_motion(&mut self, e: usize, bi: usize) {
        for (to, from) in [(f::VX, b::VX), (f::VY, b::VY), (f::AX, b::AX), (f::AY, b::AY)] {
            let v = self.bosses.r32(bi, from);
            self.w32(e, to, v);
        }
    }

    /// Start position, speed and steering shared by the three inits: `x` random across the playfield, above the top edge, falling at 3 or 5 per frame.
    fn boss_start<W: EnemyWorld>(&mut self, bi: usize, w: &mut W, wd: i32, vy: f32, ax_pos: f32, ax_neg: f32) {
        let r = w.random(32, 416 - wd);
        let bs = &mut self.bosses;
        bs.wf(bi, b::X, (r as u16) as f32);
        bs.wf(bi, b::Y, -128.0);
        bs.wf(bi, b::VX, 0.0);
        bs.wf(bi, b::VY, vy);
        let c = bs.rf(bi, b::X) + bs.rf(bi, b::HW);
        bs.wf(bi, b::AX, if c > 208.0 { ax_neg } else { ax_pos });
    }

    /// 0x433e0: type 2 (the body with four orbiting satellites).
    fn boss_init2<W: EnemyWorld>(&mut self, bi: usize, tick: u32, w: &mut W) {
        self.boss_init_common(bi, tick);
        let bs = &mut self.bosses;
        bs.w32(bi, b::W, 58);
        bs.w32(bi, b::H, 58);
        bs.wf(bi, b::HW, 29.0);
        bs.wf(bi, b::HH, 29.0);
        bs.wi32(bi, b::PARTICLE, 12);
        bs.w8(bi, b::DEATH_AUX, 0);
        bs.w8(bi, b::SND_HIT, 55);
        bs.w8(bi, b::SND_LOOP, 12);
        bs.w8(bi, b::LOOP_ON, 1);
        bs.w8(bi, b::RESTART, 0);
        bs.w32(bi, b::HANDLE, 0);
        // x = random 32..(416 - width), y above the screen, falling at 5; vx accelerates towards the middle
        let r = w.random(32, 416 - 58);
        let bs = &mut self.bosses;
        bs.wf(bi, b::X, (r as u16) as f32);
        bs.wf(bi, b::Y, -128.0);
        bs.wf(bi, b::VX, 0.0);
        bs.wf(bi, b::VY, 5.0);
        let c = bs.rf(bi, b::X) + bs.rf(bi, b::HW);
        bs.wf(bi, b::AX, if c > 208.0 { -0.2 } else { 0.2 });
        bs.wf(bi, b::AY, 0.2);
        bs.wf(bi, b::ANG, 0.0);
        bs.wf(bi, b::RAD, 30.0);
        bs.wf(bi, b::DRAD, 2.0);
        let parts = bs.ri32(bi, b::PARTS);
        for k in 0..parts.clamp(0, 5) as usize {
            let Some(e) = self.bosses.member(bi, k) else { continue };
            self.boss_part_common(e, bi, tick, 0);
            self.wi32(e, f::SCORE, 1);
            self.w8(e, f::AUX_DEATH, 5);
            let (bx, by) = (self.bosses.rf(bi, b::X), self.bosses.rf(bi, b::Y));
            let (hw, hh) = (self.bosses.rf(bi, b::HW), self.bosses.rf(bi, b::HH));
            self.wi32(e, f::STATE, 1);
            if k == 0 {
                self.wi32(e, f::ANIM, 145);
                self.wi32(e, f::W, 58);
                self.wi32(e, f::H, 58);
                self.wi32(e, f::HW, 29);
                self.wi32(e, f::HH, 29);
                self.wf(e, f::X, bx);
                self.wf(e, f::Y, by);
                let hp = self.bosses.r32(bi, b::HP);
                self.w32(e, f::HP, hp);
                self.w8(e, f::B169, 1);
                self.w8(e, f::SND_DEATH, 3);
            } else {
                self.wi32(e, f::ANIM, 146);
                self.wi32(e, f::W, 22);
                self.wi32(e, f::H, 22);
                self.wi32(e, f::HW, 11);
                self.wi32(e, f::HH, 11);
                let (x, y) = match k {
                    1 => (-30.0 + (bx + hw), by + hh),
                    2 => (30.0 + (bx + hw), by + hh),
                    3 => (bx + hw, -30.0 + (by + hh)),
                    _ => (bx + hw, 30.0 + (by + hh)),
                };
                self.wf(e, f::X, x);
                self.wf(e, f::Y, y);
                self.wi32(e, f::HP, 7);
                self.w8(e, f::SND_DEATH, 27);
            }
            self.boss_part_box(e);
            self.boss_part_motion(e, bi);
        }
    }

    /// 0x43bcc: type 4 (a body with a gun pod below it).
    fn boss_init4<W: EnemyWorld>(&mut self, bi: usize, tick: u32, w: &mut W) {
        self.boss_init_common(bi, tick);
        let bs = &mut self.bosses;
        bs.w32(bi, b::W, 61);
        bs.w32(bi, b::H, 124);
        bs.wf(bi, b::HW, 30.0);
        bs.wf(bi, b::HH, 62.0);
        bs.wi32(bi, b::PARTICLE, 10);
        bs.w8(bi, b::DEATH_AUX, 2);
        bs.w8(bi, b::SND_HIT, 58);
        bs.w8(bi, b::SND_LOOP, 57);
        bs.w8(bi, b::LOOP_ON, 1);
        bs.w8(bi, b::RESTART, 0);
        bs.w32(bi, b::HANDLE, 0);
        self.boss_start(bi, w, 61, 3.0, 0.1, -0.1);
        self.bosses.wf(bi, b::AY, 0.1);
        let parts = self.bosses.ri32(bi, b::PARTS);
        for k in 0..parts.clamp(0, 2) as usize {
            let Some(e) = self.bosses.member(bi, k) else { continue };
            self.boss_part_common(e, bi, tick, 1);
            self.wi32(e, f::ANIM, 147);
            self.wi32(e, f::STATE, k as i32 + 1);
            self.wi32(e, f::W, 61);
            self.wi32(e, f::H, 62);
            self.wi32(e, f::HW, 61 / 2);
            self.wi32(e, f::HH, 62 / 2);
            self.wi32(e, f::SCORE, 1);
            let hp = self.bosses.r32(bi, b::HP);
            self.w32(e, f::HP, hp);
            self.w8(e, f::SND_DEATH, 0);
            let (bx, by) = (self.bosses.rf(bi, b::X), self.bosses.rf(bi, b::Y));
            self.wf(e, f::X, bx);
            self.wf(e, f::Y, if k == 0 { by } else { 62.0 + by });
            self.boss_part_box(e);
            self.boss_part_motion(e, bi);
        }
    }

    /// 0x44048: type 5 (a tall body with a cannon below it that fires in bursts).
    fn boss_init5<W: EnemyWorld>(&mut self, bi: usize, tick: u32, w: &mut W) {
        self.boss_init_common(bi, tick);
        let bs = &mut self.bosses;
        bs.w8(bi, b::FLAG, 0);
        bs.w32(bi, b::W, 64);
        bs.w32(bi, b::H, 128);
        bs.wf(bi, b::HW, 32.0);
        bs.wf(bi, b::HH, 64.0);
        bs.wi32(bi, b::PARTICLE, 7);
        bs.w8(bi, b::DEATH_AUX, 0);
        bs.w8(bi, b::SND_HIT, 13);
        bs.w8(bi, b::SND_LOOP, 14);
        bs.w8(bi, b::LOOP_ON, 1);
        bs.w8(bi, b::RESTART, 0);
        bs.w32(bi, b::HANDLE, 0);
        self.boss_start(bi, w, 64, 3.0, 0.05, -0.05);
        let bs = &mut self.bosses;
        bs.wf(bi, b::AY, 0.1);
        bs.w8(bi, b::OVERLAY, 0);
        bs.w32(bi, b::OV_ANIM, 117);
        bs.w32(bi, b::OV_FRAME, 1);
        let parts = self.bosses.ri32(bi, b::PARTS);
        for k in 0..parts.clamp(0, 2) as usize {
            let Some(e) = self.bosses.member(bi, k) else { continue };
            self.boss_part_common(e, bi, tick, 1);
            self.wi32(e, f::W, 64);
            self.wi32(e, f::H, 64);
            self.wi32(e, f::HW, 64 / 2);
            self.wi32(e, f::HH, 64 / 2);
            self.wi32(e, f::SCORE, 1);
            let hp = self.bosses.r32(bi, b::HP);
            self.w32(e, f::HP, hp);
            self.w8(e, f::SND_DEATH, 0);
            let (bx, by) = (self.bosses.rf(bi, b::X), self.bosses.rf(bi, b::Y));
            self.wi32(e, f::ANIM, if k == 0 { 124 } else { 125 });
            self.wi32(e, f::STATE, 1);
            self.wf(e, f::X, bx);
            self.wf(e, f::Y, if k == 0 { by } else { 64.0 + by });
            self.boss_part_box(e);
            self.boss_part_motion(e, bi);
        }
    }

    /// `pos += vel`, then bounce at `hi` / `lo` (the position is set to the limit and the speed reverses).
    fn boss_bounce(&mut self, bi: usize, pos: usize, vel: usize, hi: f32, lo: f32) {
        let v = self.bosses.rf(bi, pos) + self.bosses.rf(bi, vel);
        self.bosses.wf(bi, pos, v);
        if v > hi {
            self.bosses.wf(bi, pos, hi);
            let a = self.bosses.rf(bi, vel);
            self.bosses.wf(bi, vel, -a);
        } else if v < lo {
            self.bosses.wf(bi, pos, lo);
            let a = self.bosses.rf(bi, vel);
            self.bosses.wf(bi, vel, -a);
        }
    }

    /// 0x42344: type 2 per frame.
    fn boss_upd2<W: EnemyWorld>(&mut self, bi: usize, w: &mut W) {
        if self.bosses.rf(bi, b::Y) > 200.0 {
            self.boss_bounce(bi, b::VX, b::AX, 4.0, -4.0);
            self.boss_bounce(bi, b::VY, b::AY, 4.0, -4.0);
        }
        let tick = w.tick();
        if tick > self.bosses.r32(bi, b::SPAWN).wrapping_add(60) && self.bosses.rf(bi, b::Y) < 100.0 {
            self.boss_bounce(bi, b::VY, b::AY, 3.0, -3.0);
        }
        // the body's animation frame 1..3 every other tick
        if let Some(e0) = self.bosses.member(bi, 0) {
            if tick > self.r32(e0, f::T88).wrapping_add(1) {
                let s = self.r32(e0, f::STATE).wrapping_add(1);
                self.w32(e0, f::STATE, s);
                self.w32(e0, f::T88, tick);
                if (s as i32) > 3 {
                    self.w32(e0, f::STATE, 1);
                }
            }
        }
        let a = self.bosses.rf(bi, b::ANG) + 4.0;
        self.bosses.wf(bi, b::ANG, a);
        if a >= 360.0 {
            self.bosses.wf(bi, b::ANG, 0.0);
        }
        let r = self.bosses.rf(bi, b::RAD) + self.bosses.rf(bi, b::DRAD);
        self.bosses.wf(bi, b::RAD, r);
        if r > 60.0 {
            self.bosses.wf(bi, b::RAD, 60.0);
            let d = self.bosses.rf(bi, b::DRAD);
            self.bosses.wf(bi, b::DRAD, -d);
        } else if r < 15.0 {
            self.bosses.wf(bi, b::RAD, 15.0);
            let d = self.bosses.rf(bi, b::DRAD);
            self.bosses.wf(bi, b::DRAD, -d);
        }
        // the four satellites circle the body
        for k in 1..=4usize {
            let Some(e) = self.bosses.member(bi, k) else { continue };
            let rad = self.bosses.rf(bi, b::RAD);
            let theta = 0.017453292f32 * (self.bosses.rf(bi, b::ANG) + i2f((k as i32 - 1) * 90));
            let c = (theta as f64).cos() as f32;
            let s = (theta as f64).sin() as f32;
            let t0 = rad * s;
            let dx = rad.mul_add(c, -t0);
            let t1 = rad * c;
            let dy = rad.mul_add(s, t1);
            let (bx, by) = (self.bosses.rf(bi, b::X), self.bosses.rf(bi, b::Y));
            let (hw, hh) = (self.bosses.rf(bi, b::HW), self.bosses.rf(bi, b::HH));
            let x = dx + (-11.0 + (bx + hw));
            let y = dy + (-11.0 + (by + hh));
            self.wf(e, f::X, x);
            self.wf(e, f::Y, y);
            self.boss_part_box(e);
        }
    }

    /// Fire a boss shot with the parameters the type 4 / 5 routines use and report whether it was created.
    fn boss_shot<W: EnemyWorld>(w: &mut W, a: [i32; 6], x: f32, y: f32) -> bool {
        w.shot_param(a, x, y) & 0xff != 0
    }

    /// Muzzle flash on part `e` (+136 set, offset of the flash from the part's position).
    fn boss_muzzle(&mut self, e: usize, dy: f32) {
        self.w32(e, f::F136, 1);
        let hw = self.ri32(e, f::HW);
        self.wf(e, 140, i2f(hw.wrapping_sub(10)));
        self.wf(e, 144, dy);
    }

    /// 0x4274c: type 4 per frame (two gun rows that fire in a burst).
    fn boss_upd4<W: EnemyWorld>(&mut self, bi: usize, w: &mut W) {
        let tick = w.tick();
        let target = if w.ship(1).alive || w.ship(2).alive { Self::pick_target(&*w, 0.0, 0.0) as i32 } else { 1 };
        let (bx, hw) = (self.bosses.rf(bi, b::X), self.bosses.rf(bi, b::HW));
        let by = self.bosses.rf(bi, b::Y);
        let cx = f2i(bx + hw);
        let y1 = f2i(21.0 + by);
        let y2 = f2i(103.0 + by);
        let lvl = w.difficulty() as i32;
        let last = self.bosses.r32(bi, b::SHOT_TICK);
        let (mut top, mut bottom) = (false, false);
        let off = tick.wrapping_sub(last);
        let fire = |w: &mut W, y: i32| Self::boss_shot(w, [9, 1, 60, target, 1, 1], i2f(cx), i2f(y));
        match off {
            0 | 4 | 8 => bottom |= fire(w, y2),
            12 => if lvl > 2 { bottom |= fire(w, y2) },
            16 => if lvl > 4 { bottom |= fire(w, y2) },
            20 | 24 | 28 => top |= fire(w, y1),
            32 => if lvl > 2 { top |= fire(w, y1) },
            _ => {
                if tick > last.wrapping_add(32) {
                    if lvl > 4 {
                        top |= fire(w, y1);
                    }
                    let iv = self.bosses.r32(bi, b::INTERVAL);
                    self.bosses.w32(bi, b::SHOT_TICK, tick.wrapping_add(iv));
                }
            }
        }
        if top {
            if let Some(e) = self.bosses.member(bi, 0) {
                if self.r8(e, f::ACTIVE) != 0 {
                    self.boss_muzzle(e, 11.0);
                }
            }
        }
        if bottom {
            if let Some(e) = self.bosses.member(bi, 1) {
                if self.r8(e, f::ACTIVE) != 0 {
                    self.boss_muzzle(e, 30.0);
                }
            }
        }
        if self.bosses.rf(bi, b::Y) > 50.0 {
            self.boss_bounce(bi, b::VX, b::AX, 3.0, -3.0);
            self.boss_bounce(bi, b::VY, b::AY, 3.0, -3.0);
        }
        let spawn = self.bosses.r32(bi, b::SPAWN);
        if tick > spawn.wrapping_add(60) {
            if tick > spawn.wrapping_add(700) {
                let a = self.bosses.rf(bi, b::AY) - 0.2;
                self.bosses.wf(bi, b::AY, a);
                if a > 3.0 {
                    self.bosses.wf(bi, b::AY, 3.0);
                }
            } else if self.bosses.rf(bi, b::Y) < 100.0 {
                self.boss_bounce(bi, b::VY, b::AY, 2.0, -2.0);
            }
        }
    }

    /// 0x42f48: type 5 per frame (the cannon part's animation state runs 2..15 and it shoots while it is fully out).
    fn boss_upd5<W: EnemyWorld>(&mut self, bi: usize, w: &mut W) {
        let tick = w.tick();
        let any = w.ship(1).alive || w.ship(2).alive;
        let late = tick > self.bosses.r32(bi, b::SPAWN).wrapping_add(700);
        if let Some(e) = self.bosses.member(bi, 1) {
            if self.bosses.r8(bi, b::FLAG) != 0 {
                // cooling down: the cannon folds back in
                let s = self.ri32(e, f::STATE);
                if s > 1 {
                    self.wi32(e, f::STATE, s - 1);
                }
                if self.ri32(e, f::STATE) == 1 {
                    self.bosses.w8(bi, b::FLAG, 0);
                }
            } else {
                let s = self.ri32(e, f::STATE);
                if s > 1 {
                    if any && !late {
                        self.wi32(e, f::STATE, s.wrapping_add(1));
                        if self.ri32(e, f::STATE) == 14 {
                            self.bosses.w32(bi, b::PHASE, 0);
                            self.bosses.w8(bi, b::OVERLAY, 1);
                            self.bosses.w32(bi, b::OV_FRAME, 1);
                        }
                        let (bx, by) = (self.bosses.rf(bi, b::X), self.bosses.rf(bi, b::Y));
                        let wd = self.bosses.ri32(bi, b::W);
                        self.bosses.wf(bi, b::OV_X, bx + i2f((wd - 48) / 2));
                        self.bosses.wf(bi, b::OV_Y, 71.0 + by);
                        if self.ri32(e, f::STATE) >= 15 {
                            self.wi32(e, f::STATE, 15);
                            let last = self.bosses.r32(bi, b::SHOT_TICK);
                            if tick >= last.wrapping_add(2) {
                                let hw = self.bosses.rf(bi, b::HW);
                                if Self::boss_shot(w, [7, 1, 56, 3, 1, 0], bx + hw, 60.0 + by) {
                                    self.bosses.w32(bi, b::SHOT_TICK, tick);
                                }
                            }
                            let p = self.bosses.ri32(bi, b::PHASE).wrapping_add(1);
                            self.bosses.wi32(bi, b::PHASE, p);
                            if p > 30 {
                                self.bosses.w8(bi, b::FLAG, 1);
                                self.bosses.w32(bi, b::SHOT_TICK, tick);
                                self.bosses.w8(bi, b::OVERLAY, 0);
                            }
                        }
                    } else {
                        self.bosses.w8(bi, b::FLAG, 1);
                        self.bosses.w32(bi, b::SHOT_TICK, tick);
                        self.bosses.w8(bi, b::OVERLAY, 0);
                    }
                } else if any && !late && tick >= self.bosses.r32(bi, b::SHOT_TICK).wrapping_add(self.bosses.r32(bi, b::INTERVAL)) {
                    self.wi32(e, f::STATE, 2);
                    self.bosses.w8(bi, b::FLAG, 0);
                }
            }
        }
        if self.bosses.rf(bi, b::Y) > 50.0 {
            self.boss_bounce(bi, b::VX, b::AX, 2.0, -2.0);
            let v = self.bosses.rf(bi, b::VY) + self.bosses.rf(bi, b::AY);
            self.bosses.wf(bi, b::VY, v);
            if v > 3.0 {
                self.bosses.wf(bi, b::VY, 3.0);
                if !late {
                    let a = self.bosses.rf(bi, b::AY);
                    self.bosses.wf(bi, b::AY, -a);
                }
            } else if v < -3.0 {
                self.bosses.wf(bi, b::VY, -3.0);
                let a = self.bosses.rf(bi, b::AY);
                self.bosses.wf(bi, b::AY, -a);
            }
            if self.bosses.rf(bi, b::Y) > 400.0 {
                self.bosses.w8(bi, b::LOOP_ON, 0);
            }
        }
        if tick > self.bosses.r32(bi, b::SPAWN).wrapping_add(60) {
            if late {
                let a = self.bosses.rf(bi, b::AY) + 0.2;
                self.bosses.wf(bi, b::AY, a);
                if a > 5.0 {
                    self.bosses.wf(bi, b::AY, 5.0);
                }
            } else if self.bosses.rf(bi, b::Y) < 100.0 {
                self.boss_bounce(bi, b::VY, b::AY, 2.0, -2.0);
            }
        }
    }

    /// 0x4127c: one frame of every boss. Returns false when a record has a type without a routine (the loop is abandoned).
    pub fn boss_update<W: EnemyWorld>(&mut self, w: &mut W) -> bool {
        if self.bosses.count < 1 {
            return false;
        }
        let b1 = Self::contact_box(&w.ship(1));
        let b2 = Self::contact_box(&w.ship(2));
        for bi in 0..BOSSES {
            if self.bosses.r8(bi, b::ACTIVE) == 0 {
                continue;
            }
            if self.bosses.r8(bi, b::LOOP_ON) != 0 && self.bosses.r8(bi, b::RESTART) != 0 {
                let h = w.sound_loop(self.bosses.r8(bi, b::SND_LOOP) as i32, 4, bi as i32);
                self.bosses.w32(bi, b::HANDLE, h);
                self.bosses.w8(bi, b::RESTART, 0);
            }
            match self.bosses.ri32(bi, b::TYPE) {
                2 => self.boss_upd2(bi, w),
                4 => self.boss_upd4(bi, w),
                5 => self.boss_upd5(bi, w),
                _ => return false,
            }
            let x = self.bosses.rf(bi, b::X) + self.bosses.rf(bi, b::VX);
            self.bosses.wf(bi, b::X, x);
            let y = self.bosses.rf(bi, b::Y) + self.bosses.rf(bi, b::VY);
            self.bosses.wf(bi, b::Y, y);
            let parts = self.bosses.ri32(bi, b::PARTS).clamp(0, 54) as usize;
            for k in 0..parts {
                let e32 = self.bosses.ri32(bi, b::MEMBERS + 4 * k);
                if !(0..SLOTS as i32).contains(&e32) {
                    continue;
                }
                let e = e32 as usize;
                if self.r8(e, f::ACTIVE) == 0 {
                    continue;
                }
                let (vx, vy) = (self.bosses.rf(bi, b::VX), self.bosses.rf(bi, b::VY));
                self.wf(e, f::VX, vx);
                self.wf(e, f::VY, vy);
                for (o, v) in [(f::X, vx), (f::Y, vy), (f::BX0, vx), (f::BY0, vy), (f::BX1, vx), (f::BY1, vy)] {
                    let nv = self.rf(e, o) + v;
                    self.wf(e, o, nv);
                }
                for (n, bx) in [(1u8, b1), (2u8, b2)] {
                    if n == 2 && self.r8(e, f::ACTIVE) == 0 {
                        break;
                    }
                    let Some((l, t, r, bt)) = bx else { continue };
                    if !w.ship(n).alive {
                        continue;
                    }
                    if l >= f2i(self.rf(e, f::BX1)) || f2i(self.rf(e, f::BX0)) >= r || t >= f2i(self.rf(e, f::BY1)) || f2i(self.rf(e, f::BY0)) >= bt {
                        continue;
                    }
                    let dmg = self.ri32(e, f::HP).wrapping_sub(self.ri32(e, f::DMG)).wrapping_add(1);
                    w.ship_damage(n, dmg);
                    if self.r8(e, f::BOSS) != 0 {
                        let bidx = self.ri32(e, f::BOSS_IDX);
                        self.boss_hit(e as i32, bidx, 5, n, w);
                    } else {
                        self.damage(e, 5, n, w);
                    }
                }
            }
        }
        true
    }

    /// 0x41c88: `amount` damage to part `e` (a slot index, or -1) of boss `bi` from ship `owner`; returns the damage that counted.
    pub fn boss_hit<W: EnemyWorld>(&mut self, e: i32, bi: i32, amount: i32, owner: u8, w: &mut W) -> i32 {
        if !(0..BOSSES as i32).contains(&bi) {
            return 0;
        }
        let bi = bi as usize;
        if self.bosses.r8(bi, b::ACTIVE) == 0 {
            return 0;
        }
        let mut ret = 0i32;
        let part = if (0..SLOTS as i32).contains(&e) && self.r8(e as usize, f::ACTIVE) != 0 { Some(e as usize) } else { None };
        if let Some(p) = part {
            if self.r8(p, f::B169) != 0 {
                // the part that carries the boss's hit points
                let old = self.bosses.ri32(bi, b::DMG);
                let new = old.wrapping_add(amount);
                self.bosses.wi32(bi, b::DMG, new);
                let hp = self.bosses.ri32(bi, b::HP);
                ret = new.wrapping_sub(old);
                if new > hp {
                    ret = ret.wrapping_sub(new.wrapping_sub(hp));
                }
            } else {
                let old = self.ri32(p, f::DMG);
                let new = old.wrapping_add(amount);
                self.wi32(p, f::DMG, new);
                let hp = self.ri32(p, f::HP);
                ret = new.wrapping_sub(old);
                if new > hp {
                    let (x, y) = (f2i(self.rf(p, f::X)).wrapping_add(self.ri32(p, f::HW)), f2i(self.rf(p, f::Y)).wrapping_add(self.ri32(p, f::HH)));
                    let snd = self.r8(p, f::SND_DEATH) as i32;
                    w.effect(x, y, 14, 0, snd, 0, 0, self.rf(p, f::VX), self.rf(p, f::VY));
                    let aux = self.r8(p, f::AUX_DEATH);
                    ret = ret.wrapping_sub(new.wrapping_sub(hp));
                    if aux as i8 != -1 {
                        w.aux(f2i(self.rf(p, f::X)), f2i(self.rf(p, f::Y)), aux as i32, 0, -1, 0);
                    }
                    let pts = self.ri32(p, f::SCORE);
                    w.score(owner, pts);
                    self.clear_slot(p);
                    let a = self.bosses.ri32(bi, b::ALIVE).wrapping_sub(1);
                    self.bosses.wi32(bi, b::ALIVE, a);
                    let k = self.bosses.ri32(bi, b::KILLED).wrapping_add(1);
                    self.bosses.wi32(bi, b::KILLED, k);
                    let parts = self.bosses.ri32(bi, b::PARTS).clamp(0, 54) as usize;
                    for m in 0..parts {
                        if self.bosses.ri32(bi, b::MEMBERS + 4 * m) == p as i32 {
                            self.bosses.wi32(bi, b::MEMBERS + 4 * m, -1);
                            break;
                        }
                    }
                } else {
                    self.w8(p, f::HIT, 1);
                }
            }
        }
        if self.bosses.ri32(bi, b::DMG) > self.bosses.ri32(bi, b::HP) {
            self.boss_destroyed(e, bi, w);
        } else {
            self.bosses.w8(bi, b::HIT, 1);
            let t = w.tick();
            self.bosses.w32(bi, b::HIT_TICK, t);
            let s = self.bosses.r8(bi, b::SND_HIT) as i32;
            w.sound(s, 3, 0);
        }
        ret
    }

    /// The boss has taken more damage than it has hit points: explosions, its parts go, the record is freed and the bonuses drop.
    fn boss_destroyed<W: EnemyWorld>(&mut self, e: i32, bi: usize, w: &mut W) {
        let particle = self.bosses.ri32(bi, b::PARTICLE);
        let (bx, by) = (self.bosses.rf(bi, b::X), self.bosses.rf(bi, b::Y));
        let (hw, hh) = (self.bosses.rf(bi, b::HW), self.bosses.rf(bi, b::HH));
        let cx = f2i(i2f(f2i(bx)) + hw);
        let cy = f2i(i2f(f2i(by)) + hh);
        for (dx, dy, id, a, snd, c) in [
            (0, 0, 14, 0, 4, 0),
            (-20, 0, 14, 6, 4, 0),
            (30, 15, 14, 10, 4, 0),
            (-20, -10, 14, 18, 11, 1),
            (0, -16, 14, 24, 62, 1),
            (18, 4, 2, 32, 11, 1),
            (-8, 22, 2, 40, 62, 1),
        ] {
            w.effect(cx.wrapping_add(dx), cy.wrapping_add(dy), id, a, snd, c, 0, 0.0, 0.0);
        }
        let aux = self.bosses.r8(bi, b::DEATH_AUX) as i32;
        for a in [0, 2, 6] {
            w.aux(cx, cy, aux, a, -1, 0);
        }
        for _ in 0..3 {
            w.sound(4, 4, 0);
        }
        if particle != -1 {
            w.particles(cx, cy, particle, 8, 4, 0);
        }
        // the sound byte of the explosions of the parts is the one of the slot that was hit (the original keeps that pointer in a register it
        // then reuses for the loop); an out-of-range slot reads as 0 here
        let hit_snd = if (0..SLOTS as i32).contains(&e) { self.r8(e as usize, f::SND_DEATH) as i32 } else { 0 };
        let parts = self.bosses.ri32(bi, b::PARTS).clamp(0, 54) as usize;
        for m in 0..parts {
            let Some(p) = self.bosses.member(bi, m) else { continue };
            if self.r8(p, f::ACTIVE) == 0 {
                continue;
            }
            let (x, y) = (f2i(self.rf(p, f::X)).wrapping_add(self.ri32(p, f::HW)), f2i(self.rf(p, f::Y)).wrapping_add(self.ri32(p, f::HH)));
            w.effect(x, y, 14, 0, hit_snd, 0, 0, self.rf(p, f::VX), self.rf(p, f::VY));
            self.clear_slot(p);
        }
        self.bosses.count = self.bosses.count.wrapping_sub(1);
        self.bosses.free(bi);
        let (px, py) = (bx + hw, by + hh);
        let n = self.bosses.ri32(bi, b::BONUS_N);
        if n > 1 {
            w.pickup(3, n, 1, 1, px, py);
        }
        let kind = self.bosses.ri32(bi, b::BONUS_KIND);
        if kind > 1 {
            w.pickup(kind, 1, 1, 1, px, py);
        }
    }

    /// 0x417a4: draw the bosses' parts (and cull bosses that left the screen).
    pub fn boss_draw<W: EnemyDrawWorld>(&mut self, w: &mut W) {
        if self.bosses.count < 1 {
            return;
        }
        let opts: DrawOpts = w.opts();
        let tick = w.tick();
        for bi in 0..BOSSES {
            if self.bosses.r8(bi, b::ACTIVE) == 0 {
                continue;
            }
            let n = self.bosses.ri32(bi, b::PARTS).clamp(0, 54) as usize;
            let (x, y) = (self.bosses.rf(bi, b::X), self.bosses.rf(bi, b::Y));
            let wd = self.bosses.ri32(bi, b::W);
            let visible = !(x < -128.0 || x + i2f(wd) > 544.0 || y < -128.0 || y > 544.0);
            if !visible {
                for k in 0..n {
                    if let Some(e) = self.bosses.member(bi, k) {
                        if self.r8(e, f::ACTIVE) != 0 {
                            self.clear_slot(e);
                        }
                    }
                }
                self.bosses.count = self.bosses.count.wrapping_sub(1);
                self.bosses.free(bi);
            } else {
                for k in 0..n {
                    let Some(e) = self.bosses.member(bi, k) else { continue };
                    if self.r8(e, f::ACTIVE) == 0 {
                        continue;
                    }
                    let (ex, ey) = (self.rf(e, f::X), self.rf(e, f::Y));
                    let (ew, eh) = (self.ri32(e, f::W), self.ri32(e, f::H));
                    let (anim, frame) = (self.ri32(e, f::ANIM), self.ri32(e, f::STATE));
                    if opts.shadows {
                        let hd = i2f64(eh);
                        let t = -(0.66f64.mul_add(hd, -hd));
                        let ys = ((f2i(ey).wrapping_add(44)) as f64 - (k as f64) * t) as i32;
                        w.blit(Prim::Shadow, f2i(ex).wrapping_sub(24), ys.wrapping_add(2 * k as i32), ew, eh, anim, frame);
                    }
                    w.blit(Prim::Anim, f2i(ex), f2i(ey), ew, eh, anim, frame);
                    if self.bosses.r8(bi, b::HIT) != 0 {
                        w.blit(Prim::Flash, f2i(ex), f2i(ey), ew, eh, anim, frame);
                        w.blit(Prim::Flash, f2i(ex), f2i(ey), ew, eh, anim, frame);
                    }
                    if self.r32(e, f::F136) != 0 {
                        let mx = f2i(i2f(f2i(ex)) + self.rf(e, 140));
                        let my = f2i(i2f(f2i(ey)) + self.rf(e, 144));
                        w.blit(Prim::Anim, mx, my, 20, 20, 50, 1);
                        self.w32(e, f::F136, 0);
                    }
                }
                if self.bosses.r8(bi, b::OVERLAY) != 0 {
                    let (ox, oy) = (f2i(self.bosses.rf(bi, b::OV_X)), f2i(self.bosses.rf(bi, b::OV_Y)));
                    let (oa, of) = (self.bosses.ri32(bi, b::OV_ANIM), self.bosses.ri32(bi, b::OV_FRAME));
                    w.blit(opts.trans_prim(), ox, oy, 48, 48, oa, of);
                }
            }
            if self.bosses.r8(bi, b::HIT) != 0 && tick > self.bosses.r32(bi, b::HIT_TICK).wrapping_add(1) {
                self.bosses.w8(bi, b::HIT, 0);
            }
        }
    }
}

#[inline]
fn i2f64(v: i32) -> f64 { v as f64 }
