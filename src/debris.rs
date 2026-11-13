//! The auxiliary effects (the "debris"): bursts of 40 coloured 3 x 3 pixel particles that fly outwards (explosion fragments, `0x29714`) or in towards a point
//! (the implosion of the ship's appearance, `0x298ec`), fading through a ramp of ten colours. Spawn `0x29714` / `0x298ec`, update / draw `0x29b10`,
//! initialisation `0x2942c` (the table of 62 random directions `0x29470`, the record reset `0x2957c`).
//!
//! The pool is 10 records of 976 bytes: 40 particles of 24 bytes (`+0` active, `+1` colour-ramp position, `+2` ramp start, `+3` ramp end, `+4` tick counter,
//! `+8` / `+12` position, `+16` / `+20` velocity) followed by the record's header (`+960` active, `+961` moves with the ground, `+962` implosion variant,
//! `+964` delay, `+968` age, `+972` sound id). The colours come from a 13 x 10 grid (PICT 960, `960_particle_colour_grid.png`: one ramp of ten colours per effect
//! id 0..12), turned into palette indices at start-up by `0x29668`; particles are drawn straight into the off-screen buffer as 3 x 3 blocks.
//!
//! Verified against the original executable by `tests/debris_golden.rs` (vectors from `tools/re/gen_debris_vectors.py`): the pool, the direction table, its index,
//! every pixel written and the exact sequence of calls to the surroundings must be identical after every operation.
//! Like the effect pool, the spawners play the effect's sound themselves (`0x45bac(id, 4, 0)` when there is no delay; the update pass does it when a delayed
//! effect starts), so the sound ids that callers pass to these routines are the only place those explosion sounds come from.

use crate::enemy::f2i;

pub const RECORDS: usize = 10;
pub const REC: usize = 976;
pub const PARTICLES: usize = 40;
pub const PART: usize = 24;
pub const STARS: usize = 62;
/// Entries of the direction table the effects use (the index wraps at 61).
pub const STAR_USE: u32 = 61;
pub const RAMPS: usize = 13;
pub const RAMP_LEN: usize = 10;

/// Record header offsets (relative to the record).
pub mod h {
    pub const ACTIVE: usize = 960;
    pub const SCROLLS: usize = 961;
    pub const IN: usize = 962;
    pub const DELAY: usize = 964;
    pub const AGE: usize = 968;
    pub const SOUND: usize = 972;
}

/// What the effects need from the game. Every method is one routine of the original.
pub trait DebrisWorld {
    /// 0xc098: scroll step per frame.
    fn scroll_step(&self) -> i32;
    /// 0xa0d4: random number in `lo..=hi`.
    fn random(&mut self, lo: i32, hi: i32) -> i32;
    /// 0x45bac: sound effect (id, priority, argument).
    fn sound(&mut self, id: i32, prio: i32, arg: i32);
    /// Write one 3 x 3 block of palette index `colour` with its top-left corner at (x, y) of the 704 x 608 off-screen buffer (the playfield starts at 64, 64).
    fn plot(&mut self, x: i32, y: i32, colour: u8);
}

#[derive(Clone)]
pub struct Debris {
    pub rec: Vec<u8>,
    /// `0xfb0`: 62 x (vx, vy), random directions with a speed of 6.
    pub stars: Vec<f32>,
    /// `D.1730`
    pub next_star: u32,
    /// `0xfa8`: palette index of every cell of the colour grid, `ramp * 10 + step`.
    pub colours: Vec<u8>,
}

impl Default for Debris {
    fn default() -> Self { Self::new() }
}

impl Debris {
    pub fn new() -> Self {
        Debris { rec: vec![0; RECORDS * REC], stars: vec![0.0; STARS * 2], next_star: 0, colours: vec![0; RAMPS * RAMP_LEN] }
    }

    // ---- raw accessors ------------------------------------------------------------------------------------------------------------
    #[inline] pub fn r8(&self, off: usize) -> u8 { self.rec[off] }
    #[inline] pub fn w8(&mut self, off: usize, v: u8) { self.rec[off] = v }
    #[inline] pub fn r32(&self, off: usize) -> u32 { u32::from_be_bytes([self.rec[off], self.rec[off + 1], self.rec[off + 2], self.rec[off + 3]]) }
    #[inline] pub fn w32(&mut self, off: usize, v: u32) { self.rec[off..off + 4].copy_from_slice(&v.to_be_bytes()) }
    #[inline] pub fn rf(&self, off: usize) -> f32 { f32::from_bits(self.r32(off)) }
    #[inline] pub fn wf(&mut self, off: usize, v: f32) { self.w32(off, v.to_bits()) }

    /// The direction table as the original keeps it in memory (big-endian f32 pairs).
    pub fn star_bytes(&self) -> Vec<u8> { self.stars.iter().flat_map(|v| v.to_bits().to_be_bytes()).collect() }

    /// Number of records in use.
    pub fn active(&self) -> usize { (0..RECORDS).filter(|&r| self.r8(r * REC + h::ACTIVE) != 0).count() }

    /// 0x29650: free one record.
    pub fn free(&mut self, r: usize) { self.w8(r * REC + h::ACTIVE, 0) }

    /// 0x2957c: free every record and every particle (the level change; the direction table and the colours stay).
    pub fn reset(&mut self) {
        for r in 0..RECORDS {
            self.free(r);
            for p in 0..PARTICLES { self.w8(r * REC + p * PART, 0); }
        }
    }

    /// 0x2942c: build the direction table (0x29470), choose the first table entry at random, free every record and every particle (0x2957c).
    /// `colours` is the 13 x 10 grid as palette indices (0x29668).
    pub fn init<W: DebrisWorld>(&mut self, w: &mut W, colours: &[u8]) {
        let (cx, cy) = (208.0f32, 240.0f32);
        for k in 0..STARS {
            let rx = (w.random(0, 416) as u16) as f32;
            let ry = (w.random(0, 480) as u16) as f32;
            let (dx, dy) = (cx - rx, cy - ry);
            let len = (dx.mul_add(dx, dy * dy) as f64).sqrt() as f32; // fmuls, fmadds, sqrt (double), frsp
            self.stars[k * 2] = (dx / len) * 6.0;
            self.stars[k * 2 + 1] = (dy / len) * 6.0;
        }
        for (i, c) in colours.iter().take(RAMPS * RAMP_LEN).enumerate() { self.colours[i] = *c; }
        self.next_star = (w.random(0, 61) as u16) as u32;
        self.reset();
    }

    fn take_star(&mut self) -> (f32, f32) {
        let k = (self.next_star as usize).min(STARS - 1);
        let v = (self.stars[k * 2], self.stars[k * 2 + 1]);
        self.next_star = self.next_star.wrapping_add(1);
        if (self.next_star as i32) >= STAR_USE as i32 { self.next_star = 0; }
        v
    }

    /// 0x29714: a burst of fragments flying outwards from (x, y), colour ramp `id` (-1..=12), starting `delay` ticks from now (0 = now, with sound `snd`),
    /// moving with the ground when `scrolls` is non-zero.
    pub fn spawn<W: DebrisWorld>(&mut self, w: &mut W, x: i32, y: i32, id: i32, delay: i32, snd: i32, scrolls: i32) {
        self.start(w, x, y, id, delay, snd, scrolls, false);
    }

    /// 0x298ec: the same, flying inwards from 240 pixels away.
    pub fn spawn_in<W: DebrisWorld>(&mut self, w: &mut W, x: i32, y: i32, id: i32, delay: i32, snd: i32, scrolls: i32) {
        self.start(w, x, y, id, delay, snd, scrolls, true);
    }

    #[allow(clippy::too_many_arguments)]
    fn start<W: DebrisWorld>(&mut self, w: &mut W, x: i32, y: i32, id: i32, delay: i32, snd: i32, scrolls: i32, inward: bool) {
        let id8 = id as i8;
        if !(-1..=12).contains(&id8) { return; }
        let Some(r) = (0..RECORDS).find(|&r| self.r8(r * REC + h::ACTIVE) == 0) else { return };
        let b = r * REC;
        self.w8(b + h::ACTIVE, 1);
        self.w32(b + h::DELAY, delay as u32);
        self.w8(b + h::SOUND, snd as u8);
        self.w32(b + h::AGE, 0);
        self.w8(b + h::SCROLLS, scrolls as u8);
        self.w8(b + h::IN, inward as u8);
        if delay == 0 { w.sound(snd, 4, 0); }
        // (first colour, last colour) of the ramp: runs forwards for fragments, backwards for the implosion
        let (a, z) = if inward {
            let end = (id as i32).wrapping_mul(10).wrapping_add(9);
            let end = if (end as u8) <= 129 { end } else { 0 };
            (end, end.wrapping_sub(9))
        } else {
            let start = (id as i32).wrapping_mul(10);
            let start = if (start as u8) <= 120 { start } else { 0 };
            (start, start.wrapping_add(9))
        };
        let (xf, yf) = (x as f32, y as f32);
        for p in 0..PARTICLES {
            let o = b + p * PART;
            self.w8(o, 1);
            let (sx, sy) = self.take_star();
            if inward {
                self.wf(o + 8, (40.0f64 * sx as f64 + x as f64) as f32);
                self.wf(o + 12, (40.0f64 * sy as f64 + y as f64) as f32);
                self.wf(o + 16, -sx);
                self.wf(o + 20, -sy);
            } else {
                self.wf(o + 16, sx);
                self.wf(o + 20, sy);
                self.wf(o + 8, xf);
                self.wf(o + 12, yf);
            }
            self.w32(o + 4, 0);
            self.w8(o + 1, a as u8);
            self.w8(o + 2, a as u8);
            self.w8(o + 3, z as u8);
        }
    }

    /// 0x29b10: move, colour and draw every fragment.
    pub fn update<W: DebrisWorld>(&mut self, w: &mut W) {
        let scroll = w.scroll_step() as f32;
        for r in 0..RECORDS {
            let b = r * REC;
            if self.r8(b + h::ACTIVE) == 0 { continue; }
            if self.r32(b + h::DELAY) == 1 {
                let s = self.r8(b + h::SOUND);
                if s as i8 != -1 { w.sound(s as i32, 4, 0); }
            }
            let d = self.r32(b + h::DELAY).wrapping_sub(1);
            self.w32(b + h::DELAY, d);
            if (d as i32) > 0 { continue; }
            let age = self.r32(b + h::AGE).wrapping_add(1);
            self.w32(b + h::AGE, age);
            if (age as i32) > 40 {
                self.free(r);
                continue;
            }
            let inward = self.r8(b + h::IN) != 0;
            let scrolls = self.r8(b + h::SCROLLS) != 0;
            for p in 0..PARTICLES {
                let o = b + p * PART;
                if self.r8(o) == 0 { continue; }
                if scrolls { let y = self.rf(o + 12) + scroll; self.wf(o + 12, y); }
                let x = self.rf(o + 8) + self.rf(o + 16);
                self.wf(o + 8, x);
                let y = self.rf(o + 12) + self.rf(o + 20);
                self.wf(o + 12, y);
                let tick = self.r32(o + 4).wrapping_add(1);
                self.w32(o + 4, tick);
                if (tick as i32) > 3 {
                    self.w32(o + 4, 0);
                    if !inward {
                        let c = self.r8(o + 1).wrapping_add(1);
                        self.w8(o + 1, c);
                        if c > self.r8(o + 3) { let e = self.r8(o + 3); self.w8(o + 1, e); }
                    } else {
                        let c = self.r8(o + 1).wrapping_sub(1);
                        self.w8(o + 1, c);
                        if c < self.r8(o + 3) { let e = self.r8(o + 3); self.w8(o + 1, e); }
                    }
                }
                let (fx, fy) = (64.0f32 + x, 64.0f32 + y);
                if fx < 61.0 || fx > 477.0 || fy < 61.0 || fy > 544.0 {
                    if !inward { self.w8(o, 0); }
                } else {
                    let colour = self.colours.get(self.r8(o + 1) as usize).copied().unwrap_or(0);
                    w.plot(f2i(fx), f2i(fy), colour);
                }
            }
        }
    }
}

impl std::fmt::Debug for Debris {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Debris").field("active", &self.active()).field("next_star", &self.next_star).finish()
    }
}
