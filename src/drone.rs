//! The two helper weapons of a ship: the drones and the side missiles.
//!
//! **Drones** (4 records of 44 bytes at `D.0x149f0`, counter `D.17a0`):
//! * 0x2c70c clear ([`Drones::clear`]), 0x2c760 add ([`Drones::add`]: a drone appears at the top of the screen and flies to its ship),
//! * 0x2c87c remove ([`Drones::remove`]: the drones of a player explode), 0x2c960 fire ([`Drones::fire`]: every docked drone of the player shoots a 5x16 shot, via 0x21288 =
//!   [`crate::ship::ShotPool::spawn_drone`]),
//! * 0x2cb40 update ([`Drones::update`]): mode 1 homes in on the ship at 12 px per tick, mode 2 (docked) follows it 41 px to the left / 10 px to the right of its box, and
//!   each tick the drone's box (30 x 35) hurts the enemies it touches (damage 1, sound 15),
//! * 0x2d1d8 draw ([`Drones::draw`]): shadow, sprite (anim 21, frame 1..6 turns with the ship's horizontal speed), muzzle flash (anim 49), hit flash.
//!
//! **Side missiles** (16 records of 172 bytes at `D.0x1079c`, counter `D.16ec`):
//! * 0x27780 clear ([`Missiles::clear`]), 0x277e4 launch ([`Missiles::launch`]: 2 / 4 / 6 missiles for level 1 / 2 / 3+), 0x27dfc update ([`Missiles::update`]: a fixed
//!   kick-off curve, then straight up with a trail of six flame points; they hit enemies for 7 and explode), 0x28260 draw ([`Missiles::draw`]).
//!
//! Verified against the executable by `tests/drone_golden.rs` (vectors from `tools/re/gen_drone_vectors.py`).
use crate::draw::Prim;
use crate::enemy::f2i;
use crate::ship::{Ship, ShotPool};

pub const DRONES: usize = 4;
pub const DRONE_SIZE: usize = 44;
pub const MISSILES: usize = 16;
pub const MISSILE_SIZE: usize = 172;

/// Drone record offsets.
pub mod d {
    pub const ACTIVE: usize = 0;
    pub const MODE: usize = 1; // 1 = flying in, 2 = docked
    pub const TICK: usize = 4; // tick of the creation / of the docking
    pub const OWNER: usize = 8; // player number
    pub const SIDE: usize = 9; // 1 = left of the ship, else right
    pub const X: usize = 12;
    pub const Y: usize = 16;
    pub const VX: usize = 20;
    pub const VY: usize = 24;
    pub const HIT: usize = 28; // byte: hurt an enemy (flash)
    pub const FIRED: usize = 29; // byte: fired this tick (muzzle flash)
    pub const HIT_TICK: usize = 32;
    pub const ANIM: usize = 36;
    pub const FRAME: usize = 40;
}

/// Missile record offsets (six trail points of 20 bytes start at +52).
pub mod m {
    pub const ACTIVE: usize = 0;
    pub const FLAME: usize = 1; // byte: the engine burns (set when the kick-off curve reaches step 12)
    pub const WAIT: usize = 2; // byte: still waiting for its launch delay
    pub const OWNER: usize = 3;
    pub const DELAY: usize = 4;
    pub const X: usize = 8;
    pub const Y: usize = 12;
    pub const VX: usize = 16;
    pub const TYPE: usize = 24;
    pub const W: usize = 28;
    pub const H: usize = 32;
    pub const ANIM: usize = 36;
    pub const FRAME: usize = 40;
    pub const FLAME_FRAME: usize = 44;
    pub const STEP: usize = 48; // step of the kick-off curve (stays at 20)
    pub const TRAIL: usize = 52; // 6 x { frame +0, x +4, y +8, start tick +12, active +16 }
}

/// Vertical kick-off curve of a missile, one entry per step (the table at `D.0x6854`).
const KICK: [i32; 21] = [0, 0, 1, 2, 2, 2, 1, 0, 0, 0, 0, 0, -2, -3, -5, -7, -9, -11, -13, -15, -17];

pub trait DroneWorld {
    /// 0xecf0.
    fn tick(&self) -> u32;
    /// 0x2ead4: damage to the enemies under a box (`hp` is the attacker's strength); returns the damage dealt.
    fn hit_enemies(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, hp: i32, owner: u8) -> i32;
    /// 0x22670.
    #[allow(clippy::too_many_arguments)]
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32);
    /// 0x45bac.
    fn sound(&mut self, id: i32, prio: i32, arg: i32);
    /// 0x2994 / 0x29a4 / 0xe364 (`D.12bc`, `D.12be`, `D.13c8`).
    fn shadows(&self) -> bool;
    fn translucent(&self) -> bool;
    fn fancy(&self) -> bool;
    /// A blit request (0x2f88 plain, 0x3170 translucent, 0x3264 dithered, 0x344c shadow, 0x3540 flash), group 0.
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32);
}

/// A byte array of equally sized records with the big-endian accessors the original's layout needs.
#[derive(Clone)]
struct Pool {
    rec: Vec<u8>,
    size: usize,
}

impl Pool {
    fn new(n: usize, size: usize) -> Pool { Pool { rec: vec![0; n * size], size } }
    #[inline] fn r8(&self, n: usize, off: usize) -> u8 { self.rec[n * self.size + off] }
    #[inline] fn w8(&mut self, n: usize, off: usize, v: u8) { self.rec[n * self.size + off] = v }
    #[inline] fn ri(&self, n: usize, off: usize) -> i32 {
        let a = n * self.size + off;
        i32::from_be_bytes([self.rec[a], self.rec[a + 1], self.rec[a + 2], self.rec[a + 3]])
    }
    #[inline] fn wi(&mut self, n: usize, off: usize, v: i32) {
        let a = n * self.size + off;
        self.rec[a..a + 4].copy_from_slice(&v.to_be_bytes());
    }
    #[inline] fn rf(&self, n: usize, off: usize) -> f32 { f32::from_bits(self.ri(n, off) as u32) }
    #[inline] fn wf(&mut self, n: usize, off: usize, v: f32) { self.wi(n, off, v.to_bits() as i32) }
}

/// `(f32)(int)` as the original converts (exact for small values, rounded to nearest otherwise).
#[inline]
fn i2f(v: i32) -> f32 { v as f32 }

#[derive(Clone)]
pub struct Drones {
    pool: Pool,
    /// D.17a0: drones in flight.
    pub count: i32,
}

impl Default for Drones {
    fn default() -> Self { Self::new() }
}

impl Drones {
    pub fn new() -> Drones { Drones { pool: Pool::new(DRONES, DRONE_SIZE), count: 0 } }

    /// The raw records (for the differential tests).
    pub fn bytes(&self) -> &[u8] { &self.pool.rec }
    pub fn set_word(&mut self, n: usize, off: usize, v: u32) { self.pool.wi(n, off, v as i32) }
    pub fn set_byte(&mut self, n: usize, off: usize, v: u8) { self.pool.w8(n, off, v) }

    /// 0x2c70c: no drone (clears the active and the "fired" bytes only, like the original).
    pub fn clear(&mut self) {
        for n in 0..DRONES {
            self.pool.w8(n, d::ACTIVE, 0);
            self.pool.w8(n, d::FIRED, 0);
        }
        self.count = 0;
    }

    /// 0x2c760: add a drone for player `number` on side `which` (1 = left). Returns false when 4 drones exist or no record is free.
    pub fn add<W: DroneWorld>(&mut self, number: u8, which: i32, w: &W) -> bool {
        if self.count >= 4 { return false; }
        let Some(n) = (0..DRONES).find(|&n| self.pool.r8(n, d::ACTIVE) == 0) else { return false };
        let p = &mut self.pool;
        p.w8(n, d::ACTIVE, 1);
        p.w8(n, d::MODE, 1);
        p.wi(n, d::TICK, w.tick() as i32);
        p.w8(n, d::OWNER, number);
        p.w8(n, d::SIDE, which as u8);
        p.wf(n, d::X, 200.0);
        p.wf(n, d::Y, 0.0);
        p.wf(n, d::VX, 0.0);
        p.wf(n, d::VY, 8.0);
        p.w8(n, d::HIT, 0);
        p.wi(n, d::ANIM, 21);
        p.wi(n, d::FRAME, 1);
        self.count = self.count.wrapping_add(1);
        true
    }

    /// 0x2c87c: the drones of player `number` explode.
    pub fn remove<W: DroneWorld>(&mut self, number: u8, w: &mut W) {
        for n in 0..DRONES {
            if self.pool.r8(n, d::OWNER) != number || self.pool.r8(n, d::ACTIVE) == 0 { continue; }
            self.pool.w8(n, d::ACTIVE, 0);
            self.count = self.count.wrapping_sub(1);
            let (x, y) = (f2i(self.pool.rf(n, d::X)).wrapping_add(15), f2i(self.pool.rf(n, d::Y)).wrapping_add(17));
            w.effect(x, y, 1, 0, -1, 0, 0, self.pool.rf(n, d::VX), self.pool.rf(n, d::VY));
        }
    }

    /// 0x2c960: every docked drone of `ship` fires a shot from its centre.
    pub fn fire(&mut self, ship: &Ship, shots: &mut ShotPool) {
        for n in 0..DRONES {
            if self.pool.r8(n, d::ACTIVE) == 0 || self.pool.r8(n, d::MODE) != 2 { continue; }
            if self.pool.r8(n, d::OWNER) != ship.number { continue; }
            let (x, y) = (15.0 + self.pool.rf(n, d::X), 17.0 + self.pool.rf(n, d::Y));
            if shots.spawn_drone(ship.number, x, y) {
                self.pool.w8(n, d::FIRED, 1);
            }
        }
    }

    /// Where a drone of the given side wants to be next to `ship`: x from the side, y from the top of the ship.
    #[inline]
    fn dock_x(side: u8, ship: &Ship) -> f32 {
        if side == 1 { -41.0 + ship.x } else { 10.0 + (ship.x + i2f(ship.w)) }
    }

    /// 0x2cb40: move the drones and let them hurt the enemies.
    pub fn update<W: DroneWorld>(&mut self, s1: &Ship, s2: &Ship, w: &mut W) {
        if self.count <= 0 { return; }
        for n in 0..DRONES {
            if self.pool.r8(n, d::ACTIVE) == 0 { continue; }
            let owner = self.pool.r8(n, d::OWNER);
            let ship = if owner == 1 { s1 } else { s2 };
            let side = self.pool.r8(n, d::SIDE);
            match self.pool.r8(n, d::MODE) {
                2 => {
                    // docked: follow the ship
                    self.pool.wf(n, d::X, Self::dock_x(side, ship));
                    self.pool.wf(n, d::Y, 10.0 + ship.y);
                }
                1 => {
                    // flying in: unit vector towards the dock position times 12
                    let (x, y) = (self.pool.rf(n, d::X), self.pool.rf(n, d::Y));
                    let dx = Self::dock_x(side, ship) - x;
                    let dy = 10.0 + (ship.y - y);
                    let sq = dx.mul_add(dx, dy * dy);
                    let dist = (sq as f64).sqrt() as f32;
                    let (ux, uy) = (dx / dist, dy / dist);
                    self.pool.wf(n, d::VX, ux * 12.0);
                    self.pool.wf(n, d::VY, uy * 12.0);
                    if y >= 10.0 + ship.y {
                        // arrived: dock
                        self.pool.wf(n, d::X, Self::dock_x(side, ship));
                        self.pool.wf(n, d::Y, 10.0 + ship.y);
                        self.pool.wf(n, d::VX, 0.0);
                        self.pool.wf(n, d::VY, 0.0);
                        self.pool.w8(n, d::MODE, 2);
                        self.pool.wi(n, d::TICK, w.tick() as i32);
                    }
                }
                _ => return, // (the original leaves the whole routine here)
            }
            let (x, y) = (self.pool.rf(n, d::X) + self.pool.rf(n, d::VX), self.pool.rf(n, d::Y) + self.pool.rf(n, d::VY));
            self.pool.wf(n, d::X, x);
            self.pool.wf(n, d::Y, y);
            if x < 0.0 { self.pool.wf(n, d::X, 0.0); }
            if 30.0 + self.pool.rf(n, d::X) > 416.0 { self.pool.wf(n, d::X, 386.0); }
            let (x, y) = (self.pool.rf(n, d::X), self.pool.rf(n, d::Y));
            let dealt = w.hit_enemies(f2i(x), f2i(y), f2i(30.0 + x), f2i(35.0 + y), 1, owner);
            if dealt > 0 {
                self.pool.w8(n, d::HIT, 1);
                self.pool.wi(n, d::HIT_TICK, w.tick() as i32);
                w.sound(15, 2, 0);
            }
        }
    }

    /// 0x2d1d8: draw the drones (the sprite frame turns with the horizontal speed of the ship they belong to).
    pub fn draw<W: DroneWorld>(&mut self, s1: &Ship, s2: &Ship, w: &mut W) {
        if self.count <= 0 { return; }
        let now = w.tick();
        for n in 0..DRONES {
            if self.pool.r8(n, d::ACTIVE) == 0 { continue; }
            let ship = if self.pool.r8(n, d::OWNER) == 1 { s1 } else { s2 };
            let sp = ship.vx as f64;
            if sp < 0.0 {
                let f = self.pool.ri(n, d::FRAME).wrapping_sub(1);
                self.pool.wi(n, d::FRAME, if f < 1 { 6 } else { f });
            } else if sp > 0.0 {
                let f = self.pool.ri(n, d::FRAME).wrapping_add(1);
                self.pool.wi(n, d::FRAME, if f > 6 { 1 } else { f });
            }
            let (x, y) = (f2i(self.pool.rf(n, d::X)), f2i(self.pool.rf(n, d::Y)));
            if self.pool.r8(n, d::FIRED) != 0 {
                w.blit(Prim::Anim, x.wrapping_add(8), y.wrapping_sub(10), 14, 14, 49, 1);
                self.pool.w8(n, d::FIRED, 0);
            }
            if self.pool.r8(n, d::HIT) != 0 && now > (self.pool.ri(n, d::HIT_TICK) as u32).wrapping_add(3) {
                self.pool.w8(n, d::HIT, 0);
            }
            let (anim, frame) = (self.pool.ri(n, d::ANIM), self.pool.ri(n, d::FRAME));
            if w.shadows() {
                w.blit(Prim::Shadow, x.wrapping_sub(24), y.wrapping_add(44), 30, 35, anim, frame);
            }
            w.blit(Prim::Anim, x, y, 30, 35, anim, frame);
            if self.pool.r8(n, d::HIT) != 0 {
                w.blit(Prim::Flash, x, y, 30, 35, anim, frame);
            }
        }
    }
}

impl std::fmt::Debug for Drones {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Drones").field("count", &self.count).finish()
    }
}

#[derive(Clone)]
pub struct Missiles {
    pool: Pool,
    /// D.16ec: missiles in flight.
    pub count: i32,
}

impl Default for Missiles {
    fn default() -> Self { Self::new() }
}

impl Missiles {
    pub fn new() -> Missiles { Missiles { pool: Pool::new(MISSILES, MISSILE_SIZE), count: 0 } }

    pub fn bytes(&self) -> &[u8] { &self.pool.rec }
    pub fn set_word(&mut self, n: usize, off: usize, v: u32) { self.pool.wi(n, off, v as i32) }
    pub fn set_byte(&mut self, n: usize, off: usize, v: u8) { self.pool.w8(n, off, v) }

    /// 0x277c4: free a record (the counter goes down even when it was free already).
    fn free(&mut self, n: usize) {
        self.pool.w8(n, m::ACTIVE, 0);
        self.count = self.count.wrapping_sub(1);
    }

    /// 0x27780: no missile in flight.
    pub fn clear(&mut self) {
        for n in 0..MISSILES { self.free(n); }
        self.count = 0;
    }

    /// 0x277e4: launch the side missiles of `ship` (level +332: 1 -> 2, 2 -> 4, 3..5 -> 6 missiles). Returns false when the level has none or the pool is too full.
    pub fn launch<W: DroneWorld>(&mut self, ship: &Ship, w: &W) -> bool {
        let n: i32 = match ship.missiles {
            1 => 2,
            2 => 4,
            3..=5 => 6,
            _ => return false,
        };
        let now = w.tick();
        if self.count > 16 - n { return false; }
        let sx = f2i(ship.x).wrapping_add(7);
        let y0 = f2i(ship.y).wrapping_add(15);
        let mut i = 0;
        for rec in 0..MISSILES {
            if self.pool.r8(rec, m::ACTIVE) != 0 { continue; }
            let p = &mut self.pool;
            p.w8(rec, m::ACTIVE, 1);
            self.count = self.count.wrapping_add(1);
            p.wi(rec, m::TYPE, 1);
            p.w8(rec, m::OWNER, ship.number);
            p.wi(rec, m::W, 8);
            p.wi(rec, m::H, 16);
            p.wi(rec, m::ANIM, 45);
            p.wi(rec, m::FRAME, 1);
            p.w8(rec, m::FLAME, 0);
            p.wi(rec, m::FLAME_FRAME, 1);
            p.wf(rec, m::Y, i2f(y0));
            p.wi(rec, m::STEP, 0);
            // (x offset from the ship, horizontal speed, wait flag, delay) per missile; the wait byte is only written where it is listed
            let (dx, vx, wait): (i32, f32, Option<(u8, i32)>) = match (n, i) {
                (2, 0) => (4, -1.2, None),
                (2, _) => (22, 1.2, None),
                (4, 0) => (0, -1.2, Some((1, 3))),
                (4, 1) => (2, -0.8, Some((0, 0))),
                (4, 2) => (24, 0.8, Some((0, 0))),
                (4, _) => (26, 1.2, Some((1, 3))),
                (_, 0) => (0, -1.2, Some((0, 0))),
                (_, 1) => (4, -0.6, Some((0, 0))),
                (_, 2) => (2, -0.8, Some((1, 8))),
                (_, 3) => (24, 0.8, Some((1, 8))),
                (_, 4) => (22, 0.6, Some((0, 0))),
                (_, _) => (26, 1.2, Some((0, 0))),
            };
            p.wf(rec, m::X, i2f(sx.wrapping_add(dx)));
            p.wf(rec, m::VX, vx);
            if let Some((flag, delay)) = wait {
                p.w8(rec, m::WAIT, flag);
                if flag != 0 { p.wi(rec, m::DELAY, delay); }
            }
            // the six trail points
            let y = f2i(p.rf(rec, m::Y));
            for k in 0..6usize {
                let t = m::TRAIL + 20 * k;
                p.wi(rec, t, k as i32 + 1);
                p.wf(rec, t + 4, p.rf(rec, m::X));
                p.wf(rec, t + 8, i2f(y.wrapping_add(9 * k as i32).wrapping_add(28)));
                p.wi(rec, t + 12, (now.wrapping_add(3 * k as u32)).wrapping_add(8) as i32);
                p.w8(rec, t + 16, 0);
            }
            i += 1;
            if i == n { return true; }
        }
        false
    }

    /// 0x27dfc: move the missiles, hit enemies, explode.
    pub fn update<W: DroneWorld>(&mut self, w: &mut W) {
        if self.count < 1 { return; }
        let now = w.tick();
        for n in 0..MISSILES {
            if self.pool.r8(n, m::ACTIVE) == 0 { continue; }
            if self.pool.r8(n, m::WAIT) != 0 {
                let t = self.pool.ri(n, m::DELAY).wrapping_sub(1);
                self.pool.wi(n, m::DELAY, t);
                if t == 0 { self.pool.w8(n, m::WAIT, 0); }
                continue;
            }
            let step = self.pool.ri(n, m::STEP);
            if step < 21 {
                let y = self.pool.rf(n, m::Y) + i2f(KICK[step.clamp(0, 20) as usize]);
                self.pool.wf(n, m::Y, y);
                self.pool.wi(n, m::STEP, step.wrapping_add(1));
                if step == 12 {
                    self.pool.w8(n, m::FLAME, 1);
                    self.pool.wi(n, m::FLAME_FRAME, 1);
                } else if step == 14 || step == 16 {
                    let f = self.pool.ri(n, m::FLAME_FRAME).wrapping_add(1);
                    self.pool.wi(n, m::FLAME_FRAME, f);
                }
                if self.pool.ri(n, m::STEP) > 20 { self.pool.wi(n, m::STEP, 20); }
            }
            if step >= 20 {
                // the sideways speed dies away
                let v = self.pool.rf(n, m::VX);
                if v > 0.0 {
                    self.pool.wf(n, m::VX, v - 0.1);
                    if self.pool.rf(n, m::VX) < 0.0 { self.pool.wf(n, m::VX, 0.0); }
                }
                let v = self.pool.rf(n, m::VX);
                if v < 0.0 {
                    self.pool.wf(n, m::VX, v + 0.1);
                    if self.pool.rf(n, m::VX) > 0.0 { self.pool.wf(n, m::VX, 0.0); }
                }
            }
            let x = self.pool.rf(n, m::X) + self.pool.rf(n, m::VX);
            self.pool.wf(n, m::X, x);
            if self.pool.r8(n, m::FLAME) != 0 {
                let y = f2i(self.pool.rf(n, m::Y));
                for k in 0..6usize {
                    let t = m::TRAIL + 20 * k;
                    if self.pool.r8(n, t + 16) == 0 && now > self.pool.ri(n, t + 12) as u32 { self.pool.w8(n, t + 16, 1); }
                    self.pool.wf(n, t + 4, x);
                    self.pool.wf(n, t + 8, i2f(y.wrapping_add(9 * k as i32).wrapping_add(28)));
                }
            }
            let (x, y) = (self.pool.rf(n, m::X), self.pool.rf(n, m::Y));
            let (mw, mh) = (self.pool.ri(n, m::W), self.pool.ri(n, m::H));
            if x + i2f(mw) < 0.0 || x > 416.0 || y + i2f(mh) < 0.0 || y > 480.0 {
                self.free(n);
                continue;
            }
            let (ix, iy) = (f2i(x), f2i(y));
            let owner = self.pool.r8(n, m::OWNER);
            let dealt = w.hit_enemies(ix, iy, ix.wrapping_add(mw), iy.wrapping_add(mh), 7, owner);
            if dealt <= 0 { continue; }
            let (cx, cy) = (ix.wrapping_add(mw / 2), iy.wrapping_add(mh / 2));
            w.effect(cx, cy, 14, 0, 39, 0, 0, 0.0, 0.0);
            w.effect(cx.wrapping_sub(7), cy.wrapping_sub(7), 13, 6, 39, 0, 0, 0.0, 0.0);
            w.effect(cx.wrapping_add(8), cy.wrapping_add(5), 13, 13, -1, 0, 0, 0.0, 0.0);
            w.effect(cx.wrapping_add(3), cy.wrapping_sub(4), 13, 22, -1, 0, 0, 0.0, 0.0);
            self.free(n);
        }
    }

    /// 0x28260: draw the missiles, their flames and trails.
    pub fn draw<W: DroneWorld>(&self, w: &mut W) {
        if self.count < 1 { return; }
        let (trans, fancy) = (w.translucent(), w.fancy());
        let soft = if trans { Prim::Trans } else { Prim::Dither };
        for n in 0..MISSILES {
            if self.pool.r8(n, m::ACTIVE) == 0 { continue; }
            let (x, y) = (f2i(self.pool.rf(n, m::X)), f2i(self.pool.rf(n, m::Y)));
            let (mw, mh) = (self.pool.ri(n, m::W), self.pool.ri(n, m::H));
            w.blit(Prim::Anim, x, y, mw, mh, self.pool.ri(n, m::ANIM), self.pool.ri(n, m::FRAME));
            if self.pool.r8(n, m::FLAME) == 0 { continue; }
            w.blit(soft, x, y.wrapping_add(mh), 8, 12, 46, self.pool.ri(n, m::FLAME_FRAME));
            if !fancy { continue; }
            for k in 0..6usize {
                let t = m::TRAIL + 20 * k;
                if self.pool.r8(n, t + 16) == 0 { continue; }
                w.blit(soft, f2i(self.pool.rf(n, t + 4)), f2i(self.pool.rf(n, t + 8)), 8, 8, 47, self.pool.ri(n, t));
            }
        }
    }
}

impl std::fmt::Debug for Missiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Missiles").field("count", &self.count).finish()
    }
}
