//! The attract-mode demo: a recorded game (`'FILM'` resource, 12068 bytes) played back by the title screen after ten idle seconds.
//!
//! Layout (read from `0xee60`, which copies the resource to `D.939c`, from the recorder `0xf728` and the player `0xf640`):
//! * `+0` the value of `randSeed` at the start (`0xc7e4` stores it into `qd + 76` before the level is set up),
//! * `+4` the level, `+8` (byte) the number of players, `+44` the level number the recording ended in,
//! * `+52` the index of the last recorded frame, `+56` / `+64` the read positions of ship 1 / 2 (zero in the resource),
//! * `+68 ..` 6000 bytes of ship 1's input, one byte per frame, and `+6068 ..` 6000 bytes for ship 2:
//!   bit 0 left, 1 right, 2 up, 3 down, 4 button 1, 5 button 2.
//!
//! A frame's input is read by `0xf640` (`pos > end` leaves the keys as they were, the main loop of `0xc6f0` ends the demo as soon as `pos > end` (`0xf5d4`), or a
//! mouse button / key goes down (`0xd648`)).
use crate::input::PlayerInput;

pub const FILM_LEN: usize = 12068;
const STREAM: usize = 6000;

#[derive(Debug, Clone)]
pub struct Film {
    pub seed: u32,
    pub level: i32,
    pub players: usize,
    pub end: i32,
    streams: [Vec<u8>; 2],
}

impl Film {
    pub fn parse(d: &[u8]) -> Option<Film> {
        if d.len() != FILM_LEN { return None; }
        let w = |o: usize| u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
        Some(Film {
            seed: w(0),
            level: w(4) as i32,
            players: if d[8] >= 2 { 2 } else { 1 },
            end: w(52) as i32,
            streams: [d[68..68 + STREAM].to_vec(), d[68 + STREAM..68 + 2 * STREAM].to_vec()],
        })
    }
}

/// The playback state (`D.939c + 56` and `+ 64`).
#[derive(Debug, Clone)]
pub struct Playback {
    pub film: Film,
    pos: [i32; 2],
}

impl Playback {
    pub fn new(film: Film) -> Playback { Playback { film, pos: [0, 0] } }

    /// `0xf5d4`: the recording has run out (ship 1's read position is past the last frame).
    pub fn finished(&self) -> bool { self.pos[0] > self.film.end }

    /// `0xf640`: the next frame's keys of `ship` (0 or 1); `None` when that ship's recording is over (the keys stay as they were).
    pub fn next(&mut self, ship: usize) -> Option<PlayerInput> {
        let p = self.pos[ship];
        if ship == 0 && p > self.film.end { return None; }
        let v = *self.film.streams[ship].get(p as usize)?;
        self.pos[ship] += 1;
        Some(PlayerInput { left: v & 1 != 0, right: v & 2 != 0, up: v & 4 != 0, down: v & 8 != 0, button1: v & 16 != 0, button2: v & 32 != 0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn film(end: i32, bytes: &[u8]) -> Film {
        let mut d = vec![0u8; FILM_LEN];
        d[0..4].copy_from_slice(&0x127c17u32.to_be_bytes());
        d[4..8].copy_from_slice(&2u32.to_be_bytes());
        d[8] = 1;
        d[52..56].copy_from_slice(&(end as u32).to_be_bytes());
        d[68..68 + bytes.len()].copy_from_slice(bytes);
        Film::parse(&d).unwrap()
    }

    #[test]
    fn header_and_bits() {
        let f = film(3, &[1, 2, 4, 8, 16, 32]);
        assert_eq!((f.seed, f.level, f.players, f.end), (0x127c17, 2, 1, 3));
        let mut p = Playback::new(f);
        let i = p.next(0).unwrap();
        assert!(i.left && !i.right && !i.up && !i.down && !i.button1 && !i.button2);
        let i = p.next(0).unwrap();
        assert!(i.right && !i.left);
        let i = p.next(0).unwrap();
        assert!(i.up);
        let i = p.next(0).unwrap();
        assert!(i.down);
    }

    #[test]
    fn runs_out_after_end_plus_one_frames() {
        // 0xf640 / 0xf5d4: the frame `end` is still read, the next read finds `pos > end`
        let mut p = Playback::new(film(2, &[16, 16, 16, 16]));
        let mut n = 0;
        while !p.finished() { assert!(p.next(0).is_some()); n += 1; }
        assert_eq!(n, 3);
        assert!(p.next(0).is_none());
    }

    #[test]
    fn rejects_wrong_sizes() {
        assert!(Film::parse(&[0u8; 100]).is_none());
    }
}
