//! Draw commands: what the original's per-object draw routines ask the sprite blitters to do (std-only, so the draw logic can be
//! checked against the original executable like the rest of the game logic).
//!
//! All coordinates are playfield pixels (416 x 480). `anim` is the 1-based entry of the sprite animation table (set 1000, see
//! `sprites/sets.json`) and `frame` the 1-based frame inside it; `frame <= 0` draws nothing (the blitters ignore it).

/// The original blitter entry points, named after their code addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prim {
    /// 0x2f88: plain masked sprite.
    Anim,
    /// 0x3170: 50 % translucent sprite (blend table `TRAN`), used when the "translucency" option (`D.12be`) is on.
    Trans,
    /// 0x3264: checkerboard-dithered sprite, the fallback of `Trans` when the option is off.
    Dither,
    /// 0x3540: the sprite's pixels recoloured through the orange DARK 129 table (hit flash / invulnerability blink).
    Flash,
    /// 0x3358 (blitter 0x444c): the full-size drop shadow of a ground object: every opaque pixel of the sprite recolours the pixel below it through DARK 128.
    Cast,
    /// 0x344c: drop shadow; the sprite squashed to 2/3 x 3/4 of its size, the pixels below recoloured through DARK 128 "System Shadow".
    Shadow,
    /// 0x29b10: a 3 x 3 block of palette index `anim` (a fragment of the auxiliary debris effect, `debris.rs`); `w`, `h` are 3, `frame` 1.
    Pixels,
}

impl Prim {
    /// Code address of the original routine (used in the test logs).
    pub fn addr(self) -> u32 {
        match self { Prim::Anim => 0x2f88, Prim::Trans => 0x3170, Prim::Dither => 0x3264, Prim::Flash => 0x3540, Prim::Shadow => 0x344c, Prim::Cast => 0x3358, Prim::Pixels => 0x29b10 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Draw {
    pub prim: Prim,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub anim: i32,
    pub frame: i32,
}

/// The two display options the draw routines read (`D.12be`, `D.12bc`; 0x29a4 / 0x2994 are their getters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawOpts {
    /// `D.12be`: translucent effects use the 50 % blend instead of the dither fallback; pickup auras are only drawn when it is on.
    pub translucent: bool,
    /// `D.12bc`: drop shadows under ships and pickups.
    pub shadows: bool,
}

impl Default for DrawOpts {
    fn default() -> Self { DrawOpts { translucent: true, shadows: true } }
}

impl DrawOpts {
    /// The primitive used for translucent effects.
    pub fn trans_prim(self) -> Prim { if self.translucent { Prim::Trans } else { Prim::Dither } }
}

/// Send one blit request to the world.
pub fn emit<W: crate::ship::World>(world: &mut W, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
    world.draw(Draw { prim, x, y, w, h, anim, frame });
}

/// Collects draw commands in call order (a ready-made sink for `World::draw` implementations).
#[derive(Debug, Default)]
pub struct DrawList(pub Vec<Draw>);

impl DrawList {
    pub fn new() -> Self { Self::default() }
    pub fn push(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.0.push(Draw { prim, x, y, w, h, anim, frame });
    }
}
