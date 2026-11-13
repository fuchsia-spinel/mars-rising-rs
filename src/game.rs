//! A minimal playable game loop (std-only) that drives the verified ship / shot / beam / power-up / draw code in the original's frame order.
//!
//! **What is original and what is not.** Everything that touches the player's ship is the verified port (`ship.rs`, `pickup.rs`,
//! `ship_draw.rs`, `blit.rs`). The *enemies are not ported yet*; this module stands in for them with a few **debug targets** (`Target`) that
//! use the original's damage convention (see `Target::damage`, read from 0x2ec14) and the original's ship-contact rule (0x2dc20), so that
//! shooting, the charged beam, pickups, shields, destruction and respawn can all be played and tested. The targets' sizes, hit points,
//! movement and drop rules are invented for testing. The side-panel HUD is the verified port (`hud.rs`).
//!
//! Frame order (from the main loop 0xc6f0, see RE_NOTES.md): tick counter +1 (0xed04); per-player state machine (0xc9d0, `Ship::session_step`);
//! input; button-1 control 0x26cf8 (`fire_step`); enemy update + ship contact (0x2dc20); shot update 0x22170; beam update 0x27434; pickup
//! update 0x2b884; ship movement 0x25264; scroll step 0xc3e0; then the draw routines 0x2242c (shots), 0x275c8 (beams), 0x2c480 (pickups) and
//! 0x257b4 (ships), which produce the list of blit requests in `Env::draws`. The original paces the loop at 2 `TickCount` ticks (60.15 Hz)
//! per frame, i.e. **30 frames per second** (`FRAME_MICROS`), as its read-me says.
use crate::bomb::{self, BombWorld, Bombs};
use crate::drone::{DroneWorld, Drones, Missiles};
use crate::debris::{Debris, DebrisWorld};
use crate::levelend::{Tally, TallyWorld};
use crate::effects::{EffectWorld, Effects};
use crate::ground::{Ground, GroundWorld};
use crate::audio::{Mixer, CB_ENEMY, CB_OVERCHARGE, DEFAULT_OUT_RATE, SAMPLES_PER_FRAME};
use crate::draw::{Draw, DrawOpts, Prim};
use crate::enemy::{Enemies, EnemyWorld, ShipView};
use crate::enemy_draw::EnemyDrawWorld;
use crate::enemy_shot::{ShotDrawWorld, ShotPool as EnemyShots, ShotWorld};
use crate::hud::{Hud, HudCall, HudWorld};
use crate::input::PlayerInput;
use crate::pickup::PickupPool;
use crate::scroll::Scroller;
use crate::ship::*;

/// One logic frame = 2 `TickCount` ticks of 1/60.15 s (the frame limiter at 0xd3b4 waits for `last + 2`).
pub const FRAME_MICROS: u64 = 33_250;

/// Enemy slots of the original (0x2ead4 walks 40 slots); the debug targets use as many.
pub const MAX_TARGETS: usize = 40;

/// The Toolbox `Random()` the original calls (only through 0xa0d4): the QuickDraw global `randSeed` (`qd + 76`) is advanced by the Lehmer step
/// `seed = seed * 16807 mod (2^31 - 1)` and the result is its low 16 bits (the value 0x8000 is returned as 0). The original seeds it with `TickCount` at the
/// start of a game and with the stored seed of a demo recording (`0xc6f0`), which is why a demo replays exactly.
#[derive(Debug, Clone)]
pub struct Rng(u32);

impl Rng {
    /// `seed` is `randSeed`; the generator has the fixed point 0 and only uses 31 bits, so callers that seed from a clock get them folded in.
    pub fn new(seed: u32) -> Rng {
        let s = seed % 0x7fff_ffff;
        Rng(if s == 0 { 0x2545_F491 % 0x7fff_ffff } else { s })
    }
    /// The seed exactly as the original would hold it (a demo recording's stored value).
    pub fn exact(seed: u32) -> Rng { Rng(seed) }
    pub fn seed(&self) -> u32 { self.0 }
    /// `Random()` as an unsigned 16-bit value (the signed result's bit pattern).
    pub fn random16(&mut self) -> u32 {
        self.0 = ((self.0 as u64 * 16807) % 0x7fff_ffff) as u32;
        let r = self.0 & 0xffff;
        if r == 0x8000 { 0 } else { r }
    }
    pub fn next_u32(&mut self) -> u32 { self.random16() << 16 | self.random16() }
    /// `((Random() as u16) * (hi - lo + 1)) / 65536 + lo` like 0xa0d4.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        let r = self.random16() as i32;
        (r * (hi - lo + 1)) / 65536 + lo
    }
}

/// A sound request as the ship / pickup code made it: `arg` is the delay in frames (0x45bac) or, for `pan == true` (0x45d54), the
/// playback-rate multiplier in 16.16 fixed point. The same requests are played by `Env::audio`; this log is for tests and debugging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundEvent {
    pub id: i32,
    pub priority: i32,
    pub arg: i32,
    pub pan: bool,
}

/// An effect request (`0x22670`): position and kind only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectEvent {
    pub x: i32,
    pub y: i32,
    pub kind: i32,
}

/// **Debug target** standing in for an enemy. Hit points follow the original's convention: `taken` damage accumulates and the target dies when
/// `taken > max_hp`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    pub active: bool,
    pub x: f32,
    pub y: f32,
    pub w: i32,
    pub h: i32,
    pub vx: f32,
    pub vy: f32,
    pub max_hp: i32,
    pub taken: i32,
    pub points: i32,
    pub born: u32,
    pub hit_tick: u32,
}

impl Default for Target {
    fn default() -> Self {
        Target { active: false, x: 0.0, y: 0.0, w: 0, h: 0, vx: 0.0, vy: 0.0, max_hp: 0, taken: 0, points: 0, born: 0, hit_tick: 0 }
    }
}

impl Target {
    pub fn as_box(&self) -> EnemyBox {
        EnemyBox { active: self.active, state: 0, left: self.x, top: self.y, right: self.x + self.w as f32, bottom: self.y + self.h as f32 }
    }
    /// 0x2ec14: add `amount` to the damage taken; returns (damage actually dealt, killed). A killing blow deals `max - taken_before + 1`.
    pub fn damage(&mut self, amount: i32) -> (i32, bool) {
        let old = self.taken;
        self.taken += amount;
        if self.taken > self.max_hp { (self.max_hp - old + 1, true) } else { (amount, false) }
    }
    /// Hit points left (the amount of damage that still kills is `remaining() + 1`... the shot that exceeds it).
    pub fn remaining(&self) -> i32 { self.max_hp - self.taken }
}

/// A kill: who got it, how many points, and where.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Kill {
    owner: u8,
    points: i32,
    x: f32,
    y: f32,
}

/// Everything the ship code asks of "the rest of the game" (`ship::World`), plus the debug targets.
#[derive(Debug, Clone)]
pub struct Env {
    pub now: u32,
    pub rng: Rng,
    /// `D.13e3 == 1` (0xf5b8): an attract-mode demo is running (`demo.rs`). The sound routines return at once (0x45bac and the other entry points test it).
    pub demo: bool,
    /// `D.172c`: the state of the message overlay (`overlay.rs`): 0 none, 1 "Sector N / Get Ready!", 2 paused, 3 GAME OVER, 4 "Sector N Cleared".
    pub msg: u8,
    pub targets: Vec<Target>,
    /// Blit requests of the current frame, in call order.
    pub draws: Vec<Draw>,
    /// Sounds requested this frame (cleared at the start of every `Game::step`).
    pub sounds: Vec<SoundEvent>,
    /// Effect requests of the debug targets this frame (spawned into `fx` by `Game::step`); everything else spawns straight into `fx`.
    pub effects: Vec<EffectEvent>,
    /// The effect pool (`effects.rs`: explosions, sparks, smoke); `Option` so that it can be moved out while it works with `&mut Env` (see `FxCtx`).
    pub fx: Option<Effects>,
    /// The auxiliary effect pool (`debris.rs`: fragments of 3 x 3 pixels); `Option` for the same reason as `fx`.
    pub debris: Option<Debris>,
    pub scroll_speed: i32,
    drops: Vec<(i32, f32, f32)>,
    kills: Vec<Kill>,
    /// The sound engine (`audio.rs`). Without samples loaded every request is a no-op, so headless runs stay deterministic.
    pub audio: Mixer,
    pub total_kills: u32,
    /// The real enemy pool and groups (`enemy.rs`). `Option` so that it can be moved out while it works with `&mut Env` (see `EnemyCtx`).
    pub enemies: Option<Enemies>,
    /// The enemy-shot pool (`enemy_shot.rs`); `Option` for the same reason as `enemies`.
    pub eshots: Option<EnemyShots>,
    /// Number of players (D.13e2).
    pub players: u8,
    /// Start weapon level / difficulty global D.13ec.
    pub start_level_value: i32,
    /// `D.13f0` (read by 0xd7a8): levels completed so far.
    pub levels_done: u32,
    /// Scores earned by enemy kills this frame (owner, points); applied by `Game::flush_enemy_events`.
    score_events: Vec<(u8, i32)>,
    /// Wiped-out groups this frame (owner, x, y): the reward is decided by `pickup::bonus_kind`.
    bonus_events: Vec<(u8, f32, f32)>,
    /// The ground objects (`ground.rs`: turrets, missile towers, tanks ...); `Option` so that it can be moved out while it works with `&mut Env`.
    pub ground: Option<Ground>,
    /// The bombs of the button-2 weapon (`bomb.rs`); `Option` for the same reason.
    pub bombs: Option<Bombs>,
    /// The drones of the two ships (`drone.rs`) and the side missiles; `Option` for the same reason.
    pub drones: Option<Drones>,
    pub missiles: Option<Missiles>,
    /// The display options, mirrored from `Game::opts` at the start of every step.
    pub opts: DrawOpts,
    /// The "extra detail" option (`D.13c8`, getter 0xe364): effect shadows and trails.
    pub detail: bool,
    /// Power-ups the ground objects want (kind, variant, param, announce, x, y); created by `Game::flush_ground_events`.
    pickup_events: Vec<(i32, i32, i32, i32, f32, f32)>,
    /// The last ground target was destroyed here (0xea4c).
    clear_events: Vec<(f32, f32)>,
    /// Draw requests of the ground objects that are drawn on the screen itself (not part of the background), in call order.
    ground_live: Vec<Draw>,
    /// The side-panel HUD (`hud.rs`): its state, and (when `hud_record` is set, the default) the drawing operations it asked for since the host last took them
    /// (`Game::take_hud_calls`).
    pub hud: Hud,
    pub hud_calls: Vec<HudCall>,
    pub hud_record: bool,
    /// Counts the HUD initialisations (0x1e950): the original draws the panel picture again before each one, so the host starts the panel over when this changes.
    /// The high 16 bits are a serial number that every new `Env` (every new game) takes from `HUD_GAME_SERIAL`, so the epochs of two games never coincide: the host's
    /// panel (shown on screen, with the clean backgrounds the HUD restores from) is always started over for a new game instead of being drawn on top of the old game's.
    pub hud_epoch: u32,
}

/// The HUD sees the game through `Env`: the tick counter, the panel origin of the original's screen buffer and the operation log.
impl HudWorld for Env {
    fn tick(&self) -> u32 { self.now }
    fn origin(&self) -> [i16; 4] { HUD_ORIGIN }
    fn call(&mut self, c: HudCall) {
        if !self.hud_record { return; }
        if self.hud_calls.len() >= 1 << 20 { self.hud_calls.clear(); } // a host that never drains the log does not care about it either
        self.hud_calls.push(c);
    }
}

/// Source of the high half of `Env::hud_epoch` (one per `Env`; starts at 1 so that an epoch is never 0, the value a host starts with).
static HUD_GAME_SERIAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// `D.9340` +64, +66, +72, +74: where the side panel sits in the panel buffer (top 64, left 480) and on the screen (top 0, left 416).
pub const HUD_ORIGIN: [i16; 4] = [64, 480, 0, 416];

impl Env {
    fn new(seed: u32) -> Env {
        Env {
            now: 0, rng: Rng::new(seed), demo: false, msg: crate::overlay::GET_READY, targets: vec![Target::default(); MAX_TARGETS], draws: Vec::new(), sounds: Vec::new(), effects: Vec::new(), fx: Some(Effects::new()), debris: Some(Debris::new()),
            scroll_speed: 1, drops: Vec::new(), kills: Vec::new(), audio: Mixer::new(DEFAULT_OUT_RATE), total_kills: 0,
            enemies: Some(Enemies::new()), eshots: Some(EnemyShots::new()), players: 1, start_level_value: 1, levels_done: 0, score_events: Vec::new(), bonus_events: Vec::new(),
            ground: Some(Ground::new()), bombs: Some(Bombs::new()), drones: Some(Drones::new()), missiles: Some(Missiles::new()), opts: DrawOpts::default(), detail: true, pickup_events: Vec::new(), clear_events: Vec::new(), ground_live: Vec::new(),
            hud: Hud::new(), hud_calls: Vec::new(), hud_record: true, hud_epoch: HUD_GAME_SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed) << 16,
        }
    }
    /// 0x1e950, after the panel picture was drawn again.
    fn hud_init(&mut self) {
        self.hud_calls.clear();
        self.hud_epoch = self.hud_epoch.wrapping_add(1);
        let mut h = std::mem::take(&mut self.hud);
        h.init(self);
        self.hud = h;
    }
    /// Run one of the HUD routines of `hud.rs` on this world.
    fn hud_with(&mut self, f: impl FnOnce(&mut Hud, &mut Env)) {
        let mut h = std::mem::take(&mut self.hud);
        f(&mut h, self);
        self.hud = h;
    }
    pub fn active_targets(&self) -> usize { self.targets.iter().filter(|t| t.active).count() }

    /// 0x22670: start an effect (kind, delay, sound id, moves with the ground, loops, velocity). The effect plays its own sound.
    #[allow(clippy::too_many_arguments)]
    pub fn fx_spawn(&mut self, x: i32, y: i32, kind: i32, delay: i32, snd: i32, scrolls: i32, looping: i32, vx: f32, vy: f32) {
        if let Some(mut fx) = self.fx.take() {
            fx.spawn(&mut FxCtx { env: self }, x, y, kind, delay, snd, scrolls, looping, vx, vy);
            self.fx = Some(fx);
        }
    }
    /// 0x23440: a burst of sparks.
    pub fn fx_particles(&mut self, x: i32, y: i32, anim: i32, count: i32, delay: i32, scrolls: i32) {
        if let Some(mut fx) = self.fx.take() {
            fx.particles(&mut FxCtx { env: self }, x, y, anim, count, delay, scrolls);
            self.fx = Some(fx);
        }
    }
    /// 0x22618: initialise the effect pool (builds the table of spark directions from the game's random numbers).
    pub fn fx_init(&mut self) {
        if let Some(mut fx) = self.fx.take() {
            fx.init(&mut FxCtx { env: self });
            self.fx = Some(fx);
        }
    }
    /// 0x29714: a burst of fragments flying outwards (the explosion debris and its sound).
    pub fn debris_spawn(&mut self, x: i32, y: i32, id: i32, delay: i32, snd: i32, scrolls: i32) {
        if let Some(mut d) = self.debris.take() {
            d.spawn(&mut DebrisCtx { env: self }, x, y, id, delay, snd, scrolls);
            self.debris = Some(d);
        }
    }
    /// 0x298ec: fragments flying inwards (the ship's appearance).
    pub fn debris_spawn_in(&mut self, x: i32, y: i32, id: i32, delay: i32, snd: i32, scrolls: i32) {
        if let Some(mut d) = self.debris.take() {
            d.spawn_in(&mut DebrisCtx { env: self }, x, y, id, delay, snd, scrolls);
            self.debris = Some(d);
        }
    }
    /// 0x2942c: initialise the debris pool (random direction table, colour grid `colours`, see `Debris::init`).
    pub fn debris_init(&mut self, colours: &[u8]) {
        if let Some(mut d) = self.debris.take() {
            d.init(&mut DebrisCtx { env: self }, colours);
            self.debris = Some(d);
        }
    }
    /// 0x29b10: move, colour and draw the fragments.
    pub fn debris_update(&mut self) {
        if let Some(mut d) = self.debris.take() {
            d.update(&mut DebrisCtx { env: self });
            self.debris = Some(d);
        }
    }
    /// 0x236b4: move, animate and draw the effects.
    pub fn fx_update(&mut self) {
        if let Some(mut fx) = self.fx.take() {
            fx.update(&mut FxCtx { env: self });
            self.fx = Some(fx);
        }
    }
}

/// What the effect pool sees (`EffectWorld`).
struct FxCtx<'a> {
    env: &'a mut Env,
}

impl EffectWorld for FxCtx<'_> {
    fn tick(&self) -> u32 { self.env.now }
    fn scroll_step(&self) -> i32 { self.env.scroll_speed }
    fn translucent(&self) -> bool { self.env.opts.translucent }
    fn fancy(&self) -> bool { self.env.detail }
    fn random(&mut self, lo: i32, hi: i32) -> i32 { self.env.rng.range(lo, hi) }
    fn sound(&mut self, id: i32, prio: i32, arg: i32) { World::sound(&mut *self.env, id, prio, arg); }
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.env.draws.push(Draw { prim, x, y, w, h, anim, frame });
    }
}

/// What the debris pool sees (`DebrisWorld`): the fragments are drawn as `Prim::Pixels` requests in playfield coordinates (the original plots into its 704 x 608
/// buffer, whose playfield starts at 64, 64).
struct DebrisCtx<'a> {
    env: &'a mut Env,
}

impl DebrisWorld for DebrisCtx<'_> {
    fn scroll_step(&self) -> i32 { self.env.scroll_speed }
    fn random(&mut self, lo: i32, hi: i32) -> i32 { self.env.rng.range(lo, hi) }
    fn sound(&mut self, id: i32, prio: i32, arg: i32) { World::sound(&mut *self.env, id, prio, arg); }
    fn plot(&mut self, x: i32, y: i32, colour: u8) {
        self.env.draws.push(Draw { prim: Prim::Pixels, x: x - 64, y: y - 64, w: 3, h: 3, anim: colour as i32, frame: 1 });
    }
}

/// What the coin bonus sees (`TallyWorld`): the ships' state, their score, the pickups on the screen and the game's sound and draw list.
struct ShipsTally<'a> {
    env: &'a mut Env,
    ships: &'a mut [Ship; 2],
    pickups: &'a mut PickupPool,
}

impl TallyWorld for ShipsTally<'_> {
    fn tick(&self) -> u32 { self.env.now }
    fn players(&self) -> u8 { self.env.players }
    fn level(&self) -> i32 { self.env.start_level_value }
    fn coin_style(&self) -> bool { false }
    fn translucent(&self) -> bool { self.env.opts.translucent }
    fn ship_active(&self, n: u8) -> bool { self.ships[n as usize - 1].active }
    fn ship_lives(&self, n: u8) -> i32 { self.ships[n as usize - 1].lives }
    fn ship_coins(&self, n: u8) -> i32 { self.ships[n as usize - 1].coins }
    fn add_score(&mut self, n: u8, points: i32, flash: i32) {
        let now = self.env.now;
        self.ships[n as usize - 1].add_score(points, flash != 0, now, &mut *self.env);
    }
    fn convert_coins(&mut self) {
        let env = &mut *self.env;
        self.pickups.convert_coins(|x, y, k, d, snd, sc, lp, vx, vy| env.fx_spawn(x, y, k, d, snd, sc, lp, vx, vy));
    }
    fn sound(&mut self, id: i32, prio: i32, arg: i32) { World::sound(&mut *self.env, id, prio, arg); }
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.env.draws.push(Draw { prim, x, y, w, h, anim, frame });
    }
}

impl World for Env {
    /// 0x2c760: a drone for `number` (1 = left of the ship, 2 = right).
    fn drone_add(&mut self, number: u8, which: i32) -> bool {
        let mut d = self.drones.take().expect("drones");
        let ok = d.add(number, which, &*self);
        self.drones = Some(d);
        ok
    }
    /// 0x2c87c: the drones of a player blow up.
    fn drones_reset(&mut self, number: u8) {
        let mut d = self.drones.take().expect("drones");
        d.remove(number, self);
        self.drones = Some(d);
    }
    /// 0x2c960: the docked drones of `ship` shoot.
    fn drones_fire(&mut self, ship: &Ship, shots: &mut ShotPool) {
        if let Some(d) = self.drones.as_mut() { d.fire(ship, shots); }
    }
    /// 0x277e4: the side missiles of `ship`.
    fn launch_missiles(&mut self, ship: &Ship) -> bool {
        let mut m = self.missiles.take().expect("missiles");
        let ok = m.launch(ship, &*self);
        self.missiles = Some(m);
        ok
    }
    fn hit_enemies(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, hp: i32, owner: u8) -> i32 {
        let mut total = 0;
        // the real enemies (0x2ead4); their damage routine reports through `EnemyCtx` without ship access (scores and bonuses are deferred)
        if let Some(mut en) = self.enemies.take() {
            let boxes = en.boxes();
            total = hit_enemy_boxes(&boxes, x0, y0, x1, y1, hp, owner, |slot, hp_left, owner| {
                let mut ctx = EnemyCtx { env: &mut *self, ships: None };
                if en.r8(slot, crate::enemy::f::BOSS) != 0 {
                    let idx = en.ri32(slot, crate::enemy::f::BOSS_IDX);
                    en.boss_hit(slot as i32, idx, hp_left, owner, &mut ctx)
                } else {
                    en.damage(slot, hp_left, owner, &mut ctx)
                }
            });
            self.enemies = Some(en);
        }
        if total >= hp { return total; }
        let hp = hp - total;
        let boxes: Vec<EnemyBox> = self.targets.iter().map(|t| t.as_box()).collect();
        let Env { targets, kills, effects, now, .. } = self;
        let now = *now;
        total + hit_enemy_boxes(&boxes, x0, y0, x1, y1, hp, owner, |slot, hp_left, owner| {
            let t = &mut targets[slot];
            let (dealt, killed) = t.damage(hp_left);
            t.hit_tick = now;
            if killed {
                t.active = false;
                kills.push(Kill { owner, points: t.points, x: t.x + t.w as f32 / 2.0, y: t.y + t.h as f32 / 2.0 });
                effects.push(EffectEvent { x: (t.x + t.w as f32 / 2.0) as i32, y: (t.y + t.h as f32 / 2.0) as i32, kind: 14 });
            }
            dealt
        })
    }
    fn effect(&mut self, x: i32, y: i32, kind: i32, delay: i32, param: i32, a: i32, b: i32, vx: f32, vy: f32) {
        self.fx_spawn(x, y, kind, delay, param, a, b, vx, vy);
    }
    fn effect_aux(&mut self, id: u32, x: i32, y: i32, kind: i32, delay: i32, param: i32) {
        // 0x29714 / 0x298ec(x, y, kind, delay, sound, scrolls = 0)
        if id == 0x298ec { self.debris_spawn_in(x, y, kind, delay, param, 0) } else { self.debris_spawn(x, y, kind, delay, param, 0) }
    }
    fn drop_pickup(&mut self, kind: i32, x: f32, y: f32) { self.drops.push((kind, x, y)); }
    fn start_level(&mut self) -> i32 { self.start_level_value } // 0xd7a0
    fn level_bonus(&mut self) -> i32 { self.levels_done as i32 } // 0xd7a8
    fn random(&mut self, lo: i32, hi: i32) -> i32 { self.rng.range(lo, hi) }
    fn demo_check(&mut self) -> bool { self.demo } // 0xf5b8
    fn scroll_speed(&mut self) -> i32 { self.scroll_speed }
    fn sound(&mut self, id: i32, priority: i32, arg: i32) {
        self.sounds.push(SoundEvent { id, priority, arg, pan: false });
        self.audio.play(id, priority, arg as u8);
    }
    fn sound_pan(&mut self, id: i32, priority: i32, pan: i32) {
        self.sounds.push(SoundEvent { id, priority, arg: pan, pan: true });
        self.audio.start_rate(id, priority, pan as u32);
    }
    fn draw(&mut self, d: Draw) { self.draws.push(d); }
    fn charge_loop_start(&mut self) -> u32 {
        self.sounds.push(SoundEvent { id: 18, priority: 3, arg: 0, pan: false });
        self.audio.start(18, 3)
    }
    fn overcharge_stream_start(&mut self, number: u8) -> u32 {
        self.sounds.push(SoundEvent { id: 2, priority: 2, arg: 0, pan: false });
        self.audio.start_callback(2, 2, CB_OVERCHARGE, number as u32)
    }
    // 0x51d2c / 0x51b4c, including the original's handling of handle 0 = "every voice"
    fn sound_playing(&mut self, handle: u32) -> bool { self.audio.playing(handle) & 0xffff != 0 }
    fn sound_stop(&mut self, handle: u32) { self.audio.stop(handle); }
    /// The charge overheated (0x26fd8 + 0x24544): the smart-bomb effect, then the ship is destroyed.
    fn ship_overheated(&mut self, ship: &mut Ship) {
        let now = self.now;
        // 0x26fd8: the ground part, the enemies and (0x38a1c) every enemy shot
        ship.smart_bomb(self);
        ship.destroy(false, now, self);
    }
    /// 0x2e6a8 is one of the three screen routines of the smart bomb; here it hurts every debug target (the real effect is not ported).
    fn ground_target(&mut self, l: i32, t: i32, r: i32, b: i32, number: u8) -> bool {
        let Some(mut g) = self.ground.take() else { return false };
        let hit = g.hit_box(l, t, r, b, 0, number, &mut GroundCtx { env: &mut *self, ships: None }) != -1;
        self.ground = Some(g);
        hit
    }
    fn hud_lives(&mut self, set: u8, lives: i32, flash: i32) { self.hud_with(|h, e| h.lives(set as u32, lives, flash as u32, e)); }
    fn hud(&mut self, id: u32, a: i32, b: i32) {
        let p = a as u32;
        match id {
            0x1fe28 => self.hud_with(|h, e| h.weapon(p, b as u32, e)),
            0x1ff94 => self.hud_with(|h, e| h.weapon_level(p, b as u32, e)),
            0x1f69c => self.hud_with(|h, e| h.shield(p, b, e)),
            0x1f51c => self.hud_with(|h, e| h.shield_target(p, b, e)),
            0x1f5c4 => self.hud_with(|h, e| h.shield_tick(p, e)),
            0x1fc50 => self.hud_with(|h, e| h.flash(p, e)),
            0x28510 => self.msg = a as u8, // the message overlay's state (0x28510 stores D.172c)
            _ => {}
        }
        if id == 0x3fbfc {
            // the smart bomb's ground part: every ground object on the screen takes damage
            if let Some(mut g) = self.ground.take() {
                g.bomb_all(a as u8, &mut GroundCtx { env: &mut *self, ships: None });
                self.ground = Some(g);
            }
        }
        if id == 0x38a1c {
            // 0x38a1c: every enemy shot explodes
            if let Some(mut pool) = self.eshots.take() {
                pool.clear_all(&mut ShotCtx { env: &mut *self, ships: None });
                self.eshots = Some(pool);
            }
        }
        if id == 0x2e6a8 {
            let owner = a as u8;
            if let Some(mut en) = self.enemies.take() {
                en.bomb(owner, &mut EnemyCtx { env: &mut *self, ships: None });
                self.enemies = Some(en);
            }
            let Env { targets, kills, effects, .. } = self;
            for t in targets.iter_mut().filter(|t| t.active) {
                let (_, killed) = t.damage(20);
                if killed {
                    t.active = false;
                    kills.push(Kill { owner, points: t.points, x: t.x + t.w as f32 / 2.0, y: t.y + t.h as f32 / 2.0 });
                    effects.push(EffectEvent { x: (t.x + t.w as f32 / 2.0) as i32, y: (t.y + t.h as f32 / 2.0) as i32, kind: 14 });
                }
            }
        }
    }
}

/// What the enemy code sees of the game (`EnemyWorld`): the random generator, sounds, effects and scores of `Env`, and, when `ships` is given,
/// the live ships (positions for targeting; damage to a ship is applied at once, like the original). Without ships (shots hitting enemies in the
/// middle of the ships' own update) every ship counts as absent.
struct EnemyCtx<'a> {
    env: &'a mut Env,
    ships: Option<&'a mut [Ship; 2]>,
}

fn ship_view(s: &Ship) -> ShipView {
    ShipView { alive: s.active && s.state == STATE_PLAYING, vulnerable: !s.invuln, x: s.x, y: s.y, w: s.w, h: s.h, hw: s.hit_w, hh: s.hit_h }
}

impl EnemyWorld for EnemyCtx<'_> {
    fn tick(&self) -> u32 { self.env.now }
    fn players(&self) -> u8 { self.env.players }
    fn ship(&self, n: u8) -> ShipView {
        match &self.ships {
            Some(s) => ship_view(&s[n as usize - 1]),
            None => ShipView::default(),
        }
    }
    fn random(&mut self, lo: i32, hi: i32) -> i32 { self.env.rng.range(lo, hi) }
    fn difficulty(&self) -> u32 { self.env.start_level_value as u32 }
    fn score(&mut self, owner: u8, points: i32) { self.env.score_events.push((owner, points)); }
    fn ship_damage(&mut self, n: u8, amount: i32) {
        let now = self.env.now;
        if let Some(s) = self.ships.as_deref_mut() {
            s[n as usize - 1].damage(amount, now, &mut *self.env);
        }
    }
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32) {
        self.env.fx_spawn(x, y, id, a, b, c, d, vx, vy);
    }
    fn aux(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32) { self.env.debris_spawn(x, y, id, a, b, c); }
    fn particles(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32) { self.env.fx_particles(x, y, id, a, b, c); }
    fn group_bonus(&mut self, owner: u8, x: f32, y: f32) { self.env.bonus_events.push((owner, x, y)); }
    fn pickup(&mut self, kind: i32, variant: i32, param: i32, announce: i32, x: f32, y: f32) {
        self.env.pickup_events.push((kind, variant, param, announce, x, y));
    }
    fn sound_loop(&mut self, id: i32, prio: i32, boss: i32) -> u32 {
        self.env.sounds.push(SoundEvent { id, priority: prio, arg: 0, pan: false });
        self.env.audio.start_callback(id, prio, CB_ENEMY, boss as u32)
    }
    fn sound(&mut self, id: i32, prio: i32, delay: i32) { World::sound(&mut *self.env, id, prio, delay); }
    fn sound_rate(&mut self, id: i32, prio: i32, rate: i32) { World::sound_pan(&mut *self.env, id, prio, rate); }
    // enemy shots (0x383b8 / 0x38754)
    fn shot_aimed(&mut self, kind: i32, target: i32, flag: i32, x: f32, y: f32) -> i32 {
        let Some(mut pool) = self.env.eshots.take() else { return 0 };
        let r = pool.spawn_aimed(&ShotCtx { env: &mut *self.env, ships: self.ships.as_deref_mut() }, kind, target, flag, x, y);
        self.env.eshots = Some(pool);
        r as i32
    }
    fn shot_param(&mut self, a: [i32; 6], x: f32, y: f32) -> i32 {
        let Some(mut pool) = self.env.eshots.take() else { return 0 };
        let r = pool.spawn_param(&mut ShotCtx { env: &mut *self.env, ships: self.ships.as_deref_mut() }, a, x, y);
        self.env.eshots = Some(pool);
        r as i32
    }
}

/// What the ground objects see of the game (`GroundWorld`). Pickups and the "all targets destroyed" bonus are queued (`Game::flush_ground_events`).
struct GroundCtx<'a> {
    env: &'a mut Env,
    ships: Option<&'a mut [Ship; 2]>,
}

impl GroundWorld for GroundCtx<'_> {
    fn tick(&self) -> u32 { self.env.now }
    fn players(&self) -> u8 { self.env.players }
    fn ship(&self, n: u8) -> ShipView {
        match &self.ships {
            Some(s) => ship_view(&s[n as usize - 1]),
            None => ShipView::default(),
        }
    }
    fn random(&mut self, lo: i32, hi: i32) -> i32 { self.env.rng.range(lo, hi) }
    fn difficulty(&self) -> u32 { self.env.start_level_value as u32 }
    fn scroll_step(&self) -> i32 { self.env.scroll_speed }
    fn buffer_offset(&self) -> i32 { 0 }
    fn shadows(&self) -> bool { self.env.opts.shadows }
    fn score(&mut self, owner: u8, points: i32) { self.env.score_events.push((owner, points)); }
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32) {
        self.env.fx_spawn(x, y, id, a, b, c, d, vx, vy);
    }
    fn aux(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32) { self.env.debris_spawn(x, y, id, a, b, c); }
    fn particles(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32) { self.env.fx_particles(x, y, id, a, b, c); }
    fn sound(&mut self, id: i32, prio: i32, delay: i32) { World::sound(&mut *self.env, id, prio, delay); }
    fn pickup(&mut self, kind: i32, variant: i32, param: i32, announce: i32, x: f32, y: f32) {
        self.env.pickup_events.push((kind, variant, param, announce, x, y));
    }
    fn beam(&mut self, kind: i32, snd: i32, target: i32, flag: i32, x: f32, y: f32, mx: f32, my: f32) {
        let Some(mut pool) = self.env.eshots.take() else { return };
        pool.spawn_beam(&mut ShotCtx { env: &mut *self.env, ships: self.ships.as_deref_mut() }, kind, x, y, mx, my, snd, target, flag);
        self.env.eshots = Some(pool);
    }
    fn shot_aimed(&mut self, kind: i32, target: i32, flag: i32, x: f32, y: f32) -> i32 {
        let Some(mut pool) = self.env.eshots.take() else { return 0 };
        let r = pool.spawn_aimed(&ShotCtx { env: &mut *self.env, ships: self.ships.as_deref_mut() }, kind, target, flag, x, y);
        self.env.eshots = Some(pool);
        r as i32
    }
    fn clear_bonus(&mut self, x: f32, y: f32) { self.env.clear_events.push((x, y)); }
    /// Only the three primitives that draw on the screen itself are kept; the ones that burn into the background buffer (0x3728 / 0x3910 / 0x381c)
    /// are replaced by `Ground::render_background`, which draws the same things from the state.
    fn prim(&mut self, which: u32, a: [i32; 7]) {
        let prim = match which {
            0x2f88 => Prim::Anim,
            0x3358 => Prim::Cast,
            0x3540 => Prim::Flash,
            _ => return,
        };
        self.env.ground_live.push(Draw { prim, x: a[1], y: a[2], w: a[3], h: a[4], anim: a[5], frame: a[6] });
    }
}

impl DroneWorld for Env {
    fn tick(&self) -> u32 { self.now }
    fn hit_enemies(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, hp: i32, owner: u8) -> i32 { World::hit_enemies(self, x0, y0, x1, y1, hp, owner) }
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32) { self.fx_spawn(x, y, id, a, b, c, d, vx, vy); }
    fn sound(&mut self, id: i32, prio: i32, arg: i32) { World::sound(self, id, prio, arg); }
    fn shadows(&self) -> bool { self.opts.shadows }
    fn translucent(&self) -> bool { self.opts.translucent }
    fn fancy(&self) -> bool { self.detail }
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.draws.push(Draw { prim, x, y, w, h, anim, frame });
    }
}

/// What the bombs see (`BombWorld`): the game's random generator, sounds, effects and draw list, and the ground objects they hit.
struct BombCtx<'a> {
    env: &'a mut Env,
    ground: &'a mut Ground,
    ships: Option<&'a mut [Ship; 2]>,
}

impl BombWorld for BombCtx<'_> {
    fn tick(&self) -> u32 { self.env.now }
    fn scroll_step(&self) -> i32 { self.env.scroll_speed }
    fn random(&mut self, lo: i32, hi: i32) -> i32 { self.env.rng.range(lo, hi) }
    fn sound(&mut self, id: i32, prio: i32, arg: i32) { World::sound(&mut *self.env, id, prio, arg); }
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32) {
        self.env.fx_spawn(x, y, id, a, b, c, d, vx, vy);
    }
    fn ground_hit(&mut self, l: i32, t: i32, r: i32, b: i32, amount: i32, owner: u8) -> i32 {
        let mut ctx = GroundCtx { env: &mut *self.env, ships: self.ships.as_deref_mut() };
        self.ground.hit_box(l, t, r, b, amount, owner, &mut ctx)
    }
    fn add_decal(&mut self, a: [i32; 8], p9: i32, p10: i32) {
        let mut ctx = GroundCtx { env: &mut *self.env, ships: None };
        self.ground.add_decal(&mut ctx, a, p9, p10);
    }
    fn shadows(&self) -> bool { self.env.opts.shadows }
    fn translucent(&self) -> bool { self.env.opts.translucent }
    fn fancy(&self) -> bool { self.env.detail }
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.env.draws.push(Draw { prim, x, y, w, h, anim, frame });
    }
}

/// What the enemy-shot code sees (`ShotWorld`). Ground objects that stop shots (0x2ca38) are not ported: shots are never blocked.
struct ShotCtx<'a> {
    env: &'a mut Env,
    ships: Option<&'a mut [Ship; 2]>,
}

impl ShotWorld for ShotCtx<'_> {
    fn tick(&self) -> u32 { self.env.now }
    fn ship(&self, n: u8) -> ShipView {
        match &self.ships {
            Some(s) => ship_view(&s[n as usize - 1]),
            None => ShipView::default(),
        }
    }
    fn scroll_speed(&self) -> i32 { self.env.scroll_speed }
    fn object_hit(&mut self, _l: i32, _t: i32, _r: i32, _b: i32) -> bool { false }
    fn ship_damage(&mut self, n: u8, amount: i32) {
        let now = self.env.now;
        if let Some(s) = self.ships.as_deref_mut() {
            s[n as usize - 1].damage(amount, now, &mut *self.env);
        }
    }
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32) {
        self.env.fx_spawn(x, y, id, a, b, c, d, vx, vy);
    }
    fn sound(&mut self, id: i32, prio: i32, delay: i32) { World::sound(&mut *self.env, id, prio, delay); }
}

/// The draw routine's view (`EnemyDrawWorld`).
struct EnemyDraws<'a> {
    env: &'a mut Env,
    opts: DrawOpts,
}

impl ShotDrawWorld for EnemyDraws<'_> {
    fn opts(&self) -> DrawOpts { self.opts }
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.env.draws.push(Draw { prim, x, y, w, h, anim, frame });
    }
}

impl EnemyDrawWorld for EnemyDraws<'_> {
    fn tick(&self) -> u32 { self.env.now }
    fn random(&mut self, lo: i32, hi: i32) -> i32 { self.env.rng.range(lo, hi) }
    fn opts(&self) -> DrawOpts { self.opts }
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.env.draws.push(Draw { prim, x, y, w, h, anim, frame });
    }
}

/// Pickup kinds the debug targets drop.
const DROP_KINDS: [u8; 14] = [2, 2, 2, 5, 6, 7, 8, 9, 10, 11, 12, 13, 18, 19];

pub struct Game {
    pub now: u32,
    pub players: usize,
    pub ships: [Ship; 2],
    pub in_game: [bool; 2],
    pub shots: ShotPool,
    pub pickups: PickupPool,
    pub scroller: Scroller,
    pub spawn_by_row: Vec<u32>,
    pub env: Env,
    pub opts: DrawOpts,
    pub over: bool,
    pub level_finished: bool,
    /// The end-of-level coin bonus (`levelend.rs`, 0x20764 / 0x208fc).
    pub tally: Tally,
    /// `D.13e8`: the tally is over, the level is complete and the next one may start (the host loads it, see `Game::next_level`).
    pub level_complete: bool,
    /// The last level was completed (0xe098(6000): the ending).
    pub won: bool,
    pub last_spawn: Option<(usize, u32)>,
    pub targets_enabled: bool,
    /// The level map the ground objects are activated from: (width, height, marker layer `GS`), see `Game::load_ground`.
    ground_map: (usize, usize, Vec<u8>),
    /// The audio produced by the last `step`: exactly `SAMPLES_PER_FRAME` unsigned 8-bit mono samples at 22254.545 Hz.
    pub audio_out: Vec<u8>,
}

impl Game {
    /// A new game: `players` = 1 or 2; `level_height` rows (13 tiles wide) and the level's spawn triggers drive the scroller.
    pub fn new(players: usize, level_height: usize, spawn_by_row: Vec<u32>, seed: u32) -> Game {
        Game::new_at(players, level_height, spawn_by_row, seed, 1)
    }

    /// A new game starting at level number `level_number` (1-based; `D.13ec`, which sets the difficulty, the weapon level the ships start with and the coin bonus).
    pub fn new_at(players: usize, level_height: usize, spawn_by_row: Vec<u32>, seed: u32, level_number: i32) -> Game {
        let players = players.clamp(1, 2);
        let solo = players == 1;
        let mut env = Env::new(seed);
        env.players = players as u8;
        env.start_level_value = level_number.max(1);
        env.fx_init(); // 0x22618 at the start of a level
        env.debris_init(&[]); // 0x2942c (the colour grid is loaded by `set_particle_colours`)
        env.hud_init(); // 0x1e950: after the panel picture, before the ships are set up (0x23de4 draws their weapon icons)
        let mut ships = [Ship::new(1, solo, 1), Ship::new(2, solo, 1)];
        if solo {
            // 0x23c04 for ship 2 of a one-player game: not active, one life (the HUD then shows no spare ships for it)
            ships[1].active = false;
            ships[1].lives = 1;
        }
        for s in ships.iter_mut().filter(|s| s.active) {
            s.reset_score(0);
            s.init(false, 0, &mut env);
        }
        World::sound(&mut env, 28, 5, 0); // 0xc6f0 after the level set-up (0xd7b0)
        let mut g = Game {
            now: 0, players, ships, in_game: [true, players == 2], shots: ShotPool::new(), pickups: PickupPool::new(),
            scroller: Scroller::new(level_height), spawn_by_row, env, opts: DrawOpts::default(), over: false, level_finished: false, tally: Tally::default(), level_complete: false, won: false,
            last_spawn: None, targets_enabled: true, ground_map: (0, 0, Vec::new()), audio_out: Vec::new(),
        };
        g.hud_level_setup();
        g
    }

    /// The HUD part of the level set-up (0xd9e8..0xdae0, then 0xc8ec / 0xd5e4): for both ships the score (forced), the lives, the shield bar at 0 and, for a ship that is
    /// not in the game, the dimming flash; afterwards the shield bars are told to grow to the ships' shield levels.
    fn hud_level_setup(&mut self) {
        for who in 0..2 {
            let set = self.ships[who].input_set as u32;
            let lives = self.ships[who].lives;
            let ship = &mut self.ships[who];
            self.env.hud_with(|h, e| {
                h.score(ship, 1, e);
                h.lives(set, lives, 0, e);
                h.shield(set, 0, e);
            });
            if !self.ships[who].active { self.env.hud_with(|h, e| h.flash(set, e)); }
        }
        for who in 0..self.players {
            let (set, shield) = (self.ships[who].input_set as u32, self.ships[who].shield);
            self.env.hud_with(|h, e| h.shield_target(set, shield, e));
        }
    }

    /// The HUD drawing operations since the last call, and the HUD epoch (see `Env::hud_epoch`): when the epoch differs from the one seen before, the host first
    /// restores the panel picture, then executes the operations.
    pub fn take_hud_calls(&mut self) -> (u32, Vec<HudCall>) { (self.env.hud_epoch, std::mem::take(&mut self.env.hud_calls)) }

    /// The 13 x 10 colour grid of the debris effects as palette indices (`assets::load_particle_colours`; 0x29668 reads it at start-up).
    pub fn set_particle_colours(&mut self, colours: &[u8]) {
        if let Some(d) = self.env.debris.as_mut() { for (i, c) in colours.iter().take(130).enumerate() { d.colours[i] = *c; } }
    }

    /// The last level is complete: the ending (0xe098(6000) plays sound 71, then 56 three times, the last two delayed by 1 and 2 ticks).
    pub fn finish_game(&mut self) {
        self.won = true;
        self.level_complete = false;
        World::sound(&mut self.env, 71, 4, 0);
        for d in 0..3 { World::sound(&mut self.env, 56, 4, d); }
    }

    /// The level number (`D.13ec`).
    pub fn level_number(&self) -> i32 { self.env.start_level_value }

    /// Starts or ends the attract-mode demo flag. The level reset (0xdae0) then sets the message state: 3 in a demo (0xdaf8), 1 otherwise (0xdb08).
    pub fn set_demo(&mut self, on: bool) {
        self.env.demo = on;
        self.env.msg = if on { crate::overlay::GAME_OVER } else { crate::overlay::GET_READY };
    }

    /// The level is complete (the coin bonus is over): start the next one (0xc6f0 at `D.13e8` and 0xd7b0). The ships keep their score, lives and weapons;
    /// everything else of the level is reset. The host then loads the new level's groups and ground objects (`load_groups`, `load_ground`).
    /// Sounds 28 and 36 mark the change, and the delayed-sound queue is emptied (0x45a38).
    pub fn next_level(&mut self, level_height: usize, spawn_by_row: Vec<u32>) {
        World::sound(&mut self.env, 28, 5, 0);
        World::sound(&mut self.env, 36, 4, 0);
        self.env.levels_done += 1; // D.13f0
        self.env.start_level_value += 1; // D.13ec
        self.now = 0; // 0xecf8
        self.env.now = 0;
        self.scroller = Scroller::new(level_height);
        self.spawn_by_row = spawn_by_row;
        self.env.scroll_speed = 1;
        self.level_finished = false;
        self.level_complete = false;
        self.tally = Tally::default();
        self.last_spawn = None;
        self.shots = ShotPool::new(); // 0x21224
        self.pickups = PickupPool::new(); // 0x29e78
        self.env.enemies = Some(Enemies::new());
        self.env.eshots = Some(EnemyShots::new()); // 0x37f60
        self.env.bombs = Some(Bombs::new()); // 0x2015c
        self.env.missiles = Some(Missiles::new()); // 0x27780
        self.env.ground = Some(Ground::new());
        self.env.targets = vec![Target::default(); MAX_TARGETS];
        self.env.drops.clear();
        self.env.kills.clear();
        self.env.score_events.clear();
        self.env.bonus_events.clear();
        self.env.pickup_events.clear();
        self.env.clear_events.clear();
        self.env.effects.clear();
        self.env.ground_live.clear();
        self.env.hud_init(); // 0x1e950 (after the panel picture has been drawn again)
        self.env.fx_init(); // 0x22618
        if let Some(d) = self.env.debris.as_mut() { d.reset(); } // 0x2957c
        let keep = self.env.start_level_value > 1 && self.env.levels_done >= 1;
        for who in 0..2 {
            if self.ships[who].active { self.ships[who].init(keep, 0, &mut self.env); }
        }
        self.env.audio.clear_queue(); // 0x45a38
        self.hud_level_setup();
        // 0xdae0..0xdb08 (the level set-up 0xd7b0 runs for every level): the message state is set again, 3 in a demo, 1 ("Sector N / Get Ready!") otherwise.
        // Without this the "Sector N Cleared" (state 4) of the level just won stayed up until the ship entered, showing the new sector's number.
        self.env.msg = if self.env.demo { crate::overlay::GAME_OVER } else { crate::overlay::GET_READY };
    }

    /// Spawn a debug target at the top of the playfield.
    pub fn spawn_target(&mut self) {
        let Some(slot) = self.env.targets.iter().position(|t| !t.active) else { return };
        let kind = self.env.rng.range(0, 2);
        let (w, h, hp, pts) = match kind { 0 => (32, 32, 6, 100), 1 => (48, 40, 20, 250), _ => (64, 48, 60, 800) };
        let x = self.env.rng.range(0, 416 - w) as f32;
        let vx = (self.env.rng.range(-8, 8) as f32) * 0.25;
        let vy = 0.75 + (self.env.rng.range(0, 3) as f32) * 0.5;
        self.env.targets[slot] = Target { active: true, x, y: -(h as f32), w, h, vx, vy, max_hp: hp, taken: 0, points: pts, born: self.now, hit_tick: 0 };
    }

    /// Debug: drop a power-up of `kind` in front of player 1.
    pub fn debug_pickup(&mut self, kind: u8) {
        let (x, y) = (self.ships[0].x + 24.0, self.ships[0].y - 120.0);
        self.debug_pickup_at(kind, x, y);
    }

    /// Debug: create a power-up of `kind` centred on (x, y).
    pub fn debug_pickup_at(&mut self, kind: u8, x: f32, y: f32) {
        let now = self.now;
        let (a, b) = self.ships.split_at_mut(1);
        self.pickups.spawn(&a[0], &b[0], kind, 1, 1, 0, x, y, now, &mut self.env);
    }

    fn flush_drops(&mut self) {
        let drops = std::mem::take(&mut self.env.drops);
        let now = self.now;
        for (kind, x, y) in drops {
            let (a, b) = self.ships.split_at_mut(1);
            self.pickups.spawn(&a[0], &b[0], kind as u8, 1, 1, 0, x, y, now, &mut self.env);
        }
    }

    /// Load the level's enemy groups (0x2d528 / 0x2d7b8). From now on the real enemies replace the debug targets.
    /// `groups` = (index, the 31 words of the 124-byte `_ASG` record) of every defined group (`assets::Level::groups`).
    pub fn load_groups(&mut self, groups: &[(usize, Vec<u32>)]) {
        if let Some(en) = self.env.enemies.as_mut() { en.load_level_groups(groups); }
        self.targets_enabled = false;
    }

    /// Level load of the ground objects (0x3ade0, count 0x3acb0, decals 0x40904, bombs 0x2015c). `res` = the 250 records of 50 bytes of the level's `__GS`
    /// resource (`ground::resource_record`), `marker` = the `GS` layer (`width * height`, 255 = nothing).
    pub fn load_ground(&mut self, res: &[u8], width: usize, height: usize, marker: Vec<u8>) {
        let mut g = Ground::new();
        g.load(res, &GroundCtx { env: &mut self.env, ships: None });
        g.count_targets(width, height, &marker);
        g.clear_decals();
        self.env.ground = Some(g);
        self.env.bombs = Some(Bombs::new());
        self.ground_map = (width, height, marker);
    }

    /// Pickups the ground objects dropped and the reward for destroying the last ground target (0xea4c).
    fn flush_ground_events(&mut self) {
        let now = self.now;
        for (kind, variant, param, announce, x, y) in std::mem::take(&mut self.env.pickup_events) {
            let (a, b) = self.ships.split_at_mut(1);
            self.pickups.spawn(&a[0], &b[0], kind as u8, variant, param, announce, x, y, now, &mut self.env);
        }
        for (x, y) in std::mem::take(&mut self.env.clear_events) {
            let armor = |s: &Ship| if s.active && s.state == STATE_PLAYING { Some(s.hud_level) } else { None };
            let reward = crate::pickup::clear_reward(armor(&self.ships[0]), armor(&self.ships[1]), self.env.start_level_value as u32);
            if let Some((kind, variant)) = reward {
                let (a, b) = self.ships.split_at_mut(1);
                self.pickups.spawn(&a[0], &b[0], kind as u8, variant, 1, 0, x, y, now, &mut self.env);
            }
        }
    }

    /// 0x2d9bc: the spawn trigger of a map row names a group; normal groups are launched (0x2da30), special ones (bosses, 0x40e7c) through the boss structures.
    fn spawn_group(&mut self, g: usize) {
        let Some(mut en) = self.env.enemies.take() else { return };
        let mut ctx = EnemyCtx { env: &mut self.env, ships: Some(&mut self.ships) };
        if en.group_is_special(g) {
            en.boss_spawn(g as i32, &mut ctx);
        } else {
            en.spawn_group(g, &mut ctx);
        }
        self.env.enemies = Some(en);
    }

    /// Scores and group-wipe rewards earned through enemy kills this frame (0xe68c / 0xe6c0 and 0x29edc).
    fn flush_enemy_events(&mut self) {
        let now = self.now;
        for (owner, pts) in std::mem::take(&mut self.env.score_events) {
            let who = if owner == 2 { 1 } else { 0 };
            if self.ships[who].active { self.ships[who].add_score(pts, false, now, &mut self.env); }
        }
        for (owner, x, y) in std::mem::take(&mut self.env.bonus_events) {
            let mut wipes = [self.ships[0].wipes, self.ships[1].wipes];
            let kind = {
                let env = &mut self.env;
                crate::pickup::bonus_kind(owner, &mut wipes, |lo, hi| env.rng.range(lo, hi))
            };
            self.ships[0].wipes = wipes[0];
            self.ships[1].wipes = wipes[1];
            let (a, b) = self.ships.split_at_mut(1);
            self.pickups.spawn(&a[0], &b[0], kind, 1, 1, 1, x, y, now, &mut self.env);
        }
    }

    fn flush_kills(&mut self) {
        let kills = std::mem::take(&mut self.env.kills);
        let now = self.now;
        for k in kills {
            self.env.total_kills += 1;
            let who = if k.owner == 2 { 1 } else { 0 };
            if self.ships[who].active { self.ships[who].add_score(k.points, false, now, &mut self.env); }
            World::sound(&mut self.env, 4, 3, 0);
            if self.env.rng.range(0, 99) < 35 {
                let kind = DROP_KINDS[self.env.rng.range(0, DROP_KINDS.len() as i32 - 1) as usize];
                self.env.drops.push((kind as i32, k.x - 24.0, k.y - 14.0));
            }
        }
        self.flush_drops();
    }

    /// Move the debug targets and apply the ship-contact rule of 0x2dc20: a ship in state 3 that is not invulnerable and overlaps a target's box
    /// (strict inequalities against the ship's body box, `Ship::enemy_hitbox`) takes `remaining hp + 1` damage and the target takes 5.
    fn update_targets(&mut self) {
        let now = self.now;
        for t in self.env.targets.iter_mut().filter(|t| t.active) {
            t.x += t.vx;
            t.y += t.vy;
            if t.x < 0.0 { t.x = 0.0; t.vx = -t.vx; }
            if t.x + t.w as f32 > SCREEN_W as f32 { t.x = (SCREEN_W - t.w) as f32; t.vx = -t.vx; }
            if t.y > SCREEN_H as f32 { t.active = false; }
        }
        for who in 0..2 {
            if !self.ships[who].active || self.ships[who].state != STATE_PLAYING || self.ships[who].invuln { continue; }
            let (l, t, r, b) = self.ships[who].enemy_hitbox();
            for slot in (0..MAX_TARGETS).rev() {
                let e = self.env.targets[slot];
                if !e.active || self.ships[who].state != STATE_PLAYING { continue; }
                let (el, et, er, eb) = (e.x as i32, e.y as i32, (e.x + e.w as f32) as i32, (e.y + e.h as f32) as i32);
                if l >= er || el >= r || t >= eb || et >= b { continue; }
                let amount = e.remaining() + 1;
                self.ships[who].damage(amount, now, &mut self.env);
                let owner = self.ships[who].number;
                let tg = &mut self.env.targets[slot];
                let (_, killed) = tg.damage(5);
                if killed {
                    tg.active = false;
                    let k = Kill { owner, points: tg.points, x: tg.x + tg.w as f32 / 2.0, y: tg.y + tg.h as f32 / 2.0 };
                    self.env.kills.push(k);
                    self.env.effects.push(EffectEvent { x: k.x as i32, y: k.y as i32, kind: 14 });
                }
            }
        }
    }

    /// One logic frame (30 per second in the original).
    pub fn step(&mut self, inputs: [PlayerInput; 2]) {
        self.now = self.now.wrapping_add(1);
        let now = self.now;
        self.env.now = now;
        self.env.draws.clear();
        self.env.sounds.clear();
        self.env.opts = self.opts;
        self.env.ground_live.clear();

        // 0xc9d0: per-player state machines
        if !self.over {
            for who in 0..self.players {
                let other = if self.players == 2 { self.in_game[1 - who] } else { false };
                let ctx = SessionCtx { solo: self.players == 1, other_in_game: other, demo: self.env.demo, mode: self.env.demo as u8 };
                if self.in_game[who] {
                    let out = self.ships[who].session_step(now, &ctx, self.in_game[who], &mut self.env);
                    self.in_game[who] = out.in_game;
                    if out.game_over { self.over = true; }
                }
            }
        }

        // input (0x25074 / 0x250a0) and button 1 (0x26cf8)
        for who in 0..self.players {
            let s = &mut self.ships[who];
            s.input = if s.state == STATE_PLAYING { ShipInput::from(inputs[who]) } else { ShipInput::default() };
            s.fire_step(now, &mut self.shots, &mut self.env);
            // 0x26b58: button 2, the bombs
            let mut bombs = self.env.bombs.take().expect("bombs");
            let mut ground = self.env.ground.take().expect("ground");
            bomb::special_step(&mut self.ships[who], &mut bombs, &mut BombCtx { env: &mut self.env, ground: &mut ground, ships: None });
            self.env.bombs = Some(bombs);
            self.env.ground = Some(ground);
        }

        // 0x3d0fc: ground objects (turrets, missile towers ...)
        {
            let mut ground = self.env.ground.take().expect("ground");
            ground.update(&mut GroundCtx { env: &mut self.env, ships: Some(&mut self.ships) });
            self.env.ground = Some(ground);
        }

        // 0x2dc20: enemies and ship contact
        if self.targets_enabled && !self.level_finished && now % 60 == 1 && self.env.active_targets() < 4 { self.spawn_target(); }
        self.update_targets();
        if let Some(mut en) = self.env.enemies.take() {
            en.frame(&mut EnemyCtx { env: &mut self.env, ships: Some(&mut self.ships) });
            en.boss_update(&mut EnemyCtx { env: &mut self.env, ships: Some(&mut self.ships) }); // 0x4127c
            self.env.enemies = Some(en);
        }

        // 0x22170 shots, 0x27434 beams, 0x2b884 pickups, 0x25264 movement
        self.shots.update(now, &mut self.env);
        for who in 0..self.players { self.ships[who].update_beam(&mut self.env); }
        // 0x27dfc: side missiles
        {
            let mut m = self.env.missiles.take().expect("missiles");
            m.update(&mut self.env);
            self.env.missiles = Some(m);
        }
        // 0x38b0c: enemy shots
        if let Some(mut pool) = self.env.eshots.take() {
            pool.update(&mut ShotCtx { env: &mut self.env, ships: Some(&mut self.ships) });
            self.env.eshots = Some(pool);
        }
        // 0x203a4: bombs in flight
        {
            let mut bombs = self.env.bombs.take().expect("bombs");
            let mut ground = self.env.ground.take().expect("ground");
            bombs.update(&mut BombCtx { env: &mut self.env, ground: &mut ground, ships: Some(&mut self.ships) });
            self.env.bombs = Some(bombs);
            self.env.ground = Some(ground);
        }
        {
            let (a, b) = self.ships.split_at_mut(1);
            self.pickups.update(&mut a[0], &mut b[0], now, &mut self.env);
        }
        for who in 0..self.players { self.ships[who].step_movement(); }
        // 0x2cb40: drones (both ship structures are passed, a missing second player is a ship that never moves)
        {
            let mut d = self.env.drones.take().expect("drones");
            d.update(&self.ships[0], &self.ships[1], &mut self.env);
            self.env.drones = Some(d);
        }

        // 0x40b50: the decals scroll with the ground
        if let Some(mut ground) = self.env.ground.take() {
            ground.scroll_decals(&GroundCtx { env: &mut self.env, ships: None });
            self.env.ground = Some(ground);
        }

        // 0xc3e0: scroll step. A completed row first activates the ground objects of the row after it (0x3b450), then fires its spawn trigger.
        let r = self.scroller.tick(&self.spawn_by_row);
        if let Some(row) = r.row_completed {
            if let Some(mut ground) = self.env.ground.take() {
                let (w, h, marker) = &self.ground_map;
                if !marker.is_empty() {
                    ground.activate_row(row as i32, *w, *h, marker, &GroundCtx { env: &mut self.env, ships: None });
                }
                self.env.ground = Some(ground);
            }
        }
        if let (Some(row), Some(g)) = (r.row_completed, r.spawn_group) {
            self.last_spawn = Some((row, g));
            self.spawn_group(g as usize);
        }
        if r.finished {
            self.level_finished = true;
            self.env.scroll_speed = 0; // 0xc0a8
        }
        if r.near_end {
            // five rows before the end: sound 21, the silent smart bomb of player 1 (0xdb54), then the coin bonus is set up (0x20764)
            World::sound(&mut self.env, 21, 5, 0);
            self.env.msg = crate::overlay::CLEARED; // 0xc564: 0x28510(4)
            self.ships[0].smart_bomb_sweep(&mut self.env);
            self.tally.setup(&ShipsTally { env: &mut self.env, ships: &mut self.ships, pickups: &mut self.pickups });
        }

        self.flush_kills();
        self.flush_enemy_events();
        self.flush_ground_events();
        self.flush_drops();

        // 0x45a6c: the delayed-sound pump, then this frame's share of audio (2 ticks at 22254.545 Hz = exactly 740 samples). The voices'
        // end/stop callbacks (0x2766c: the over-charge stream of a player ended -> the ship may start a new one) are applied here.
        self.env.audio.frame(now);
        self.audio_out.resize(SAMPLES_PER_FRAME, 128);
        self.env.audio.render(&mut self.audio_out);
        for e in self.env.audio.take_events() {
            if e.callback == CB_OVERCHARGE && (1..=2).contains(&e.param) { self.ships[e.param as usize - 1].stream_ready = true; }
            if e.callback == CB_ENEMY {
                // 0x4123c: the boss alarm ended or was stopped: start it again next frame
                if let Some(en) = self.env.enemies.as_mut() { en.bosses.sound_ended(e.sel as i32, e.param as i32); }
            }
        }

        // 0x3f554: the ground objects (moved with the scroll here), 0x205ec: the bombs. What the original burns into its background buffer (objects
        // that already are part of the ground, wrecks, scorch marks) comes first, then what is drawn on the screen.
        if let Some(mut ground) = self.env.ground.take() {
            ground.move_draw(&mut GroundCtx { env: &mut self.env, ships: None });
            let (draws, shadows) = (&mut self.env.draws, self.opts.shadows);
            ground.render_background(shadows, &mut |prim, x, y, w, h, anim, frame| draws.push(Draw { prim, x, y, w, h, anim, frame }));
            let live = std::mem::take(&mut self.env.ground_live);
            self.env.draws.extend(live);
            self.env.ground = Some(ground);
        }
        if let Some(bombs) = self.env.bombs.take() {
            let mut ground = self.env.ground.take().expect("ground");
            bombs.draw(&mut BombCtx { env: &mut self.env, ground: &mut ground, ships: None });
            self.env.bombs = Some(bombs);
            self.env.ground = Some(ground);
        }

        // draw routines: 0x2e118 (enemies, which also drops the ones that left the screen), 0x2242c, 0x275c8 (both players), 0x2c480, 0x257b4
        if let Some(pool) = self.env.eshots.take() {
            pool.draw(&mut EnemyDraws { env: &mut self.env, opts: self.opts }, false); // 0x39020
            self.env.eshots = Some(pool);
        }
        if let Some(mut en) = self.env.enemies.take() {
            en.draw(&mut EnemyDraws { env: &mut self.env, opts: self.opts });
            en.boss_draw(&mut EnemyDraws { env: &mut self.env, opts: self.opts }); // 0x417a4
            self.env.enemies = Some(en);
        }
        if let Some(pool) = self.env.eshots.take() {
            pool.draw(&mut EnemyDraws { env: &mut self.env, opts: self.opts }, true); // 0x39338
            self.env.eshots = Some(pool);
        }
        // 0x236b4: the effects (explosions, sparks, smoke); the debug targets' explosions are started here
        for e in std::mem::take(&mut self.env.effects) { self.env.fx_spawn(e.x, e.y, e.kind, 0, -1, 0, 0, 0.0, 0.0); }
        self.env.fx_update();
        self.env.debris_update(); // 0x29b10
        // 0x28260: side missiles
        if let Some(m) = self.env.missiles.take() {
            m.draw(&mut self.env);
            self.env.missiles = Some(m);
        }
        self.shots.draw(&mut self.env);
        for who in 0..2 { self.ships[who].draw_beam(&mut self.env); }
        self.pickups.draw(self.opts, &mut self.env);
        // 0x2d1d8: drones
        {
            let mut d = self.env.drones.take().expect("drones");
            d.draw(&self.ships[0], &self.ships[1], &mut self.env);
            self.env.drones = Some(d);
        }
        for who in 0..self.players { self.ships[who].draw_step(now, self.opts, &mut self.env); }
        // 0xd2e4..0xd380: the score and the charge (wave-motion) bar of every ship
        for who in 0..self.players {
            let ship = &mut self.ships[who];
            let (set, charge, hot) = (ship.input_set as u32, ship.charge, ship.heat_flash as u32);
            self.env.hud_with(|h, e| {
                h.score(ship, 0, e);
                h.power(set, charge, hot, e);
            });
        }
        // 0x28518 (0xd384): the message overlay's sprites (GAME OVER); its texts are drawn by the host (`overlay::draw_text`)
        let over = crate::overlay::sprites(self.env.msg, self.opts.translucent);
        self.env.draws.extend(over);
        // 0x208fc: the coin bonus runs while the level is in its last five rows; when it is over the level is complete (D.13e8)
        if self.scroller.ending() && !self.level_complete && !self.won && self.tally.step(&mut ShipsTally { env: &mut self.env, ships: &mut self.ships, pickups: &mut self.pickups }) {
            self.level_complete = true;
        }
    }

    /// Hash of the whole game state (for determinism tests).
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u64| { h ^= v; h = h.wrapping_mul(0x1000_0000_01b3); };
        mix(self.now as u64);
        for s in &self.ships {
            mix(s.x.to_bits() as u64); mix(s.y.to_bits() as u64); mix(s.state as u64); mix(s.lives as u64); mix(s.shield as u64);
            mix(s.score as u64); mix(s.level as u64); mix(s.spread as u64); mix(s.charge as u64);
        }
        for sh in self.shots.shots.iter().filter(|s| s.active) { mix(sh.x.to_bits() as u64); mix(sh.y.to_bits() as u64); }
        for p in self.pickups.slots.iter().filter(|p| p.active) { mix(p.x.to_bits() as u64); mix(p.y.to_bits() as u64); mix(p.kind as u64); }
        for t in self.env.targets.iter().filter(|t| t.active) { mix(t.x.to_bits() as u64); mix(t.y.to_bits() as u64); mix(t.taken as u64); }
        mix(self.env.total_kills as u64);
        if let Some(en) = &self.env.enemies { for b in en.pool.iter() { mix(*b as u64); } mix(en.live_groups as u64); }
        if let Some(sp) = &self.env.eshots { for b in sp.rec.iter() { mix(*b as u64); } mix(sp.live as u64); mix(sp.gate as u64); }
        if let Some(g) = &self.env.ground { for b in g.pool.iter() { mix(*b as u64); } mix(g.targets as u64); mix(g.destroyed as u64); }
        if let Some(b) = &self.env.bombs { for x in b.rec.iter() { mix(*x as u64); } }
        if let Some(d) = &self.env.drones { for x in d.bytes().iter() { mix(*x as u64); } }
        if let Some(m) = &self.env.missiles { for x in m.bytes().iter() { mix(*x as u64); } }
        if let Some(f) = &self.env.fx { for x in f.rec.iter() { mix(*x as u64); } }
        if let Some(f) = &self.env.debris { for x in f.rec.iter() { mix(*x as u64); } }
        for d in &self.env.draws { mix(d.x as u64 ^ ((d.y as u64) << 16) ^ ((d.anim as u64) << 32) ^ ((d.frame as u64) << 48)); }
        h
    }
}

#[cfg(test)]
mod tests {
    /// The Lehmer generator of the Toolbox `Random()`: the classic check value of "minstd" (seed 1, 10000 steps -> 1043618065), the 16 bit result and `range`.
    #[test]
    fn random_is_the_toolbox_generator() {
        let mut r = Rng::exact(1);
        let first = r.random16();
        assert_eq!(first, 16807);
        for _ in 1..10000 { r.random16(); }
        assert_eq!(r.seed(), 1043618065);
        // range: ((r as u16) * n) / 65536 + lo
        let mut a = Rng::exact(12345);
        let mut b = Rng::exact(12345);
        let v = b.random16() as i32;
        assert_eq!(a.range(10, 19), (v * 10) / 65536 + 10);
    }

    use super::*;

    fn game(players: usize) -> Game { Game::new(players, 150, vec![255; 150], 12345) }

    #[test]
    fn ship_enters_after_the_start_delay_with_three_lives_left() {
        let mut g = game(1);
        assert_eq!(g.ships[0].state, 1);
        assert_eq!(g.ships[0].lives, 4);
        for _ in 0..70 { g.step([PlayerInput::default(); 2]); }
        assert_eq!(g.ships[0].state, STATE_PLAYING);
        assert_eq!(g.ships[0].lives, 3);
        assert_eq!(g.ships[0].next_life, 10000);
        assert!(!g.over);
        // the ship is drawn (at least the bomb sight and the ship itself)
        assert!(g.env.draws.iter().any(|d| d.anim == 14));
        assert!(g.env.draws.iter().any(|d| d.anim == 22));
    }

    #[test]
    fn holding_fire_shoots_and_kills_targets() {
        // A blind left-right sweep only hits when the (seeded) targets happen to line up, so try a few fixed seeds: every one must keep the books straight
        // and at least one must destroy a target.
        let mut killed = false;
        for seed in [12345u32, 1, 2, 3, 4, 5] {
            let mut g = Game::new(1, 150, vec![255; 150], seed);
            for _ in 0..70 { g.step([PlayerInput::default(); 2]); }
            let mut seen_shot = false;
            for f in 0..3000 {
                let sweep = PlayerInput { button1: true, left: (f / 50) % 2 == 0, right: (f / 50) % 2 == 1, ..Default::default() };
                g.step([sweep, PlayerInput::default()]);
                seen_shot |= g.shots.count > 0;
            }
            assert!(seen_shot);
            assert_eq!(g.shots.count as usize, g.shots.shots.iter().filter(|s| s.active).count());
            assert_eq!(g.pickups.count as usize, g.pickups.slots.iter().filter(|p| p.active).count());
            if g.env.total_kills > 0 {
                assert!(g.ships[0].score > 0);
                killed = true;
            }
        }
        assert!(killed, "no target was destroyed with any seed");
    }

    #[test]
    fn a_ship_that_never_dodges_is_eventually_destroyed_and_the_game_ends() {
        let mut g = game(1);
        g.ships[0].lives = 2;
        // no shooting: the debug targets drift down and ram the ship
        let mut frames = 0;
        while !g.over && frames < 20000 {
            g.step([PlayerInput::default(); 2]);
            frames += 1;
        }
        assert!(g.over, "game did not end");
    }

    #[test]
    fn the_message_overlay_follows_the_original_states() {
        use crate::overlay as ov;
        // a level starts with "Sector N / Get Ready!" (0xdb08) and the message goes away when the ship enters (0xcafc: only when no demo runs)
        let mut g = game(1);
        assert_eq!(g.env.msg, ov::GET_READY);
        for _ in 0..70 { g.step([PlayerInput::default(); 2]); }
        assert_eq!(g.ships[0].state, STATE_PLAYING);
        assert_eq!(g.env.msg, ov::NONE);
        // a demo shows the GAME OVER sprites from the start (0xdaf8) and never clears them (the clear is skipped while D.13e3 == 1)
        let mut d = game(1);
        d.set_demo(true);
        assert_eq!(d.env.msg, ov::GAME_OVER);
        for _ in 0..70 { d.step([PlayerInput::default(); 2]); }
        assert_eq!(d.ships[0].state, STATE_PLAYING);
        assert_eq!(d.env.msg, ov::GAME_OVER);
        assert_eq!(d.env.draws.iter().filter(|x| x.anim == 34 && x.w == 64 && x.h == 33).count(), 2);
        d.set_demo(false);
        assert_eq!(d.env.msg, ov::GET_READY);
    }

    #[test]
    fn the_message_overlay_shows_game_over_and_the_cleared_message() {
        use crate::overlay as ov;
        let mut g = game(1);
        g.ships[0].lives = 2;
        let mut frames = 0;
        let mut seen_over = false;
        while !g.over && frames < 20000 {
            g.step([PlayerInput::default(); 2]);
            frames += 1;
            if g.env.msg == ov::GAME_OVER {
                seen_over = true;
                let n = g.env.draws.iter().filter(|x| x.anim == 34 && x.y == 206 && (x.x == 153 || x.x == 217)).count();
                assert_eq!(n, 2, "the two GAME OVER sprites are drawn every frame while the state is 3");
            }
        }
        assert!(g.over && seen_over, "the last ship's destruction sets state 3");
        // five rows before the end: state 4 (after sound 21)
        let mut g = Game::new_at(1, 26, vec![255; 26], 7, 3);
        g.targets_enabled = false;
        let mut cleared = false;
        for _ in 0..400 {
            g.step([PlayerInput::default(); 2]);
            if g.env.msg == ov::CLEARED { cleared = true; break; }
        }
        assert!(cleared);
        assert!(g.env.sounds.iter().any(|e| e.id == 21) || g.scroller.ending());
    }

    #[test]
    fn debug_pickups_are_collected_and_applied() {
        let mut g = game(1);
        g.targets_enabled = false;
        for _ in 0..70 { g.step([PlayerInput::default(); 2]); }
        let level = g.ships[0].level;
        let (x, y) = (g.ships[0].x + 24.0, g.ships[0].y + 24.0);
        g.debug_pickup_at(6, x, y);
        for _ in 0..30 { g.step([PlayerInput::default(); 2]); }
        assert_eq!(g.ships[0].level, level + 1, "weapon level pickup not applied");
    }

    #[test]
    fn invariants_hold_under_random_play_and_the_run_is_deterministic() {
        let run = |seed: u32| -> u64 {
            let mut g = Game::new(2, 150, vec![255; 150], seed);
            let mut r = Rng::new(seed ^ 0x9e37);
            let mut inp = [PlayerInput::default(); 2];
            for f in 0..6000 {
                if f % 7 == 0 {
                    for i in inp.iter_mut() {
                        let v = r.next_u32();
                        *i = PlayerInput { up: v & 1 != 0, down: v & 2 != 0, left: v & 4 != 0, right: v & 8 != 0, button1: v & 48 != 0, button2: false };
                    }
                }
                g.step(inp);
                for s in g.ships.iter().filter(|s| s.active && s.state == STATE_PLAYING) {
                    assert!(s.x >= 0.0 && s.x <= (SCREEN_W - s.w) as f32, "x out of range {}", s.x);
                    assert!(s.y >= 0.0 && s.y <= (SCREEN_H - s.h) as f32, "y out of range {}", s.y);
                    assert!(s.lives >= 0 && s.lives <= 13);
                    assert!(s.shield <= 12);
                }
                assert_eq!(g.shots.count as usize, g.shots.shots.iter().filter(|s| s.active).count());
                assert_eq!(g.pickups.count as usize, g.pickups.slots.iter().filter(|p| p.active).count());
                assert!(g.env.draws.len() < 400);
            }
            g.digest()
        };
        assert_eq!(run(1), run(1));
        assert_ne!(run(1), run(2));
    }

    #[test]
    fn rng_range_matches_the_original_scaling() {
        let mut r = Rng::new(7);
        for _ in 0..1000 {
            let v = r.range(3, 9);
            assert!((3..=9).contains(&v));
        }
    }

    fn with_sounds(g: &mut Game) {
        for id in 0..crate::audio::SOUND_COUNT {
            g.env.audio.set_sample(id, crate::audio::Sample::new(&vec![40i16; 2048], DEFAULT_OUT_RATE));
        }
    }

    #[test]
    fn every_frame_makes_740_samples_and_firing_is_audible() {
        let mut g = game(1);
        with_sounds(&mut g);
        let mut loud = false;
        let mut logged = false;
        for f in 0..200 {
            let tap = PlayerInput { button1: f % 12 < 2, ..Default::default() };
            g.step([tap, PlayerInput::default()]);
            assert_eq!(g.audio_out.len(), SAMPLES_PER_FRAME);
            loud |= g.audio_out.iter().any(|&b| b != 128);
            logged |= !g.env.sounds.is_empty();
        }
        assert!(logged && loud);
    }

    #[test]
    fn without_samples_the_game_is_silent_and_unchanged() {
        let run = |sounds: bool| {
            let mut g = game(1);
            if sounds { with_sounds(&mut g); }
            for f in 0..400 {
                let i = PlayerInput { button1: f % 20 < 10, left: (f / 40) % 2 == 0, right: (f / 40) % 2 == 1, ..Default::default() };
                g.step([i, PlayerInput::default()]);
                if !sounds { assert!(g.audio_out.iter().all(|&b| b == 128)); }
            }
            g.digest()
        };
        assert_eq!(run(false), run(true), "sound must not influence the game state");
    }

    #[test]
    fn overcharge_stream_end_re_arms_the_ship() {
        let mut g = game(1);
        with_sounds(&mut g);
        for _ in 0..70 { g.step([PlayerInput::default(); 2]); }
        let hold = PlayerInput { button1: true, ..Default::default() };
        let (mut started, mut rearmed) = (false, false);
        for _ in 0..110 {
            g.step([hold, PlayerInput::default()]);
            if !g.ships[0].stream_ready { started = true; }
            if started && g.ships[0].stream_ready { rearmed = true; }
        }
        assert!(started, "the over-charge stream (sound 2) never started");
        assert!(rearmed, "its end callback did not re-arm the ship");
    }

    #[test]
    fn releasing_a_charged_beam_stops_every_other_sound() {
        // the original asks "is handle 0 playing?" when no stream was started, which means "is anything playing?", and then stops all voices
        let mut g = game(1);
        with_sounds(&mut g);
        g.env.audio.set_sample(7, crate::audio::Sample::new(&vec![40i16; 60 * 1024], DEFAULT_OUT_RATE));
        for _ in 0..70 { g.step([PlayerInput::default(); 2]); }
        let other = g.env.audio.start(7, 1);
        assert!(g.env.audio.playing(other) > 0);
        let hold = PlayerInput { button1: true, ..Default::default() };
        for _ in 0..30 { g.step([hold, PlayerInput::default()]); }
        assert!(g.env.audio.playing(other) > 0 || g.ships[0].charge < 20, "still playing while charging");
        g.step([PlayerInput::default(); 2]);
        assert_eq!(g.env.audio.playing(other), 0);
    }

    fn enemy_game(ty: u32, count: u32) -> Game {
        let mut g = Game::new(1, 150, vec![0; 150], 777);
        let mut w = vec![0u32; 31];
        w[11] = ty;
        w[12] = count;
        w[16] = 3;
        w[17] = 3;
        g.load_groups(&[(0, w)]);
        g
    }

    #[test]
    fn real_enemies_spawn_are_drawn_and_can_be_shot_down() {
        for ty in [1u32, 5, 8, 15, 19] {
            let mut g = enemy_game(ty, 3);
            assert!(!g.targets_enabled, "debug targets must be off once a level's groups are loaded");
            for _ in 0..70 { g.step([PlayerInput::default(); 2]); }
            let (mut max_alive, mut drawn) = (0, false);
            for f in 0..4000 {
                let sweep = PlayerInput { button1: true, left: (f / 40) % 2 == 0, right: (f / 40) % 2 == 1, up: f % 300 < 100, ..Default::default() };
                g.step([sweep, PlayerInput::default()]);
                max_alive = max_alive.max(g.env.enemies.as_ref().unwrap().count());
                drawn |= g.env.draws.iter().any(|d| d.anim >= 119 && d.anim <= 159 && d.prim == crate::draw::Prim::Anim);
            }
            assert!(max_alive >= 3, "type {}: the group never appeared", ty);
            assert!(drawn, "type {}: no enemy sprite was drawn", ty);
            assert!(g.ships[0].score > 0, "type {}: no enemy was shot down", ty);
        }
    }

    #[test]
    fn enemies_that_nobody_shoots_hurt_the_ship() {
        let mut g = enemy_game(1, 5);
        for _ in 0..70 { g.step([PlayerInput::default(); 2]); }
        let (lives, shield) = (g.ships[0].lives, g.ships[0].shield);
        for _ in 0..3000 { g.step([PlayerInput::default(); 2]); }
        assert!(g.ships[0].lives < lives || g.ships[0].shield < shield || g.over, "the enemies never touched the ship");
    }

    #[test]
    fn shooting_enemies_fire_back_and_their_shots_are_drawn_and_can_hurt() {
        for ty in [2u32, 4, 7, 10, 15] {
            let mut g = enemy_game(ty, 4);
            for _ in 0..70 { g.step([PlayerInput::default(); 2]); }
            let (mut max_live, mut drawn, mut hurt) = (0, false, false);
            let (mut lives, mut shield) = (g.ships[0].lives, g.ships[0].shield);
            for _ in 0..3000 {
                g.step([PlayerInput::default(); 2]);
                max_live = max_live.max(g.env.eshots.as_ref().unwrap().live);
                drawn |= g.env.draws.iter().any(|d| [76, 77, 78, 79, 80, 126, 127, 156].contains(&d.anim));
                if g.ships[0].lives < lives || g.ships[0].shield < shield { hurt = true; }
                lives = g.ships[0].lives; shield = g.ships[0].shield;
            }
            assert!(max_live >= 1, "type {}: the enemies never fired", ty);
            assert!(drawn, "type {}: no enemy shot was drawn", ty);
            assert!(hurt || g.over, "type {}: nothing hurt the ship", ty);
        }
    }

    #[test]
    fn real_enemy_games_are_deterministic() {
        let run = || {
            let mut g = enemy_game(20, 2);
            for f in 0..2500 {
                let inp = PlayerInput { button1: f % 7 < 5, left: (f / 31) % 2 == 0, right: (f / 31) % 2 == 1, ..Default::default() };
                g.step([inp, PlayerInput::default()]);
            }
            g.digest()
        };
        assert_eq!(run(), run());
    }

    /// A level of 40 rows with one ground object of `kind` in the marker layer at (column, row).
    fn ground_game(kind: i32, col: usize, row: usize) -> Game {
        let mut g = Game::new(1, 40, vec![255; 40], 4242);
        let rec = crate::ground::resource_record(&[0, 1], [kind, 0, 1], (col * 32) as f32, row as f32, [-1, -1], &[0; 5]);
        let mut res = vec![0u8; crate::ground::RECORDS * crate::ground::RES_REC];
        res[..crate::ground::RES_REC].copy_from_slice(&rec);
        let mut marker = vec![255u8; 13 * 40];
        marker[row * 13 + col] = 0;
        g.load_ground(&res, 13, 40, marker);
        g
    }

    fn ground_y(g: &Game) -> Option<f32> {
        let gr = g.env.ground.as_ref()?;
        gr.list.iter().find(|&&i| i == 0).map(|_| gr.rf(0, crate::ground::o::Y))
    }

    #[test]
    fn a_ground_object_scrolls_in_with_its_row_and_is_drawn() {
        let mut g = ground_game(4, 3, 20);
        assert_eq!(g.env.ground.as_ref().unwrap().targets, 1);
        let mut first = None;
        for f in 0..300 {
            g.step([PlayerInput::default(); 2]);
            if first.is_none() && ground_y(&g).is_some() { first = Some(f); }
        }
        // row 20 is activated when the row counter (starting at 16) has completed row 19: 4 rows of 32 steps
        assert_eq!(first, Some(4 * 32 - 1));
        // it is moving down with the scroll and was drawn at its position
        let y = ground_y(&g).unwrap();
        assert!(y > -64.0 && y < 300.0, "{}", y);
    }

    /// The drop shadow darkens what is below it, so drawing it twice in one frame shows as a darker spot. The original copies its background
    /// buffer to the screen (0xC5B4) BEFORE the ground pass (0x3F554), so on the step where an object is burnt into the buffer it is drawn live
    /// only (the buffer shows it from the next step on): every step must draw the shadow and the sprite exactly once, and the object must move
    /// by exactly one pixel per step, also across the step where it becomes part of the background. Run for every object kind.
    #[test]
    fn a_ground_object_is_drawn_exactly_once_per_step_also_when_it_becomes_background() {
        let mut baked_kinds = 0;
        for kind in 0..=25 {
            let mut g = ground_game(kind, 3, 20);
            let mut last: Option<i32> = None;
            let mut baked = false;
            for f in 0..400 {
                g.step([PlayerInput::default(); 2]);
                let gr = g.env.ground.as_ref().unwrap();
                if !gr.list.iter().any(|&i| i == 0) { continue; }
                let (x, yf) = (gr.rf(0, crate::ground::o::X), gr.rf(0, crate::ground::o::Y));
                let (x, y) = (x as i32, yf as i32);
                baked |= gr.r8(0, crate::ground::o::F49) != 0;
                if gr.r8(0, crate::ground::o::B104) != 0 || y < 1 || y > 480 { last = None; continue; }
                let shadow = gr.r8(0, crate::ground::o::SHADOW) != 0;
                let sprites = g.env.draws.iter().filter(|d| d.prim == Prim::Anim && d.x == x && d.y == y).count();
                let shadows = g.env.draws.iter().filter(|d| d.prim == Prim::Cast && d.x == x - 6 && d.y == y + 8).count();
                assert_eq!(sprites, 1, "kind {} step {}: the sprite is drawn {} times at y {}", kind, f, sprites, y);
                assert_eq!(shadows, shadow as usize, "kind {} step {}: the shadow is drawn {} times at y {}", kind, f, shadows, y);
                // (objects with a velocity of their own move by more or less than the scroll)
                let still = gr.rf(0, crate::ground::o::VX) == 0.0 && gr.rf(0, crate::ground::o::VY) == 0.0;
                if let (Some(py), true) = (last, still) { assert_eq!(y - py, 1, "kind {} step {}: moved {} pixels", kind, f, y - py); }
                last = Some(y);
            }
            baked_kinds += baked as usize;
        }
        assert!(baked_kinds > 0, "no object ever became part of the background");
    }

    #[test]
    fn bombs_destroy_a_ground_object_and_the_last_one_pays_a_reward() {
        let mut g = ground_game(4, 3, 18);
        g.ships[0].hud_level = 5; // armor level 5: the reward for the last target is a big score item
        let mut score0 = None;
        let mut destroyed = false;
        for f in 0..900 {
            // aim the bomb sight at the turret whenever it is on the screen, hold button 2 and stay out of the way
            if let Some(y) = ground_y(&g) {
                if g.ships[0].state == STATE_PLAYING {
                    // bombs fly straight up from the ship's nose and explode at the sight's height
                    g.ships[0].x = (3 * 32 + 25) as f32 - g.ships[0].w as f32 / 2.0;
                    g.ships[0].trail_x = 3 * 32 + 25;
                    g.ships[0].trail_y = y as i32 + 25;
                }
            }
            let mut inp = PlayerInput::default();
            inp.button2 = f % 30 < 12; // a burst needs a fresh press
            g.step([inp, PlayerInput::default()]);
            if f == 70 { score0 = Some(g.ships[0].score); }
            if g.env.ground.as_ref().unwrap().destroyed > 0 { destroyed = true; break; }
        }
        assert!(destroyed, "the turret was never destroyed");
        assert!(g.ships[0].score > score0.unwrap());
        // the destroyed target was the last one: 0xea4c created its reward
        assert!(g.pickups.count >= 1, "no reward");
        // the object is gone from the active list and the wreck stays behind as a decal
        for _ in 0..3 { g.step([PlayerInput::default(); 2]); }
        assert!(ground_y(&g).is_none());
    }

    /// The explosion of a destroyed ground object is an effect from the pool, and the effect plays its own sound (0x22670 calls 0x45bac itself).
    #[test]
    fn destroying_a_ground_object_starts_effects_and_their_sounds() {
        let mut g = ground_game(4, 3, 18);
        let (mut fx_seen, mut snd_after, mut debris_seen, mut pixels_seen) = (0usize, 0usize, 0usize, 0usize);
        let mut destroyed_at = None;
        for f in 0..900 {
            if let Some(y) = ground_y(&g) {
                if g.ships[0].state == STATE_PLAYING {
                    g.ships[0].x = (3 * 32 + 25) as f32 - g.ships[0].w as f32 / 2.0;
                    g.ships[0].trail_x = 3 * 32 + 25;
                    g.ships[0].trail_y = y as i32 + 25;
                }
            }
            let mut inp = PlayerInput::default();
            inp.button2 = f % 30 < 12;
            g.step([inp, PlayerInput::default()]);
            if destroyed_at.is_none() && g.env.ground.as_ref().unwrap().destroyed > 0 { destroyed_at = Some(f); }
            if destroyed_at.is_some() {
                fx_seen = fx_seen.max(g.env.fx.as_ref().unwrap().active());
                debris_seen = debris_seen.max(g.env.debris.as_ref().unwrap().active());
                pixels_seen += g.env.draws.iter().filter(|d| d.prim == Prim::Pixels).count();
                snd_after += g.env.sounds.iter().filter(|e| e.priority == 4).count();
            }
            if destroyed_at.map_or(false, |d| f > d + 20) { break; }
        }
        assert!(destroyed_at.is_some(), "the turret was never destroyed");
        assert!(fx_seen > 0, "no effect was started");
        assert!(snd_after > 0, "the effects played no sound");
        assert!(debris_seen > 0, "no debris was started");
        assert!(pixels_seen > 0, "the debris drew nothing");
        // the sparks loop until they have flown off the screen; then nothing is left behind
        for _ in 0..600 {
            if g.env.fx.as_ref().unwrap().active() == 0 { break; }
            g.step([PlayerInput::default(); 2]);
        }
        assert_eq!(g.env.fx.as_ref().unwrap().active(), 0);
    }

    #[test]
    fn debris_plays_its_sound_once_draws_blocks_in_its_colour_and_ends() {
        let mut g = game(1);
        let mut colours = vec![0u8; 130];
        for (i, c) in colours.iter_mut().enumerate() { *c = 100 + (i % 10) as u8 + 10 * (i / 10) as u8; }
        g.set_particle_colours(&colours);
        for _ in 0..100 { g.env.debris_update(); } // let the ship's own appearance (ramp 9, three implosions) run out first
        assert_eq!(g.env.debris.as_ref().unwrap().active(), 0);
        g.env.sounds.clear();
        g.env.debris_spawn(200, 240, 2, 0, 11, 0); // now: the spawner plays sound 11 at priority 4
        assert_eq!(g.env.sounds.iter().filter(|e| e.id == 11 && e.priority == 4).count(), 1);
        g.env.debris_spawn(200, 240, 2, 3, 12, 0); // delayed: silent until the update pass starts it
        assert!(g.env.sounds.iter().all(|e| e.id != 12));
        let mut blocks = 0;
        let mut later = 0;
        for _ in 0..60 {
            g.env.sounds.clear();
            g.env.draws.clear();
            g.env.debris_update();
            later += g.env.sounds.iter().filter(|e| e.id == 12 && e.priority == 4).count();
            for d in g.env.draws.iter().filter(|d| d.prim == Prim::Pixels) {
                blocks += 1;
                assert_eq!((d.w, d.h), (3, 3));
                assert!((120..130).contains(&d.anim), "colour {} is not from ramp 2", d.anim);
            }
            if g.env.debris.as_ref().unwrap().active() == 0 { break; }
        }
        assert_eq!(later, 1, "the delayed fragment burst must play its sound exactly once");
        assert!(blocks > 40, "only {blocks} blocks drawn");
        assert_eq!(g.env.debris.as_ref().unwrap().active(), 0, "the bursts end after 40 steps");
    }

    #[test]
    fn the_level_ends_with_the_coin_bonus_and_the_next_level_keeps_the_ships() {
        let mut g = Game::new_at(1, 26, vec![255; 26], 7, 3);
        g.targets_enabled = false;
        assert_eq!(g.level_number(), 3);
        for _ in 0..100 { g.step([PlayerInput::default(); 2]); }
        assert_eq!(g.ships[0].state, STATE_PLAYING);
        g.ships[0].coins = 4;
        g.ships[0].spread = 3;
        let score0 = g.ships[0].score;
        // a coin lying around when the bonus ends turns into an explosion
        g.debug_pickup_at(2, 200.0, 100.0);
        let (mut s21, mut s34, mut s54, mut first_21, mut complete_at) = (0, 0, 0, None, None);
        for f in 0..1200 {
            g.step([PlayerInput::default(); 2]);
            for e in &g.env.sounds {
                match (e.id, e.priority) {
                    (21, 5) => { s21 += 1; first_21.get_or_insert(f); }
                    (34, 4) => s34 += 1,
                    (54, 4) => s54 += 1,
                    _ => {}
                }
            }
            if g.level_complete { complete_at = Some(f); break; }
        }
        let done = complete_at.expect("the level never completed");
        assert!(done > 160 && done < 400, "completed at step {done}");
        assert_eq!(s21, 1, "sound 21 plays once at the near-end event (the delayed one of the tally has priority 4)");
        assert_eq!(s34, 4, "one sound per coin");
        assert!(s54 >= 2, "54 plays when the bonus panel opens and when it is paid");
        // 4 coins x min(3 * 100, 1000) points, times the armor level
        let armor = g.ships[0].hud_level as i32;
        assert_eq!(g.ships[0].score as i32 - score0 as i32, 4 * 300 * armor);
        assert_eq!(g.tally.p1_count, 4);
        assert!(g.pickups.slots.iter().all(|p| !p.active || p.kind != 2), "the coin on the screen should have exploded");
        // the next level: numbering, score, lives and weapons stay; the level itself starts over
        let (lives, score) = (g.ships[0].lives, g.ships[0].score);
        g.next_level(30, vec![255; 30]);
        assert_eq!(g.level_number(), 4);
        assert!(!g.level_complete && !g.level_finished);
        assert_eq!(g.ships[0].score, score);
        assert_eq!(g.ships[0].lives, lives + 1, "0x23de4 adds the life that the entering ship uses up again");
        assert_eq!(g.ships[0].spread, 3);
        assert_eq!(g.ships[0].coins, 0);
        assert_eq!(g.scroller.row(), crate::scroll::START_ROW);
        assert_eq!(g.now, 0);
        // 0xdae0..0xdb08: the new level starts with "Sector N / Get Ready!", not with the "Sector N Cleared" of the level just won
        assert_eq!(g.env.msg, crate::overlay::GET_READY);
        for _ in 0..100 { g.step([PlayerInput::default(); 2]); }
        assert_eq!(g.ships[0].state, STATE_PLAYING);
        assert_eq!(g.ships[0].lives, lives);
        assert!(!g.level_complete);
    }

    /// A level whose spawn row 0 launches the special group 5 (a boss of type `ty`).
    fn boss_game(ty: u32) -> Game {
        let mut g = Game::new(1, 150, vec![5; 150], 4242);
        let mut w = vec![0u32; 31];
        w[0] = 0x0100_0000;
        w[11] = ty;
        w[12] = 4;
        w[16] = 3;
        w[17] = 2; // 1 hit point + level * 10
        w[19] = 3;
        w[20] = 5;
        w[22] = 1;
        g.load_groups(&[(5, w)]);
        g
    }

    #[test]
    fn bosses_appear_loop_their_alarm_are_drawn_and_can_be_destroyed() {
        for ty in [2u32, 4, 5] {
            let mut g = boss_game(ty);
            let (mut seen, mut drawn, mut alarm, mut pickups) = (0, false, 0usize, 0usize);
            for f in 0..3000 {
                // keep the ship under the boss and shoot
                let sweep = PlayerInput { button1: true, left: (f / 50) % 2 == 0, right: (f / 50) % 2 == 1, ..Default::default() };
                g.step([sweep, PlayerInput::default()]);
                let en = g.env.enemies.as_ref().unwrap();
                seen = seen.max(en.bosses.count);
                drawn |= g.env.draws.iter().any(|d| d.prim == Prim::Anim && (124..=125).contains(&d.anim) || d.anim == 145 || d.anim == 147);
                alarm += g.env.sounds.iter().filter(|e| e.priority == 4 && (e.id == 12 || e.id == 57 || e.id == 14)).count();
                pickups += g.env.pickup_events.len();
                if seen > 0 && en.bosses.count == 0 { break; }
            }
            assert!(seen >= 1, "type {}: no boss appeared", ty);
            assert!(drawn, "type {}: no boss sprite was drawn", ty);
            assert!(alarm > 0, "type {}: the alarm never played", ty);
            assert_eq!(g.env.enemies.as_ref().unwrap().bosses.count, 0, "type {}: the boss was not destroyed", ty);
            assert_eq!(g.env.enemies.as_ref().unwrap().count(), 0, "type {}: parts left behind", ty);
            let _ = pickups;
        }
    }

    #[test]
    fn drones_dock_fire_and_missiles_fly() {
        let mut g = Game::new(1, 40, vec![255; 40], 4242);
        assert!(World::drone_add(&mut g.env, 1, 1));
        g.ships[0].drones = 1;
        g.ships[0].drone_a = true;
        g.ships[0].missiles = 3;
        g.ships[0].level = 3;
        let (mut docked, mut flash, mut shots3, mut anim21, mut anim45, mut maxm) = (false, false, false, false, false, 0);
        for f in 0..160 {
            let inp = PlayerInput { button1: true, left: (f / 40) % 2 == 0, right: (f / 40) % 2 == 1, ..Default::default() };
            g.step([inp, PlayerInput::default()]);
            let d = g.env.drones.as_ref().unwrap();
            docked |= d.bytes()[1] == 2;
            anim21 |= g.env.draws.iter().any(|x| x.anim == 21);
            anim45 |= g.env.draws.iter().any(|x| x.anim == 45);
            flash |= g.env.draws.iter().any(|x| x.anim == 49);
            shots3 |= g.shots.shots.iter().any(|s| s.active && s.kind == 3);
            maxm = maxm.max(g.env.missiles.as_ref().unwrap().count);
        }
        assert!(docked, "the drone never docked");
        assert!(anim21, "the drone was never drawn");
        assert!(flash, "no muzzle flash");
        assert!(shots3, "the drone never fired");
        assert!(maxm > 0 && anim45, "no missiles flew");
    }

    #[test]
    fn hud_follows_the_game() {
        use crate::hud::{HudCall, Port};
        let mut g = Game::new(1, 200, vec![0; 200], 5);
        let (epoch, calls) = g.take_hud_calls();
        // every game has epochs of its own: a second game must never look like the first one's panel (the host would draw on top of the old panel)
        assert_ne!(epoch, 0);
        let other = Game::new(1, 200, vec![0; 200], 5);
        assert_ne!(other.env.hud_epoch >> 16, epoch >> 16, "two games share an epoch range");
        let first_epoch = epoch;
        // init (stores), the forced score + lives + shield of both ships, the dimming of the absent player 2, the shield target of player 1
        assert_eq!(calls[0], HudCall::Port(Port::Store));
        assert_eq!(calls.iter().filter(|c| matches!(c, HudCall::Store(..))).count(), 12);
        assert_eq!(calls.iter().filter(|c| matches!(c, HudCall::Paint(_))).count(), 2, "one flash over player 2's panel");
        assert_eq!(g.ships[1].lives, 1, "0x23c04: the absent ship 2 has one life");
        assert!(!calls.iter().any(|c| matches!(c, HudCall::Blit { anim: 27, .. })), "no spare ships are shown for the absent player (original screenshot)");
        assert_eq!(calls.iter().filter(|c| matches!(c, HudCall::Blit { anim: 26, .. })).count(), 3, "player 1 starts with three ships shown");
        let presents = |v: &[HudCall]| v.iter().filter_map(|c| if let HudCall::Present(_, r) = c { Some(*r) } else { None }).collect::<Vec<_>>();
        let first = presents(&calls);
        assert!(first.iter().any(|r| r.top == 95), "score of player 1 shown");
        assert!(first.iter().any(|r| r.top == 328), "score of player 2 shown");
        // a bonus: the digits are redrawn (and flash)
        g.ships[0].add_score(250, true, g.now, &mut g.env);
        g.step([PlayerInput::default(), PlayerInput::default()]);
        let (_, calls) = g.take_hud_calls();
        assert!(presents(&calls).iter().any(|r| r.top == 95), "the new score is drawn");
        assert!(!presents(&calls).iter().any(|r| r.top == 328), "player 2 is not touched");
        // holding the fire button charges the beam: the power bar of player 1 grows (blits at its y)
        for _ in 0..150 { g.step([PlayerInput { button1: true, ..Default::default() }, PlayerInput::default()]); }
        let (_, calls) = g.take_hud_calls();
        assert!(presents(&calls).iter().any(|r| r.top == 149), "power bar of player 1");
        // a new level starts the panel over
        g.next_level(200, vec![0; 200]);
        let (epoch, calls) = g.take_hud_calls();
        assert_eq!(epoch, first_epoch + 1);
        assert_eq!(calls[0], HudCall::Port(Port::Store));
    }

    #[test]
    fn ground_games_are_deterministic() {
        let run = || {
            let mut g = ground_game(4, 5, 19);
            for f in 0..400 {
                let mut inp = PlayerInput::default();
                inp.button2 = f % 7 < 3;
                inp.button1 = f % 5 < 2;
                inp.left = f % 40 < 10;
                g.step([inp, PlayerInput::default()]);
            }
            g.digest()
        };
        assert_eq!(run(), run());
    }
}
