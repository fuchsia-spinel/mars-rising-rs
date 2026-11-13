//! Player ship: movement, hit boxes, player shots, fire control and the charged "wave" beam.
//!
//! Everything here was ported from the original PowerPC executable (see RE_NOTES.md, "Ship and weapons") and checked by
//! *executing the original machine code* on random states with `tools/re/ppcemu.py` and comparing the results bit for bit
//! (`tests/ship_golden.rs`, vectors in `tests/vectors/`). Addresses in comments are code-section offsets of the original;
//! `+N` is the byte offset of the field in the original 408-byte player struct (or 48-byte shot record).
//!
//! The module is deliberately free of rendering, audio and enemy code: everything it needs from the rest of the game goes
//! through the [`World`] trait. Floating point is `f32` with the same operation order as the original (`fadds`, `fsubs`, ...),
//! integer<->float conversions truncate toward zero like `fctiwz`.
//!
//! Not in this module: power-ups (pickup.rs), drones and side missiles (drone.rs), drawing (ship_draw.rs); not ported yet: the button-2 special (0x26b58).

/// Playfield size in pixels (13 tiles of 32 px wide, 480 px tall).
pub const SCREEN_W: i32 = 416;
pub const SCREEN_H: i32 = 480;
/// Velocity change per frame while a direction is held / released (`D[-26944]`, 0x40200000).
pub const ACCEL: f32 = 2.5;
/// Default maximum speed (`D[-26928]`, 0x40e66666 = 7.2f) and the speed-up power-up value (`D[-26940]`, 9.0f).
pub const SPEED_NORMAL: f32 = 7.2;
pub const SPEED_BOOST: f32 = 9.0;
/// Size of a ship sprite (set at 0x23de4).
pub const SHIP_W: i32 = 48;
pub const SHIP_H: i32 = 49;
/// Number of slots in the player-shot pool (0x22170 loops over 60 records of 48 bytes).
pub const SHOT_SLOTS: usize = 60;

/// Ship `state` (+1) value while the ship is alive and controllable. (1 = appearing, 2 = destroyed; set by the session loop.)
pub const STATE_PLAYING: u8 = 3;

/// The six per-frame input flags (bytes +96..+101). Direction names are *screen* directions, proven by the movement code:
/// +96 decreases vy (y grows downward, shots fly with negative vy), +97 increases it. This corrects the earlier guess in input.rs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ShipInput {
    pub up: bool,    // +96
    pub down: bool,  // +97
    pub left: bool,  // +98
    pub right: bool, // +99
    pub fire: bool,  // +100 button 1
    pub alt: bool,   // +101 button 2
}

/// What the ship routines need from the rest of the game (enemies, effects, sound, other weapons).
pub trait World {
    /// 0x2ead4: test the box (x0,y0)-(x1,y1) against all enemies (slot 39 down to 0), applying damage. `hp` is the attacker's
    /// remaining hit points, `owner` its player number. Returns the total damage dealt (the attacker loses that much `hp`).
    fn hit_enemies(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, hp: i32, owner: u8) -> i32;
    /// 0x22670(x, y, kind, delay, param, a, b, vx, vy): spawn an effect (vx/vy: initial drift, 0 for the ship code). Shots use (13 = impact, 1 = shot/beam destroyed) with delay 0, param -1, a = b = 0.
    fn effect(&mut self, x: i32, y: i32, kind: i32, delay: i32, param: i32, a: i32, b: i32, vx: f32, vy: f32);
    /// 0x29714 / 0x298ec(x, y, kind, delay, param, 0): the two "ring/flash" effect spawners used by the ship's destruction and appearance.
    fn effect_aux(&mut self, _id: u32, _x: i32, _y: i32, _kind: i32, _delay: i32, _param: i32) {}
    /// 0x2a040(0, 0, kind, 1, 1, 0, x, y): drop a power-up of `kind` (2 coin, 5 spread, 6 level, 7 missiles, 10 rear, 12 drone, 18 special, 19 speed, 21 big coin).
    fn drop_pickup(&mut self, _kind: i32, _x: f32, _y: f32) {}
    /// 0xd7a0 (data 0x13ec): weapon level a ship starts (and restarts) with.
    fn start_level(&mut self) -> i32 { 1 }
    /// 0xd7a8 (data 0x13f0): "keep progress" bonus flag used when a ship is set up.
    fn level_bonus(&mut self) -> i32 { 0 }
    /// HUD / screen routines called by the ship code: 0x1fe28, 0x1ff94, 0x1f69c, 0x1f848, 0x1f51c, 0x1f5c4, 0x1fc50, 0x28510, 0x4560c. `id` is the
    /// original address, a and b the first two integer arguments.
    fn hud(&mut self, _id: u32, _a: i32, _b: i32) {}
    /// 0x2c87c(number): remove the drones of a player.
    fn drones_reset(&mut self, _number: u8) {}
    /// 0xf5b8: "demo / replay" test used by the session loop before the respawn sound.
    fn demo_check(&mut self) -> bool { false }
    /// 0xa0d4(lo, hi): Toolbox `Random()` scaled to lo..=hi: `((rnd as u16) * (hi - lo + 1)) / 65536 + lo`.
    fn random(&mut self, lo: i32, _hi: i32) -> i32 { lo }
    /// data byte 0x13d8 (read by 0xe090): chooses the coin look (26x26 + animation vs. 30x28).
    fn coin_style(&mut self) -> bool { false }
    /// 0x2c760(player, which): add drone number `which` (1 or 2) for a player; returns whether it was added.
    fn drone_add(&mut self, _number: u8, _which: i32) -> bool { false }
    /// data 0x13c4 (read by 0xc098): vertical scroll speed in px per frame (0 or 1).
    fn scroll_speed(&mut self) -> i32 { 0 }
    /// 0x45bac(id, priority, delay): play sound effect `id` (0..71); `priority` is clamped to 1..5; a non-zero `delay` (frames) queues it.
    fn sound(&mut self, id: i32, priority: i32, arg: i32);
    /// 0x277e4: launch the side missiles; returns whether any were launched.
    fn launch_missiles(&mut self, _ship: &Ship) -> bool { false }
    /// 0x26a80 -> 0x2c960: let the drones of `ship` fire (only called while `ship.drones > 0`).
    fn drones_fire(&mut self, _ship: &Ship, _shots: &mut ShotPool) {}
    /// 0x26fd8 + 0x24544: the charge overheated, destroy the ship.
    fn ship_overheated(&mut self, _ship: &mut Ship) {}
    /// 0x45ca4(18, 3): start the charge-loop sound, returns its handle (0 when the sound was refused).
    fn charge_loop_start(&mut self) -> u32 { 0 }
    /// 0x51d2c: is this sound handle still playing? Handle 0 means "any voice" in the original, so the original's checks with an unused handle
    /// (the over-charge stream of a beam released below charge 55) are true while anything at all plays, and `sound_stop(0)` stops everything.
    fn sound_playing(&mut self, _handle: u32) -> bool { false }
    /// 0x51b4c: stop a sound handle.
    fn sound_stop(&mut self, _handle: u32) {}
    /// 0x45e24(2, 2, ..., callback 0x2766c, player number): start the over-charge stream, returns its handle. When the voice ends or is stopped the
    /// callback sets the player's `stream_ready` (see `Game::step`).
    fn overcharge_stream_start(&mut self, _number: u8) -> u32 { 0 }
    /// 0x3fec8(l, t, r, b, 0, number): is there a ground target under the bomb-sight box? (`true` = the original returned something other than -1.)
    fn ground_target(&mut self, _l: i32, _t: i32, _r: i32, _b: i32, _number: u8) -> bool { false }
    /// 0x45d54(id, priority, rate): like `sound` at a playback-rate multiplier (16.16 fixed; the original passes 36864 = 0.5625 for player 2's warning).
    fn sound_pan(&mut self, _id: i32, _priority: i32, _pan: i32) {}
    /// A blit request from one of the draw routines (0x2f88 / 0x3170 / 0x3264 / 0x3540 / 0x344c), in call order.
    fn draw(&mut self, _d: crate::draw::Draw) {}
    /// 0x1f848(input_set, lives, flash): redraw the lives counter in the HUD (`flash` = 1 when a life was just won, 0 when the flash is over).
    fn hud_lives(&mut self, _input_set: u8, _lives: i32, _flash: i32) {}
}

/// One record of the player-shot pool (48 bytes at data `0xd09c`).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Shot {
    pub active: bool,   // +0
    pub piercing: u8,   // +1  nonzero: survives hits until `hp` < 1
    pub owner: u8,      // +2  player number (1 or 2)
    pub x: f32,         // +4
    pub y: f32,         // +8
    pub vx: f32,        // +12
    pub vy: f32,        // +16
    pub kind: i32,      // +20 0 = small, 1 = beam segment, 2 = large (3 = drone shot, not used here)
    pub w: i32,         // +24
    pub h: i32,         // +28
    pub sprite: i32,    // +32 animation id in the sprite table
    pub frame: i32,     // +36
    pub hp: i32,        // +40 damage it can still deal
    pub start_tick: u32, // +44 kind 1 only: does not move before this tick
}

/// The 60-slot pool plus the live counter `D[-27172]` (0x15dc).
#[derive(Debug, Clone, PartialEq)]
pub struct ShotPool {
    pub shots: [Shot; SHOT_SLOTS],
    pub count: i32,
}

impl Default for ShotPool {
    fn default() -> Self { ShotPool { shots: [Shot::default(); SHOT_SLOTS], count: 0 } }
}

impl ShotPool {
    pub fn new() -> Self { Self::default() }

    /// 0x21224: free every slot.
    pub fn clear(&mut self) {
        for i in 0..SHOT_SLOTS { self.free(i); }
        self.count = 0;
    }

    /// 0x21268: deactivate a slot and decrement the counter (the original decrements unconditionally).
    pub fn free(&mut self, i: usize) {
        self.shots[i].active = false;
        self.count -= 1;
    }

    /// Claim the next free slot at or after `from`, as every spawn routine does (active=1, counter+1).
    fn claim(&mut self, from: usize) -> Option<usize> {
        for i in from..SHOT_SLOTS {
            if !self.shots[i].active {
                self.shots[i].active = true;
                self.count += 1;
                return Some(i);
            }
        }
        None
    }

    /// 0x217c8 (+ 0x21ad8): the forward volley of the current spread (`ship.spread` 0..5 -> 1,2,3,4,6,6 shots).
    /// Handles weapon kinds 0 (small) and 2 (large); kind 1 uses [`spawn_beam`](Self::spawn_beam). Returns false when the pool is full.
    pub fn spawn_front(&mut self, ship: &mut Ship) -> bool {
        let n: i32 = match ship.spread {
            0 => 1, 1 => 2, 2 => 3, 3 => 4, 4 | 5 => 6,
            _ => return false,
        };
        if self.count > 60 - n { return false; }
        let kind = ship.weapon_kind;
        if kind != 0 && kind != 2 { return false; } // the original reports a fatal error here
        let mut placed = 0;
        let mut from = 0;
        while let Some(slot) = self.claim(from) {
            from = slot + 1;
            let s = &mut self.shots[slot];
            s.kind = kind;
            s.owner = ship.number;
            s.vx = 0.0;
            s.vy = -20.0;
            s.piercing = 0;
            s.frame = 1;
            s.hp = 1;
            if kind == 0 { s.w = 3; s.h = 10; s.sprite = 35; } else { s.w = 4; s.h = 32; s.sprite = 38; }
            self.place_front(slot, placed, n, ship);
            placed += 1;
            if placed == n { return true; }
        }
        false
    }

    /// 0x21ad8: position shot `i` of an `n`-shot volley in front of the ship.
    fn place_front(&mut self, slot: usize, i: i32, n: i32, ship: &mut Ship) {
        let s = &mut self.shots[slot];
        s.x = ((ship.x as i32) + 2) as f32;
        s.y = ((ship.y as i32) + 2) as f32;
        match n {
            2 => { s.x += if i == 0 { 17.0 } else { 25.0 }; ship.pattern = 1; }
            3 => {
                match i { 0 => s.x += 12.0, 1 => s.x += 20.0, 2 => s.x += 28.0, _ => {} }
                ship.pattern = 1;
            }
            4 => {
                match i { 0 => s.x += 8.0, 1 => s.x += 16.0, 2 => s.x += 24.0, 3 => s.x += 32.0, _ => {} }
                ship.pattern = 3;
            }
            6 => {
                match i {
                    0 => s.x += 6.0,
                    1 => s.x += 12.0,
                    2 => { s.x += 17.0; s.y -= 25.0; }
                    3 => { s.x += 23.0; s.y -= 25.0; }
                    4 => s.x += 28.0,
                    5 => s.x += 34.0,
                    _ => {}
                }
                ship.pattern = 3;
            }
            _ => {
                // n == 1: centred on the ship
                let d = (ship.w - s.w) / 2;
                s.x += d as f32;
                ship.pattern = 1;
            }
        }
    }

    /// 0x21538 (+ 0x21a00): weapon kind 1, a column of four tall beam segments launched two ticks apart.
    pub fn spawn_beam(&mut self, ship: &mut Ship, now: u32) -> bool {
        const N: i32 = 4;
        if self.count > 60 - N { return false; }
        let mut placed: i32 = 0;
        let mut from = 0;
        while let Some(slot) = self.claim(from) {
            from = slot + 1;
            let s = &mut self.shots[slot];
            s.kind = 1;
            s.owner = ship.number;
            s.w = 3;
            s.h = 60;
            s.vx = 0.0;
            s.vy = -30.0;
            s.sprite = 37;
            s.frame = if placed < 3 { placed + 1 } else { 4 };
            s.hp = 1;
            s.piercing = 0;
            // 0x21a00
            s.x = (ship.x as i32) as f32;
            s.y = -60.0 + ship.y;
            let d = (ship.w - s.w) / 2;
            s.x += d as f32;
            ship.pattern = 1;
            s.start_tick = now.wrapping_add((2 * placed) as u32);
            placed += 1;
            if placed == N { return true; }
        }
        false
    }

    /// 0x2169c (+ 0x21e1c): two shots flying *backwards* (vy = +20) from the ship's tail.
    pub fn spawn_rear(&mut self, ship: &mut Ship) -> bool {
        const N: i32 = 2;
        if self.count > 60 - N { return false; }
        let mut placed: i32 = 0;
        let mut from = 0;
        while let Some(slot) = self.claim(from) {
            from = slot + 1;
            let s = &mut self.shots[slot];
            s.kind = 0;
            s.owner = ship.number;
            s.w = 3;
            s.h = 10;
            s.hp = 1;
            s.piercing = 0;
            s.sprite = 35;
            s.frame = 1;
            s.vx = 0.0;
            s.vy = 20.0;
            // 0x21e1c
            s.x = ((ship.x as i32) + 7) as f32;
            s.y = ((ship.y as i32) + ship.h) as f32;
            s.x += if placed == 0 { 12.0 } else { 20.0 };
            ship.pattern = 1;
            placed += 1;
            if placed == N { return true; }
        }
        false
    }

    /// 0x21288 (called with kind 3 by the drone fire routine 0x2c960): one 5 x 16 shot at (`x - 2`, `y`) flying up at 20 px per tick for the ship `owner`.
    /// Returns false when the pool is too full (more than 59 shots) or has no free slot.
    pub fn spawn_drone(&mut self, owner: u8, x: f32, y: f32) -> bool {
        if self.count > 59 { return false; }
        let Some(slot) = self.claim(0) else { return false };
        let s = &mut self.shots[slot];
        s.kind = 3;
        s.owner = owner;
        s.vx = 0.0;
        s.vy = -20.0;
        s.w = 5;
        s.h = 16;
        s.sprite = 36;
        s.frame = 1;
        s.hp = 1;
        s.piercing = 0;
        s.x = -2.0 + x;
        s.y = y;
        true
    }

    /// 0x22170: move every shot, drop the ones that left the screen, hit enemies.
    pub fn update<W: World>(&mut self, now: u32, world: &mut W) {
        if self.count <= 0 { return; }
        for i in 0..SHOT_SLOTS {
            let s = self.shots[i];
            if !s.active { continue; }
            if s.kind == 1 && (now as i32) < (s.start_tick as i32) { continue; }
            let x = s.x + s.vx;
            let y = s.y + s.vy;
            self.shots[i].x = x;
            self.shots[i].y = y;
            let wf = s.w as f32;
            let hf = s.h as f32;
            if x + wf < 0.0 || x > SCREEN_W as f32 || y + hf < 0.0 || y > SCREEN_H as f32 {
                self.free(i);
                continue;
            }
            let x0 = x as i32;
            let y0 = y as i32;
            let x1 = (x + wf) as i32;
            let y1 = (y + hf) as i32;
            let dmg = world.hit_enemies(x0, y0, x1, y1, s.hp, s.owner);
            if dmg <= 0 { continue; }
            self.shots[i].hp = s.hp - dmg;
            let cx = x0 + s.w / 2;
            let cy = y0 + s.h / 2;
            if s.piercing == 0 {
                world.effect(cx, cy, 13, 0, -1, 0, 0, 0.0, 0.0);
                self.free(i);
            } else if self.shots[i].hp < 1 {
                world.effect(cx, cy, 1, 0, -1, 0, 0, 0.0, 0.0);
                self.free(i);
            }
        }
    }
}

/// An enemy as seen by the shot-vs-enemy test 0x2ead4 (176-byte slot in the original).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct EnemyBox {
    pub active: bool, // +0
    pub state: i32,   // +20  >= 1: cannot be hit (not yet launched / dying)
    pub left: f32,    // +56
    pub top: f32,     // +60
    pub right: f32,   // +64
    pub bottom: f32,  // +68
}

/// 0x2ead4: walk the 40 enemy slots from 39 down to 0 and apply `damage(slot, remaining_hp, owner)` to each one the box overlaps (edges inclusive),
/// until the attacker's `hp` is used up. `damage` is the enemy-side routine (0x2ec14, or 0x41c88 for enemies with +168 set) and returns the
/// damage actually dealt. Returns the total.
pub fn hit_enemy_boxes(enemies: &[EnemyBox], x0: i32, y0: i32, x1: i32, y1: i32, mut hp: i32, owner: u8, mut damage: impl FnMut(usize, i32, u8) -> i32) -> i32 {
    let mut total = 0;
    for slot in (0..enemies.len().min(40)).rev() {
        let e = &enemies[slot];
        if !e.active || e.state >= 1 { continue; }
        if y1 < e.top as i32 || y0 > e.bottom as i32 || x1 < e.left as i32 || x0 > e.right as i32 { continue; }
        let d = damage(slot, hp, owner);
        hp -= d;
        total += d;
        if hp <= 0 { break; }
    }
    total
}

/// The charged "wave" beam (+208..+276, +356..+364), fired when button 1 is released after more than 20 frames of holding.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Beam {
    pub active: bool, // +212
    pub x: i32,       // +216
    pub y: i32,       // +220
    pub vx: i32,      // +224
    pub vy: i32,      // +228
    pub w: i32,       // +232
    pub h: i32,       // +236
    pub anim: i32,    // +240
    pub pat: i32,     // +244
    pub power: i32,   // +248 remaining damage
    pub delay: i32,   // +252 frames before it appears
}

/// The player ship. Only the fields used by the ported routines are present.
#[derive(Debug, Clone, PartialEq)]
pub struct Ship {
    pub active: bool,   // +0
    pub state: u8,      // +1
    pub number: u8,     // +8  player 1 or 2 (also the owner id stored in shots)
    pub input_set: u8,  // +9
    pub solo: u8,       // +10 1 = one-player game
    pub x: f32,         // +20
    pub y: f32,         // +24
    pub vx: f32,        // +28
    pub vy: f32,        // +32
    pub w: i32,         // +36
    pub h: i32,         // +40
    pub hit_w: i32,     // +44
    pub hit_h: i32,     // +48
    pub vmax: f32,      // +52 maximum speed
    pub frame: i32,     // +60 bank-angle animation frame 1..5
    pub input: ShipInput, // +96..+101
    pub trail_x: i32,   // +312
    pub trail_y: i32,   // +316
    // weapons
    pub level: i32,       // +296 weapon level 1..16 = shots per burst
    pub burst: i32,       // +300 shots left in the current burst
    pub last_shot: u32,   // +336 tick of the last shot
    pub weapon_kind: i32, // +324 0 / 1 / 2
    pub spread: i32,      // +328 0..5, number of parallel shots (1,2,3,4,6,6)
    pub pattern: i32,     // +344 written by the spawn routines (1 or 3)
    pub missiles: i32,    // +332 side-missile level 0..3
    pub missile_tick: u32, // +348
    pub rear: bool,       // +341 rear shots enabled
    pub drones: i32,      // +304
    // charge / beam
    pub latch: bool,      // +108 button-1 "pressed" latch
    pub charge: i32,      // +208 frames button 1 has been held (cap 64)
    pub held: i32,        // +280
    pub heat: i32,        // +284
    pub heat_flash: bool, // +288
    pub heat_stage: i32,  // +292
    pub charge_fx: bool,  // +256
    pub fx_tick_a: u32,   // +260
    pub fx_a: i32,        // +264
    pub fx_b: bool,       // +268
    pub fx_tick_b: u32,   // +272
    pub fx_c: i32,        // +276
    pub charge_sound: u32, // +356
    pub stream_ready: bool, // +360
    pub stream: u32,      // +364
    pub beam: Beam,
    // life cycle, shield and respawn
    pub state_time: u32,  // +4   tick of the last state change
    pub lives: i32,       // +12
    pub sprite_id: i32,   // +56  14 (player 1) / 15 (player 2); the destruction effect differs for 15
    pub invuln: bool,     // +64  blinking invulnerability after a respawn
    pub blink: bool,      // +65
    pub blink_tick: u32,  // +68
    pub flag_72: bool,    // +72  HUD refresh flag (cleared by the draw routine)
    pub tick_76: u32,     // +76
    pub special_max: i32, // +80  charges of the button-2 special (1..10)
    pub special_flag: bool, // +84
    pub special_tick: u32,  // +88
    pub special_left: i32,  // +92
    pub shield: i32,      // +112 hits the ship can take (6..12); at <= 0 the next hit destroys it
    pub shield_fx: bool,  // +116 hit flash running
    pub shield_dir: bool, // +117
    pub shield_tick: u32, // +120
    pub shield_frame: i32, // +124
    pub warn: bool,       // +128 low-shield warning running
    pub warn_tick: u32,   // +132
    pub warn_blink: bool, // +136
    pub warn_n: i32,      // +140
    pub hit_flash: bool,  // +144
    pub hit_flash_tick: u32, // +148
    pub misc_104: i32,    // +104 (set to 1 on spawn)
    pub hud_level: u8,    // +172
    pub drone_a: bool,    // +308
    pub drone_b: bool,    // +309
    pub coins: i32,       // +320 collected coins; dropped as pickups when the ship dies
    pub keep: bool,       // +340 "keep weapons" insurance: the ship still dies but keeps its weapons (sound 22 when collected)
    pub score: i32,       // +152
    pub next_life: i32,   // +156 score at which the next extra life is awarded
    pub score_flag: u8, // +160 HUD score refresh (set to 1)
    pub score_flash: u8, // +161 score flashes (extra life or pickup; set to 1)
    pub score_tick: u32,  // +164
    pub lives_awarded: i32, // +168 extra lives earned by score so far
    pub wipes: i32,       // +352 enemy groups this player has wiped out since the last (re)start (0x29edc: every 8th/9th pays a power-up)
}

impl Ship {
    /// A fresh ship as built by 0x23c04 / 0x23de4 / 0x26680 (state is left at 1 = "appearing"; the session loop sets 3).
    pub fn new(number: u8, solo: bool, level: i32) -> Ship {
        let mut s = Ship {
            active: true, state: 1, number, input_set: number, solo: solo as u8,
            x: 0.0, y: 0.0, vx: 0.0, vy: 0.0, w: SHIP_W, h: SHIP_H, hit_w: 24, hit_h: 24,
            vmax: SPEED_NORMAL, frame: 1, input: ShipInput::default(), trail_x: 0, trail_y: 0,
            level: level.clamp(1, 16), burst: 0, last_shot: 0, weapon_kind: 0, spread: 1, pattern: 0,
            missiles: 0, missile_tick: 0, rear: false, drones: 0,
            latch: false, charge: 0, held: 0, heat: 0, heat_flash: false, heat_stage: 0,
            charge_fx: false, fx_tick_a: 0, fx_a: 0, fx_b: false, fx_tick_b: 0, fx_c: 0,
            charge_sound: 0, stream_ready: true, stream: 0, beam: Beam::default(),
            state_time: 0, lives: 3, sprite_id: if number == 1 { 14 } else { 15 }, invuln: false, blink: false, blink_tick: 0,
            flag_72: false, tick_76: 0, special_max: 1, special_flag: false, special_tick: 0, special_left: 0,
            shield: 6, shield_fx: false, shield_dir: false, shield_tick: 0, shield_frame: 1, warn: false, warn_tick: 0,
            warn_blink: false, warn_n: 0, hit_flash: false, hit_flash_tick: 0, misc_104: 1, hud_level: 1,
            drone_a: false, drone_b: false, coins: 0, keep: false,
            score: 0, next_life: 10000, score_flag: 0, score_flash: 0, score_tick: 0, lives_awarded: 0, wipes: 0,
        };
        s.reset_position();
        s.trail_x = (s.x as i32) + 14;
        s.trail_y = (s.y as i32) - 140;
        s
    }

    /// 0x26188: score reset at the start of a game (score 0, first extra life at 10000, HUD flags cleared, timestamp = now).
    pub fn reset_score(&mut self, now: u32) {
        self.score = 0;
        self.next_life = 10000;
        self.score_flash = 0;
        self.score_tick = now;
        self.score_flag = 0;
        self.lives_awarded = 0;
    }

    /// 0x23fb4: start position. One player: centred; two players: P1 at x=128, P2 at x=288-w. y = 320, at rest.
    pub fn reset_position(&mut self) {
        if self.solo == 1 {
            self.x = ((SCREEN_W - self.w) / 2) as f32;
        } else if self.number == 1 {
            self.x = 128.0;
        } else {
            self.x = (288 - self.w) as f32;
        }
        self.y = 320.0;
        self.vx = 0.0;
        self.vy = 0.0;
    }

    /// 0x24128: speed-up power-up (maximum speed 9.0). 0x240f0 restores 7.2.
    pub fn set_speed_boost(&mut self, on: bool) { if self.active { self.vmax = if on { SPEED_BOOST } else { SPEED_NORMAL }; } }

    /// 0x25264: one frame of acceleration, deceleration, banking frame, screen clamp and the "trail" follower.
    pub fn step_movement(&mut self) {
        if self.state != STATE_PLAYING { return; }
        let vmax = self.vmax;
        let i = self.input;
        if i.up {
            self.vy -= ACCEL;
            if self.vy < -vmax { self.vy = -vmax; }
        }
        if i.down {
            self.vy += ACCEL;
            if self.vy > vmax { self.vy = vmax; }
        }
        if i.left {
            self.vx -= ACCEL;
            if self.vx < -vmax { self.vx = -vmax; }
        }
        if i.right {
            self.vx += ACCEL;
            if self.vx > vmax { self.vx = vmax; }
        }
        if !i.left && !i.right {
            if self.vx > 0.0 {
                self.vx -= ACCEL;
                if self.vx < 0.0 { self.vx = 0.0; }
            }
            if self.vx < 0.0 {
                self.vx += ACCEL;
                if self.vx > 0.0 { self.vx = 0.0; }
            }
        }
        if !i.up && !i.down {
            if self.vy > 0.0 {
                self.vy -= ACCEL;
                if self.vy < 0.0 { self.vy = 0.0; }
            }
            if self.vy < 0.0 {
                self.vy += ACCEL;
                if self.vy > 0.0 { self.vy = 0.0; }
            }
        }
        if self.vx == 0.0 { self.frame = 1; }
        if self.vx < 0.0 { self.frame = if self.vx == -vmax { 3 } else { 2 }; }
        if self.vx > 0.0 { self.frame = if self.vx == vmax { 5 } else { 4 }; }

        self.x += self.vx;
        self.y += self.vy;

        if self.x < 0.0 {
            self.x = 0.0;
            self.vx = 0.0;
        } else if self.x + (self.w as f32) > SCREEN_W as f32 {
            self.x = (SCREEN_W - self.w) as f32;
            self.vx = 0.0;
        }
        if self.y < 0.0 {
            self.y = 0.0;
            self.vy = 0.0;
        } else if self.y + (self.h as f32) > SCREEN_H as f32 {
            self.y = (SCREEN_H - self.h) as f32;
            self.vy = 0.0;
        }

        self.trail_x = (self.x as i32) + 14;
        let target = (self.y as i32) - 140;
        if self.y >= (SCREEN_H - self.h) as f32 && i.down {
            self.trail_y += 4;
            if self.trail_y >= (self.y as i32) - 40 { self.trail_y = (self.y as i32) - 40; }
            return;
        }
        if self.trail_y > target {
            self.trail_y -= self.vmax as i32;
            if self.trail_y < target { self.trail_y = target; }
        } else {
            self.trail_y += self.vmax as i32;
            if self.trail_y > target { self.trail_y = target; }
        }
        if self.trail_y < 0 { self.trail_y = 0; }
    }

    /// Hit box against enemy bodies, as built in 0x2dc20: the sprite shrunk by 6 px on every side (right/bottom = x+w-6, y+h-6).
    /// Returns (left, top, right, bottom); an enemy hit box overlaps when `left < enemy.right && enemy.left < right && ...` (strict).
    pub fn enemy_hitbox(&self) -> (i32, i32, i32, i32) {
        let xi = self.x as i32;
        let yi = self.y as i32;
        (xi + 6, yi + 6, xi + (self.w - 6), yi + (self.h - 6))
    }

    /// Hit box against power-ups, as built in 0x2b884: shrunk by 2 px on the left/top and 2 px on the right/bottom.
    pub fn pickup_hitbox(&self) -> (i32, i32, i32, i32) {
        let xi = self.x as i32;
        let yi = self.y as i32;
        (xi + 2, yi + 2, xi + 2 + (self.w - 4), yi + 2 + (self.h - 4))
    }

    /// 0x26c94: add to the weapon level, clamped to 1..=16.
    pub fn add_level(&mut self, delta: i32) { self.level = (self.level + delta).clamp(1, 16); }

    /// 0x27180: end/clear the beam and the charge state.
    pub fn clear_charge(&mut self) {
        self.beam.active = false;
        self.charge = 0;
        self.held = 0;
        self.heat_flash = false;
        self.heat = 0;
        self.heat_stage = 0;
        self.charge_fx = false;
        self.beam.power = 0;
        self.fx_b = false;
        self.charge_sound = 0;
        self.stream_ready = true;
        self.stream = 0;
    }

    /// 0x27080: the over-charge timer that runs every frame the charge keeps growing; ends in an explosion.
    fn overheat_tick<W: World>(&mut self, world: &mut W) {
        self.held += 1;
        self.heat_flash = false;
        if self.held < 120 { return; }
        self.heat += 1;
        let thr = (30 - self.heat_stage * 6).max(6);
        if self.heat < thr - 3 { return; }
        self.heat_flash = true;
        if self.heat == thr - 3 { world.sound(26, 4, 0); }
        if self.heat <= thr { return; }
        self.heat = 0;
        self.heat_stage += 1;
        if self.heat_stage <= 7 { return; }
        if self.state == STATE_PLAYING { world.ship_overheated(self); }
    }

    /// 0x271bc: release the charged beam.
    fn fire_beam<W: World>(&mut self, world: &mut W) {
        if self.state != STATE_PLAYING || self.beam.active { return; }
        self.charge_fx = false;
        self.fx_b = true;
        let h = self.charge_sound;
        if world.sound_playing(h) { world.sound_stop(h); }
        // 0x27730
        let sh = self.stream;
        if world.sound_playing(sh) {
            world.sound_stop(sh);
            self.stream_ready = true;
        }
        self.beam.active = true;
        self.beam.delay = 5;
        let (w, hgt, anim, pat, power, base, sounds);
        if self.charge < 20 { w = 15; hgt = 32; anim = 39; pat = 1; power = 1; base = 1; sounds = 1; }
        else if self.charge < 35 { w = 17; hgt = 48; anim = 40; pat = 2; power = 3; base = 2; sounds = 2; }
        else if self.charge < 50 { w = 19; hgt = 72; anim = 41; pat = 3; power = 6; base = 3; sounds = 3; }
        else { w = 21; hgt = 96; anim = 42; pat = 3; power = 16; base = 4; sounds = 4; }
        self.beam.w = w;
        self.beam.h = hgt;
        self.beam.anim = anim;
        self.beam.pat = pat;
        self.beam.power = power;
        for _ in 0..sounds { world.sound(24, 4, 0); }
        self.beam.power = self.beam.power * self.spread + base * (self.level - 1);
        self.beam.vx = 0;
        self.beam.vy = -20;
        self.charge = 0;
        self.held = 0;
        self.heat = 0;
        self.heat_flash = false;
        self.heat_stage = 0;
    }

    /// 0x27434: move the beam, hit enemies, end it when it leaves the screen or its power is used up.
    pub fn update_beam<W: World>(&mut self, world: &mut W) {
        if self.state != STATE_PLAYING || !self.beam.active { return; }
        self.beam.delay -= 1;
        if self.beam.delay > 0 { return; }
        if self.beam.delay == 0 {
            self.beam.x = (self.x as i32) + (self.w - self.beam.w) / 2;
            self.beam.y = (self.y as i32) - self.beam.h;
        }
        self.beam.x += self.beam.vx;
        self.beam.y += self.beam.vy;
        let b = self.beam;
        if b.x + b.w < 0 || b.x > SCREEN_W || b.y + b.h < 0 || b.y > SCREEN_H {
            self.clear_charge();
            return;
        }
        let dmg = world.hit_enemies(b.x, b.y, b.x + b.w, b.y + b.h, b.power, self.number);
        if dmg > 0 {
            self.beam.power -= dmg;
            if self.beam.power < 1 {
                world.effect(b.x + b.w / 2, b.y + b.h / 2, 1, 0, -1, 0, 0, 0.0, 0.0);
                self.clear_charge();
            }
        }
    }

    /// 0x26cf8: button-1 handling. A press fires a burst of `level` volleys (>= 2 ticks apart, a new burst only > 4 ticks after
    /// the last shot); keeping the button down charges the beam (released after > 20 frames: beam). Returns nothing; shots go to `pool`.
    pub fn fire_step<W: World>(&mut self, now: u32, pool: &mut ShotPool, world: &mut W) {
        if self.state != STATE_PLAYING { return; }
        let mut started = false;
        let mut fired = false;
        let mut sound: i32 = -1;
        let mut normal = true;
        if self.burst < 1 {
            if !self.input.fire {
                if self.charge > 20 {
                    if self.latch {
                        self.fire_beam(world);
                        self.latch = false;
                        self.charge_fx = false;
                        return;
                    }
                    self.latch = false;
                    self.charge_fx = false;
                } else {
                    self.charge = 0;
                    self.held = 0;
                    self.heat = 0;
                    self.heat_stage = 0;
                    self.heat_flash = false;
                    self.charge_fx = false;
                    self.latch = false;
                    return;
                }
            } else if self.latch {
                // holding: charge
                self.latch = true;
                self.charge += 1;
                self.overheat_tick(world);
                if self.charge > 54 && self.stream_ready {
                    self.stream = world.overcharge_stream_start(self.number);
                    self.stream_ready = false;
                }
                if self.charge > 64 { self.charge = 64; }
                if self.charge == 20 {
                    self.charge_sound = world.charge_loop_start();
                    self.charge_fx = true;
                    self.fx_tick_a = now;
                    self.fx_a = 1;
                    self.fx_b = false;
                    self.fx_tick_b = now;
                    self.fx_c = 1;
                }
                normal = false;
            }
        }
        if !normal { return; }
        self.latch = true;
        if self.burst <= 0 {
            if now <= self.last_shot.wrapping_add(4) { return; }
            self.burst = self.level;
            started = true;
        }
        if !started && now < self.last_shot.wrapping_add(2) { return; }
        if self.burst > 0 {
            match self.weapon_kind {
                0 => { fired = pool.spawn_front(self); sound = 10; }
                1 => { fired = pool.spawn_beam(self, now); sound = 33; }
                2 => { fired = pool.spawn_front(self); sound = 7; }
                _ => {}
            }
        }
        if fired {
            self.last_shot = now;
            self.burst -= 1;
            if sound != -1 { world.sound(sound, 3, 0); }
        }
        if self.missiles != 0 && now > self.missile_tick.wrapping_add(40) && world.launch_missiles(self) {
            world.sound(41, 2, 5);
            self.missile_tick = now;
        }
        if self.rear { pool.spawn_rear(self); }
        if self.drones > 0 { world.drones_fire(self, pool); }
    }
}

impl From<crate::input::PlayerInput> for ShipInput {
    fn from(i: crate::input::PlayerInput) -> Self {
        ShipInput { up: i.up, down: i.down, left: i.left, right: i.right, fire: i.button1, alt: i.button2 }
    }
}

/// Values the session loop (0xc6f0) keeps in locals/globals around the per-player state machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct SessionCtx {
    /// `r27 == 1`: one-player game.
    pub solo: bool,
    /// The *other* player is still in the game (`r21`/`r22`; always false in a one-player game).
    pub other_in_game: bool,
    /// `r23` (attract/demo flag): suppresses the respawn sound.
    pub demo: bool,
    /// data byte 0x13e3 (display mode); when it is not 1 the HUD routine 0x28510(0) runs after a respawn.
    pub mode: u8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionOut {
    /// This player's "still in the game" flag after the step (cleared when the last life is gone).
    pub in_game: bool,
    /// The session ends (data byte 0x13f4 cleared by the original).
    pub game_over: bool,
}

impl Ship {
    /// 0x26c8c / 0x26c94 live in [`add_level`]. 0x26af4: add to the button-2 charge count, clamped to 1..=10.
    pub fn add_special(&mut self, delta: i32) { self.special_max = (self.special_max + delta).clamp(1, 10); }

    /// 0x24228: reset the shield (to 6, or keep a value in 6..=12 when `keep` is set) and the hit/warning animations.
    pub fn shield_reset(&mut self, now: u32, keep: bool) {
        if !self.active { return; }
        if self.shield < 6 || self.shield > 12 { self.shield = 6; }
        if !keep { self.shield = 6; }
        self.shield_fx = false;
        self.shield_dir = false;
        self.shield_frame = 1;
        self.shield_tick = now;
        self.warn = false;
        self.warn_tick = now;
        self.warn_blink = false;
        self.warn_n = 0;
    }

    /// 0x243a0: the ship takes `amount` damage (enemy bodies deal 5, see 0x2dc20). With no shield left the ship is destroyed instead.
    pub fn damage<W: World>(&mut self, amount: i32, now: u32, world: &mut W) {
        if self.state != STATE_PLAYING { return; }
        if self.shield <= 0 {
            self.shield_fx = false;
            self.destroy(false, now, world);
            return;
        }
        self.shield -= amount;
        self.shield_fx = true;
        self.shield_dir = false;
        self.shield_frame = 1;
        self.shield_tick = now;
        world.hud(0x1f69c, self.input_set as i32, self.shield);
        let id = if self.input_set == 1 { 66 } else { 67 };
        world.sound(id, 5, 0);
        world.sound(id, 5, 0);
        world.sound(id, 5, 0);
        if self.shield == 0 {
            self.warn = true;
            self.warn_tick = now.wrapping_add(10);
            self.warn_blink = false;
            self.warn_n = 0;
        }
    }

    /// 0x26680: put the weapons back to their starting values. `keep` = true leaves the weapon type/spread/level/missiles/rear/drones/special as they are
    /// (used between levels); the common tail always clears the charge, the burst and the special-weapon state.
    pub fn reset_weapons<W: World>(&mut self, keep: bool, now: u32, world: &mut W) {
        let lvl0 = world.start_level();
        if !keep {
            self.weapon_kind = 0;
            self.spread = 1;
            self.level = world.start_level();
            self.missiles = 0;
            self.missile_tick = now;
            self.rear = false;
            self.keep = false;
            world.hud(0x1fe28, self.input_set as i32, 0);
            self.drones = 0;
            self.drone_a = false;
            self.drone_b = false;
            self.special_max = world.start_level();
        }
        if self.keep { world.hud(0x1fe28, self.input_set as i32, 1); }
        let target = lvl0.min(16);
        while self.level < target { self.add_level(1); }
        if self.special_max < target { self.special_max = target; }
        if self.special_max > 10 { self.special_max = 10; }
        world.hud(0x1ff94, self.input_set as i32, self.hud_level as i32);
        self.clear_charge();
        self.last_shot = now;
        self.pattern = 0;
        self.missile_tick = now;
        self.latch = false;
        self.special_tick = 0;
        self.special_flag = false;
        self.special_left = 0;
        self.burst = 0;
    }

    /// 0x24544: destroy the ship. `quiet` (r4 != 0) skips the first big explosion and the effect delays. Drops the weapons as power-ups (unless the
    /// `keep` insurance is set, which only drops the drones), then puts the ship into state 2 at its start position.
    pub fn destroy<W: World>(&mut self, quiet: bool, now: u32, world: &mut W) {
        for _ in 0..3 { world.sound(31, 5, 0); }
        for _ in 0..3 { world.sound(4, 4, 20); }
        for _ in 0..5 { world.sound(11, 4, 20); }
        let d: i32 = if quiet { 0 } else {
            world.effect(self.x as i32, self.y as i32, 3, 0, -1, 0, 0, 0.0, 0.0);
            16
        };
        let cx = (self.x as i32) + self.hit_w;
        let cy = (self.y as i32) + self.hit_h;
        world.effect(cx, cy, 14, d, 11, 0, 0, 0.0, 0.0);
        world.effect_aux(0x29714, cx, cy, 9, d, -1);
        for _ in 0..8 { world.effect(cx, cy, 6, d, -1, 0, 1, 0.0, 0.0); }
        for _ in 0..4 { world.effect(cx, cy, 11, d, -1, 0, 1, 0.0, 0.0); }
        let k = if self.sprite_id == 15 { 8 } else { 7 };
        for _ in 0..4 { world.effect(cx, cy, k, d + 4, -1, 0, 1, 0.0, 0.0); }
        for _ in 0..8 { world.effect(cx, cy, 6, d + 8, -1, 0, 1, 0.0, 0.0); }
        world.effect(cx - 20, cy - 10, 14, d + 8, 11, 1, 0, 0.0, 0.0);
        world.effect(cx, cy - 16, 14, d + 14, 62, 1, 0, 0.0, 0.0);
        world.effect(cx + 18, cy + 4, 2, d + 22, 11, 1, 0, 0.0, 0.0);
        world.effect(cx - 8, cy - 32, 2, d + 30, 11, 1, 0, 0.0, 0.0);
        let (fx, fy) = (cx as f32, cy as f32);
        if !self.keep {
            let lvl0 = world.start_level();
            let (spread, level, special, missiles, rear, drones) = (self.spread, self.level, self.special_max, self.missiles, self.rear, self.drones);
            let speedy = !self.active || self.vmax == SPEED_BOOST;
            self.reset_weapons(false, now, world);
            for _ in 1..spread { world.drop_pickup(5, fx, fy); }
            for _ in lvl0..level { world.drop_pickup(6, fx, fy); }
            for _ in lvl0..special { world.drop_pickup(18, fx, fy); }
            if missiles != 0 { for _ in 0..missiles { world.drop_pickup(7, fx, fy); } }
            if rear { world.drop_pickup(10, fx, fy); }
            for _ in 0..drones { world.drop_pickup(12, fx, fy); }
            if speedy {
                world.drop_pickup(19, fx, fy);
                self.vmax = SPEED_NORMAL;
            }
            let mut rem = self.coins;
            let mut tens = 0;
            while rem >= 10 { rem -= 10; tens += 1; }
            for _ in 0..rem { world.drop_pickup(2, fx, fy); }
            for _ in 0..tens { world.drop_pickup(21, fx, fy); }
            self.coins = 0;
        } else {
            self.keep = false;
            let mut i = 0;
            while i < self.drones { world.drop_pickup(12, fx, fy); i += 1; }
        }
        // 0x26a3c
        world.drones_reset(self.number);
        self.drones = 0;
        self.drone_a = false;
        self.drone_b = false;
        // 0x268f4(ship, 0) and 0x265b4
        self.keep = false;
        world.hud(0x1fe28, self.input_set as i32, 0);
        self.hud_level = 1;
        world.hud(0x1ff94, self.input_set as i32, 1);
        self.state = 2;
        self.state_time = now;
        self.invuln = true;
        self.blink = true;
        self.blink_tick = now;
        self.hit_flash = false;
        self.reset_position();
    }

    /// 0x256b4: the three flashes shown where a ship (re)appears.
    pub fn appear_effects<W: World>(&self, world: &mut W) {
        let cx = (self.x + self.hit_w as f32) as i32;
        let cy = (self.y + self.hit_h as f32) as i32;
        world.effect_aux(0x298ec, cx, cy, 9, 16, 32);
        world.effect_aux(0x298ec, cx, cy, 9, 22, -1);
        world.effect_aux(0x298ec, cx, cy, 9, 24, -1);
    }

    /// 0x23de4: (re)start a ship at the beginning of a level or game. `keep` keeps the weapons of the previous level.
    pub fn init<W: World>(&mut self, keep: bool, now: u32, world: &mut W) {
        if !self.active { return; }
        self.w = SHIP_W; self.h = SHIP_H; self.hit_w = 24; self.hit_h = 24;
        self.reset_position();
        self.trail_x = (self.x as i32) + 14;
        self.trail_y = (self.y as i32) - 140;
        self.lives += 1;
        self.state = 1;
        self.state_time = now;
        self.invuln = false;
        self.blink = false;
        self.blink_tick = now;
        self.flag_72 = false;
        self.tick_76 = now;
        self.hit_flash = false;
        self.hit_flash_tick = now;
        self.frame = 1;
        self.misc_104 = 1;
        self.wipes = 0;
        self.coins = 0;
        self.sprite_id = if self.input_set == 1 { 14 } else { 15 };
        if !keep { self.vmax = SPEED_NORMAL; }
        let lvl = self.level;
        let sp = self.special_max;
        self.shield_reset(now, keep);
        self.reset_weapons(keep, now, world);
        if world.level_bonus() > 0 {
            if lvl >= world.start_level() { self.add_level(1); }
            if sp >= world.start_level() { self.add_special(1); }
        }
        self.appear_effects(world);
    }

    /// The per-player state machine of the session loop (0xc9d0..0xcc9c): 1 = waiting to (re)enter (60 ticks, 120 for the last life), then a life is
    /// used and the ship plays (state 3); 3 = playing, the invulnerability ends 90 ticks after entering; 2 = destroyed, back to state 1 after 60 ticks
    /// (40 for the last life), or out of the game when it was the last life.
    pub fn session_step<W: World>(&mut self, now: u32, ctx: &SessionCtx, in_game: bool, world: &mut W) -> SessionOut {
        let mut out = SessionOut { in_game, game_over: false };
        match self.state {
            1 => {
                let delay = if self.lives == 1 { 120 } else { 60 };
                if now > self.state_time.wrapping_add(delay) {
                    if !ctx.demo && !world.demo_check() { world.hud(0x4560c, 1, 0); }
                    self.lives -= 1;
                    if self.lives > 0 {
                        self.state = 3;
                        self.state_time = now;
                        if ctx.mode != 1 { world.hud(0x28510, 0, 0); }
                        world.hud_lives(self.input_set, self.lives, 0);
                    } else {
                        out.in_game = false;
                        if ctx.solo || !ctx.other_in_game { out.game_over = true; }
                    }
                } else {
                    world.hud(0x1f5c4, self.input_set as i32, 0);
                }
            }
            2 => {
                let delay = if self.lives == 1 { 40 } else { 60 };
                if now > self.state_time.wrapping_add(delay) {
                    self.state = 1;
                    self.state_time = now;
                    if self.lives == 1 {
                        self.active = false;
                        world.hud(0x1fc50, self.input_set as i32, 0);
                        if ctx.solo || !ctx.other_in_game {
                            world.hud(0x28510, 3, 0);
                            world.sound(28, 5, 0);
                        }
                    } else {
                        self.appear_effects(world);
                        self.shield_reset(now, false);
                        world.hud(0x1f51c, self.input_set as i32, self.shield);
                    }
                }
            }
            3 => {
                if self.invuln && now > self.state_time.wrapping_add(90) {
                    self.invuln = false;
                    self.blink = false;
                }
            }
            _ => {}
        }
        out
    }
}
