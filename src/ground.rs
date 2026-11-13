//! Ground objects: the turrets, missile towers, tanks and other things on the ground that move with the scrolling, shoot at the ships and can be
//! destroyed. This is the object system of the original (pool `D.0934` of 250 records of 168 bytes, the active list `D.108c` of 50 entries, the decal
//! list `D.1054` of 50 entries of 32 bytes), with
//!
//! * level load 0x3ade0 (the `__GS` resource -> records, conversions 0x191ac / 0x19248, per-kind set-up 0x3b9b0 = `ground_init.rs`, anchoring 0x3b248),
//! * target counting 0x3acb0 and activation of the row that scrolls in (0x3b450 / 0x3b4f8),
//! * the per-frame update 0x3d0fc with the nine behaviours (kinds 1, 2, 4, 7, 8, 15, 22, 24, 25 and the default shooter) and the movement/blocking step,
//! * damage 0x40040 (hit flash, destruction, effects, score, drops, wreck decals, chain explosions), box damage 0x3fec8, "everything on screen" 0x3fbfc,
//!   the blocking test 0x3fd1c, the progress counter 0x40854 and the decal list (0x40954 add, 0x40b50 scroll, 0x40bf4 draw, 0x40904 clear),
//! * the move / cull / draw pass 0x3f554 and the baked-object pass 0x3fa24.
//!
//! Verified against the original executable by `tests/ground_golden.rs` (vectors from `tools/re/gen_ground_vectors.py`, which runs the original PowerPC
//! routines): the pool, the active list, the decal list, the counters and the sequence of calls to the surroundings must be identical after every step.
//!
//! Records are raw bytes with the original offsets (see [`o`]). Floating point is single precision like the original (`fadds`, `fsubs`, ...).

use crate::draw::Prim;
use crate::enemy::{angle_between, arctan_table, f2i, Enemies, ShipView};
use crate::ground_init;

pub const RECORDS: usize = 250;
pub const REC: usize = 168;
pub const LIST: usize = 50;
pub const DECALS: usize = 50;
pub const DECAL: usize = 32;
/// Size of one record of the `__GS` resource.
pub const RES_REC: usize = 50;

/// Record offsets.
pub mod o {
    pub const ACTIVE: usize = 0; // u8: record exists / is active
    pub const SHADOW: usize = 1; // u8: casts a drop shadow
    pub const START: usize = 4; // u32 activation tick
    pub const KIND: usize = 8;
    pub const STATE: usize = 12; // i32 (1 = stopped)
    pub const W: usize = 16; // i32 size
    pub const H: usize = 20;
    pub const X: usize = 24; // f32 position (top-left)
    pub const Y: usize = 28;
    pub const HX: usize = 32; // f32 half size
    pub const HY: usize = 36;
    pub const VX: usize = 40; // f32 velocity
    pub const VY: usize = 44;
    pub const F48: usize = 48; // u8: animated every frame (never baked into the background)
    pub const F49: usize = 49; // u8: already baked into the background
    pub const F50: usize = 50; // u8: shoots (default shooter)
    pub const F51: usize = 51; // u8: solid for other objects
    pub const ANIM: usize = 52; // i32 animation index (draw)
    pub const FRAME: usize = 56; // i32 frame (draw, 1-based)
    pub const FLASH: usize = 60; // u32 tick of the last hit
    pub const DAMAGE: usize = 64; // i32 damage taken so far
    pub const HIT: usize = 68; // u8: hit flash
    pub const LAST: usize = 72; // u32 tick of the last shot
    pub const VARIANT: usize = 76;
    pub const WRECK_ANIM: usize = 80; // i32 animation of the wreck decal (-1: none)
    pub const WRECK_FRAME: usize = 84; // i32
    pub const PART: usize = 88; // i32 particle id on death (-1: none)
    pub const PREV: usize = 92; // i32 record this one is attached to (-1: none)
    pub const NEXT: usize = 96; // i32 attached record (-1: none)
    pub const SCORE: usize = 100; // i32
    pub const B104: usize = 104; // u8: does not count as a target (decoration)
    pub const B105: usize = 105; // u8
    pub const B106: usize = 106; // u8: behaviour flag
    pub const B107: usize = 107; // u8: can be hit
    pub const N108: usize = 108;
    pub const INTERVAL: usize = 112; // u32 ticks between shots (converted at load)
    pub const HP: usize = 116; // i32 damage it takes (converted at load)
    pub const DROP_A: usize = 120; // i32 drop (kind 3 pickup variant)
    pub const DROP_B: usize = 124; // i32 drop (pickup kind)
    pub const ANIM_TS: usize = 132; // u32 tick of the last animation step
    pub const DEATH_AUX: usize = 136; // u8 (as i8): aux effect id on death (-1: none)
    pub const T140: usize = 140; // u32
    pub const START_DELAY: usize = 144; // u32 tick when it starts acting
}

const MUZZLE_A: [(i16, i16); 24] = ground_init::MUZZLE_A;
const MUZZLE_B: [(i16, i16); 24] = ground_init::MUZZLE_B;

/// What the ground objects need from, or hand to, the rest of the game. Every method is one routine of the original.
pub trait GroundWorld {
    /// 0xecf0: the frame tick.
    fn tick(&self) -> u32;
    /// 0xece0: number of players.
    fn players(&self) -> u8;
    /// Ship 1 or 2 (state, position at +20 / +24).
    fn ship(&self, n: u8) -> ShipView;
    /// 0xa0d4: random number in `lo..=hi`.
    fn random(&mut self, lo: i32, hi: i32) -> i32;
    /// 0xd7a0: the level number / difficulty `D.13ec`.
    fn difficulty(&self) -> u32;
    /// 0xc098: scroll step per frame (`D.13c4`); 0 = the world stands still.
    fn scroll_step(&self) -> i32;
    /// 0xc08c: `D.9394` (s16), the vertical offset of the background buffer.
    fn buffer_offset(&self) -> i32;
    /// 0x2994: shadows enabled.
    fn shadows(&self) -> bool;
    /// 0xe68c / 0xe6c0: score for ship `owner`.
    fn score(&mut self, owner: u8, points: i32);
    /// 0x22670 effect, 0x29714 aux effect, 0x23440 particle burst.
    #[allow(clippy::too_many_arguments)]
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32);
    fn aux(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32);
    fn particles(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32);
    /// 0x45bac: sound effect (id, priority, delay).
    fn sound(&mut self, id: i32, prio: i32, delay: i32);
    /// 0x2a040: power-up (kind, variant, param, announce) centred on (x, y).
    fn pickup(&mut self, kind: i32, variant: i32, param: i32, announce: i32, x: f32, y: f32);
    /// 0x37fd4: beam shot (kind, sound, target ship, flag) from (x, y) with the muzzle flash at (mx, my).
    #[allow(clippy::too_many_arguments)]
    fn beam(&mut self, kind: i32, snd: i32, target: i32, flag: i32, x: f32, y: f32, mx: f32, my: f32);
    /// 0x383b8: aimed shot (kind, target ship, flag) from (x, y).
    fn shot_aimed(&mut self, kind: i32, target: i32, flag: i32, x: f32, y: f32) -> i32;
    /// 0xea4c: the last ground target was destroyed: bonus at (x, y).
    fn clear_bonus(&mut self, x: f32, y: f32);
    /// The draw primitives of the original, called with `(0, x, y, w, h, anim, frame)`:
    /// 0x2f88 sprite on the screen, 0x3358 its drop shadow, 0x3540 hit flash, 0x3728 sprite burnt into the background, 0x3910 its shadow, 0x381c.
    fn prim(&mut self, which: u32, a: [i32; 7]);
}

/// The 50-byte record of the level's `__GS` resource for one ground object, rebuilt from the fields the extractor stores (`assets::GroundSprite`):
/// `[0]` / `[1]` = flag bytes, `[2..14]` = kind, parameter, variant (i32), `[14..22]` = x, y (f32), `[22..30]` = previous / next link, `[30..50]` = five more words.
pub fn resource_record(flags: &[u8], abc: [i32; 3], x: f32, y: f32, links: [i32; 2], tail: &[i32]) -> [u8; RES_REC] {
    let mut r = [0u8; RES_REC];
    r[0] = flags.first().copied().unwrap_or(0);
    r[1] = flags.get(1).copied().unwrap_or(0);
    for (k, v) in abc.iter().enumerate() {
        r[2 + k * 4..6 + k * 4].copy_from_slice(&v.to_be_bytes());
    }
    r[14..18].copy_from_slice(&x.to_be_bytes());
    r[18..22].copy_from_slice(&y.to_be_bytes());
    r[22..26].copy_from_slice(&links[0].to_be_bytes());
    r[26..30].copy_from_slice(&links[1].to_be_bytes());
    for k in 0..5 {
        let v = tail.get(k).copied().unwrap_or(0);
        r[30 + k * 4..34 + k * 4].copy_from_slice(&v.to_be_bytes());
    }
    r
}

/// 0x9604: is ship 2 "farther" than ship 1 from (ox, oy)? (The original's distance measure: literally what the code computes.)
pub fn farther(ox: f32, oy: f32, s1x: f32, s1y: f32, s2x: f32, s2y: f32) -> bool {
    let zero = 0.0f32;
    let d = |sx: f32, sy: f32| -> f32 {
        let dx = sx - ox;
        if dx < zero {
            -dx
        } else {
            let dy = sy - oy;
            let s = dx + dy;
            if s < zero { -dy } else { dy }
        }
    };
    let a = d(s1x, s1y);
    let b = d(s2x, s2y);
    b > a
}

/// The object system.
#[derive(Clone)]
pub struct Ground {
    pub pool: Vec<u8>,
    pub list: [i32; LIST],
    pub decals: Vec<u8>,
    /// D.1a60: number of targets (records with `+104 == 0` placed in the level).
    pub targets: i32,
    /// D.1a64: targets destroyed so far.
    pub destroyed: i32,
    /// D.1a68: records activated so far.
    pub activated: i32,
    /// Records that [`Ground::move_draw`] burnt into the background in its latest call. The original copies its background buffer to the screen
    /// (0xC5B4) before the ground pass (0x3F554), so such an object is drawn live in that frame and appears in the buffer only from the next one;
    /// [`Ground::render_background`] leaves these out for that one frame (drawing both doubles the sprite and darkens its shadow twice).
    baked_now: Vec<usize>,
    arctan: Box<[i16; 1024]>,
}

impl Default for Ground {
    fn default() -> Self { Self::new() }
}

/// Everything about one ship the aiming code needs.
#[derive(Clone, Copy)]
struct Ships {
    one: bool,
    a1: bool,
    a2: bool,
    p1: (f32, f32),
    p2: (f32, f32),
}

impl Ships {
    fn get<W: GroundWorld>(w: &W) -> Ships {
        let one = w.players() == 1;
        let s1 = w.ship(1);
        let s2 = w.ship(2);
        Ships { one, a1: s1.alive, a2: s2.alive, p1: (s1.x, s1.y), p2: (s2.x, s2.y) }
    }
}

impl Ground {
    /// 0x3a998: the object system at level start: everything free.
    pub fn new() -> Self {
        Ground { pool: vec![0; RECORDS * REC], list: [-1; LIST], decals: vec![0; DECALS * DECAL], targets: 0, destroyed: 0, activated: 0, baked_now: Vec::new(), arctan: Box::new(arctan_table()) }
    }

    // ---- raw accessors ------------------------------------------------------------------------------------------------------------
    #[inline] pub fn r8(&self, i: usize, off: usize) -> u8 { self.pool[i * REC + off] }
    #[inline] pub fn w8(&mut self, i: usize, off: usize, v: u8) { self.pool[i * REC + off] = v }
    #[inline] pub fn r32(&self, i: usize, off: usize) -> u32 {
        let a = i * REC + off;
        u32::from_be_bytes([self.pool[a], self.pool[a + 1], self.pool[a + 2], self.pool[a + 3]])
    }
    #[inline] pub fn ri32(&self, i: usize, off: usize) -> i32 { self.r32(i, off) as i32 }
    #[inline] pub fn w32(&mut self, i: usize, off: usize, v: u32) {
        let a = i * REC + off;
        self.pool[a..a + 4].copy_from_slice(&v.to_be_bytes());
    }
    #[inline] pub fn wi32(&mut self, i: usize, off: usize, v: i32) { self.w32(i, off, v as u32) }
    #[inline] pub fn rf(&self, i: usize, off: usize) -> f32 { f32::from_bits(self.r32(i, off)) }
    #[inline] pub fn wf(&mut self, i: usize, off: usize, v: f32) { self.w32(i, off, v.to_bits()) }
    #[inline] fn d8(&self, s: usize, off: usize) -> u8 { self.decals[s * DECAL + off] }
    #[inline] fn d32(&self, s: usize, off: usize) -> u32 {
        let a = s * DECAL + off;
        u32::from_be_bytes([self.decals[a], self.decals[a + 1], self.decals[a + 2], self.decals[a + 3]])
    }
    #[inline] fn dw32(&mut self, s: usize, off: usize, v: u32) {
        let a = s * DECAL + off;
        self.decals[a..a + 4].copy_from_slice(&v.to_be_bytes());
    }

    /// Is the record in the level and still alive (not destroyed / scrolled away)?
    pub fn alive(&self, i: usize) -> bool { self.r8(i, o::ACTIVE) != 0 }

    // ---- list handling ------------------------------------------------------------------------------------------------------------
    /// 0x3ad74: put a record in the first free list entry (nothing happens when the list is full).
    fn list_add(&mut self, i: usize) {
        for k in 0..LIST {
            if self.list[k] == -1 {
                self.list[k] = i as i32;
                return;
            }
        }
    }

    /// 0x3ada8: take a record out of the list.
    fn list_remove(&mut self, i: usize) {
        for k in 0..LIST {
            if self.list[k] == i as i32 {
                self.list[k] = -1;
                return;
            }
        }
    }

    /// 0x3ac80: deactivate a record.
    pub fn deactivate(&mut self, i: usize) {
        self.w8(i, o::ACTIVE, 0);
        self.list_remove(i);
    }

    // ---- level load ---------------------------------------------------------------------------------------------------------------
    /// 0x3ade0: build the pool from the level's `__GS` resource (250 records of 50 bytes).
    pub fn load<W: GroundWorld>(&mut self, res: &[u8], w: &W) {
        let tick = w.tick();
        let scroll = w.scroll_step();
        for i in 0..RECORDS {
            let r = &res[i * RES_REC..(i + 1) * RES_REC];
            let u = |a: usize| u32::from_be_bytes([r[a], r[a + 1], r[a + 2], r[a + 3]]);
            self.w8(i, o::ACTIVE, r[0]);
            self.w8(i, o::SHADOW, r[1]);
            self.w32(i, o::KIND, u(2));
            self.w32(i, o::STATE, u(6));
            self.w32(i, o::VARIANT, u(10));
            self.w32(i, o::X, u(14));
            self.w32(i, o::Y, u(18));
            self.w32(i, o::PREV, u(22));
            self.w32(i, o::NEXT, u(26));
            self.w32(i, o::N108, u(30));
            self.w32(i, o::INTERVAL, u(34));
            self.w32(i, o::HP, u(38));
            self.w32(i, o::DROP_A, u(42));
            self.w32(i, o::DROP_B, u(46));
        }
        for i in 0..RECORDS {
            self.w8(i, o::B104, 0);
            self.w8(i, o::B105, 1);
            self.w8(i, o::B106, 0);
            self.w32(i, o::ANIM_TS, tick);
            self.w8(i, o::B107, 1);
            let iv = Enemies::convert_a(self.r32(i, o::INTERVAL));
            self.w32(i, o::INTERVAL, iv);
            let hp = Enemies::convert_b(self.r32(i, o::HP));
            self.w32(i, o::HP, hp);
            self.init_kind(i);
            self.w8(i, o::F49, 0);
            self.w8(i, o::F51, 0);
            self.w32(i, o::N108, 1);
            let hw = (self.ri32(i, o::W) / 2) as f32;
            self.wf(i, o::HX, hw);
            let hh = (self.ri32(i, o::H) / 2) as f32;
            self.wf(i, o::HY, hh);
            // objects that start later or come in from the side
            let state = self.ri32(i, o::STATE);
            let mut delayed = false;
            let mut start = tick;
            if state >= 11 {
                let variant = self.r32(i, o::VARIANT);
                let wd = self.ri32(i, o::W) as f32;
                let mut shifted = true;
                match variant {
                    1 | 5 => {
                        self.wi32(i, o::STATE, 1);
                        shifted = false;
                    }
                    2..=4 => self.wf(i, o::X, -wd),
                    6..=8 => self.wf(i, o::X, 416.0),
                    _ => {
                        self.w32(i, o::VARIANT, 7);
                        self.wf(i, o::X, -wd);
                    }
                }
                if shifted {
                    start = (state.wrapping_sub(10)).wrapping_mul(30) as u32;
                    let v = self.r32(i, o::VARIANT);
                    self.w32(i, o::STATE, v.wrapping_add(1));
                    delayed = true;
                }
            }
            let _ = delayed;
            self.w32(i, o::START_DELAY, start);
            let prev = self.ri32(i, o::PREV);
            if prev == -1 {
                let mut x = self.rf(i, o::X);
                let mut y = (-64 - scroll) as f32;
                self.anchor(i, &mut x, &mut y);
                self.wf(i, o::X, x);
                self.wf(i, o::Y, y);
            } else {
                let p = prev as usize;
                let dx = ((self.ri32(p, o::W)).wrapping_sub(self.ri32(i, o::W)) / 2) as f32;
                let nx = self.rf(p, o::X) + dx;
                self.wf(i, o::X, nx);
                let dy = ((self.ri32(p, o::H)).wrapping_sub(self.ri32(i, o::H)) / 2) as f32;
                let ny = self.rf(p, o::Y) + dy;
                self.wf(i, o::Y, ny);
            }
            self.w32(i, o::DAMAGE, 0);
            self.w8(i, o::HIT, 0);
            self.w32(i, o::LAST, 0);
        }
    }

    /// 0x3b9b0: per-kind set-up of a record.
    fn init_kind(&mut self, i: usize) {
        let kind = self.r32(i, o::KIND);
        let variant = self.r32(i, o::VARIANT);
        let linked = self.ri32(i, o::PREV) != -1;
        for &(off, len, v) in ground_init::set_for(kind, variant, linked) {
            if len == 4 {
                self.w32(i, off as usize, v);
            } else {
                self.w8(i, off as usize, v as u8);
            }
        }
        if variant > 9 {
            for &off in ground_init::variant_copy(kind, linked) {
                self.w32(i, off as usize, variant);
            }
        }
    }

    /// 0x3b248: centre the sprite in its 32 / 64 pixel box (and the per-variant nudge of kinds 1 and 24).
    fn anchor(&self, i: usize, x: &mut f32, y: &mut f32) {
        let w = self.ri32(i, o::W);
        let h = self.ri32(i, o::H);
        *x += (if w > 32 { (64 - w) / 2 } else { (32 - w) / 2 }) as f32;
        *y += (if h > 32 { (64 - h) / 2 } else { (32 - h) / 2 }) as f32;
        let kind = self.r32(i, o::KIND);
        let variant = self.r32(i, o::VARIANT);
        if variant > 7 {
            return;
        }
        if kind == 1 {
            match variant {
                1 => *x -= 6.0,
                3 => *y -= 6.0,
                5 => *x += 7.0,
                7 => *y += 7.0,
                _ => {}
            }
        } else if kind == 24 {
            match variant {
                1 => *x -= 6.0,
                3 => *y -= 3.0,
                5 => *x += 7.0,
                7 => *y += 7.0,
                _ => {}
            }
        }
    }

    /// 0x3acb0: count the targets (placed records with `+104 == 0`) in the level's marker layer.
    pub fn count_targets(&mut self, width: usize, height: usize, marker: &[u8]) {
        self.targets = 0;
        for r in 0..height {
            for c in 0..width {
                let m = marker[r * width + c];
                if m != 255 && self.r8(m as usize, o::B104) == 0 {
                    self.targets += 1;
                }
            }
        }
    }

    // ---- activation ---------------------------------------------------------------------------------------------------------------
    /// 0x3b4f8: a record enters the game.
    fn activate(&mut self, i: usize, tick: u32) {
        self.w8(i, o::ACTIVE, 1);
        self.w32(i, o::START, tick);
        let d = self.r32(i, o::START_DELAY).wrapping_add(tick);
        self.w32(i, o::START_DELAY, d);
        self.list_add(i);
        self.w32(i, o::LAST, tick);
        self.w32(i, o::FLASH, tick);
        self.w32(i, o::T140, tick.wrapping_sub(60));
        self.activated = self.activated.wrapping_add(1);
    }

    /// 0x3b450: the row after `current_row` (the level's row counter) scrolls in: activate its records (and their attached partners).
    pub fn activate_row<W: GroundWorld>(&mut self, current_row: i32, width: usize, height: usize, marker: &[u8], w: &W) {
        let row = current_row.wrapping_add(1);
        if row >= height as i32 {
            return;
        }
        let tick = w.tick();
        for c in 0..width {
            let m = marker[row as usize * width + c];
            if m == 255 {
                continue;
            }
            let i = m as usize;
            self.activate(i, tick);
            let next = self.ri32(i, o::NEXT);
            if next != -1 {
                self.activate(next as usize, tick);
            }
        }
    }

    // ---- the per-frame update (0x3d0fc) --------------------------------------------------------------------------------------------
    pub fn update<W: GroundWorld>(&mut self, w: &mut W) {
        if w.scroll_step() == 0 {
            return;
        }
        let sh = Ships::get(w);
        let tick = w.tick();
        for n in 0..LIST {
            let idx = self.list[n];
            if idx == -1 {
                continue;
            }
            let i = idx as usize;
            if tick < self.r32(i, o::START_DELAY) {
                let v = tick.wrapping_add(self.r32(i, o::INTERVAL));
                self.w32(i, o::LAST, v);
                continue;
            }
            if !sh.a1 && !sh.a2 {
                let r = w.random(0, 7) as u16 as u32;
                let v = tick.wrapping_add(self.r32(i, o::INTERVAL)).wrapping_sub(r);
                self.w32(i, o::LAST, v);
            }
            let kind = self.r32(i, o::KIND);
            match kind {
                1 => self.tower(i, &sh, tick, w, false),
                24 => self.tower(i, &sh, tick, w, true),
                2 => self.k2(i, &sh, tick, w),
                4 => self.k4(i, &sh, tick, w, false),
                25 => self.k4(i, &sh, tick, w, true),
                7 => self.k7(i, &sh, tick, w),
                8 => self.k8(i, tick, w),
                15 => self.k15(i, tick),
                22 => self.k22(i, tick),
                _ => self.shooter(i, n, &sh, tick, w),
            }
            // movement, or stop when something solid is in the way
            if self.ri32(i, o::STATE) != 1 {
                let x = f2i(self.rf(i, o::X));
                let y = f2i(self.rf(i, o::Y));
                let r = x.wrapping_add(self.ri32(i, o::W));
                let b = y.wrapping_add(self.ri32(i, o::H));
                if self.blocked(x, y, r, b) {
                    self.wi32(i, o::STATE, 1);
                    self.w8(i, o::F51, 1);
                } else {
                    let nx = self.rf(i, o::X) + self.rf(i, o::VX);
                    self.wf(i, o::X, nx);
                    let ny = self.rf(i, o::Y) + self.rf(i, o::VY);
                    self.wf(i, o::Y, ny);
                }
            }
        }
    }

    /// The centre of a record (position plus half size).
    fn centre(&self, i: usize) -> (f32, f32) { (self.rf(i, o::X) + self.rf(i, o::HX), self.rf(i, o::Y) + self.rf(i, o::HY)) }

    /// The animation frame (1..=24, 15 degrees each) that points at the chosen ship.
    fn want_frame(&self, cx: f32, cy: f32, target: (f32, f32)) -> i32 {
        let a = angle_between(&self.arctan, f2i(cx), f2i(cy), f2i(target.0), f2i(target.1));
        let q = (a as i16 as i32) / 15 + 1;
        if (q as i16) < 1 {
            24
        } else if (q as i16) > 24 {
            1
        } else {
            q
        }
    }

    /// Turn the frame one step toward `want` (the short way round 1..=24). Returns `true` when it already points there.
    fn turn(&mut self, i: usize, want: i32) -> bool {
        let cur = self.ri32(i, o::FRAME);
        if want == cur {
            return true;
        }
        if cur >= want {
            self.wi32(i, o::FRAME, cur - 1);
        } else {
            let d = want - cur;
            let other = (d - 24).abs();
            if d < other {
                let n = cur + 1;
                self.wi32(i, o::FRAME, if n > 24 { 1 } else { n });
            } else {
                let n = cur - 1;
                self.wi32(i, o::FRAME, if n < 1 { 24 } else { n });
            }
        }
        false
    }

    /// Muzzle position for the frame `want`: (first + y, second + x) with the table `a` or `b`.
    fn muzzle(&self, i: usize, want: i32, table: &[(i16, i16); 24]) -> (f32, f32) {
        let idx = ((want - 1) as i16) as i32;
        let (first, second) = table[idx as usize];
        let mx = second as f32 + self.rf(i, o::X);
        let my = first as f32 + self.rf(i, o::Y);
        (mx, my)
    }

    /// Kinds 1 and 24: towers. Attached to a base they aim and fire; the base itself only animates and smoulders when damaged.
    fn tower<W: GroundWorld>(&mut self, i: usize, sh: &Ships, tick: u32, w: &mut W, k24: bool) {
        let (bk, snd, frames, table) = if k24 { (9, 60, 2, &MUZZLE_A) } else { (0, 37, 3, &MUZZLE_A) };
        if self.ri32(i, o::PREV) != -1 {
            let (cx, cy) = self.centre(i);
            let t = if sh.one {
                if sh.a1 { Some(true) } else { None }
            } else if sh.a1 || sh.a2 {
                Some(if sh.a1 && sh.a2 { farther(cx, cy, sh.p1.0, sh.p1.1, sh.p2.0, sh.p2.1) } else { sh.a1 })
            } else {
                None
            };
            let Some(t1) = t else { return };
            let target = if t1 { sh.p1 } else { sh.p2 };
            let want = self.want_frame(cx, cy, target);
            if !self.turn(i, want) {
                return;
            }
            let y = self.rf(i, o::Y);
            if !(y > 15.0 && y < 400.0) {
                return;
            }
            let last = self.r32(i, o::LAST);
            let (mx, my) = self.muzzle(i, want, table);
            let tg = if t1 { 1 } else { 2 };
            if k24 {
                if tick == last.wrapping_add(2) {
                    w.beam(bk, snd, tg, 0, mx, my, mx, my);
                    return;
                }
                if tick < last.wrapping_add(6) {
                    return;
                }
                w.beam(bk, snd, tg, 0, mx, my, mx, my);
                let v = tick.wrapping_add(self.r32(i, o::INTERVAL));
                self.w32(i, o::LAST, v);
            } else {
                if tick < last.wrapping_add(self.r32(i, o::INTERVAL)) {
                    return;
                }
                w.beam(bk, snd, tg, 0, mx, my, mx, my);
                self.w32(i, o::LAST, tick);
            }
        } else {
            let state = self.ri32(i, o::STATE);
            if state == 1 {
                return;
            }
            // smoke / wreck decals now and then
            if tick > self.r32(i, o::T140).wrapping_add(50) {
                self.w32(i, o::T140, tick);
                if (w.random(0, 4) as u16) != 0 {
                    let mut yoff = 0;
                    let mut r6 = 1;
                    if k24 {
                        match state {
                            3 | 7 => r6 = 2,
                            4 | 8 => r6 = 3,
                            5 | 9 => {
                                yoff = 5;
                                r6 = 4;
                            }
                            _ => {}
                        }
                    } else {
                        match state {
                            3 | 7 => r6 = 2,
                            4 | 8 => r6 = 3,
                            5 | 9 => r6 = 4,
                            _ => {}
                        }
                    }
                    let x = f2i(self.rf(i, o::X));
                    let y = f2i(self.rf(i, o::Y)).wrapping_add(yoff);
                    self.add_decal(w, [x, y, 55, r6, 52, 52, 0, 0], 0, 1);
                }
            }
            if tick > self.r32(i, o::ANIM_TS).wrapping_add(2) {
                let up = !(5..9).contains(&state);
                let f = self.ri32(i, o::FRAME);
                if up {
                    let n = f + 1;
                    self.wi32(i, o::FRAME, if n > frames { 1 } else { n });
                } else {
                    let n = f - 1;
                    self.wi32(i, o::FRAME, if n < 1 { frames } else { n });
                }
                self.w32(i, o::ANIM_TS, tick);
            }
        }
    }

    /// Kinds 4 and 25: a turret that follows the nearest ship and fires bursts.
    fn k4<W: GroundWorld>(&mut self, i: usize, sh: &Ships, tick: u32, w: &mut W, k25: bool) {
        if !sh.a1 && !sh.a2 {
            return;
        }
        let (cx, cy) = self.centre(i);
        let t1 = if sh.one {
            if !sh.a1 {
                return;
            }
            true
        } else if sh.a1 && sh.a2 {
            farther(cx, cy, sh.p1.0, sh.p1.1, sh.p2.0, sh.p2.1)
        } else {
            sh.a1
        };
        let target = if t1 { sh.p1 } else { sh.p2 };
        let want = self.want_frame(cx, cy, target);
        if !self.turn(i, want) {
            return;
        }
        let (mx, my) = self.muzzle(i, want, &MUZZLE_B);
        let (bk, snd) = if k25 { (9, 37) } else { (0, 37) };
        let tg = if t1 { 1 } else { 2 };
        // one volley: the original fires the "second beam" of kind 25 only at ship 1
        let volley = |w: &mut W| {
            if k25 {
                if t1 {
                    w.beam(bk, snd, 1, 0, mx, my, mx, my);
                    w.beam(bk, snd, 1, 0, 5.0 + mx, 5.0 + my, mx, my);
                } else {
                    w.beam(bk, snd, 2, 0, mx, my, mx, my);
                }
            } else {
                w.beam(bk, snd, tg, 0, mx, my, mx, my);
            }
        };
        let last = self.r32(i, o::LAST);
        if self.r8(i, o::B106) != 0 {
            if tick == last.wrapping_add(4) || tick == last.wrapping_add(8) {
                volley(w);
            } else if tick >= last.wrapping_add(12) {
                if w.difficulty() > 4 as u32 {
                    volley(w);
                }
                self.w8(i, o::B106, 0);
                self.w32(i, o::LAST, tick);
            }
        } else if tick >= last.wrapping_add(self.r32(i, o::INTERVAL)) {
            volley(w);
            self.w32(i, o::LAST, tick);
            self.w8(i, o::B106, 1);
        }
    }

    /// Kind 2: a fixed gun that fires at the nearest ship at its interval.
    fn k2<W: GroundWorld>(&mut self, i: usize, sh: &Ships, tick: u32, w: &mut W) {
        if sh.one {
            if !sh.a1 {
                return;
            }
        } else if !sh.a1 && !sh.a2 {
            return;
        }
        if tick < self.r32(i, o::LAST).wrapping_add(self.r32(i, o::INTERVAL)) {
            return;
        }
        let (cx, cy) = self.centre(i);
        let t1 = if sh.one {
            true
        } else if sh.a1 && sh.a2 {
            farther(cx, cy, sh.p1.0, sh.p1.1, sh.p2.0, sh.p2.1)
        } else {
            sh.a1
        };
        w.beam(0, 37, if t1 { 1 } else { 2 }, 0, cx, cy, cx, cy);
        self.w32(i, o::LAST, tick);
    }

    /// Kind 7: a pop-up gun: it opens (frames 1..5), fires at the top and closes again.
    fn k7<W: GroundWorld>(&mut self, i: usize, sh: &Ships, tick: u32, w: &mut W) {
        let f = self.ri32(i, o::FRAME);
        let delay = if f == 1 {
            self.w8(i, o::B107, 0);
            self.r32(i, o::INTERVAL)
        } else {
            self.w8(i, o::B107, 1);
            if f == 5 { self.r32(i, o::INTERVAL).wrapping_add(10) } else { 2 }
        };
        if tick < delay.wrapping_add(self.r32(i, o::ANIM_TS)) {
            return;
        }
        if self.r8(i, o::B106) != 0 {
            let nf = self.ri32(i, o::FRAME) + 1;
            self.wi32(i, o::FRAME, nf);
            if nf == 2 {
                let y = self.rf(i, o::Y);
                if y > 0.0 && y < 480.0 {
                    w.sound(44, 2, 0);
                }
            }
            if self.ri32(i, o::FRAME) == 5 {
                let (cx, cy) = self.centre(i);
                let mut fire = false;
                let mut t1 = true;
                if sh.one {
                    if sh.a1 {
                        fire = true;
                    }
                } else if sh.a1 && sh.a2 {
                    fire = true;
                    t1 = farther(cx, cy, sh.p1.0, sh.p1.1, sh.p2.0, sh.p2.1);
                } else {
                    if sh.a1 {
                        fire = true;
                        t1 = true;
                    }
                    if sh.a2 {
                        fire = true;
                        t1 = false;
                    }
                }
                if fire {
                    w.beam(0, -1, if t1 { 1 } else { 2 }, 0, cx, cy, cx, cy);
                    self.w32(i, o::LAST, tick);
                }
            }
            if self.ri32(i, o::FRAME) > 5 {
                self.wi32(i, o::FRAME, 4);
                self.w8(i, o::B106, 0);
            }
        } else {
            let nf = self.ri32(i, o::FRAME) - 1;
            self.wi32(i, o::FRAME, nf);
            if nf < 1 {
                self.wi32(i, o::FRAME, 1);
                self.w8(i, o::B106, 1);
            }
        }
        self.w32(i, o::ANIM_TS, tick);
    }

    /// Kind 8: a target that blows up in four frames once it has taken damage, and may leave a power-up.
    fn k8<W: GroundWorld>(&mut self, i: usize, tick: u32, w: &mut W) {
        if self.ri32(i, o::DAMAGE) <= 0 {
            return;
        }
        let f = self.ri32(i, o::FRAME);
        if f == 1 {
            self.wi32(i, o::FRAME, f + 1);
            self.w32(i, o::ANIM_TS, tick);
            return;
        }
        if tick <= self.r32(i, o::ANIM_TS).wrapping_add(2) {
            return;
        }
        let nf = f + 1;
        self.wi32(i, o::FRAME, nf);
        self.w32(i, o::ANIM_TS, tick);
        if nf != 5 {
            return;
        }
        let mut flag = self.r8(i, o::SHADOW);
        if flag != 0 && self.r8(i, o::F49) != 0 {
            flag = 0;
        }
        let px = self.rf(i, o::X) + (self.ri32(i, o::W) / 2) as f32;
        let py = self.rf(i, o::Y) + (self.ri32(i, o::H) / 2) as f32;
        let r = w.random(0, 9) as u16;
        if r == 0 {
            if (w.random(0, 9) as u16) == 0 {
                w.pickup(3, 8, 1, 1, px, py);
            } else {
                w.pickup(3, 3, 1, 1, px, py);
            }
        } else if r == 9 && (w.random(0, 1) as u16) != 0 {
            w.pickup(20, 1, 1, 1, px, py);
        }
        let x = f2i(self.rf(i, o::X));
        let y = f2i(self.rf(i, o::Y));
        let (wd, ht) = (self.ri32(i, o::W), self.ri32(i, o::H));
        let frame = self.ri32(i, o::FRAME);
        let s = self.r8(i, o::SHADOW) as i32;
        self.add_decal(w, [x, y, 64, frame, wd, ht, 0, s], flag as i32, 0);
        self.deactivate(i);
        let (fx, fy) = (self.rf(i, o::X), self.rf(i, o::Y));
        self.progress(w, fx, fy);
    }

    /// Kind 15: a two-state blinker (frames 1..5 and back).
    fn k15(&mut self, i: usize, tick: u32) {
        if tick <= self.r32(i, o::ANIM_TS).wrapping_add(3) {
            return;
        }
        let toggle = |g: &mut Ground| {
            let v = g.r8(i, o::B106);
            g.w8(i, o::B106, if v != 0 { 0 } else { 1 });
        };
        if self.r8(i, o::B106) == 0 {
            let nf = self.ri32(i, o::FRAME) + 1;
            self.wi32(i, o::FRAME, nf);
            if nf == 5 {
                toggle(self);
            }
        } else {
            let nf = self.ri32(i, o::FRAME) - 1;
            self.wi32(i, o::FRAME, nf);
            if nf == 1 {
                toggle(self);
            }
        }
        self.w32(i, o::ANIM_TS, tick);
    }

    /// Kind 22: a 24-frame spinner.
    fn k22(&mut self, i: usize, tick: u32) {
        if tick <= self.r32(i, o::ANIM_TS).wrapping_add(2) {
            return;
        }
        let nf = self.ri32(i, o::FRAME) + 1;
        self.wi32(i, o::FRAME, if nf > 24 { 1 } else { nf });
        self.w32(i, o::ANIM_TS, tick);
    }

    /// Every other kind: a gun that fires aimed shots (0x383b8) at its interval. (The interval is read from the record at the *list position*,
    /// as the original does.)
    fn shooter<W: GroundWorld>(&mut self, i: usize, n: usize, sh: &Ships, tick: u32, w: &mut W) {
        if self.r8(i, o::F50) == 0 || self.ri32(i, o::NEXT) != -1 {
            return;
        }
        let due = self.r32(i, o::LAST).wrapping_add(self.r32(n, o::INTERVAL));
        if tick < due {
            return;
        }
        let (cx, cy) = self.centre(i);
        let t1 = sh.one || farther(cx, cy, sh.p1.0, sh.p1.1, sh.p2.0, sh.p2.1);
        w.shot_aimed(0, if t1 { 1 } else { 2 }, 0, cx, cy);
        self.w32(i, o::LAST, tick);
    }

    // ---- damage and hit tests -----------------------------------------------------------------------------------------------------
    /// 0x40854: one more target destroyed; when only a few are left the bonus is awarded (the number depends on the level).
    fn progress<W: GroundWorld>(&mut self, w: &mut W, x: f32, y: f32) {
        self.destroyed = self.destroyed.wrapping_add(1);
        let d = w.difficulty() as i32;
        let k = if d > 20 { 6 } else if d > 12 { 2 } else if d > 4 { 1 } else { 0 };
        if self.destroyed == self.targets.wrapping_sub(k) {
            w.clear_bonus(x, y);
        }
    }

    /// 0x40040: damage to a record from ship `owner`.
    pub fn damage<W: GroundWorld>(&mut self, i: usize, amount: i32, owner: u8, w: &mut W) {
        if self.r8(i, o::ACTIVE) == 0 {
            return;
        }
        let d = self.r32(i, o::DAMAGE).wrapping_add(amount as u32);
        self.w32(i, o::DAMAGE, d);
        let kind = self.r32(i, o::KIND);
        if kind == 8 {
            w.sound(43, 4, 0);
        }
        if self.ri32(i, o::DAMAGE) <= self.ri32(i, o::HP) {
            self.w8(i, o::HIT, 1);
            self.w32(i, o::FLASH, w.tick());
            w.sound(39, 2, 0);
            return;
        }
        let part = self.ri32(i, o::PART);
        let cx = f2i((f2i(self.rf(i, o::X)) as f32) + self.rf(i, o::HX));
        let cy = f2i((f2i(self.rf(i, o::Y)) as f32) + self.rf(i, o::HY));
        w.effect(cx, cy, 0, 0, 0, 1, 0, 0.0, 0.0);
        if part != -1 {
            w.particles(cx, cy, part, 6, 0, 1);
        }
        let aux = self.r8(i, o::DEATH_AUX);
        if aux as i8 != -1 {
            w.aux(cx, cy, aux as i32, 0, -1, 1);
        }
        if kind != 7 && kind != 1 && kind != 24 {
            let f = self.r32(i, o::FRAME);
            self.w32(i, o::WRECK_FRAME, f);
        }
        if kind == 4 || kind == 15 || kind == 22 || kind == 25 {
            self.w32(i, o::WRECK_FRAME, 1);
        }
        let mut chain = false;
        if kind == 21 || kind == 23 {
            w.aux(cx - 20, cy - 10, 2, 8, 11, 1);
            w.aux(cx, cy - 16, 2, 14, 11, 1);
            w.aux(cx + 18, cy + 4, 2, 22, 11, 1);
            w.aux(cx - 8, cy + 22, 2, 30, -1, 1);
            w.effect(cx - 20, cy - 10, 2, 8, 62, 1, 0, 0.0, 0.0);
            w.effect(cx, cy - 16, 0, 14, 62, 1, 0, 0.0, 0.0);
            w.effect(cx + 18, cy + 4, 2, 22, 62, 1, 0, 0.0, 0.0);
            w.effect(cx - 8, cy + 22, 2, 30, 62, 1, 0, 0.0, 0.0);
            chain = true;
        } else {
            w.effect(cx - 20, cy - 10, 2, 8, 62, 1, 0, 0.0, 0.0);
            w.effect(cx, cy - 16, 2, 14, 62, 1, 0, 0.0, 0.0);
            w.effect(cx + 18, cy + 6, 2, 22, 62, 1, 0, 0.0, 0.0);
        }
        let (cxf, cyf) = (cx as f32, cy as f32);
        let a = self.ri32(i, o::DROP_A);
        if a > 1 {
            w.pickup(3, a, 1, 1, cxf, cyf);
        }
        let b = self.ri32(i, o::DROP_B);
        if b > 1 {
            w.pickup(b, 1, 1, 1, cxf, cyf);
        }
        if kind == 22 || kind == 23 {
            w.pickup(2, 1, 1, 1, cxf, cyf);
        }
        if self.ri32(i, o::WRECK_ANIM) != -1 {
            let mut flag = self.r8(i, o::SHADOW) as i32;
            if flag != 0 && self.r8(i, o::F49) != 0 {
                flag = 0;
            }
            let (x, y) = (f2i(self.rf(i, o::X)), f2i(self.rf(i, o::Y)));
            let a80 = self.ri32(i, o::WRECK_ANIM);
            let a84 = self.ri32(i, o::WRECK_FRAME);
            let (wd, ht) = (self.ri32(i, o::W), self.ri32(i, o::H));
            let r9 = if self.ri32(i, o::STATE) == 1 { 0 } else { 1 };
            let s = self.r8(i, o::SHADOW) as i32;
            self.add_decal(w, [x, y, a80, a84, wd, ht, r9, s], flag, 0);
        }
        let next = self.ri32(i, o::NEXT);
        if next != -1 {
            self.deactivate(next as usize);
        }
        if self.r8(i, o::B104) != 0 {
            let (x, y) = (self.rf(i, o::X), self.rf(i, o::Y));
            match kind {
                6 => w.pickup(3, 4, 0, 1, x, y),
                5 => w.pickup(8, 1, 0, 1, x, y),
                20 => w.pickup(20, 1, 0, 1, x, y),
                _ => {}
            }
        } else {
            let (x, y) = (self.rf(i, o::X), self.rf(i, o::Y));
            self.progress(w, x, y);
        }
        let pts = self.ri32(i, o::SCORE);
        w.score(owner, pts);
        self.deactivate(i);
        if chain {
            let (x, y) = (self.rf(i, o::X), self.rf(i, o::Y));
            let (wd, ht) = (self.ri32(i, o::W), self.ri32(i, o::H));
            self.hit_box(f2i(x) - 60, f2i(y) - 60, f2i(x).wrapping_add(wd + 60), f2i(y).wrapping_add(ht + 60), 4, owner, w);
        }
    }

    /// 0x3fec8: damage `amount` to every record whose box touches (left, top, right, bottom). Returns 1 when something was hit, -1 otherwise.
    #[allow(clippy::too_many_arguments)]
    pub fn hit_box<W: GroundWorld>(&mut self, left: i32, top: i32, right: i32, bottom: i32, amount: i32, owner: u8, w: &mut W) -> i32 {
        let mut hit = false;
        for k in 0..LIST {
            let idx = self.list[k];
            if idx == -1 {
                continue;
            }
            let i = idx as usize;
            if self.r8(i, o::B107) == 0 {
                continue;
            }
            let y = self.rf(i, o::Y);
            if !((bottom as f32) >= y) {
                continue;
            }
            if !((top as f32) <= y + self.ri32(i, o::H) as f32) {
                continue;
            }
            let x = self.rf(i, o::X);
            if !((right as f32) >= x) {
                continue;
            }
            if !((left as f32) <= x + self.ri32(i, o::W) as f32) {
                continue;
            }
            if amount > 0 {
                self.damage(i, amount, owner, w);
            }
            hit = true;
        }
        if hit { 1 } else { -1 }
    }

    /// 0x3fbfc: everything that counts and is on the screen takes 10000 damage (the smart bomb).
    pub fn bomb_all<W: GroundWorld>(&mut self, owner: u8, w: &mut W) {
        for k in 0..LIST {
            let idx = self.list[k];
            if idx == -1 {
                continue;
            }
            let i = idx as usize;
            if self.r8(i, o::ACTIVE) == 0 || self.r8(i, o::B104) != 0 || self.r8(i, o::F50) == 0 {
                continue;
            }
            let x = self.rf(i, o::X);
            let y = self.rf(i, o::Y);
            if !(x + self.ri32(i, o::W) as f32 >= -64.0) || x >= 480.0 {
                continue;
            }
            if !(y + self.ri32(i, o::H) as f32 >= -64.0) || y >= 544.0 {
                continue;
            }
            self.damage(i, 10000, owner, w);
        }
    }

    /// 0x3fd1c: is the box (left, top, right, bottom) blocked by a solid decal or a solid record?
    pub fn blocked(&self, left: i32, top: i32, right: i32, bottom: i32) -> bool {
        for s in 0..DECALS {
            if self.d8(s, 0) == 0 || self.d8(s, 2) == 0 {
                continue;
            }
            let x = self.d32(s, 8) as i32;
            if left >= x.wrapping_add(60) || x >= right {
                continue;
            }
            let y = self.d32(s, 12) as i32;
            if top >= y.wrapping_add(60) || y >= bottom {
                continue;
            }
            return true;
        }
        for k in 0..LIST {
            let idx = self.list[k];
            if idx == -1 {
                continue;
            }
            let i = idx as usize;
            if self.r8(i, o::ACTIVE) == 0 || self.r8(i, o::F51) == 0 {
                continue;
            }
            let x = self.rf(i, o::X);
            if left as f32 >= x + self.ri32(i, o::W) as f32 || x >= right as f32 {
                continue;
            }
            let y = self.rf(i, o::Y);
            if top as f32 >= y + self.ri32(i, o::H) as f32 || y >= bottom as f32 {
                continue;
            }
            return true;
        }
        false
    }

    // ---- decals (wrecks) ----------------------------------------------------------------------------------------------------------
    /// 0x40954: add a decal. `a = [x, y, anim, frame, w, h, r9, r10]`; `p9` / `p10` are the two stack parameters.
    pub fn add_decal<W: GroundWorld>(&mut self, w: &mut W, a: [i32; 8], p9: i32, p10: i32) {
        let [x, y, anim, frame, wd, ht, r9, r10] = a;
        let mut clip = 0u8;
        if x < 0 {
            clip = 1;
            if x.wrapping_add(wd) < 0 {
                return;
            }
        } else if x.wrapping_add(wd) > 416 {
            clip = 1;
            if x > 416 {
                return;
            }
        }
        if y < 0 {
            clip = 1;
            if y.wrapping_add(ht) < 0 {
                return;
            }
        } else if y.wrapping_add(ht) > 480 {
            clip = 1;
            if y > 480 {
                return;
            }
        }
        let Some(s) = (0..DECALS).find(|&s| self.d8(s, 0) == 0) else { return };
        let b = s * DECAL;
        self.decals[b] = 1;
        self.decals[b + 1] = clip;
        self.decals[b + 3] = r10 as u8;
        self.dw32(s, 8, x as u32);
        self.dw32(s, 12, y as u32);
        self.dw32(s, 16, wd as u32);
        self.dw32(s, 20, ht as u32);
        self.dw32(s, 24, anim as u32);
        self.dw32(s, 28, frame as u32);
        self.decals[b + 2] = r9 as u8;
        self.decals[b + 4] = p10 as u8;
        if clip != 0 {
            return;
        }
        let off = w.buffer_offset();
        if w.shadows() && (r10 as u8) != 0 && (p9 as u8) != 0 {
            w.prim(0x3910, [0, x - 6, y.wrapping_add(off).wrapping_add(8), wd, ht, anim, frame]);
        }
        if (p10 as u8) == 0 {
            w.prim(0x3728, [0, x, y.wrapping_add(off), wd, ht, anim, frame]);
        } else {
            w.prim(0x381c, [0, x, y.wrapping_add(off), wd, ht, anim, frame]);
        }
    }

    /// 0x40904: clear the decal list (level start).
    pub fn clear_decals(&mut self) {
        for s in 0..DECALS {
            self.decals[s * DECAL] = 0;
        }
    }

    /// 0x40b50: the decals scroll with the ground and disappear at the bottom.
    pub fn scroll_decals<W: GroundWorld>(&mut self, w: &W) {
        let step = w.scroll_step();
        if step == 0 {
            return;
        }
        for s in 0..DECALS {
            if self.d8(s, 0) != 0 {
                let y = self.d32(s, 12).wrapping_add(step as u32);
                self.dw32(s, 12, y);
                if (y as i32) >= 480 {
                    self.decals[s * DECAL] = 0;
                }
            }
        }
    }

    /// 0x40bf4: draw the decals that were not clipped when they were created (into the background buffer).
    pub fn draw_decals<W: GroundWorld>(&self, w: &mut W) {
        if w.scroll_step() == 0 {
            return;
        }
        let sh = w.shadows();
        let off = w.buffer_offset();
        for s in 0..DECALS {
            if self.d8(s, 0) == 0 || self.d8(s, 1) != 0 {
                continue;
            }
            let (x, y) = (self.d32(s, 8) as i32, self.d32(s, 12) as i32);
            let (wd, ht, anim, frame) = (self.d32(s, 16) as i32, self.d32(s, 20) as i32, self.d32(s, 24) as i32, self.d32(s, 28) as i32);
            if self.d8(s, 3) != 0 && sh {
                w.prim(0x3910, [0, x.wrapping_sub(6), y.wrapping_add(off).wrapping_add(8), wd, ht, anim, frame]);
            }
            if self.d8(s, 4) == 0 {
                w.prim(0x3728, [0, x, off.wrapping_add(y), wd, ht, anim, frame]);
            } else {
                w.prim(0x381c, [0, x, off.wrapping_add(y), wd, ht, anim, frame]);
            }
        }
    }

    // ---- drawing / scrolling pass (0x3f554) ---------------------------------------------------------------------------------------
    /// Move every object with the scrolling, cull those that left the screen and draw what is not part of the background.
    pub fn move_draw<W: GroundWorld>(&mut self, w: &mut W) {
        self.baked_now.clear();
        let scroll = w.scroll_step();
        if scroll == 0 {
            return;
        }
        let sh = w.shadows();
        let off = w.buffer_offset();
        let tick = w.tick();
        for s in 0..DECALS {
            if self.d8(s, 0) == 0 || self.d8(s, 1) == 0 {
                continue;
            }
            let (x, y) = (self.d32(s, 8) as i32, self.d32(s, 12) as i32);
            let (wd, ht, anim, frame) = (self.d32(s, 16) as i32, self.d32(s, 20) as i32, self.d32(s, 24) as i32, self.d32(s, 28) as i32);
            if sh && self.d8(s, 3) != 0 {
                w.prim(0x3358, [0, x.wrapping_sub(6), y.wrapping_add(8), wd, ht, anim, frame]);
            }
            w.prim(0x2f88, [0, x, y, wd, ht, anim, frame]);
        }
        for n in 0..LIST {
            let idx = self.list[n];
            if idx == -1 {
                continue;
            }
            let i = idx as usize;
            let ny = self.rf(i, o::Y) + scroll as f32;
            self.wf(i, o::Y, ny);
            if tick < self.r32(i, o::START_DELAY) {
                continue;
            }
            let x = self.rf(i, o::X);
            let wd = self.ri32(i, o::W);
            let ht = self.ri32(i, o::H);
            let y = ny;
            let culled = x < -64.0 || x + wd as f32 > 480.0 || y < -64.0 || !(y <= 544.0);
            if culled {
                self.deactivate(i);
            } else if self.r8(i, o::B104) == 0 {
                let (anim, frame) = (self.ri32(i, o::ANIM), self.ri32(i, o::FRAME));
                if self.r8(i, o::F49) == 0 && self.r8(i, o::B105) != 0 {
                    if sh && self.r8(i, o::SHADOW) != 0 {
                        w.prim(0x3358, [0, f2i(-6.0 + x), f2i(8.0 + y), wd, ht, anim, frame]);
                    }
                    w.prim(0x2f88, [0, f2i(x), f2i(y), wd, ht, anim, frame]);
                }
                if self.r8(i, o::HIT) != 0 {
                    w.prim(0x3540, [0, f2i(x), f2i(y), wd, ht, anim, frame]);
                }
                if self.r8(i, o::F48) == 0 && self.r8(i, o::F49) == 0 && y > 0.0 {
                    let offf = off as f32;
                    if sh && self.r8(i, o::SHADOW) != 0 {
                        w.prim(0x3910, [0, f2i(-6.0 + x), f2i(offf + (8.0 + y)), wd, ht, anim, frame]);
                    }
                    w.prim(0x3728, [0, f2i(x), f2i(offf + y), wd, ht, anim, frame]);
                    self.w8(i, o::F49, 1);
                    self.baked_now.push(i);
                }
            }
            if self.r8(i, o::HIT) != 0 && tick > self.r32(i, o::FLASH).wrapping_add(1) {
                self.w8(i, o::HIT, 0);
            }
        }
    }

    /// 0x3fa24: draw the objects that are already part of the background (into the background buffer).
    pub fn draw_baked<W: GroundWorld>(&self, w: &mut W) {
        let scroll = w.scroll_step();
        if scroll == 0 {
            return;
        }
        let sh = w.shadows();
        let off = w.buffer_offset() as f32;
        for n in 0..LIST {
            let idx = self.list[n];
            if idx == -1 {
                continue;
            }
            let i = idx as usize;
            if self.r8(i, o::ACTIVE) == 0 || self.r8(i, o::F49) == 0 {
                continue;
            }
            let (x, y) = (self.rf(i, o::X), self.rf(i, o::Y));
            let (wd, ht, anim, frame) = (self.ri32(i, o::W), self.ri32(i, o::H), self.ri32(i, o::ANIM), self.ri32(i, o::FRAME));
            let s = scroll as f32;
            if sh && self.r8(i, o::SHADOW) != 0 {
                w.prim(0x3910, [0, f2i(-6.0 + x), f2i(s + (off + (8.0 + y))), wd, ht, anim, frame]);
            }
            w.prim(0x3728, [0, f2i(x), f2i(s + (off + y)), wd, ht, anim, frame]);
        }
    }

    /// What the original burns into its 512-line background buffer, drawn from the state instead: the objects that are already part of the
    /// background (+49, with their shadow), then the decals (wrecks, scorch marks, smoke) that were not clipped when they were made. (The clipped
    /// decals and everything else are drawn live by [`Ground::move_draw`].) `emit(prim, x, y, w, h, anim, frame)`.
    pub fn render_background(&self, shadows: bool, emit: &mut dyn FnMut(Prim, i32, i32, i32, i32, i32, i32)) {
        for n in 0..LIST {
            let idx = self.list[n];
            if idx == -1 {
                continue;
            }
            let i = idx as usize;
            if self.r8(i, o::ACTIVE) == 0 || self.r8(i, o::F49) == 0 || self.baked_now.contains(&i) {
                continue;
            }
            let (x, y) = (self.rf(i, o::X), self.rf(i, o::Y));
            let (wd, ht, anim, frame) = (self.ri32(i, o::W), self.ri32(i, o::H), self.ri32(i, o::ANIM), self.ri32(i, o::FRAME));
            if shadows && self.r8(i, o::SHADOW) != 0 {
                emit(Prim::Cast, f2i(-6.0 + x), f2i(8.0 + y), wd, ht, anim, frame);
            }
            emit(Prim::Anim, f2i(x), f2i(y), wd, ht, anim, frame);
        }
        for s in 0..DECALS {
            if self.d8(s, 0) == 0 || self.d8(s, 1) != 0 {
                continue;
            }
            let (x, y) = (self.d32(s, 8) as i32, self.d32(s, 12) as i32);
            let (wd, ht, anim, frame) = (self.d32(s, 16) as i32, self.d32(s, 20) as i32, self.d32(s, 24) as i32, self.d32(s, 28) as i32);
            if shadows && self.d8(s, 3) != 0 {
                emit(Prim::Cast, x.wrapping_sub(6), y.wrapping_add(8), wd, ht, anim, frame);
            }
            emit(if self.d8(s, 4) == 0 { Prim::Anim } else { Prim::Trans }, x, y, wd, ht, anim, frame);
        }
    }
}

impl std::fmt::Debug for Ground {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ground").field("targets", &self.targets).field("destroyed", &self.destroyed).field("activated", &self.activated).finish()
    }
}
