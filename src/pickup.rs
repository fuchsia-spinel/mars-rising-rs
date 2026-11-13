//! Power-ups ("pickups"): the 20-slot pool at `D.0fc4` (128-byte records, live counter `D[0x1754]`), the spawner 0x2a040, the per-frame
//! update 0x2b884 (expiry, animation, drift/bounce, collection by a ship) and the effect a ship gets from each kind (0x2c134).
//!
//! Verified against the original executable with the same differential method as `ship.rs` (see `tests/ship_golden.rs`).
//! Pickup kinds (record `+8`): 2 coin, 3 score item (variant 0..9 = points), 5 spread +1, 6 weapon level +1, 7 side missile, 8 extra life,
//! 9 keep-weapons insurance, 10 rear shots, 11 smart bomb, 12 drone, 13 shield +6, 14..17 armor level 2..5, 18 special charge, 19 speed-up,
//! 20 (never stored: becomes 10 coins), 21 big coin (10 coins). 0, 1, 4 and anything above 21 are never created.

use crate::draw::{emit, DrawOpts, Prim};
use crate::ship::{Ship, World, SCREEN_H, SCREEN_W, SPEED_BOOST, STATE_PLAYING};

pub const PICKUP_SLOTS: usize = 20;

/// One 128-byte pool record.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Pickup {
    pub active: bool,    // +0
    pub spawn_tick: u32, // +4
    pub kind: u32,       // +8
    pub f12: i32,        // +12 always 1
    pub x: f32,          // +16 top-left
    pub y: f32,          // +20
    pub vx: f32,         // +24
    pub vy: f32,         // +28
    pub ax: f32,         // +32 added to vx every frame
    pub ay: f32,         // +36
    pub w: i32,          // +40 collision size
    pub h: i32,          // +44
    pub half_w: i32,     // +48
    pub half_h: i32,     // +52
    pub variant: i32,    // +56 (kind 3: points index; other kinds 1)
    pub scatter: u8,     // +60 non-zero: bounces around the screen on its own; zero: drifts down with the scroll
    pub spr_a: i32,      // +64 draw data
    pub spr_b: i32,      // +68
    pub spr_w: i32,      // +72
    pub spr_h: i32,      // +76
    pub off_x: i32,      // +80
    pub off_y: i32,      // +84
    pub blink: bool,     // +88 visibility flag, toggled every frame in the last 45 ticks of its life
    pub dir: bool,       // +89 animation direction
    pub sprite: i32,     // +92
    pub frame: i32,      // +96
    pub anim_tick: u32,  // +100
    pub anim_max: i32,   // +104
    pub action: i32,     // +108 draw/collect id (101..112, -1 for coins and score items)
    pub f112: i32,       // +112 (w - f120) / 2
    pub f116: i32,       // +116 always 39
    pub f120: i32,       // +120 draw width
    pub f124: i32,       // +124 always 12
}

#[derive(Debug, Clone, PartialEq)]
pub struct PickupPool {
    pub slots: [Pickup; PICKUP_SLOTS],
    pub count: i32,
}

impl Default for PickupPool {
    fn default() -> Self { PickupPool { slots: [Pickup::default(); PICKUP_SLOTS], count: 0 } }
}

/// Layout of kinds 5..=19 (record fields written by the spawner): (sprite, +104 or `None` = 5 (a register left over from the common prologue), action id, draw width,
/// w, h). Kind 8 (extra life) is the only one with its own size.
fn kind_table(kind: u8) -> Option<(i32, Option<i32>, i32, i32, i32, i32)> {
    Some(match kind {
        5 => (90, Some(2), 101, 50, 48, 29),
        6 => (91, Some(2), 102, 35, 48, 29),
        7 => (93, None, 103, 38, 48, 29),
        8 => (26, Some(0), 105, 45, 29, 28),
        9 => (94, None, 104, 20, 48, 29),
        10 => (95, None, 106, 20, 48, 29),
        11 => (96, Some(2), 108, 52, 48, 29),
        12 => (134, None, 107, 35, 48, 29),
        13 => (97, None, 109, 33, 48, 29),
        14 => (120, None, 110, 64, 48, 29),
        15 => (121, None, 110, 64, 48, 29),
        16 => (122, None, 110, 64, 48, 29),
        17 => (123, None, 110, 64, 48, 29),
        18 => (98, None, 111, 19, 48, 29),
        19 => (92, None, 112, 25, 48, 29),
        _ => return None,
    })
}

impl PickupPool {
    pub fn new() -> Self { Self::default() }

    /// 0x29ebc: free a slot (the counter is decremented unconditionally).
    pub fn free(&mut self, i: usize) {
        self.slots[i].active = false;
        self.count -= 1;
    }

    /// 0x2b73c: at the end of the level's coin bonus every coin (kind 2) and 10-coin pickup (kind 20) still on the screen turns into an explosion (effect 14, sound 39,
    /// moving with the ground unless it was bouncing around) and disappears. `effect` is 0x22670(x, y, kind, delay, sound, scrolls, loops, vx, vy).
    pub fn convert_coins<F: FnMut(i32, i32, i32, i32, i32, i32, i32, f32, f32)>(&mut self, mut effect: F) {
        for i in 0..PICKUP_SLOTS {
            let p = &self.slots[i];
            if !p.active || (p.kind != 2 && p.kind != 20) { continue; }
            let (x, y) = (p.x + (p.w / 2) as f32, p.y + (p.h / 2) as f32);
            let (scrolls, vx, vy) = (if p.scatter != 0 { 0 } else { 1 }, p.vx, p.vy);
            self.free(i);
            effect(x as i32, y as i32, 14, 0, 39, scrolls, 0, vx, vy);
        }
    }

    /// 0x2a040: create a pickup of `kind` centred on (x, y). `variant` (r6) selects the look of kind 3, `param` (r7) is the animation parameter and
    /// also the "scatter" flag (non-zero: random initial velocity, bounces), `announce` (r8) plays sound 16. Kinds whose upgrade every player already
    /// has are replaced by kind 20 which pays out as 10 coins.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn<W: World>(&mut self, p1: &Ship, p2: &Ship, kind: u8, variant: i32, param: i32, announce: i32, x: f32, y: f32, now: u32, world: &mut W) {
        if kind == 1 { return; }
        if (3..=20).contains(&kind) {
            let both = |f: fn(&Ship) -> bool| f(p1) && f(p2);
            let convert = match kind {
                3 => {
                    let n = match variant { 5 => 2, 6 => 3, 7 => 4, 8 => 5, 9 => 10, _ => 0 };
                    if n > 0 {
                        for _ in 0..n { self.spawn(p1, p2, 3, 4, 1, 0, x, y, now, world); }
                        return;
                    }
                    false
                }
                4 => true,
                5 => both(|s| !s.active || s.spread >= 5),
                6 => both(|s| !s.active || s.level >= 16),
                7 => both(|s| !s.active || s.missiles >= 3),
                9 => both(|s| !s.active || s.keep),
                10 => both(|s| !s.active || s.rear),
                18 => both(|s| !s.active || s.special_max >= 10),
                19 => both(|s| !s.active || s.vmax == SPEED_BOOST),
                20 => {
                    for _ in 0..3 { world.sound(59, 5, 5); }
                    for _ in 0..10 { self.spawn(p1, p2, 2, 1, 1, 0, x, y, now, world); }
                    return;
                }
                _ => false,
            };
            if convert {
                self.spawn(p1, p2, 20, 1, 1, 0, x, y, now, world);
                return;
            }
        }
        self.create(kind, variant, param, announce, x, y, now, world);
    }

    #[allow(clippy::too_many_arguments)]
    fn create<W: World>(&mut self, kind: u8, variant: i32, param: i32, announce: i32, x: f32, y: f32, now: u32, world: &mut W) {
        if self.count >= PICKUP_SLOTS as i32 { return; }
        let Some(i) = (0..PICKUP_SLOTS).find(|&i| !self.slots[i].active) else { return };
        {
            let p = &mut self.slots[i];
            p.active = true;
            p.spawn_tick = now;
            p.kind = kind as u32;
            p.scatter = param as u8;
            p.blink = true;
            p.spr_a = 113;
            p.spr_b = 5;
            p.spr_w = 48;
            p.spr_h = 48;
            p.f124 = 12;
            p.anim_tick = now;
            p.dir = true;
            p.frame = 1;
        }
        match kind {
            2 | 21 => {
                let p = &mut self.slots[i];
                p.f12 = 1;
                p.vx = 0.0; p.vy = 0.0; p.ax = 0.0; p.ay = 0.0;
                if kind == 21 {
                    p.w = 26; p.h = 26; p.sprite = 136; p.anim_max = 8;
                } else if world.coin_style() {
                    p.sprite = 137; p.anim_max = 0; p.w = 30; p.h = 28;
                } else {
                    p.sprite = 135; p.anim_max = 8; p.w = 26; p.h = 26;
                }
                p.variant = 1;
                p.spr_a = 114; p.spr_b = 1; p.spr_w = 38; p.spr_h = 38;
                p.action = -1;
            }
            3 => {
                let p = &mut self.slots[i];
                p.f12 = 1;
                p.vx = 0.0; p.vy = 0.0; p.ax = 0.0; p.ay = 0.0;
                p.variant = variant;
                match variant {
                    1 => {}
                    2 => { p.sprite = 87; p.w = 24; p.h = 20; p.anim_max = 23; }
                    3 => { p.sprite = 88; p.w = 29; p.h = 20; p.anim_max = 23; }
                    4 => { p.sprite = 89; p.w = 29; p.h = 20; p.anim_max = 23; }
                    _ => { p.variant = 2; p.w = 24; p.h = 20; p.anim_max = 0; }
                }
                p.action = -1;
            }
            5..=19 => {
                let (sprite, anim, action, fw, w, h) = kind_table(kind).unwrap();
                let p = &mut self.slots[i];
                p.w = w; p.h = h; p.f12 = 1;
                p.vx = 0.0; p.vy = 0.0; p.ax = 0.0; p.ay = 0.0;
                p.sprite = sprite;
                p.variant = 1;
                p.anim_max = anim.unwrap_or(5); // r7 still holds the constant 5 written to +68 just before the switch
                p.action = action;
                p.f120 = fw;
                p.f112 = (p.w - p.f120) / 2;
                p.f116 = 39;
            }
            _ => {
                // 0, 1, 4, 20 and everything above 21: the record is dropped again
                self.slots[i].active = false;
                return;
            }
        }
        self.finish(i, param, announce, x, y, world);
    }

    /// 0x2b4bc: size-dependent fields, position (centred on x, y; never above the screen) and the random scatter velocity.
    fn finish<W: World>(&mut self, i: usize, param: i32, announce: i32, x: f32, y: f32, world: &mut W) {
        {
            let p = &mut self.slots[i];
            p.half_w = p.w / 2;
            p.half_h = p.h / 2;
            p.x = x - p.half_w as f32;
            p.y = y - p.half_h as f32;
            if !(p.y >= 0.0) { p.y = 0.0; }
            p.off_x = (p.spr_w - p.w) / 2;
            p.off_y = (p.spr_h - p.h) / 2;
            if p.h == 29 { p.off_y += 3; }
        }
        if (param as u8) != 0 {
            let spd = (world.random(1, 3) as u16) as f32;
            let mut ex = (world.random(10, 16) as u16) as f32;
            if (world.random(0, 1) as u16) != 0 { ex = -ex; }
            let mut ey = (world.random(10, 16) as u16) as f32;
            if self.slots[i].y > 120.0 && (world.random(0, 1) as u16) != 0 { ey = -ey; }
            // fmuls f0 = ey*ey; fmadds f1 = ex*ex + f0 (double arithmetic, one rounding to single); sqrt in double; frsp
            let t = ey * ey;
            let sum = ((ex as f64 * ex as f64) + t as f64) as f32;
            let len = (sum as f64).sqrt() as f32;
            let (ex, ey) = (ex / len, ey / len);
            let p = &mut self.slots[i];
            p.vx = ex * spd;
            p.vy = ey * spd;
            p.ax = (0.4f64 * ex as f64) as f32;
            p.ay = (0.4f64 * ey as f64) as f32;
        }
        self.count += 1;
        if (announce as u8) != 0 { world.sound(16, 4, 0); }
    }

    /// 0x2b884: one frame for every live pickup. `p1` and `p2` are the two players (only ships in state 3 can collect).
    pub fn update<W: World>(&mut self, p1: &mut Ship, p2: &mut Ship, now: u32, world: &mut W) {
        if self.count < 1 { return; }
        let speed = world.scroll_speed() as f32;
        let b1 = if p1.state == STATE_PLAYING { Some(p1.pickup_hitbox()) } else { None };
        let b2 = if p2.state == STATE_PLAYING { Some(p2.pickup_hitbox()) } else { None };
        for i in 0..PICKUP_SLOTS {
            if !self.slots[i].active { continue; }
            let life: u32 = if self.slots[i].kind == 4 { 290 } else { 185 };
            let spawn = self.slots[i].spawn_tick;
            if now >= life.wrapping_add(spawn) {
                let p = self.slots[i];
                self.free(i);
                let cx = (p.x + (p.w / 2) as f32) as i32;
                let cy = (p.y + (p.h / 2) as f32) as i32;
                world.effect(cx, cy, 14, 0, 39, (p.scatter == 0) as i32, 0, p.vx, p.vy);
                continue;
            }
            if now >= (life - 45).wrapping_add(spawn) {
                let p = &mut self.slots[i];
                p.blink = !p.blink;
            }
            Self::animate(&mut self.slots[i], now);
            Self::step(&mut self.slots[i], speed);
            if let Some(b) = b1 {
                if Self::overlaps(&self.slots[i], b) {
                    let p = self.slots[i];
                    p1.collect(&p, now, world);
                    self.free(i);
                }
            }
            if self.slots[i].active {
                if let Some(b) = b2 {
                    if Self::overlaps(&self.slots[i], b) {
                        let p = self.slots[i];
                        p2.collect(&p, now, world);
                        self.free(i);
                    }
                }
            }
        }
    }

    /// 0x2c480: draw every live pickup (shadow, translucent aura, the item, its caption). Pickups that drifted more than 64 px outside the screen
    /// are freed here (not in the update).
    pub fn draw<W: World>(&mut self, opts: DrawOpts, world: &mut W) {
        if self.count < 1 { return; }
        for i in 0..PICKUP_SLOTS {
            let p = self.slots[i];
            if !p.active { continue; }
            if p.x < -64.0 || p.x + (p.w as f32) > 480.0 || p.y < -64.0 || p.y + (p.h as f32) > 544.0 {
                self.free(i);
                continue;
            }
            let (xi, yi) = (p.x as i32, p.y as i32);
            if opts.shadows {
                emit(world, Prim::Shadow, xi - 24, yi + 44, p.w, p.h, p.sprite, p.frame);
            }
            if opts.translucent {
                emit(world, Prim::Trans, xi - p.off_x, yi - p.off_y, p.spr_w, p.spr_h, p.spr_a, p.spr_b);
            }
            if p.blink {
                emit(world, Prim::Anim, xi, yi, p.w, p.h, p.sprite, p.frame);
                if p.action != -1 {
                    emit(world, Prim::Anim, xi + p.f112, yi + p.f116, p.f120, p.f124, p.action, 1);
                }
            }
        }
    }

    fn overlaps(p: &Pickup, b: (i32, i32, i32, i32)) -> bool {
        let (l, t, r, bt) = b;
        let px = p.x as i32;
        let py = p.y as i32;
        !(l >= px + p.w || px >= r || t >= py + p.h || py >= bt)
    }

    /// Per-kind frame animation (jump table at data 0x6adc).
    fn animate(p: &mut Pickup, now: u32) {
        let due = |t: u32, d: u32| now >= t.wrapping_add(d);
        match p.kind {
            2 | 21 => {
                if p.anim_max > 0 && due(p.anim_tick, 3) {
                    p.anim_tick = now;
                    p.frame -= 1;
                    if p.frame < 1 { p.frame = p.anim_max; }
                }
            }
            3 => {
                if due(p.anim_tick, 3) {
                    p.anim_tick = now;
                    p.frame += 1;
                    if p.frame > p.anim_max { p.frame = 1; }
                }
            }
            5 | 6 | 12 => {
                if due(p.anim_tick, 4) {
                    p.frame = if p.frame == 1 { 2 } else { 1 };
                    p.anim_tick = now;
                }
            }
            11 => {
                if due(p.anim_tick, 3) {
                    p.frame = if p.frame == 1 { 2 } else { 1 };
                    p.anim_tick = now;
                }
            }
            7 | 9 | 10 | 13 | 14..=19 => {
                let d = if p.kind >= 14 { 4 } else { 2 };
                if due(p.anim_tick, d) {
                    if p.dir {
                        p.frame += 1;
                        if p.frame > p.anim_max { p.frame = p.anim_max - 1; p.dir = false; }
                    } else {
                        p.frame -= 1;
                        if p.frame < 1 { p.frame = 2; p.dir = true; }
                    }
                    p.anim_tick = now;
                }
            }
            _ => {}
        }
    }

    /// 0x2bd78: movement. Scatter pickups bounce inside the screen (speed limited to 6 per axis, the acceleration flips at the limit);
    /// the others just drift down with the scroll.
    fn step(p: &mut Pickup, speed: f32) {
        if p.scatter == 0 {
            p.y += speed;
        } else {
            if p.vx > 6.0 { p.vx = 6.0; p.ax = -p.ax; }
            if p.vx < -6.0 { p.vx = -6.0; p.ax = -p.ax; }
            if p.vy > 6.0 { p.vy = 6.0; p.ay = -p.ay; }
            if p.vy < -6.0 { p.vy = -6.0; p.ay = -p.ay; }
            p.vx += p.ax;
            p.vy += p.ay;
            if p.x < 0.0 { p.vx = -p.vx; p.x = 0.0; }
            if p.x + (p.w as f32) > SCREEN_W as f32 { p.x = (SCREEN_W - p.w) as f32; p.vx = -p.vx; }
            if p.y < 0.0 { p.vy = -p.vy; p.y = 0.0; }
            if p.y + (p.h as f32) > SCREEN_H as f32 { p.vy = -p.vy; p.y = (SCREEN_H - p.h) as f32; }
        }
        p.x += p.vx;
        p.y += p.vy;
    }
}

impl Ship {
    /// 0x24348: coins, at most 99.
    pub fn add_coins(&mut self, n: i32) {
        if !self.active { return; }
        self.coins += n;
        if self.coins > 99 { self.coins = 99; }
    }

    /// 0x26838: one more parallel shot, at most 5.
    pub fn add_spread(&mut self, n: i32) {
        self.spread += n;
        if self.spread > 5 { self.spread = 5; }
    }

    /// 0x268c4: side missile level, at most 3.
    pub fn add_missile(&mut self) {
        self.missiles += 1;
        if self.missiles > 3 { self.missiles = 3; }
    }

    /// 0x24164: extra life (at most 12) with HUD flash and sound 65.
    pub fn extra_life<W: World>(&mut self, now: u32, world: &mut W) {
        if !self.active { return; }
        self.lives += 1;
        if self.lives > 12 {
            self.lives = 12;
            return;
        }
        self.flag_72 = true;
        self.tick_76 = now;
        world.hud_lives(self.input_set, self.lives, 1);
        for _ in 0..3 { world.sound(65, 4, 0); }
    }

    /// 0x242c4: shield change, kept within 0..=12.
    pub fn shield_add<W: World>(&mut self, n: i32, world: &mut W) {
        if !self.active { return; }
        self.shield += n;
        if self.shield > 12 { self.shield = 12; } else if self.shield < 0 { self.shield = 0; }
        world.hud(0x1f69c, self.input_set as i32, self.shield);
    }

    /// 0x265ec: armor level (`+172`, 1..5): only a higher level is taken (sound 17), otherwise sound 53.
    pub fn set_armor<W: World>(&mut self, level: u8, world: &mut W) {
        if self.hud_level >= level {
            world.sound(53, 4, 0);
        } else {
            self.hud_level = level;
            world.sound(17, 4, 0);
            world.hud(0x1ff94, self.input_set as i32, level as i32);
        }
    }

    /// 0x268f4(ship, 1): the keep-weapons insurance.
    pub fn set_keep<W: World>(&mut self, world: &mut W) {
        self.keep = true;
        world.hud(0x1fe28, self.input_set as i32, 1);
        world.sound(22, 4, 0);
    }

    /// 0x26fd8(ship, 1): the smart bomb (kills/damages everything on screen; the screen routines are hooks).
    pub fn smart_bomb<W: World>(&mut self, world: &mut W) {
        self.smart_bomb_sweep(world);
        for _ in 0..3 { world.sound(11, 4, 0); }
    }

    /// 0x26fd8(ship, 0): the silent smart bomb (the level's end uses it, 0xdb54).
    pub fn smart_bomb_sweep<W: World>(&mut self, world: &mut W) {
        world.hud(0x3fbfc, self.number as i32, 0);
        world.hud(0x2e6a8, self.number as i32, 0);
        world.hud(0x38a1c, 0, 0);
    }

    /// 0x2699c: add a drone (at most 2; the first takes slot 1, the second slot 2).
    pub fn add_drone<W: World>(&mut self, world: &mut W) {
        if self.state != STATE_PLAYING || self.drones >= 2 { return; }
        let which = if self.drone_a { 2 } else { 1 };
        if world.drone_add(self.number, which) {
            self.drones += 1;
            if which == 1 { self.drone_a = true; } else { self.drone_b = true; }
        }
    }

    /// 0x26210: add `points_index` score (index 2..=9 = 500, 1000, 2000, 4000, 6000, 8000, 10000, 20000; larger values are the points themselves,
    /// 1 does nothing), multiplied by the armor level (`+172`); an extra life is given every time the score passes the next threshold.
    pub fn add_score<W: World>(&mut self, points_index: i32, flash: bool, now: u32, world: &mut W) {
        if !self.active { return; }
        let pts: i32 = match points_index {
            1 => return,
            2 => 500, 3 => 1000, 4 => 2000, 5 => 4000, 6 => 6000, 7 => 8000, 8 => 10000, 9 => 20000,
            v => v,
        };
        self.score = self.score.wrapping_add(pts.wrapping_mul(self.hud_level as i32));
        self.score_flag = 1;
        if flash {
            self.score_flash = 1;
            self.score_tick = now;
        }
        if self.score < self.next_life { return; }
        self.extra_life(now, world);
        self.score_flash = 1;
        self.score_tick = now;
        if self.next_life < 30000 {
            self.next_life = 30000;
        } else {
            self.next_life = self.next_life.wrapping_add(self.lives_awarded.wrapping_mul(10000).wrapping_add(30000));
        }
        self.lives_awarded += 1;
    }

    /// 0x2c134: the ship collects `p`: sound, hit flash, effect, then the kind's bonus.
    pub fn collect<W: World>(&mut self, p: &Pickup, now: u32, world: &mut W) {
        let snd = match p.kind { 2 => 34, 3 => 1, _ => 8 };
        world.sound(snd, 4, 0);
        if self.state == STATE_PLAYING {
            self.hit_flash = true;
            self.hit_flash_tick = now;
        }
        let cx = (p.x + p.half_w as f32) as i32;
        let cy = (p.y + p.half_h as f32) as i32;
        world.effect_aux(0x29714, cx, cy, 5, 0, -1);
        match p.kind {
            2 => self.add_coins(1),
            3 => { if p.variant != 1 { self.add_score(p.variant, true, now, world); } }
            5 => self.add_spread(1),
            6 => {
                world.sound(63, 4, 0);
                world.sound(63, 4, 0);
                self.add_level(1);
            }
            7 => self.add_missile(),
            8 => self.extra_life(now, world),
            9 => self.set_keep(world),
            10 => {
                world.sound(25, 4, 0);
                self.rear = true;
            }
            11 => self.smart_bomb(world),
            12 => self.add_drone(world),
            13 => {
                world.sound(40, 4, 0);
                self.shield_add(6, world);
            }
            14 => self.set_armor(2, world),
            15 => self.set_armor(3, world),
            16 => self.set_armor(4, world),
            17 => self.set_armor(5, world),
            18 => {
                world.sound(64, 4, 0);
                self.add_special(1);
            }
            19 => self.set_speed_boost(true),
            21 => {
                world.sound(59, 4, 2);
                world.sound(59, 4, 2);
                self.add_coins(10);
            }
            _ => {}
        }
    }
}

/// 0x29edc: what a wiped-out enemy group leaves behind. Each player has a counter of wiped groups (`Ship::wipes`, +352): every wipe adds 1; once
/// it has reached 8 (for player 1) or exceeds 8 (for every other owner: the original compares `< 8` for the first and `<= 8` for the second, so
/// player 2 needs a ninth) it is reset and a random power-up is made, otherwise a coin (kind 2). Returns the kind (spawned with variant 1, param 1,
/// announce 1 at the group's last position).
pub fn bonus_kind(owner: u8, wipes: &mut [i32; 2], mut random: impl FnMut(i32, i32) -> i32) -> u8 {
    let idx = if owner == 1 { 0 } else { 1 };
    wipes[idx] = wipes[idx].wrapping_add(1);
    let special = if owner == 1 { wipes[idx] >= 8 } else { wipes[idx] > 8 };
    if !special {
        return 2;
    }
    wipes[idx] = 0;
    let r = random(0, 20) as u16;
    match r {
        0..=2 => 6,
        3..=5 => 18,
        6..=12 => 13,
        13 | 14 => 9,
        15 => 19,
        16..=20 => 20,
        _ => 6,
    }
}

/// 0xea4c: the reward for destroying the last ground target of a level. `armor1` / `armor2` = the armor level (`+172`) of each ship that is alive
/// (state 3), `None` otherwise; `difficulty` = `D.13ec`. Returns `(kind, variant)` of the pickup to spawn at the last target with param 1 and no
/// announcement (`kind 3` variants 8 / 9 are the big score items), or `None`.
pub fn clear_reward(armor1: Option<u8>, armor2: Option<u8>, difficulty: u32) -> Option<(i32, i32)> {
    let (mut r28, mut r27) = (1u8, 1u8);
    let (mut big, mut medium) = (false, false);
    match armor1 { Some(1) => r28 = 2, Some(2) => r28 = 3, Some(3) => r28 = 4, Some(4) => r28 = 5, Some(5) => big = true, _ => {} }
    match armor2 { Some(1) => r27 = 2, Some(2) => r27 = 3, Some(3) => r27 = 4, Some(4) => r27 = 5, Some(5) => big = true, _ => {} }
    let mut r4 = if r28 > 1 { r28 } else { 1 };
    if r27 > 1 && r27 < r4 { r4 = r27; }
    let d = difficulty as i32;
    let mut kind = 1;
    match r4 {
        3 => { if d >= 3 { kind = 15 } else { medium = true } }
        2 => { if d >= 2 { kind = 14 } }
        4 => { if d >= 5 { kind = 16 } else { medium = true } }
        5 => { if d >= 7 { kind = 17 } else { big = true } }
        _ => {}
    }
    if big { Some((3, 9)) } else if medium { Some((3, 8)) } else if kind != 1 { Some((kind, 1)) } else { None }
}
