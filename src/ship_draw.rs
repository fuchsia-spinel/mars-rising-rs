//! Per-frame draw step of a ship (0x257b4), the wave beam (0x275c8) and the shot pool (0x2242c).
//!
//! 0x257b4 is both the ship's draw routine and the place where the ship's cosmetic timers run (hud refresh flag, muzzle flashes, charge glow,
//! special aura, invulnerability blink, hit flash, low-shield warning, shield ring), so it mutates the ship. Verified against the original by
//! `tests/draw_golden.rs` (every draw call and every changed field).
use crate::draw::{emit, DrawOpts, Prim};
use crate::ship::{Ship, ShotPool, World, SHOT_SLOTS, STATE_PLAYING};

impl Ship {
    /// 0x260b8: the bomb-sight marker at the trail position (anim 22, or 25 for the second input set; anim 23 while it is over a ground target).
    fn draw_sight<W: World>(&self, world: &mut W) {
        let anim = if self.input_set == 2 { 25 } else { 22 };
        let (tx, ty) = (self.trail_x, self.trail_y);
        let hit = world.ground_target(tx + 4, ty + 4, tx + 20 - 4, ty + 20 - 4, self.number);
        emit(world, Prim::Anim, tx, ty, 20, 20, if hit { 23 } else { anim }, 1);
    }

    /// 0x257b4: advance the cosmetic timers and emit the draw calls for one ship.
    pub fn draw_step<W: World>(&mut self, now: u32, opts: DrawOpts, world: &mut W) {
        if self.flag_72 && now > self.tick_76.wrapping_add(3) {
            world.hud_lives(self.input_set, self.lives, 0);
            self.flag_72 = false;
        }
        if self.state != STATE_PLAYING { return; }
        let tr = opts.trans_prim();
        self.draw_sight(world);

        // muzzle flashes of the shot just fired
        if self.last_shot == now {
            let (xi, yi) = (self.x as i32, self.y as i32);
            if self.pattern >= 1 {
                emit(world, tr, xi + (self.hit_w - 7), yi - 4, 14, 14, 49, 1);
            }
            if self.pattern == 3 {
                emit(world, tr, xi + 5, yi + 18, 14, 14, 49, 1);
                emit(world, tr, xi + 28, yi + 18, 14, 14, 49, 1);
            }
            if self.rear {
                emit(world, tr, xi + (self.hit_w - 7), yi + (self.h - 6), 14, 14, 49, 1);
            }
            self.pattern = 0;
        }
        if opts.shadows {
            emit(world, Prim::Shadow, (self.x as i32) - 24, (self.y as i32) + 44, self.w, self.h, self.sprite_id, self.frame);
        }

        // charge glow in front of the nose
        if self.charge_fx {
            if now > self.fx_tick_a.wrapping_add(3) {
                self.fx_a += 1;
                self.fx_tick_a = now;
                if self.fx_a > 5 { self.fx_a = 1; }
            }
            emit(world, Prim::Anim, (self.x as i32) + 7, (self.y as i32) - 22, 34, 22, 28, self.fx_a);
        }
        // charge aura: grows while charging (frames 1..5), shrinks again afterwards
        if self.charge_fx || self.fx_b {
            if !self.fx_b {
                if self.fx_c != 5 && now > self.fx_tick_b.wrapping_add(1) {
                    self.fx_tick_b = now;
                    self.fx_c += 1;
                    if self.fx_c > 5 { self.fx_c = 5; }
                }
            } else {
                self.fx_c -= 1;
                if self.fx_c < 1 {
                    self.fx_c = 1;
                    self.fx_b = false;
                }
            }
            if self.charge_fx || self.fx_b {
                emit(world, tr, self.x as i32, (self.y as i32) - 22, 48, 48, 113, self.fx_c);
            }
        }

        if self.invuln && now > self.blink_tick.wrapping_add(2) {
            self.blink = !self.blink;
            self.blink_tick = now;
        }
        if self.hit_flash && now > self.hit_flash_tick.wrapping_add(5) {
            self.hit_flash = false;
        }
        let (xi, yi) = (self.x as i32, self.y as i32);
        emit(world, Prim::Anim, xi, yi, self.w, self.h, self.sprite_id, self.frame);
        if self.blink || self.hit_flash {
            emit(world, Prim::Flash, xi, yi, self.w, self.h, self.sprite_id, self.frame);
        }
        if self.hit_flash {
            emit(world, Prim::Flash, xi, yi, self.w, self.h, self.sprite_id, self.frame);
        }

        // low-shield warning ("SHIELDS DOWN", blinks for 80 ticks)
        if self.warn {
            if now >= self.warn_tick.wrapping_add(80) {
                self.warn = false;
            } else if now >= self.warn_tick {
                self.warn_n += 1;
                if self.warn_n > 8 {
                    self.warn_n = 0;
                    self.warn_blink = !self.warn_blink;
                    if self.warn_blink {
                        if self.input_set == 1 { world.sound(69, 4, 0); } else { world.sound_pan(69, 4, 36864); }
                    }
                }
                if self.warn_blink {
                    let anim = if self.input_set == 1 { 19 } else { 20 };
                    emit(world, Prim::Anim, (4.0f32 + self.x) as i32, (-35.0f32 + self.y) as i32, 39, 28, anim, 1);
                }
            }
        }

        // shield ring after a hit
        if self.shield_fx {
            if now > self.shield_tick.wrapping_add(1) {
                if !self.shield_dir {
                    self.shield_frame += 1;
                    if self.shield_frame > 5 {
                        self.shield_frame = 1;
                        self.shield_dir = true;
                    }
                } else {
                    self.shield_frame -= 1;
                    if self.shield_frame < 1 { self.shield_fx = false; }
                }
                self.shield_tick = now;
            }
            if self.shield_fx {
                emit(world, tr, (self.x as i32) - 8, (self.y as i32) - 7, 64, 64, 18, self.shield_frame);
            }
        }
    }

    /// 0x275c8: the wave beam, a column of 32-pixel segments.
    pub fn draw_beam<W: World>(&self, world: &mut W) {
        if self.state != STATE_PLAYING || !self.beam.active || self.beam.delay > 0 { return; }
        let b = &self.beam;
        let mut i = 0;
        while i < b.pat {
            emit(world, Prim::Anim, b.x, b.y + 32 * i, b.w, 32, b.anim, i + 1);
            i += 1;
        }
    }
}

impl ShotPool {
    /// 0x2242c: draw every live shot at its integer position.
    pub fn draw<W: World>(&self, world: &mut W) {
        if self.count <= 0 { return; }
        for s in self.shots.iter().take(SHOT_SLOTS) {
            if s.active {
                emit(world, Prim::Anim, s.x as i32, s.y as i32, s.w, s.h, s.sprite, s.frame);
            }
        }
    }
}
