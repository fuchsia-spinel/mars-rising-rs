//! Enemy runtime: the 40-slot enemy pool (`D.0fd8`, 176-byte records), the 100 spawn groups (`D.0ca4`, 124-byte records), group spawning
//! (0x2da30), the per-frame loop (0x2dc20: type handler, movement, ship contact), damage (0x2ec14) and the helper routines the type handlers share
//! (target choice 0x2e7a8, nearest ship 0x2e8e0, angle table 0x9350/0x94bc).
//!
//! The per-type routines (22 init routines, 20 update routines) are in `enemy_ai.rs`.
//!
//! Verified against the original executable (`tests/enemy_golden.rs`, vectors made by `tools/re/gen_enemy_vectors.py`, which runs the original
//! PowerPC code in `tools/re/ppcemu.py`): the whole pool, the group table, the live-group counter and the sequence of calls made to the surrounding
//! game (random numbers, score, damage to the ships, effects, sounds, enemy shots) must be identical.
//!
//! Records are kept as raw bytes with the original offsets so that every store has the same width and order as the original code; the
//! offsets are named in [`f`] (enemy slot) and [`g`] (group record).
//!
//! Floating point: the original does single-precision arithmetic (`fadds`, `fmuls`, ... round to f32 after every operation); Rust `f32`
//! operators do the same. `fmadds` is `mul_add`. Conversions float -> int are `fctiwz` (truncate, saturate, NaN -> i32::MIN).

pub const SLOTS: usize = 40;
pub const SLOT_SIZE: usize = 176;
pub const GROUPS: usize = 100;
pub const GROUP_SIZE: usize = 124;

/// Offsets inside an enemy slot.
pub mod f {
    pub const ACTIVE: usize = 0; // u8
    pub const SPAWN_TICK: usize = 4;
    pub const TYPE: usize = 8;
    pub const F16: usize = 16;
    pub const DELAY: usize = 20; // frames until the enemy starts acting (counts down, signed)
    pub const X: usize = 24; // f32 position (top left)
    pub const Y: usize = 28;
    pub const VX: usize = 32; // f32 velocity
    pub const VY: usize = 36;
    pub const W: usize = 48; // size
    pub const H: usize = 52;
    pub const AX: usize = 40; // f32 acceleration (some types)
    pub const AY: usize = 44;
    pub const BX0: usize = 56; // f32 hit box (left, top, right, bottom)
    pub const BY0: usize = 60;
    pub const BX1: usize = 64;
    pub const BY1: usize = 68;
    pub const HW: usize = 72; // half size
    pub const HH: usize = 76;
    pub const ANIM: usize = 80; // animation table index
    pub const STATE: usize = 84;
    pub const T88: usize = 88;
    pub const HP: usize = 96; // damage it can take before it dies
    pub const DMG: usize = 100; // damage taken so far
    pub const B104: usize = 104;
    pub const HIT: usize = 105; // set when hurt (but not killed)
    pub const HIT_TICK: usize = 108;
    pub const TIMER: usize = 112; // generic tick stamp
    pub const FX_DEATH: usize = 116; // effect id on death (-1: none)
    pub const PART_DEATH: usize = 120; // particle id on death (-1: none)
    pub const AUX_DEATH: usize = 124; // i8 aux effect id on death (-1: none)
    pub const SND_HIT: usize = 125; // i8 sound when hurt (-1: none)
    pub const SND_DEATH: usize = 126; // i8 sound when killed (-1: none)
    pub const GROUP: usize = 128;
    pub const SCORE: usize = 132;
    pub const F136: usize = 136;
    pub const B148: usize = 148;
    pub const BOSS: usize = 168; // u8: non-zero when the slot belongs to a boss structure
    pub const B169: usize = 169;
    pub const BOSS_IDX: usize = 172; // i32
}

/// Offsets inside a group record.
pub mod g {
    pub const ACTIVE: usize = 0; // u8
    pub const MEMBERS: usize = 4; // up to 10 x i32 slot indices (-1: none)
    pub const TYPE: usize = 44;
    pub const COUNT: usize = 48;
    pub const ALIVE: usize = 52;
    pub const KILLED: usize = 56;
    pub const P60: usize = 60;
    pub const A64: usize = 64; // converted by 0x191ac at spawn
    pub const B68: usize = 68; // converted by 0x19248 at spawn
    pub const P72: usize = 72;
    pub const P76: usize = 76;
    pub const P80: usize = 80;
    pub const TVEC: usize = 84;
}

/// What an enemy needs to know about one ship (the getters 0xe36c.. in the original).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ShipView {
    pub alive: bool,      // state == 3
    pub vulnerable: bool, // not shielded (invulnerability byte == 0)
    pub x: f32,
    pub y: f32,
    pub w: i32,
    pub h: i32,
    pub hw: i32, // +44 / +48 of the ship (hit half size)
    pub hh: i32,
}

/// Everything the enemy code takes from, or hands to, the rest of the game. Every method corresponds to one routine of the original.
pub trait EnemyWorld {
    /// 0xecf0: the frame tick (30 per second).
    fn tick(&self) -> u32;
    /// 0xece0: number of players (1 or 2).
    fn players(&self) -> u8;
    /// Ship 1 or 2.
    fn ship(&self, n: u8) -> ShipView;
    /// 0xa0d4: random number in `lo..=hi`.
    fn random(&mut self, lo: i32, hi: i32) -> i32;
    /// 0xd7a0: the global at `D.13ec` (level difficulty).
    fn difficulty(&self) -> u32;
    /// 0xe68c / 0xe6c0: score for ship 1 / 2.
    fn score(&mut self, owner: u8, points: i32);
    /// 0xe6f4 / 0xe720: damage done to ship 1 / 2 by touching an enemy.
    fn ship_damage(&mut self, n: u8, amount: i32);
    /// 0x22670: effect (explosion, sparks..).
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32);
    /// 0x29714: auxiliary effect (debris).
    fn aux(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32);
    /// 0x23440: particle burst.
    fn particles(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32);
    /// 0x29edc: group wiped out -> bonus item at (x, y) for the ship `owner`.
    fn group_bonus(&mut self, owner: u8, x: f32, y: f32);
    /// 0x45bac: sound effect (id, priority, delay).
    fn sound(&mut self, id: i32, prio: i32, delay: i32);
    /// 0x45d54: sound with playback rate.
    fn sound_rate(&mut self, id: i32, prio: i32, rate: i32);
    /// 0x383b8: aimed enemy shot (`kind`, `target` ship 1/2, `flag`, x, y); returns non-zero when a shot was created.
    fn shot_aimed(&mut self, kind: i32, target: i32, flag: i32, x: f32, y: f32) -> i32;
    /// 0x38754: enemy shot with explicit parameters (r5..r10 of the call, x, y).
    fn shot_param(&mut self, a: [i32; 6], x: f32, y: f32) -> i32;
    /// 0x2a040: power-up (kind, variant, param, announce) centred on (x, y); dropped by destroyed bosses.
    fn pickup(&mut self, _kind: i32, _variant: i32, _param: i32, _announce: i32, _x: f32, _y: f32) {}
    /// 0x45e24: start a boss's looping alarm sound (`id`, priority, boss index); the audio callback 0x4123c asks for a restart when it ends.
    /// Returns the voice handle.
    fn sound_loop(&mut self, _id: i32, _prio: i32, _boss: i32) -> u32 { 0 }
}

/// `fctiwz`: truncate toward zero, saturating; NaN gives the "invalid" value `i32::MIN`.
#[inline]
pub fn f2i(x: f32) -> i32 {
    if x.is_nan() { i32::MIN } else { x as i32 }
}

#[inline]
pub fn f2i64(x: f64) -> i32 {
    if x.is_nan() { i32::MIN } else { x as i32 }
}

/// 0x929c: `|x|` as the original writes it (`x <= 0 ? -x : x`).
#[inline]
fn abs_d(x: f64) -> f64 {
    if !(x > 0.0) { -x } else { x }
}

/// The arctangent table at `D.8b2c` (1024 x i16), filled once at start-up by 0x92b4: `trunc(atan(i * 0.01) * 57.2957795)`.
pub fn arctan_table() -> [i16; 1024] {
    let mut t = [0i16; 1024];
    for (i, v) in t.iter_mut().enumerate() {
        let a = ((i as f64) * f64::from_bits(0x3f847ae147ae147b)).atan() * 57.2957795;
        *v = f2i64(a) as i16;
    }
    t
}

/// 0x9350: angle in degrees (0..359) of the vector (x, y), computed through the arctangent table. Note the original's quadrant logic.
pub fn angle(table: &[i16; 1024], x: f64, y: f64) -> i32 {
    let ay = abs_d(y);
    let ax = abs_d(x);
    let q = if ax < ay { abs_d(x / y) } else { abs_d(y / x) };
    let mut idx = f2i64(100.0 * q);
    if (idx as i16) < 0 {
        idx = 0;
    }
    if (idx as i16) > 1023 {
        idx = 1023;
    }
    let mut r = table[(idx as i16) as usize] as i32;
    if (r as i16) < 0 {
        r = r.wrapping_neg();
    }
    if ax < ay {
        r = 90 - r;
    }
    if x < 0.0 && y >= 0.0 {
        r = 180 - r;
    }
    if x < 0.0 && y < 0.0 {
        r += 180;
    }
    if x >= 0.0 && y < 0.0 {
        r = r.wrapping_neg();
    }
    r -= 90;
    if (r as i16) < 0 {
        r += 360;
    }
    if (r as i16) >= 360 {
        r -= 360;
    }
    r
}

/// 0x94bc: angle from the point (x2, y2) to (x1, y1), all 16-bit.
pub fn angle_between(table: &[i16; 1024], x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    let dx = (x1 as i16 as i32) - (x2 as i16 as i32);
    let dy = (y1 as i16 as i32) - (y2 as i16 as i32);
    angle(table, dx as f64, dy as f64)
}

/// The enemy pool and the spawn groups.
#[derive(Debug, Clone)]
pub struct Enemies {
    pub pool: Vec<u8>,   // SLOTS * SLOT_SIZE
    pub groups: Vec<u8>, // GROUPS * GROUP_SIZE
    /// D.17e4: number of live groups.
    pub live_groups: i32,
    /// The boss structures (`boss.rs`).
    pub bosses: crate::boss::Bosses,
    pub arctan: Box<[i16; 1024]>,
}

impl Default for Enemies {
    fn default() -> Self { Self::new() }
}

impl Enemies {
    pub fn new() -> Self {
        Enemies { pool: vec![0; SLOTS * SLOT_SIZE], groups: vec![0; GROUPS * GROUP_SIZE], live_groups: 0, bosses: crate::boss::Bosses::new(), arctan: Box::new(arctan_table()) }
    }

    // ---- raw accessors (slot i, offset o) -------------------------------------------------------------------------------------------
    #[inline] pub fn r8(&self, i: usize, o: usize) -> u8 { self.pool[i * SLOT_SIZE + o] }
    #[inline] pub fn ri8(&self, i: usize, o: usize) -> i8 { self.pool[i * SLOT_SIZE + o] as i8 }
    #[inline] pub fn w8(&mut self, i: usize, o: usize, v: u8) { self.pool[i * SLOT_SIZE + o] = v }
    #[inline] pub fn r32(&self, i: usize, o: usize) -> u32 {
        let a = i * SLOT_SIZE + o;
        u32::from_be_bytes([self.pool[a], self.pool[a + 1], self.pool[a + 2], self.pool[a + 3]])
    }
    #[inline] pub fn ri32(&self, i: usize, o: usize) -> i32 { self.r32(i, o) as i32 }
    #[inline] pub fn w32(&mut self, i: usize, o: usize, v: u32) {
        let a = i * SLOT_SIZE + o;
        self.pool[a..a + 4].copy_from_slice(&v.to_be_bytes());
    }
    #[inline] pub fn wi32(&mut self, i: usize, o: usize, v: i32) { self.w32(i, o, v as u32) }
    #[inline] pub fn rf(&self, i: usize, o: usize) -> f32 { f32::from_bits(self.r32(i, o)) }
    #[inline] pub fn wf(&mut self, i: usize, o: usize, v: f32) { self.w32(i, o, v.to_bits()) }

    #[inline] pub fn gr8(&self, g: usize, o: usize) -> u8 { self.groups[g * GROUP_SIZE + o] }
    #[inline] pub fn gw8(&mut self, g: usize, o: usize, v: u8) { self.groups[g * GROUP_SIZE + o] = v }
    #[inline] pub fn gr32(&self, g: usize, o: usize) -> u32 {
        let a = g * GROUP_SIZE + o;
        u32::from_be_bytes([self.groups[a], self.groups[a + 1], self.groups[a + 2], self.groups[a + 3]])
    }
    #[inline] pub fn gri32(&self, g: usize, o: usize) -> i32 { self.gr32(g, o) as i32 }
    #[inline] pub fn gw32(&mut self, g: usize, o: usize, v: u32) {
        let a = g * GROUP_SIZE + o;
        self.groups[a..a + 4].copy_from_slice(&v.to_be_bytes());
    }

    /// 0x2d98c: free a slot.
    pub fn clear_slot(&mut self, i: usize) {
        self.w8(i, f::ACTIVE, 0);
        self.w8(i, f::B148, 0);
    }

    /// 0x191ac: converts the group's `+64` parameter (a table index) to its value.
    pub fn convert_a(v: u32) -> u32 {
        const T: [u32; 15] = [99999, 99999, 10, 15, 20, 25, 30, 40, 50, 60, 70, 80, 90, 120, 150];
        if v > 14 { 99999 } else { T[v as usize] }
    }

    /// 0x19248: converts the group's `+68` parameter (a table index) to its value (hit points of the members).
    pub fn convert_b(v: u32) -> u32 {
        const T: [u32; 34] = [
            0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 15, 20, 30, 40, 50, 60, 70, 80, 90, 100, 150, 200, 250, 300, 400, 500, 600, 700, 800, 900, 1000, 9999,
        ];
        if v > 33 { 0 } else { T[v as usize] }
    }

    /// 0x2da30: launch group `gi`: needs `count` free slots; calls the group type's init routine.
    pub fn spawn_group<W: EnemyWorld>(&mut self, gi: usize, w: &mut W) {
        if !(w.ship(1).alive || w.ship(2).alive) {
            return;
        }
        if self.gr8(gi, g::ACTIVE) != 0 {
            return;
        }
        let ty = self.gr32(gi, g::TYPE);
        let mut count = self.gri32(gi, g::COUNT);
        if !(0..=10).contains(&count) {
            count = 10;
        }
        if count == 0 {
            return; // the original never finishes this case (it scribbles over the stack); nothing is spawned
        }
        let mut found = [-1i32; 10];
        let mut n = 0usize;
        let mut ok = false;
        for s in 0..SLOTS {
            if self.r8(s, f::ACTIVE) == 0 {
                found[n] = s as i32;
                n += 1;
                if n as i32 == count {
                    ok = true;
                    break;
                }
            }
        }
        if !ok {
            return;
        }
        self.gw8(gi, g::ACTIVE, 1);
        self.gw32(gi, g::TYPE, ty);
        self.gw32(gi, g::COUNT, count as u32);
        self.gw32(gi, g::ALIVE, count as u32);
        self.gw32(gi, g::KILLED, 0);
        let a = self.gr32(gi, g::A64);
        self.gw32(gi, g::A64, Self::convert_a(a));
        let b = self.gr32(gi, g::B68);
        self.gw32(gi, g::B68, Self::convert_b(b));
        for k in 0..count as usize {
            self.gw32(gi, g::MEMBERS + 4 * k, found[k] as u32);
        }
        self.init_type(ty, gi, w);
        self.live_groups += 1;
    }

    /// Level start (0x2d528 / 0x2d7b8): the level's group records (`_ASG`, 100 x 124 bytes, given as the 31 big-endian words of each defined
    /// record) are copied in, every group is marked "not launched" (0x2d9a8 clears byte 0 of all of them), and the enemy pool is emptied.
    pub fn load_level_groups(&mut self, recs: &[(usize, Vec<u32>)]) {
        self.pool.iter_mut().for_each(|b| *b = 0);
        self.groups.iter_mut().for_each(|b| *b = 0);
        self.live_groups = 0;
        self.bosses.free_all();
        let mut defined = Vec::new();
        for (idx, words) in recs {
            if *idx >= GROUPS {
                continue;
            }
            for (k, w) in words.iter().take(GROUP_SIZE / 4).enumerate() {
                self.gw32(*idx, k * 4, *w);
            }
            if self.gr8(*idx, g::ACTIVE) != 0 {
                defined.push(*idx);
            }
            self.gw8(*idx, g::ACTIVE, 0);
        }
        // 0x2d7a8: every defined group with word 22 != 0 is registered as a boss (in group order); the main loop's set-up (0xd9b0) then frees all
        // records, which keeps what was registered but leaves them unused until 0x40e7c launches them
        defined.sort_unstable();
        for idx in defined {
            if self.gr32(idx, 88) != 0 {
                self.boss_register(idx);
            }
        }
        self.bosses.free_all();
    }

    /// Is this group a "special" one (word 22 non-zero: boss / scripted launch through 0x40e7c, which is not ported)?
    pub fn group_is_special(&self, gi: usize) -> bool {
        gi < GROUPS && self.gr32(gi, 88) != 0
    }

    /// The shot-vs-enemy view of the pool for 0x2ead4 (`state` is the start delay at +20).
    pub fn boxes(&self) -> Vec<crate::ship::EnemyBox> {
        (0..SLOTS)
            .map(|i| crate::ship::EnemyBox {
                active: self.r8(i, f::ACTIVE) != 0,
                state: self.ri32(i, f::DELAY),
                left: self.rf(i, f::BX0),
                top: self.rf(i, f::BY0),
                right: self.rf(i, f::BX1),
                bottom: self.rf(i, f::BY1),
            })
            .collect()
    }

    /// Number of live enemies.
    pub fn count(&self) -> usize {
        (0..SLOTS).filter(|&i| self.r8(i, f::ACTIVE) != 0).count()
    }

    /// 0x2e6a8: the smart bomb. Every enemy whose box is (roughly) on screen takes 10000 damage from `owner`. The original's range test mixes the
    /// axes up (it compares the box's x with 480 and its y with 544); that is kept. Boss parts go to 0x41c88.
    pub fn bomb<W: EnemyWorld>(&mut self, owner: u8, w: &mut W) {
        for i in 0..SLOTS {
            if self.r8(i, f::ACTIVE) == 0 {
                continue;
            }
            if !(self.rf(i, f::BX1) >= -64.0) || self.rf(i, f::BX0) >= 480.0 || !(self.rf(i, f::BY1) >= -64.0) || self.rf(i, f::BY0) >= 544.0 {
                continue;
            }
            if self.r8(i, f::BOSS) != 0 {
                let bi = self.ri32(i, f::BOSS_IDX);
                self.boss_hit(i as i32, bi, 10000, owner, w);
            } else {
                self.damage(i, 10000, owner, w);
            }
        }
    }

    /// 0x2ec14: `amount` damage to slot `i` from ship `owner` (1/2). Returns the damage that counted.
    pub fn damage<W: EnemyWorld>(&mut self, i: usize, amount: i32, owner: u8, w: &mut W) -> i32 {
        if self.r8(i, f::ACTIVE) == 0 {
            return 0;
        }
        let old = self.ri32(i, f::DMG);
        let new = old.wrapping_add(amount);
        self.wi32(i, f::DMG, new);
        let hp = self.ri32(i, f::HP);
        if new > hp {
            let part = self.ri32(i, f::PART_DEATH);
            let fx = self.ri32(i, f::FX_DEATH);
            let x = f2i(self.rf(i, f::X)).wrapping_add(self.ri32(i, f::HW));
            let y = f2i(self.rf(i, f::Y)).wrapping_add(self.ri32(i, f::HH));
            let ret = (new.wrapping_sub(old)).wrapping_sub(new.wrapping_sub(hp)).wrapping_add(1);
            let aux = self.r8(i, f::AUX_DEATH);
            if aux as i8 != -1 {
                w.aux(x, y, aux as i32, 0, -1, 0);
            }
            if fx != -1 {
                let snd = self.r8(i, f::SND_DEATH);
                w.effect(x, y, fx, 0, snd as i32, 0, 0, self.rf(i, f::VX), self.rf(i, f::VY));
            } else {
                let snd = self.r8(i, f::SND_DEATH);
                if snd as i8 != -1 {
                    w.sound(snd as i32, 3, 0);
                }
            }
            if part != -1 {
                w.particles(x, y, part, 8, 0, 0);
            }
            let pts = self.ri32(i, f::SCORE);
            w.score(owner, pts);
            let gi = self.r32(i, f::GROUP) as usize;
            let alive = self.gri32(gi, g::ALIVE).wrapping_sub(1);
            self.gw32(gi, g::ALIVE, alive as u32);
            let killed = self.gri32(gi, g::KILLED).wrapping_add(1);
            self.gw32(gi, g::KILLED, killed as u32);
            if killed == self.gri32(gi, g::COUNT) {
                let bx = self.rf(i, f::X) + (self.ri32(i, f::HW) as f32);
                let by = self.rf(i, f::Y) + (self.ri32(i, f::HH) as f32);
                w.group_bonus(owner, bx, by);
            }
            if self.gri32(gi, g::ALIVE) == 0 {
                self.gw8(gi, g::ACTIVE, 0);
                self.live_groups -= 1;
            }
            self.clear_slot(i);
            ret
        } else {
            self.w8(i, f::HIT, 1);
            let t = w.tick();
            self.w32(i, f::HIT_TICK, t);
            let snd = self.r8(i, f::SND_HIT);
            if snd as i8 != -1 {
                w.sound(snd as i32, 2, 0);
            }
            amount
        }
    }

    /// The contact box of a ship (0x2dc20 prologue): `Some((left, top, right, bottom))` when the ship is alive and not shielded.
    pub(crate) fn contact_box(s: &ShipView) -> Option<(i32, i32, i32, i32)> {
        if !s.alive || !s.vulnerable {
            return None;
        }
        let ix = f2i(s.x);
        let iy = f2i(s.y);
        Some((ix.wrapping_add(6), iy.wrapping_add(6), ix.wrapping_add(s.w.wrapping_sub(6)), iy.wrapping_add(s.h.wrapping_sub(6))))
    }

    /// 0x2dc20: one frame of every enemy: type routine, movement, contact with the ships.
    pub fn frame<W: EnemyWorld>(&mut self, w: &mut W) {
        let b1 = Self::contact_box(&w.ship(1));
        let b2 = Self::contact_box(&w.ship(2));
        for i in 0..SLOTS {
            if self.r8(i, f::ACTIVE) == 0 || self.r8(i, f::BOSS) != 0 {
                continue;
            }
            let d = self.ri32(i, f::DELAY).wrapping_sub(1);
            self.wi32(i, f::DELAY, d);
            if d > 0 {
                continue;
            }
            let ty = self.r32(i, f::TYPE);
            if ty > 23 {
                return;
            }
            self.update_type(ty, i, w); // types 0 and 22 have no routine: they only move
            // movement
            let vx = self.rf(i, f::VX);
            let vy = self.rf(i, f::VY);
            let v = self.rf(i, f::X) + vx;
            self.wf(i, f::X, v);
            let v = self.rf(i, f::Y) + vy;
            self.wf(i, f::Y, v);
            let vx = self.rf(i, f::VX);
            let v = self.rf(i, f::BX0) + vx;
            self.wf(i, f::BX0, v);
            let vy = self.rf(i, f::VY);
            let v = self.rf(i, f::BY0) + vy;
            self.wf(i, f::BY0, v);
            let vx = self.rf(i, f::VX);
            let v = self.rf(i, f::BX1) + vx;
            self.wf(i, f::BX1, v);
            let vy = self.rf(i, f::VY);
            let v = self.rf(i, f::BY1) + vy;
            self.wf(i, f::BY1, v);
            for (n, bx) in [(1u8, b1), (2u8, b2)] {
                if n == 2 && self.r8(i, f::ACTIVE) == 0 {
                    continue; // (only ship 2 is tested against a slot that a type routine or ship 1's contact has just freed)
                }
                // `bx` is None unless the ship was alive and unshielded when the frame started; being alive *now* is tested again
                let Some((l, t, r, b)) = bx else { continue };
                if !w.ship(n).alive {
                    continue;
                }
                if l >= f2i(self.rf(i, f::BX1)) || f2i(self.rf(i, f::BX0)) >= r || t >= f2i(self.rf(i, f::BY1)) || f2i(self.rf(i, f::BY0)) >= b {
                    continue;
                }
                let dmg = self.ri32(i, f::HP).wrapping_sub(self.ri32(i, f::DMG)).wrapping_add(1);
                w.ship_damage(n, dmg);
                if self.r8(i, f::BOSS) == 0 {
                    self.damage(i, 5, n, w);
                } else {
                    let bi = self.ri32(i, f::BOSS_IDX);
                    self.boss_hit(i as i32, bi, 5, n, w);
                }
            }
        }
    }
}
