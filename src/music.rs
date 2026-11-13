//! The game's music: which track plays when (the rules of `Game_Run` 0xc6f0 and the music API 0x45418..0x45850) and the player deck the
//! audio thread pulls its samples from. The engine itself is `madplay.rs`.
//!
//! What the original does (all with the default preferences: music on):
//!   * the title screen plays the interface track (`interface_music`); on December 24 and 25 the `special_music` ("Jingle Bells") instead
//!     (the date test 0x1e2a0 / 0x1e260, applied in the loader 0x44bb0 for the interface track only);
//!   * starting a game stops it and loads the level music, group 1 (`level_1_music`);
//!   * after every third finished level (counter `r17`, 0xd568..) the group alternates 1 -> 2 -> 1 -> ... (`level_1_music`, `level_2_music`);
//!   * finishing the last level plays group 3 (`level_3_music`) during the ending (0xd4f4..0xd51c);
//!   * the game being over stops the music (0xd754 -> 0x45594).
use crate::assets::{Error, Result};
use crate::madh::Music;
use crate::madplay::Player;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub const SAMPLE_RATE: u32 = 44100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    Interface,
    /// level music group 1..=3
    Level(u8),
}

/// December 24 / 25 (0x1e260).
pub fn is_special_date(month: u32, day: u32) -> bool { month == 12 && (day == 24 || day == 25) }

/// (month, day) of a Unix time in UTC (the original asks the Mac for the local date; std has no time zones).
pub fn month_day_utc(unix_secs: u64) -> (u32, u32) {
    let z = (unix_secs / 86400) as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let _ = era;
    (m, d)
}

/// The five modules of `assets/music`.
pub struct Library {
    interface: Arc<Music>,
    special: Arc<Music>,
    level: [Arc<Music>; 3],
}

impl Library {
    pub fn load(root: &Path) -> Result<Library> {
        let get = |name: &str| -> Result<Arc<Music>> {
            let p = root.join("music").join(format!("{}_music.madh", name));
            let b = std::fs::read(&p).map_err(|e| Error::Msg(format!("{}: {}", p.display(), e)))?;
            Ok(Arc::new(Music::parse(&b)?))
        };
        Ok(Library { interface: get("interface")?, special: get("special")?, level: [get("level_1")?, get("level_2")?, get("level_3")?] })
    }

    pub fn module(&self, t: Track, special_date: bool) -> Arc<Music> {
        match t {
            Track::Interface => if special_date { self.special.clone() } else { self.interface.clone() },
            Track::Level(g) => self.level[(g.clamp(1, 3) - 1) as usize].clone(),
        }
    }
}

/// The player the audio thread renders from. `on` = the music option; while it is off (or nothing is loaded, or paused) the deck is silent
/// and the song does not advance.
pub struct Deck {
    player: Option<Player>,
    pub on: bool,
    paused: bool,
}

pub type SharedDeck = Arc<Mutex<Deck>>;

impl Deck {
    pub fn new() -> Deck { Deck { player: None, on: true, paused: false } }
    pub fn shared() -> SharedDeck { Arc::new(Mutex::new(Deck::new())) }

    /// Load a module and start it from the beginning (Music_Load + Music_Start).
    pub fn play(&mut self, m: Arc<Music>) {
        self.player = Some(Player::shared(m));
        self.paused = false;
    }
    /// Music_Stop / unload.
    pub fn stop(&mut self) { self.player = None; self.paused = false; }
    pub fn pause(&mut self) { self.paused = true; }
    pub fn resume(&mut self) { self.paused = false; }
    pub fn is_playing(&self) -> bool { self.player.is_some() && !self.paused && self.on }

    /// Interleaved stereo samples at `SAMPLE_RATE`.
    pub fn render(&mut self, out: &mut [i16]) {
        match (&mut self.player, self.on && !self.paused) {
            (Some(p), true) => p.render(out),
            _ => out.fill(0),
        }
    }
}

impl Default for Deck { fn default() -> Deck { Deck::new() } }

/// The level-music sequence of one game (the locals `r16` / `r17` of `Game_Run`).
#[derive(Clone, Debug)]
pub struct Sequence {
    group: u8,
    levels_done: u32,
}

impl Sequence {
    /// A new game: the level music of group 1.
    pub fn new_game() -> (Sequence, Track) { (Sequence { group: 1, levels_done: 0 }, Track::Level(1)) }

    /// A level (not the last) has been finished (0xd568..0xd5a4): after the third one the next group starts.
    pub fn level_done(&mut self) -> Option<Track> {
        self.levels_done += 1;
        if self.levels_done > 2 {
            self.levels_done = 0;
            self.group += 1;
            if self.group > 2 { self.group = 1; }
            Some(Track::Level(self.group))
        } else { None }
    }

    /// The last level has been finished: the ending's music.
    pub fn game_won(&self) -> Track { Track::Level(3) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_dates() {
        assert!(is_special_date(12, 24) && is_special_date(12, 25));
        assert!(!is_special_date(12, 26) && !is_special_date(1, 24) && !is_special_date(11, 25));
    }

    #[test]
    fn utc_dates() {
        assert_eq!(month_day_utc(0), (1, 1));
        assert_eq!(month_day_utc(1_735_084_800), (12, 25)); // 2024-12-25 00:00 UTC
        assert_eq!(month_day_utc(1_735_084_799), (12, 24));
        assert_eq!(month_day_utc(1_709_164_800), (2, 29));  // 2024-02-29
        assert_eq!(month_day_utc(1_709_251_200), (3, 1));
    }

    #[test]
    fn groups_alternate_every_third_level() {
        let (mut s, t) = Sequence::new_game();
        assert_eq!(t, Track::Level(1));
        let seq: Vec<Option<Track>> = (0..9).map(|_| s.level_done()).collect();
        assert_eq!(seq, vec![None, None, Some(Track::Level(2)), None, None, Some(Track::Level(1)), None, None, Some(Track::Level(2))]);
        assert_eq!(s.game_won(), Track::Level(3));
    }
}
