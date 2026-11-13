//! Gamepad -> player input (an addition to the original, which read InputSprocket axes and buttons; no original code is involved).
//!
//! The ship is steered with eight digital directions (the original turns an axis into a "low" / "high" flag, bytes +96..+99 of the player
//! struct), so the analogue stick is turned into the same four flags with a small hysteresis: a direction switches on at 50 % of the stick's
//! travel and off again below 35 %, which keeps a stick resting near the threshold from flickering. The D-pad works too, and both are OR-ed.
use crate::input::PlayerInput;

/// What a pad reports at one moment (already read from the device by the window code).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct RawPad {
    /// Left stick, -1.0 .. 1.0; y is negative when the stick is pushed up (raylib's convention).
    pub lx: f32,
    pub ly: f32,
    pub dpad_up: bool,
    pub dpad_down: bool,
    pub dpad_left: bool,
    pub dpad_right: bool,
    /// Any of the fire buttons (south face button, right shoulder / trigger).
    pub fire: bool,
    /// Any of the second-weapon buttons (east / west face button, left shoulder / trigger).
    pub alt: bool,
}

pub const ENGAGE: f32 = 0.5;
pub const RELEASE: f32 = 0.35;

/// Per-pad memory for the stick hysteresis.
#[derive(Debug, Default, Clone, Copy)]
pub struct StickMapper {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
}

fn axis(neg: &mut bool, pos: &mut bool, v: f32) {
    let v = if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
    *neg = if *neg { v < -RELEASE } else { v < -ENGAGE };
    *pos = if *pos { v > RELEASE } else { v > ENGAGE };
}

impl StickMapper {
    pub fn map(&mut self, raw: &RawPad) -> PlayerInput {
        axis(&mut self.left, &mut self.right, raw.lx);
        axis(&mut self.up, &mut self.down, raw.ly);
        PlayerInput {
            up: self.up || raw.dpad_up,
            down: self.down || raw.dpad_down,
            left: self.left || raw.dpad_left,
            right: self.right || raw.dpad_right,
            button1: raw.fire,
            button2: raw.alt,
        }
    }
}

/// OR of two inputs (keyboard + pad).
pub fn merge(a: PlayerInput, b: PlayerInput) -> PlayerInput {
    PlayerInput {
        up: a.up || b.up,
        down: a.down || b.down,
        left: a.left || b.left,
        right: a.right || b.right,
        button1: a.button1 || b.button1,
        button2: a.button2 || b.button2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_has_hysteresis() {
        let mut m = StickMapper::default();
        let mut p = RawPad::default();
        p.lx = 0.45;
        assert!(!m.map(&p).right, "below the engage point");
        p.lx = 0.55;
        assert!(m.map(&p).right);
        p.lx = 0.40;
        assert!(m.map(&p).right, "still on between release and engage");
        p.lx = 0.30;
        assert!(!m.map(&p).right);
        p.lx = -0.9;
        let i = m.map(&p);
        assert!(i.left && !i.right);
    }

    #[test]
    fn up_is_negative_y_and_diagonals_work() {
        let mut m = StickMapper::default();
        let i = m.map(&RawPad { lx: 0.8, ly: -0.8, ..Default::default() });
        assert!(i.up && i.right && !i.down && !i.left);
        let i = m.map(&RawPad { lx: -0.8, ly: 0.8, ..Default::default() });
        assert!(i.down && i.left && !i.up && !i.right);
    }

    #[test]
    fn dpad_buttons_and_merge() {
        let mut m = StickMapper::default();
        let i = m.map(&RawPad { dpad_left: true, fire: true, alt: true, ..Default::default() });
        assert!(i.left && i.button1 && i.button2 && !i.right);
        let k = PlayerInput { up: true, ..Default::default() };
        let j = merge(i, k);
        assert!(j.up && j.left && j.button1 && j.button2);
        assert_eq!(merge(PlayerInput::default(), PlayerInput::default()), PlayerInput::default());
    }

    #[test]
    fn garbage_axis_values_are_harmless() {
        let mut m = StickMapper::default();
        let i = m.map(&RawPad { lx: f32::NAN, ly: f32::INFINITY, ..Default::default() });
        assert_eq!(i, PlayerInput::default(), "non-finite readings count as centred");
        let i = m.map(&RawPad { lx: 7.0, ly: -3.0, ..Default::default() });
        assert!(i.right && i.up, "out-of-range readings are clamped to full travel");
    }
}
