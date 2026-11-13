//! Gamepads through raylib: up to four pads are polled; connected pads are handed out to the players in order (first connected pad = player 1,
//! the second = player 2). Layout: left stick or D-pad moves, south face button (A / cross) or right shoulder / trigger fires (hold = charge),
//! east / west face button (B, X) or left shoulder / trigger = button 2; Start = pause (restart on the game-over screen).
use mars_rising::gamepad::{RawPad, StickMapper};
use mars_rising::input::PlayerInput;
use raylib::prelude::{GamepadAxis, GamepadButton, RaylibHandle};

const MAX_PADS: i32 = 4;

#[derive(Default)]
pub struct Pads {
    maps: [StickMapper; 2],
}

fn read(win: &RaylibHandle, id: i32) -> RawPad {
    let b = |btn: GamepadButton| win.is_gamepad_button_down(id, btn);
    RawPad {
        lx: win.get_gamepad_axis_movement(id, GamepadAxis::GAMEPAD_AXIS_LEFT_X),
        ly: win.get_gamepad_axis_movement(id, GamepadAxis::GAMEPAD_AXIS_LEFT_Y),
        dpad_up: b(GamepadButton::GAMEPAD_BUTTON_LEFT_FACE_UP),
        dpad_down: b(GamepadButton::GAMEPAD_BUTTON_LEFT_FACE_DOWN),
        dpad_left: b(GamepadButton::GAMEPAD_BUTTON_LEFT_FACE_LEFT),
        dpad_right: b(GamepadButton::GAMEPAD_BUTTON_LEFT_FACE_RIGHT),
        fire: b(GamepadButton::GAMEPAD_BUTTON_RIGHT_FACE_DOWN) || b(GamepadButton::GAMEPAD_BUTTON_RIGHT_TRIGGER_1) || b(GamepadButton::GAMEPAD_BUTTON_RIGHT_TRIGGER_2),
        alt: b(GamepadButton::GAMEPAD_BUTTON_RIGHT_FACE_RIGHT) || b(GamepadButton::GAMEPAD_BUTTON_RIGHT_FACE_LEFT)
            || b(GamepadButton::GAMEPAD_BUTTON_LEFT_TRIGGER_1) || b(GamepadButton::GAMEPAD_BUTTON_LEFT_TRIGGER_2),
    }
}

/// Ids of the connected pads, in order.
pub fn connected(win: &RaylibHandle) -> Vec<i32> {
    (0..MAX_PADS).filter(|&i| win.is_gamepad_available(i)).collect()
}

impl Pads {
    /// Input of player `n` (0 or 1) from the pad it owns (nothing when there is none).
    pub fn input(&mut self, win: &RaylibHandle, n: usize) -> PlayerInput {
        match connected(win).get(n) {
            Some(&id) => self.maps[n].map(&read(win, id)),
            None => PlayerInput::default(),
        }
    }

    /// Start pressed on any pad this frame.
    pub fn start_pressed(win: &RaylibHandle) -> bool {
        connected(win).into_iter().any(|id| win.is_gamepad_button_pressed(id, GamepadButton::GAMEPAD_BUTTON_MIDDLE_RIGHT))
    }
}
