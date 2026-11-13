//! 0x2e118: the enemy draw routine, which also releases enemies that have left the screen area. Two passes over the 40 slots:
//!  1. culling (an enemy whose box leaves x -128..544 / y -128..608 is dropped like a kill without score: its group's `alive` count falls,
//!     the group ends when it reaches 0, and the slot is cleared by 0x2d98c) and, with the shadow option, the drop shadows;
//!  2. the sprites: muzzle flash (+136), the enemy itself (anim +80, frame +84), the hit flash (+105), the type-7 extra sprite (+148) and the
//!     type-20 overlay, and the hit flag is cleared two ticks after the hit.
//! Slots that are bosses (+168 != 0) are skipped here (their parts are drawn by the boss code); slots still waiting to start (+20 > 0) are
//! not drawn at all.
use crate::draw::{DrawOpts, Prim};
use crate::enemy::{f, f2i, g, Enemies, SLOTS};

/// What the draw routine needs from the rest of the game.
pub trait EnemyDrawWorld {
    fn tick(&self) -> u32;
    fn random(&mut self, lo: i32, hi: i32) -> i32;
    fn opts(&self) -> DrawOpts;
    /// A blit request (`group` is always 0, the enemy sprite set).
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32);
}

impl Enemies {
    pub fn draw<W: EnemyDrawWorld>(&mut self, w: &mut W) {
        let tick = w.tick();
        let opts = w.opts();
        for i in 0..SLOTS {
            if self.r8(i, f::ACTIVE) == 0 || self.r8(i, 168) != 0 || self.ri32(i, f::DELAY) > 0 {
                continue;
            }
            let (x, y) = (self.rf(i, f::X), self.rf(i, f::Y));
            let (wd, ht) = (self.ri32(i, f::W), self.ri32(i, f::H));
            let out = x < -128.0 || x + wd as f32 > 544.0 || y < -128.0 || y + ht as f32 > 608.0;
            if out {
                let gi = self.r32(i, f::GROUP) as usize;
                let alive = self.gri32(gi, g::ALIVE).wrapping_sub(1);
                self.gw32(gi, g::ALIVE, alive as u32);
                if alive == 0 {
                    self.gw8(gi, g::ACTIVE, 0);
                    self.live_groups -= 1;
                }
                self.clear_slot(i);
            } else if opts.shadows {
                w.blit(Prim::Shadow, f2i(x) - 24, f2i(y) + 44, wd, ht, self.ri32(i, f::ANIM), self.ri32(i, f::STATE));
            }
        }
        for i in 0..SLOTS {
            if self.r8(i, f::ACTIVE) == 0 {
                continue;
            }
            if self.r8(i, 168) == 0 {
                if self.ri32(i, f::DELAY) > 0 {
                    continue;
                }
                if self.r32(i, f::F136) != 0 {
                    let (mx, my) = (self.rf(i, 140), self.rf(i, 144));
                    w.blit(Prim::Anim, f2i(mx), f2i(my), 20, 20, 50, 1);
                    self.w32(i, f::F136, 0);
                }
                let (x, y) = (self.rf(i, f::X), self.rf(i, f::Y));
                let (wd, ht) = (self.ri32(i, f::W), self.ri32(i, f::H));
                let (anim, frame) = (self.ri32(i, f::ANIM), self.ri32(i, f::STATE));
                w.blit(Prim::Anim, f2i(x), f2i(y), wd, ht, anim, frame);
                if self.r8(i, f::HIT) != 0 {
                    w.blit(Prim::Flash, f2i(x), f2i(y), wd, ht, anim, frame);
                    w.blit(Prim::Flash, f2i(x), f2i(y), wd, ht, anim, frame);
                }
                if self.r8(i, f::B148) != 0 {
                    let ex = f2i((f2i(x) as f32) + self.rf(i, 160));
                    let ey = f2i((f2i(y) as f32) + self.rf(i, 164));
                    w.blit(opts.trans_prim(), ex, ey, 38, 38, self.ri32(i, 152), self.ri32(i, 156));
                }
                if self.ri32(i, f::TYPE) == 20 {
                    let r = w.random(1, 4);
                    let prim = if opts.translucent { Prim::Trans } else { Prim::Anim };
                    w.blit(prim, f2i(x) + 11, f2i(y) + 11, 42, 42, 154, r);
                }
            }
            if self.r8(i, f::HIT) != 0 && tick > self.r32(i, f::HIT_TICK).wrapping_add(1) {
                self.w8(i, f::HIT, 0);
            }
        }
    }
}
