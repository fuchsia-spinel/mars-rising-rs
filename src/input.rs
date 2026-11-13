//! Player input model and the original's default keyboard bindings.
//!
//! Recovered from the original executable and its `tset`/`setl` resources (see RE_NOTES.md): the game reads InputSprocket
//! elements (two axes + two buttons per player) and turns them into six per-frame flags in the player struct
//! (bytes +96..+101). Default keyboard bindings are Mac virtual key codes stored as `code << 16` in the `tset` resources.

/// The six input flags of a player, in the original's struct order (+96 .. +101).
/// Verified in the disassembly: the keyboard/InputSprocket poll (0x8fec) turns an axis value below 0x3000_0000 into the *low-end* flag and above
/// 0xC000_0000 into the *high-end* flag; X low = left (+98), X high = right (+99), Y low = down (+97), Y high = up (+96). The tset key order
/// left, right, down, up therefore matches, and the movement routine 0x25264 confirms +96 decreases vy (up) and +97 increases it (down).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PlayerInput {
    pub up: bool,      // +96  vy -= 2.5 (screen up; verified in the movement routine 0x25264)
    pub down: bool,    // +97  vy += 2.5 (screen down)
    pub left: bool,    // +98  (vx -= 2.5)
    pub right: bool,   // +99  (vx += 2.5)
    pub button1: bool, // +100 fire
    pub button2: bool, // +101 second button (secondary weapon / special)
}

/// Decode a `tset` binding value to a Mac virtual key code.
pub const fn tset_keycode(v: u32) -> u8 { (v >> 16) as u8 }

/// Mac virtual key codes of one player's bindings: left, right, down, up, button 1, button 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bindings {
    pub left: u8,
    pub right: u8,
    pub down: u8,
    pub up: u8,
    pub button1: u8,
    pub button2: u8,
}

/// "1 Player" set: arrows + Command / Option.
pub const ONE_PLAYER: Bindings = Bindings { left: 123, right: 124, down: 125, up: 126, button1: 55, button2: 58 };
/// "2 Players" set, player 1: J / L / K / I + Command / Option.
pub const TWO_PLAYERS_P1: Bindings = Bindings { left: 38, right: 37, down: 40, up: 34, button1: 55, button2: 58 };
/// "2 Players" set, player 2: keypad 4 / 6 / 5 / 8 + Page Down / End.
pub const TWO_PLAYERS_P2: Bindings = Bindings { left: 86, right: 88, down: 87, up: 91, button1: 121, button2: 119 };

impl Bindings {
    /// Build a `PlayerInput` from a key-state query keyed by Mac virtual key code.
    pub fn read(&self, is_down: impl Fn(u8) -> bool) -> PlayerInput {
        PlayerInput {
            down: is_down(self.down),
            up: is_down(self.up),
            left: is_down(self.left),
            right: is_down(self.right),
            button1: is_down(self.button1),
            button2: is_down(self.button2),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tset_values_decode_to_the_bindings() {
        // raw values from the original's 'tset' resources (1 Player / 2 Players)
        assert_eq!([8060928u32, 8126464, 8192000, 8257536, 3604480, 3801088].map(tset_keycode), [123, 124, 125, 126, 55, 58]);
        assert_eq!([2490368u32, 2424832, 2621440, 2228224].map(tset_keycode), [38, 37, 40, 34]);
        assert_eq!([5636096u32, 5767168, 5701632, 5963776, 7929856, 7798784].map(tset_keycode), [86, 88, 87, 91, 121, 119]);
        assert_eq!(ONE_PLAYER.left, 123);
        assert_eq!(TWO_PLAYERS_P2.button1, 121);
    }

    #[test]
    fn read_maps_keys_to_flags() {
        let down = |k: u8| k == 123 || k == 55;
        let i = ONE_PLAYER.read(down);
        assert!(i.left && i.button1 && !i.right && !i.up && !i.down && !i.button2);
    }
}
