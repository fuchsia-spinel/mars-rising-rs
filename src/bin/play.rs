//! Playable build (work in progress):
//!     cargo run --release --bin play -- [<extracted-assets-dir>] [--level 1..28] [--players 1|2] [--seed N] [--scale 1..8] [--mute] [--stats] [--no-menu] [--show-register] [--lazy-upload] [--no-dwm-sync]
//!
//! Without an assets folder on the command line the program uses `assets` next to the executable (or, failing that, in the current directory).
//!
//! The program opens on the title screen (`title.rs`; the button logic is verified against the original, see `menu.rs`): mouse, or arrows / W S +
//! Enter / Space / Z, or a pad (D-pad / stick, A / Start). 1 Player / 2 Players start a game at `--level` (default 1), Quit leaves, Credits shows the original's slideshow
//! (`credits.rs`; Ctrl / Alt / Shift held at the click select the other page sets; needs `layout/title_text.txt`), the other buttons show a "not ported yet" notice. Esc in a game goes back to the title, Esc on the title quits. `--no-menu` starts the game directly
//! (with `--players`); `--show-register` shows the Register button of an unregistered copy.
//!
//! `--stats` prints a frame-time report to the console every 5 seconds (longest frame, longest compose, longest logic step, number of sprites
//! that had to be read from disk during play): please include it when reporting stutter.
//!
//! The window (raylib, vsynced) can be resized and goes fullscreen with F11; the picture is scaled by a whole number when it fits (F4: free
//! scaling that keeps the aspect ratio) with black bars around it. Sound: the sound engine of `audio.rs` (verified against the original's
//! mixer) is driven by the game loop and its output is played through `rodio` (cargo feature `audio`, on by default).
//!
//! The ship, its weapons (bursts, charged wave beam), shots, shield, destruction/respawn, power-ups and all of their drawing are the verified port.
//! The enemies are ported too (all 22 behaviour routines, spawn groups, damage, contact, drawing, and their shots, which move, hit the ships and
//! are cleared by the smart bomb); the red boxes ("debug targets", F3) are only an optional extra.
//! The ground objects (turrets, missile towers, tanks ... and their shots) and the button-2 bombs are ported (`ground.rs`, `bomb.rs`); the bombs are the
//! only thing that damages them (and the smart bomb).
//! The side panel is the original's HUD (`hud.rs`: score digits, charge bar, shield, lives, weapon icons, hit flash; `hud_panel.rs` draws it on the panel picture).
//! Music: the original's PlayerPRO module player (`madplay.rs`), tracks chosen like the original does (`music.rs`).
//! Missing: the original's pause / key handling, dialogs.
//! The logic runs at 30 frames per second like the original (2 Mac ticks per frame).
//!
//! Gamepads work too (first connected pad = player 1, second = player 2, together with the keyboard): left stick or D-pad moves, A / right
//! shoulder or trigger fires, B / X / left shoulder or trigger = button 2, Start = pause (restart after game over).
//!
//! Keys (PC layout; the original's Mac bindings are in `input.rs`):
//!   one player   arrows move, Z fire (hold to charge the beam, release to fire it), X button 2 (bombs: hold to drop a burst on the sight marker, ground targets only)
//!   two players  P1: W A S D, Space, Left Ctrl     P2: arrows, Enter, Right Shift
//!   P pause   R restart   [ / ] previous / next level   F1 shadows   F2 translucency   F3 debug targets on/off   F4 integer / free scaling   F5 key help / diagnostics overlay
//!   F11 fullscreen   M sound effects on/off   N music on/off   Esc back to the title (quit without a title)
//!   power-up cheats (in front of player 1): 1..9 = spread, level, missiles, life, keep, rear, bomb, drone, shield; 0 coin; - special; = speed
use mars_rising::assets::*;
use mars_rising::compose::Blitter;
use mars_rising::font::draw_text;
use mars_rising::game::*;
use mars_rising::input::PlayerInput;
use mars_rising::render::*;
use mars_rising::ship::Ship;
use mars_rising::gamepad::merge;
use mars_rising::hud_panel::{load_panel_picture, HudPanel};
use mars_rising::audio::{Mixer, DEFAULT_OUT_RATE, SAMPLES_PER_FRAME};
use mars_rising::menu::Action;
use mars_rising::music::{self, Deck, Library, Sequence, SharedDeck, Track};
use mars_rising::present::{window_to_picture, DisplayState, HitchMeter, Pacer};
use mars_rising::credits::{Credits, CreditsArt, CreditsData, Event as CreditsEvent, Outcome as CreditsOutcome};
use mars_rising::title::{load_credits, load_ending, load_portraits, Title, TitleArt};
use mars_rising::levelsel::{Button as LevelButton, Key as LevelKey, LevelUi};
use mars_rising::prefs::{Closed as PrefsClosed, EscKey, Op as PrefsOp, Prefs};
use mars_rising::prefs_panel::{DialogText, PrefsUi, Target as PrefsTarget, UiKey};
use mars_rising::demo::{Film, Playback};
use mars_rising::scores::{Button as EntryButton, EntryKey, EntryPanel, Input as ScoreInput, NameEntry, Op as ScoreOp, Outcome as ScoreOutcome, ScoreCanvas, ScoreScreen, Strings as ScoreStrings, Table as ScoreTable};
use raylib::prelude::{KeyboardKey as Key, MouseButton, RaylibHandle};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[path = "../bin_common/display.rs"]
mod display;
#[path = "../bin_common/pads.rs"]
mod pads;

/// Audio output: the samples the game produces every frame go into a small queue that a `rodio` source drains.
#[cfg(feature = "audio")]
mod audio_out {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    /// The master volume (an `f32` in an atomic): the Preferences' sound volume.
    fn gain_of(g: &AtomicU32) -> f32 { f32::from_bits(g.load(Ordering::Relaxed)) }

    /// Samples to collect before playback starts (and restarts after an underrun): 2 game frames.
    const PRIME: usize = 2 * mars_rising::audio::SAMPLES_PER_FRAME;
    /// Longest queue kept: older samples are dropped beyond this (the game loop runs slightly faster than the device).
    const CAP: usize = 8 * mars_rising::audio::SAMPLES_PER_FRAME;
    /// The queue length the rate control aims for: 3 game frames (100 ms).
    const TARGET: usize = 3 * mars_rising::audio::SAMPLES_PER_FRAME;

    struct Ring {
        q: VecDeque<f32>,
        primed: bool,
        underruns: u32,
    }

    struct Feed {
        shared: Arc<Mutex<Ring>>,
        local: Vec<f32>,
        pos: usize,
        gain: Arc<AtomicU32>,
    }

    impl Iterator for Feed {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            if self.pos >= self.local.len() {
                self.local.clear();
                self.pos = 0;
                let mut r = self.shared.lock().unwrap_or_else(|e| e.into_inner());
                if !r.primed {
                    if r.q.len() < PRIME { return Some(0.0); }
                    r.primed = true;
                }
                let n = r.q.len().min(512);
                if n == 0 {
                    r.primed = false; // underrun: wait until the queue has filled up again
                    r.underruns += 1;
                    return Some(0.0);
                }
                self.local.extend(r.q.drain(..n));
            }
            let v = self.local[self.pos] * gain_of(&self.gain);
            self.pos += 1;
            Some(v)
        }
    }

    impl rodio::Source for Feed {
        fn current_frame_len(&self) -> Option<usize> { None }
        fn channels(&self) -> u16 { 1 }
        fn sample_rate(&self) -> u32 { 22255 } // the engine's 22254.545 Hz
        fn total_duration(&self) -> Option<std::time::Duration> { None }
    }

    /// The music: 44.1 kHz stereo straight from the player deck, pulled by the audio thread in the player's own 732-frame buffers.
    struct MusicFeed {
        deck: mars_rising::music::SharedDeck,
        buf: Vec<i16>,
        pos: usize,
        gain: Arc<AtomicU32>,
    }

    impl Iterator for MusicFeed {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            if self.pos >= self.buf.len() {
                self.buf.resize(2 * mars_rising::madplay::FRAMES, 0);
                self.deck.lock().unwrap_or_else(|e| e.into_inner()).render(&mut self.buf);
                self.pos = 0;
            }
            let v = self.buf[self.pos] as f32 / 32768.0 * gain_of(&self.gain);
            self.pos += 1;
            Some(v)
        }
    }

    impl rodio::Source for MusicFeed {
        fn current_frame_len(&self) -> Option<usize> { None }
        fn channels(&self) -> u16 { 2 }
        fn sample_rate(&self) -> u32 { mars_rising::music::SAMPLE_RATE }
        fn total_duration(&self) -> Option<std::time::Duration> { None }
    }

    pub struct Output {
        shared: Arc<Mutex<Ring>>,
        handle: rodio::OutputStreamHandle,
        _stream: rodio::OutputStream,
        _sink: rodio::Sink,
        _music: Option<rodio::Sink>,
        gain: Arc<AtomicU32>,
    }

    impl Output {
        pub fn new() -> Result<Output, String> {
            let (stream, handle) = rodio::OutputStream::try_default().map_err(|e| e.to_string())?;
            let sink = rodio::Sink::try_new(&handle).map_err(|e| e.to_string())?;
            let shared = Arc::new(Mutex::new(Ring { q: VecDeque::new(), primed: false, underruns: 0 }));
            let gain = Arc::new(AtomicU32::new(1.0f32.to_bits()));
            sink.append(Feed { shared: shared.clone(), local: Vec::new(), pos: 0, gain: gain.clone() });
            Ok(Output { shared, handle, _stream: stream, _sink: sink, _music: None, gain })
        }
        /// Start pulling the music from `deck` (a second sink: the device mixes it with the sound effects).
        pub fn attach_music(&mut self, deck: mars_rising::music::SharedDeck) {
            match rodio::Sink::try_new(&self.handle) {
                Ok(sink) => {
                    sink.append(MusicFeed { deck, buf: Vec::new(), pos: 0, gain: self.gain.clone() });
                    self._music = Some(sink);
                }
                Err(e) => eprintln!("no music: {}", e),
            }
        }
        /// The master volume, 0.0..=1.0 (the Preferences' sound volume).
        pub fn set_gain(&self, g: f32) { self.gain.store(g.to_bits(), Ordering::Relaxed); }
        /// Queue one logic frame of unsigned 8-bit mono samples (the engine's output format). The frame is stretched or squeezed by up to a few
        /// percent so that the queue stays near `TARGET` samples whatever the display's refresh rate does to the logic frame time (see
        /// `present::audio_frame_len`): without that the queue runs dry every half minute (the logic runs 0.25% slower than the device) and the
        /// sound breaks off for a moment.
        pub fn push(&self, samples: &[u8]) {
            let mut r = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            let len = mars_rising::present::audio_frame_len(samples.len(), r.q.len(), TARGET);
            let mut tmp = Vec::with_capacity(len);
            mars_rising::present::resample_linear(samples, len, &mut tmp);
            r.q.extend(tmp);
            if r.q.len() > CAP {
                let extra = r.q.len() - CAP;
                r.q.drain(..extra);
            }
        }
        /// (samples queued, underruns so far).
        pub fn status(&self) -> (usize, u32) {
            let r = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            (r.q.len(), r.underruns)
        }
    }
}

/// Without the `audio` feature the sound engine still runs (and the game behaves the same) but nothing is played.
#[cfg(not(feature = "audio"))]
mod audio_out {
    pub struct Output;
    impl Output {
        pub fn new() -> Result<Output, String> { Err("built without the `audio` feature".into()) }
        pub fn attach_music(&mut self, _deck: mars_rising::music::SharedDeck) {}
        pub fn set_gain(&self, _g: f32) {}
        pub fn push(&self, _samples: &[u8]) {}
        pub fn status(&self) -> (usize, u32) { (0, 0) }
    }
}

/// Frame-time report (`--stats`).
#[derive(Default)]
struct Stats {
    frames: u32,
    steps: u32,
    over22: u32,
    max_dt: f32,
    max_step: f32,
    max_compose: f32,
    since: f32,
    /// Display frames since the last logic step, and how often each gap (1, 2, ... 7 or more display frames) occurred.
    gap: u32,
    hist: [u32; 8],
}

impl Stats {
    /// `pacer` and `audio` are one-line descriptions of the pacer (measured refresh, lock) and of the audio queue (length, underruns).
    fn frame(&mut self, dt: f32, due: u32, step_ms: f32, compose_ms: f32, late: u32, pacer: &str, audio: &str, report: &mut Vec<String>) {
        self.frames += 1;
        self.steps += due;
        self.since += dt;
        self.gap += 1;
        if due > 0 {
            self.hist[(self.gap as usize).min(7)] += 1;
            self.gap = 0;
        }
        if dt > 0.022 { self.over22 += 1; }
        self.max_dt = self.max_dt.max(dt);
        self.max_step = self.max_step.max(step_ms);
        self.max_compose = self.max_compose.max(compose_ms);
        if self.since >= 5.0 {
            let h = &self.hist;
            report.push(format!(
                "stats: {} frames, {} logic steps | longest frame {:.1} ms ({} over 22 ms) | longest compose+upload {:.2} ms, longest logic step(s) {:.2} ms | sprites read from disk during play: {}",
                self.frames, self.steps, self.max_dt * 1000.0, self.over22, self.max_compose, self.max_step, late
            ));
            report.push(format!(
                "       display frames between logic steps: 1:{} 2:{} 3:{} 4:{} 5:{} 6:{} 7+:{} | {} | {}",
                h[1], h[2], h[3], h[4], h[5], h[6], h[7], pacer, audio
            ));
            let gap = self.gap;
            *self = Stats::default();
            self.gap = gap;
        }
    }
}

struct Keys {
    left: Key,
    right: Key,
    up: Key,
    down: Key,
    fire: Key,
    alt: Key,
}

const SOLO: Keys = Keys { left: Key::KEY_LEFT, right: Key::KEY_RIGHT, up: Key::KEY_UP, down: Key::KEY_DOWN, fire: Key::KEY_Z, alt: Key::KEY_X };
const DUO_P1: Keys = Keys { left: Key::KEY_A, right: Key::KEY_D, up: Key::KEY_W, down: Key::KEY_S, fire: Key::KEY_SPACE, alt: Key::KEY_LEFT_CONTROL };
const DUO_P2: Keys = Keys { left: Key::KEY_LEFT, right: Key::KEY_RIGHT, up: Key::KEY_UP, down: Key::KEY_DOWN, fire: Key::KEY_ENTER, alt: Key::KEY_RIGHT_SHIFT };

fn read_keys(win: &RaylibHandle, k: &Keys) -> PlayerInput {
    PlayerInput {
        up: win.is_key_down(k.up),
        down: win.is_key_down(k.down),
        left: win.is_key_down(k.left),
        right: win.is_key_down(k.right),
        button1: win.is_key_down(k.fire),
        button2: win.is_key_down(k.alt),
    }
}

/// Pickup cheats: key -> pickup kind (see the table in `pickup.rs`).
const CHEATS: [(Key, u8); 12] = [
    (Key::KEY_ONE, 5), (Key::KEY_TWO, 6), (Key::KEY_THREE, 7), (Key::KEY_FOUR, 8), (Key::KEY_FIVE, 9), (Key::KEY_SIX, 10), (Key::KEY_SEVEN, 11),
    (Key::KEY_EIGHT, 12), (Key::KEY_NINE, 13), (Key::KEY_ZERO, 2), (Key::KEY_MINUS, 18), (Key::KEY_EQUAL, 19),
];

struct Args {
    root: PathBuf,
    level: u32,
    players: usize,
    seed: u32,
    scale: i32,
    mute: bool,
    stats: bool,
    no_menu: bool,
    show_register: bool,
    /// `--lazy-upload`: send the picture to the GPU only when the game stepped. The default is to send it on every refresh, so that every refresh does the
    /// same GPU work in the same order (this made the hitching that the program cannot see rarer in the user's tests).
    lazy_upload: bool,
    /// `--no-dwm-sync`: do not wait for the Windows desktop compositor after each swap (see `compositor_sync`).
    no_dwm_sync: bool,
    /// `--date MM-DD`: pretend it is that day (December 24 / 25 play the special title music); the default is today's date in UTC.
    date: Option<(u32, u32)>,
    /// `--trace FILE`: one CSV line per displayed frame (timing of everything), for finding hitches.
    trace: Option<PathBuf>,
}

fn parse_args() -> Option<Args> {
    let mut a = std::env::args().skip(1).peekable();
    // the assets folder is the first argument unless that is a flag (or there is none): then `assets` next to the executable
    let named = a.peek().map_or(false, |f| !f.starts_with("--"));
    let root = if named { PathBuf::from(a.next()?) } else { mars_rising::assets::default_assets_dir() };
    let mut args = Args { root, level: 1, players: 1, seed: 1998, scale: 2, mute: false, stats: false, no_menu: false, show_register: false, lazy_upload: false, no_dwm_sync: false, date: None, trace: None };
    while let Some(flag) = a.next() {
        if flag == "--mute" { args.mute = true; continue; }
        if flag == "--stats" { args.stats = true; continue; }
        if flag == "--no-menu" { args.no_menu = true; continue; }
        if flag == "--show-register" { args.show_register = true; continue; }
        if flag == "--lazy-upload" { args.lazy_upload = true; continue; }
        if flag == "--no-dwm-sync" { args.no_dwm_sync = true; continue; }
        let v = a.next()?;
        match flag.as_str() {
            "--level" => args.level = v.parse().ok().filter(|l| (1..=28).contains(l))?,
            "--players" => args.players = v.parse().ok().filter(|p| (1..=2).contains(p))?,
            "--seed" => args.seed = v.parse().ok()?,
            "--date" => args.date = Some(v.split_once('-').and_then(|(m, d)| Some((m.parse().ok()?, d.parse().ok()?)))?),
            "--trace" => args.trace = Some(PathBuf::from(v)),
            "--scale" => args.scale = v.parse().ok().filter(|s| (1..=8).contains(s))?,
            _ => return None,
        }
    }
    Some(args)
}

fn main() {
    let Some(args) = parse_args() else {
        eprintln!("usage: play [extracted-assets-dir (default: the assets folder next to the executable)] [--level 1..28] [--players 1|2] [--seed N] [--scale 1..8] [--mute] [--stats] [--no-menu] [--show-register] [--lazy-upload] [--no-dwm-sync] [--trace FILE.csv]");
        std::process::exit(2);
    };
    if let Err(e) = run(&args) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

/// Load a level's enemy groups and ground objects into a game (the level-start part of 0xd7b0: 0x2d528, 0x3ade0, 0x3acb0).
const LAST_LEVEL: u32 = 28;

/// Execute the HUD operations the game logged since the last call on the panel surface (starting it over from the picture when the HUD was re-initialised).
fn sync_hud(game: &mut Game, panel: &mut HudPanel, picture: &Frame, palette: &Palette, seen: &mut u32, blitter: &Blitter, bank: &mut SpriteBank) -> Result<()> {
    let (epoch, calls) = game.take_hud_calls();
    if epoch != *seen {
        *panel = HudPanel::new(picture, palette);
        *seen = epoch;
    }
    panel.apply(&calls, blitter, bank)
}

/// Windows, windowed mode: wait for the desktop compositor's next refresh (`DwmFlush`).
/// With the window composed by the desktop (not fullscreen) the buffer swap does not wait for anything (the trace shows 0.07 ms), so the program was paced
/// by the GPU queue and its frames reached the compositor at a phase that drifted against the compositor's refresh; whenever that phase crossed the
/// refresh, a picture stayed on screen for 1 or 3 refreshes instead of 2 (a hitch the program's own timing cannot see; fullscreen does not have it).
/// Waiting for the compositor right after the swap starts every frame just after a refresh. Does nothing on other systems.
#[cfg(windows)]
fn compositor_sync() {
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmFlush() -> i32;
    }
    unsafe { DwmFlush(); }
}
#[cfg(not(windows))]
fn compositor_sync() {}

/// F6 capture: one file with the palette and the last composed playfield pictures (palette indices, 416 x 480), each with the game tick and the scroll position.
/// Layout (little endian): `MRCAP1`, u32 count, u32 width, u32 height, 256 x RGB, then per picture u32 tick, i32 scroll_top, width * height bytes.
fn save_capture(path: &Path, palette: &Palette, ring: &std::collections::VecDeque<(u32, i32, Vec<u8>)>) -> std::io::Result<usize> {
    use std::io::Write;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"MRCAP1")?;
    f.write_all(&(ring.len() as u32).to_le_bytes())?;
    f.write_all(&(PLAYFIELD_W as u32).to_le_bytes())?;
    f.write_all(&(SCREEN_H as u32).to_le_bytes())?;
    for i in 0..256 { f.write_all(&palette.colors.get(i).copied().unwrap_or([0, 0, 0]))?; }
    for (tick, top, px) in ring {
        f.write_all(&tick.to_le_bytes())?;
        f.write_all(&top.to_le_bytes())?;
        f.write_all(px)?;
    }
    f.flush()?;
    Ok(ring.len())
}

fn load_level_into(g: &mut Game, level: &Level) {
    let groups: Vec<(usize, Vec<u32>)> = level.groups.iter().map(|gr| (gr.index as usize, gr.fields.clone())).collect();
    g.load_groups(&groups);
    g.load_ground(&level.ground_resource(), level.width, level.height, level.ground_marker.clone());
}

fn new_game(level: &Level, level_number: u32, players: usize, seed: u32, sounds: &[(usize, Arc<mars_rising::audio::Sample>)], particles: &[u8]) -> Game {
    let mut g = Game::new_at(players, level.height, level.spawn_trigger_by_row.clone(), seed, level_number as i32);
    g.set_particle_colours(particles);
    for (id, s) in sounds { g.env.audio.set_sample_arc(*id, s.clone()); }
    load_level_into(&mut g, level);
    g
}

fn ship_lines(s: &Ship, label: &str) -> Vec<String> {
    vec![
        format!("{} SCORE {}", label, s.score),
        format!("LIVES {}  SHIELD {}", (s.lives - if s.state == 1 { 1 } else { 0 }).max(0), s.shield.max(0)),
        format!("WEAPON {}  SPREAD {}", s.level, s.spread),
        format!("ARMOR {}  COINS {}", s.hud_level, s.coins),
        format!("REAR {}  KEEP {}", if s.rear { "Y" } else { "-" }, if s.keep { "Y" } else { "-" }),
        format!("CHARGE {}", s.charge),
    ]
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Title,
    Game,
    /// the end of a game: the high score name entry over the last picture of the game
    Entry,
    /// the High Scores screen
    Scores,
    /// the ending picture after the last level (`0xe098(6000)`)
    Ending,
}

/// Where the port keeps its files (the original keeps the high scores and the preferences in one preferences file; the port has two of its own).
fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("mars_rising_rs")
}

fn scores_path() -> PathBuf { data_dir().join("highscores.bin") }

fn prefs_path() -> PathBuf { data_dir().join("prefs.bin") }

/// The preferences that act on the sound: the master volume (the system volume option leaves the output as it is, `0x45f6c`) and the music type
/// (3 = none).
fn apply_audio_prefs(p: &Prefs, audio: &Option<audio_out::Output>, deck: &SharedDeck) {
    if let Some(a) = audio { a.set_gain(if p.system_volume() { 1.0 } else { p.volume_scale() as f32 / 256.0 }); }
    deck.lock().unwrap_or_else(|e| e.into_inner()).on = p.music_type() != 3;
}

/// The display preferences of the game (`D.12be`, `D.12bc`, `D.13c8`).
fn apply_game_prefs(p: &Prefs, game: &mut Game) {
    game.opts.shadows = p.shadows();
    game.opts.translucent = p.translucency();
    game.env.detail = p.extra_detail();
}

/// The saved table, or the factory table of the extracted assets (`misc/default_scores.bin`).
fn load_scores(root: &Path, path: &Path) -> Option<ScoreTable> {
    if let Some(t) = std::fs::read(path).ok().and_then(|b| ScoreTable::from_bytes(&b)) { return Some(t); }
    std::fs::read(root.join("misc/default_scores.bin")).ok().and_then(|b| ScoreTable::from_bytes(&b))
}

fn save_scores(path: &Path, t: &ScoreTable) {
    if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
    if let Err(e) = std::fs::write(path, t.to_bytes()) { eprintln!("cannot save the high scores to {}: {}", path.display(), e); }
}

/// The end of a game in progress: the name entries still to do, and what the High Scores screen needs afterwards.
struct EndFlow {
    /// players (1 / 2) that still get their name entry, in order
    queue: Vec<u8>,
    scores: [i32; 2],
    /// the rows of the new entries (-1 none)
    rows: [i32; 2],
    entered: bool,
    /// ticks to wait before the next entry opens
    wait: u32,
    entry: Option<NameEntry>,
    /// the last picture of the game, under the panel
    base: Vec<u32>,
    clock: u32,
}

fn action_name(a: Action) -> &'static str {
    match a {
        Action::OnePlayer => "1 PLAYER",
        Action::TwoPlayers => "2 PLAYERS",
        Action::Register => "REGISTER",
        Action::Controls => "CONTROLS",
        Action::Preferences => "PREFERENCES",
        Action::HighScores => "HIGH SCORES",
        Action::Credits => "CREDITS",
        Action::Quit => "QUIT",
    }
}

/// The title screen's picture: art, hover state, the port's notice line and key help (`buf` is 640 x 480).
fn draw_title_picture(art: &TitleArt, t: &Title, notice: &Option<(String, u32)>, buf: &mut [u32]) {
    art.compose(&t.shown, t.menu.registered, buf);
    if art.text_box.is_some() { art.draw_box_text(buf, &art.line.to_uppercase(), 0xFFFF_FFFF); }
    if let Some((text, _)) = notice {
        let w = mars_rising::font::text_width(text, 1) as i32;
        draw_text(buf, SCREEN_W, (SCREEN_W as i32 - w) / 2, 440, text, 1, 0xFFFF_C040);
    }
    let help = "MOUSE, OR ARROWS + ENTER, OR PAD    ESC QUIT";
    let w = mars_rising::font::text_width(help, 1) as i32;
    draw_text(buf, SCREEN_W, (SCREEN_W as i32 - w) / 2, 462, help, 1, 0xFF60_6068);
}

/// Keys that end the Credits slideshow (any key does in the original).
const ANY_KEYS: [Key; 53] = [
    Key::KEY_ENTER, Key::KEY_SPACE, Key::KEY_ESCAPE, Key::KEY_LEFT, Key::KEY_RIGHT, Key::KEY_UP, Key::KEY_DOWN, Key::KEY_BACKSPACE, Key::KEY_TAB,
    Key::KEY_A, Key::KEY_B, Key::KEY_C, Key::KEY_D, Key::KEY_E, Key::KEY_F, Key::KEY_G, Key::KEY_H, Key::KEY_I, Key::KEY_J, Key::KEY_K, Key::KEY_L, Key::KEY_M,
    Key::KEY_N, Key::KEY_O, Key::KEY_P, Key::KEY_Q, Key::KEY_R, Key::KEY_S, Key::KEY_T, Key::KEY_U, Key::KEY_V, Key::KEY_W, Key::KEY_X, Key::KEY_Y, Key::KEY_Z,
    Key::KEY_ZERO, Key::KEY_ONE, Key::KEY_TWO, Key::KEY_THREE, Key::KEY_FOUR, Key::KEY_FIVE, Key::KEY_SIX, Key::KEY_SEVEN, Key::KEY_EIGHT, Key::KEY_NINE,
    Key::KEY_MINUS, Key::KEY_EQUAL, Key::KEY_LEFT_BRACKET, Key::KEY_RIGHT_BRACKET, Key::KEY_F1, Key::KEY_F2, Key::KEY_F3, Key::KEY_F4,
];

fn run(args: &Args) -> Result<()> {
    let root: &Path = &args.root;
    let palette = Palette::load_system(root)?;
    let tiles = TileSet::load(root)?;
    let mut bank = SpriteBank::new(root, &palette);
    let t0 = std::time::Instant::now();
    let n = bank.preload_all()?;
    eprintln!("{} sprites loaded in {:.1} s", n, t0.elapsed().as_secs_f32());
    let blitter = Blitter::load(root)?;
    let particles = mars_rising::assets::load_particle_colours(root, &palette)?;
    let mut level_id = args.level;
    let mut level = load_level(root, level_id)?;
    let sounds: Vec<(usize, Arc<mars_rising::audio::Sample>)> = load_samples(root)?.into_iter().map(|(i, s)| (i, Arc::new(s))).collect();
    let mut players = args.players;
    let mut game = new_game(&level, level_id, players, args.seed, &sounds, &particles);
    let panel_picture = load_panel_picture(root, &palette)?;
    let mut hud_panel = HudPanel::new(&panel_picture, &palette);
    let mut hud_seen = 0u32;
    sync_hud(&mut game, &mut hud_panel, &panel_picture, &palette, &mut hud_seen, &blitter, &mut bank)?;
    // F5 cycles the overlay on the playfield: 1 = key help (shown for the first ten seconds), 2 = diagnostics, 0 = off
    let mut info = 1u8;
    let mut info_left = 600u32;
    let mut audio = if args.mute { None } else {
        match audio_out::Output::new() {
            Ok(o) => Some(o),
            Err(e) => { eprintln!("no sound: {}", e); None }
        }
    };
    // The music (see music.rs): the player deck the audio thread renders from, and the rules that pick the track.
    let deck: SharedDeck = Deck::shared();
    let library = match Library::load(root) {
        Ok(l) => Some(l),
        Err(e) => { eprintln!("no music: {}", e); None }
    };
    let (month, day) = args.date.unwrap_or_else(|| {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        music::month_day_utc(now)
    });
    let special_date = music::is_special_date(month, day);
    if library.is_some() {
        if let Some(a) = audio.as_mut() { a.attach_music(deck.clone()); }
    }
    let play_track = |t: Track| {
        if let Some(l) = &library { deck.lock().unwrap_or_else(|e| e.into_inner()).play(l.module(t, special_date)); }
    };
    let stop_music = || deck.lock().unwrap_or_else(|e| e.into_inner()).stop();
    let mut seq = Sequence::new_game().0;
    // the preferences (prefs.rs): the saved file or the first-run settings; the dialog is `PrefsUi` (opened from the title screen)
    let prefs_file = prefs_path();
    let mut prefs = Prefs::load(&prefs_file);
    let prefs_text: Option<DialogText> = std::fs::read_to_string(root.join("ui/dialog_text.txt")).ok().and_then(|t| DialogText::parse(&t));
    let mut prefs_ui: Option<PrefsUi> = None;
    let mut prefs_hover: Option<PrefsTarget> = None;
    // the level chooser (levelsel.rs): opened by 1 Player / 2 Players with Alt (the original's Option key) held
    let mut level_ui: Option<LevelUi> = None;
    let mut level_hover: Option<LevelButton> = None;
    let mut level_players = 1usize;
    let interface_on = std::cell::Cell::new(prefs.interface_music());
    apply_audio_prefs(&prefs, &audio, &deck);
    apply_game_prefs(&prefs, &mut game);
    // the music of the title, the Credits and the High Scores screens only plays with the "Play Interface Music" preference
    let play_interface = || if interface_on.get() { play_track(Track::Interface) } else { stop_music() };
    let mut esc_key = EscKey::new(); // the quit key with its delay option (0xd3fc)
    let mut esc_fire = false;

    // The title screen (the application's main window in the original): its pictures and button logic, and a sound engine of its own for the
    // hover / press sounds.
    let mut title: Option<(TitleArt, Title)> = if args.no_menu {
        None
    } else {
        let art = TitleArt::load(root)?;
        let t = Title::new(art.screen, !args.show_register);
        Some((art, t))
    };
    // the Credits slideshow: its data comes from the extractor (layout/title_text.txt); without it the button says so
    let credits_setup = match &title {
        Some((art, _)) => Some(load_credits(root, &art.background)),
        None => None,
    };
    let (credits_setup, credits_err): (Option<(std::rc::Rc<CreditsData>, CreditsArt)>, Option<String>) = match credits_setup {
        Some(Ok(c)) => (Some(c), None),
        Some(Err(e)) => (None, Some(format!("{}", e))),
        None => (None, None),
    };
    if let (Some((art, _)), Some((cdata, _))) = (title.as_mut(), credits_setup.as_ref()) {
        art.line = cdata.string("copyright").iter().map(|&b| b as char).collect(); // the line in the copyright box
    }
    let mut credits: Option<Credits> = None;
    let mut title_buf: Vec<u32> = vec![0; SCREEN_W * SCREEN_H]; // the title picture the slideshow returns to
    let mut mode = if title.is_some() { Mode::Title } else { Mode::Game };
    if mode == Mode::Title { play_interface(); } else { let (s, t) = Sequence::new_game(); seq = s; play_track(t); }
    let mut fresh_music = false; // a new game starts: the level music of group 1
    let mut music_stopped = false; // the game is over (0xd754 stops the music)
    let mut title_mix = Mixer::new(DEFAULT_OUT_RATE);
    for (id, s) in &sounds { title_mix.set_sample_arc(*id, s.clone()); }
    let mut title_now = 0u32;
    let mut title_audio = vec![128u8; SAMPLES_PER_FRAME];
    let mut notice: Option<(String, u32)> = None;
    let mut prev_pad = PlayerInput::default();
    // the high scores (they need the title screen and the extracted credits data: the font and the name entry's texts)
    let hs_path = scores_path();
    let hs_strings: ScoreStrings = credits_setup.as_ref().map(|(d, _)| ScoreStrings::from_data(d)).unwrap_or_default();
    let mut hs_table: Option<ScoreTable> = if title.is_some() && credits_setup.is_some() && !hs_strings.default_name.is_empty() { load_scores(root, &hs_path) } else { None };
    if title.is_some() && hs_table.is_none() { eprintln!("high scores are off: run the extractor again (it writes layout/title_text.txt and misc/default_scores.bin)"); }
    let hs_portraits = if hs_table.is_some() { load_portraits(root).ok() } else { None };
    let mut flow: Option<EndFlow> = None;
    let mut score_screen: Option<(ScoreScreen, ScoreCanvas)> = None;
    let mut score_input: Option<ScoreInput> = None;
    let mut new_session = true; // the next (re)start begins a game from the title; R and [ ] in a game do not count
    let mut session_start_level = level_id;
    let mut session_cheated = false;
    let mut want_end = false; // the game ended (or Esc): the end of game flow starts
    let mut entry_hover: Option<EntryButton> = None;
    // The attract-mode demo (demo.rs): the recorded games (`misc/film_N.bin`, resources 'FILM' 1..), `D.13d4` = which one is next, the idle timer of the title
    // screen's main loop (`r26`, counted in logic steps of 2 ticks) and `r24`, which alternates the demo and the High Scores screen when ten seconds pass
    let films: Vec<Film> = (1..).map_while(|n| std::fs::read(root.join(format!("misc/film_{}.bin", n))).ok().and_then(|d| Film::parse(&d))).collect();
    let mut demo: Option<Playback> = None;
    let mut demo_next = 0usize;
    let mut demo_sound = true;
    let mut end_demo = false;
    let mut idle_steps = 0u32;
    let mut idle_fire = false;
    let mut idle_demo_turn = true;
    let mut scores_from_title = false;
    // the wipe back to the title after the High Scores screen (0x1aa68 + 0x1bafc(12)): the wipe, the title picture it shows and the picture on the window
    let mut score_wipe: Option<(mars_rising::credits::Wipe, Vec<u32>, Vec<u32>)> = None;

    let window_title = "Mars Rising (Rust port) - playable test build";
    let (mut win, thread) = display::open(window_title, SCREEN_W, SCREEN_H, args.scale);
    let mut screen = display::Display::new(&mut win, &thread, SCREEN_W, SCREEN_H).map_err(Error::Msg)?;

    // The window is vsynced; the logic advances in whole frames of FRAME_MICROS when they are due (see present::Pacer), the picture is drawn
    // on every refresh.
    let mut pacer = Pacer::new(FRAME_MICROS as f64 / 1e6, 4);

    let mut frame = Frame::new(PLAYFIELD_W, SCREEN_H);
    let mut buf = vec![0xFF00_0000u32; SCREEN_W * SCREEN_H];
    let mut paused = false;
    let mut seed = args.seed;
    let mut st = Stats::default();
    let mut hm = HitchMeter::new();
    let mut last_present_ms = 0.0f32;
    let mut warned_vsync = false;
    let mut trace = args.trace.as_ref().and_then(|p| std::fs::File::create(p).ok()).map(|f| std::io::BufWriter::with_capacity(16 << 20, f));
    if let Some(t) = trace.as_mut() {
        use std::io::Write;
        let _ = writeln!(t, "t_s,dt_ms,logic_steps,step_ms,compose_ms,present_ms,audio_queue,display_state,hitch_gap,tiles_ms,sprites_ms,palette_hud_ms,upload_ms,late_loads,watchdog_max_ms,composed,game_tick,scroll_top,draws,ship_x,ship_y,shield,lives,marker,repeat_ms,dwm_ms");
    }
    let clock = std::time::Instant::now();
    // Nothing is written to the console or to disk while the game runs (a console write on Windows can take milliseconds, a file write too, and
    // that would be a hitch of its own): `--stats` collects its report here and prints it when the game ends.
    let mut report: Vec<String> = Vec::new();
    if args.stats || args.trace.is_some() {
        eprintln!("diagnostics are collected silently while you play; the report is printed (and the trace file written) when you quit with Esc");
    }
    // Watchdog (always on, it only sleeps; F5 shows what it saw during the last missed refresh): a thread that sleeps 1 ms in a loop and records the longest time a sleep took. If it stalls together
    // with the main loop the whole process (or machine) was held up; if only the main loop stalls it was blocked inside a graphics call.
    let hb_gap = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let hb_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let (g, stop) = (hb_gap.clone(), hb_stop.clone());
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let t0 = std::time::Instant::now();
                std::thread::sleep(std::time::Duration::from_millis(1));
                g.fetch_max(t0.elapsed().as_micros().min(u32::MAX as u128) as u32, std::sync::atomic::Ordering::Relaxed);
            }
        });
    }
    let mut pad = pads::Pads::default();
    // F6: write the last 60 composed pictures to `capture_<tick>.bin` (see `save_capture`), to look at a hitch afterwards
    let mut ring: std::collections::VecDeque<(u32, i32, Vec<u8>)> = std::collections::VecDeque::new();
    let mut dwm_sync = !args.no_dwm_sync; // see `compositor_sync`
    let (mut dwm_frames, mut dwm_time, mut last_dwm_ms) = (0u32, 0.0f32, 0.0f32);
    // the previous loop iteration's times (ms: logic step, compose + upload, swap, compositor wait) and the description of the last missed refresh (F5)
    let mut prev_times = [0.0f32; 5];
    let mut last_slow: Option<String> = None;
    let mut dirty = true; // the picture needs composing again (a logic step happened, or a toggle changed what is shown)
    let mut quit = false;

    // set by the title screen, the keys and the credits' N key; handled at the top of the next frame
    let mut restart = false;
    // the ending picture (`overlay::ENDING_TICKS`, 'IMAG' 130): shown once after the last level; the ticks passed and whether a click / key ended it
    let mut ending_shown = false;
    let mut ending_pic: Option<Vec<u32>> = None;
    let mut ending_ticks = 0u32;
    let mut ending_click = false;
    while !win.window_should_close() && !quit {
        let mut marker = 0u8; // F6 pressed on this refresh (a column of the trace)
        let mut repeat_ms = 0.0f32; // time of the repeated upload on refreshes without a step
        // the game's picture is the playfield plus the side panel (576 wide); the title screen is 640 wide. The window follows (keeping its height).
        let want_w = if matches!(mode, Mode::Game | Mode::Entry) { PLAYFIELD_W + 160 } else { SCREEN_W };
        if screen.view_width() != want_w {
            screen.set_view_width(want_w);
            if !win.is_window_fullscreen() {
                let h = win.get_screen_height();
                win.set_window_size((h as i64 * want_w as i64 / SCREEN_H as i64) as i32, h);
            }
            pacer.reset();
            hm.reset();
            dirty = true;
        }
        if win.is_key_pressed(Key::KEY_F4) { screen.integer = !screen.integer; }
        if win.is_key_pressed(Key::KEY_F6) && mode == Mode::Game {
            marker = 1;
            let name = format!("capture_{}.bin", game.now);
            match save_capture(Path::new(&name), &palette, &ring) {
                Ok(n) => eprintln!("saved the last {} pictures to {}", n, name),
                Err(e) => eprintln!("cannot save {}: {}", name, e),
            }
        }
        if win.is_key_pressed(Key::KEY_F11) { win.toggle_fullscreen(); }
        // characters typed this frame (the name entry); M and N are also the sound / music switches, but not while keys mean something else
        let typed: Vec<char> = std::iter::from_fn(|| win.get_char_pressed()).collect();
        let keys_taken = matches!(mode, Mode::Entry | Mode::Scores | Mode::Ending) || credits.is_some() || prefs_ui.is_some() || level_ui.is_some();
        if !keys_taken && win.is_key_pressed(Key::KEY_M) {
            game.env.audio.sound_on = !game.env.audio.sound_on;
            title_mix.sound_on = game.env.audio.sound_on;
            dirty = true;
        }
        if !keys_taken && win.is_key_pressed(Key::KEY_N) {
            let mut d = deck.lock().unwrap_or_else(|e| e.into_inner());
            d.on = !d.on;
            dirty = true;
        }
        let start = pads::Pads::start_pressed(&win);

        match mode {
            Mode::Ending => {
                // a click or any key ends the picture; the first frame is ignored (the original flushes the event queue after drawing it)
                let p = merge(pad.input(&win, 0), pad.input(&win, 1));
                if ending_ticks > 0 && (win.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_LEFT) || start || (p.button1 && !prev_pad.button1) || ANY_KEYS.iter().any(|k| win.is_key_pressed(*k))) {
                    ending_click = true;
                }
                prev_pad = p;
            }
            Mode::Scores => {
                // a click or any key leaves, N starts a game (`0x194ac`)
                let p = merge(pad.input(&win, 0), pad.input(&win, 1));
                if win.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_LEFT) || start || (p.button1 && !prev_pad.button1) { score_input = Some(ScoreInput::Click); }
                for k in ANY_KEYS.iter() {
                    if win.is_key_pressed(*k) { score_input = Some(ScoreInput::Key(if *k == Key::KEY_N { b'N' } else { 0 })); }
                }
                prev_pad = p;
            }
            Mode::Entry => {
                let p = merge(pad.input(&win, 0), pad.input(&win, 1));
                let (ww, wh) = (win.get_screen_width(), win.get_screen_height());
                let m = win.get_mouse_position();
                let (mh, mv) = window_to_picture(ww, wh, (PLAYFIELD_W + 160) as i32, SCREEN_H as i32, screen.integer, m.x, m.y);
                let panel = EntryPanel::centered((PLAYFIELD_W + 160) as i32, SCREEN_H as i32);
                let hover = panel.hit(mh, mv);
                if hover != entry_hover { entry_hover = hover; dirty = true; }
                if let Some(e) = flow.as_mut().and_then(|f| f.entry.as_mut()) {
                    for ch in &typed {
                        if (' '..='~').contains(ch) { e.key(EntryKey::Char(*ch as u8)); }
                    }
                    if win.is_key_pressed(Key::KEY_BACKSPACE) { e.key(EntryKey::Backspace); }
                    if win.is_key_pressed(Key::KEY_ENTER) || win.is_key_pressed(Key::KEY_KP_ENTER) || start || (p.button1 && !prev_pad.button1) { e.key(EntryKey::Return); }
                    if win.is_key_pressed(Key::KEY_ESCAPE) || (p.button2 && !prev_pad.button2) { e.cancel(); }
                    if win.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_LEFT) {
                        match hover { Some(EntryButton::Ok) => e.ok(), Some(EntryButton::Cancel) => e.cancel(), None => {} }
                    }
                    dirty = true;
                }
                prev_pad = p;
            }
            Mode::Title if credits.is_some() => {
                // the slideshow: a click or any key ends it, N starts a game (the original's `0x10b80`)
                if let Some(c) = credits.as_mut() {
                    let p = merge(pad.input(&win, 0), pad.input(&win, 1));
                    if win.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_LEFT) || start || (p.button1 && !prev_pad.button1) { c.push(CreditsEvent::Click); }
                    for k in ANY_KEYS.iter() {
                        if win.is_key_pressed(*k) { c.push(CreditsEvent::Key { n: *k == Key::KEY_N }); }
                    }
                    prev_pad = p;
                }
            }
            Mode::Title if level_ui.is_some() => {
                let (ww, wh) = (win.get_screen_width(), win.get_screen_height());
                let m = win.get_mouse_position();
                let (mh, mv) = window_to_picture(ww, wh, SCREEN_W as i32, SCREEN_H as i32, screen.integer, m.x, m.y);
                let p = merge(pad.input(&win, 0), pad.input(&win, 1));
                let mut ops = Vec::new();
                if let Some(ui) = level_ui.as_mut() {
                    if !ui.busy() {
                        level_hover = ui.hit(mh, mv);
                        if win.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_LEFT) { ops.extend(ui.click(mh, mv)); }
                        for ch in &typed { if ch.is_ascii() && !ch.is_ascii_control() { ops.extend(ui.typed(*ch as u8)); } }
                        if win.is_key_pressed(Key::KEY_BACKSPACE) { ops.extend(ui.key(LevelKey::Backspace)); }
                        if win.is_key_pressed(Key::KEY_ENTER) || win.is_key_pressed(Key::KEY_KP_ENTER) || start || (p.button1 && !prev_pad.button1) { ops.extend(ui.key(LevelKey::Return)); }
                        if win.is_key_pressed(Key::KEY_ESCAPE) || (p.button2 && !prev_pad.button2) { ops.extend(ui.key(LevelKey::Escape)); }
                    }
                }
                prev_pad = p;
                for op in &ops { if let mars_rising::levelsel::Op::Snd(id, prio, delay) = op { title_mix.play(*id, *prio, *delay as u8); } }
                dirty = true;
            }
            Mode::Title if prefs_ui.is_some() => {
                let (ww, wh) = (win.get_screen_width(), win.get_screen_height());
                let m = win.get_mouse_position();
                let (mh, mv) = window_to_picture(ww, wh, SCREEN_W as i32, SCREEN_H as i32, screen.integer, m.x, m.y);
                let p = merge(pad.input(&win, 0), pad.input(&win, 1));
                let mut ops: Vec<PrefsOp> = Vec::new();
                if let Some(ui) = prefs_ui.as_mut() {
                    let h = ui.target_at(mh, mv);
                    if h != prefs_hover { prefs_hover = h; }
                    if ui.hover_help(mh, mv) { dirty = true; }
                    if win.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_LEFT) { ops.extend(ui.click(mh, mv, false)); }
                    if win.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_RIGHT) { ops.extend(ui.click(mh, mv, true)); }
                    for (k, uk) in [(Key::KEY_UP, UiKey::Up), (Key::KEY_DOWN, UiKey::Down), (Key::KEY_LEFT, UiKey::Left), (Key::KEY_RIGHT, UiKey::Right),
                                    (Key::KEY_ENTER, UiKey::Activate), (Key::KEY_KP_ENTER, UiKey::Activate), (Key::KEY_SPACE, UiKey::Activate),
                                    (Key::KEY_ESCAPE, UiKey::Escape), (Key::KEY_TAB, UiKey::NextPage), (Key::KEY_PAGE_DOWN, UiKey::NextPage), (Key::KEY_PAGE_UP, UiKey::PrevPage)] {
                        if win.is_key_pressed(k) { ops.extend(ui.key(uk)); }
                    }
                    if p.up && !prev_pad.up { ops.extend(ui.key(UiKey::Up)); }
                    if p.down && !prev_pad.down { ops.extend(ui.key(UiKey::Down)); }
                    if (p.button1 && !prev_pad.button1) || start { ops.extend(ui.key(UiKey::Activate)); }
                    if p.button2 && !prev_pad.button2 { ops.extend(ui.key(UiKey::Escape)); }
                }
                prev_pad = p;
                for op in &ops { if let PrefsOp::Snd(id, prio, delay) = op { title_mix.play(*id, *prio, *delay as u8); } }
                let mut closed = false;
                if let Some(ui) = prefs_ui.as_ref() {
                    // the settings act at once (the original applies the volume and the display switches while the dialog is open)
                    if !ops.is_empty() { apply_audio_prefs(ui.prefs(), &audio, &deck); apply_game_prefs(ui.prefs(), &mut game); }
                    if let Some(c) = ui.closed() {
                        if c == PrefsClosed::Save {
                            prefs = ui.prefs().clone();
                            if let Err(e) = prefs.save(&prefs_file) { eprintln!("cannot save the preferences to {}: {}", prefs_file.display(), e); }
                        }
                        closed = true;
                    }
                }
                if closed {
                    prefs_ui = None;
                    let before = interface_on.get();
                    interface_on.set(prefs.interface_music());
                    apply_audio_prefs(&prefs, &audio, &deck);
                    apply_game_prefs(&prefs, &mut game);
                    if before != interface_on.get() { play_interface(); }
                    if let Some((_, t)) = title.as_mut() { t.dirty = true; }
                }
                dirty = true;
            }
            Mode::Title => {
                if win.is_key_pressed(Key::KEY_ESCAPE) { quit = true; }
                if let Some((_, t)) = title.as_mut() {
                    let (ww, wh) = (win.get_screen_width(), win.get_screen_height());
                    let m = win.get_mouse_position();
                    let (mh, mv) = window_to_picture(ww, wh, SCREEN_W as i32, SCREEN_H as i32, screen.integer, m.x, m.y);
                    let down = win.is_mouse_button_down(MouseButton::MOUSE_BUTTON_LEFT);
                    let mut act = t.pointer(mh, mv, down);
                    // keyboard and pads: an addition of the port (the original is driven by the mouse)
                    let p = merge(pad.input(&win, 0), pad.input(&win, 1));
                    if win.is_key_pressed(Key::KEY_UP) || (p.up && !prev_pad.up) { t.key_move(-1); }
                    if win.is_key_pressed(Key::KEY_DOWN) || (p.down && !prev_pad.down) { t.key_move(1); }
                    if win.is_key_pressed(Key::KEY_ENTER) || win.is_key_pressed(Key::KEY_SPACE) || win.is_key_pressed(Key::KEY_Z) || start
                        || (p.button1 && !prev_pad.button1) {
                        act = act.or(t.key_activate());
                    }
                    prev_pad = p;
                    // the original's key handler (0x1be60): 1 / N / Enter one player, 2 two players, P preferences, Q quit, C controls, H / S the High Scores,
                    // D the demo; each plays the click sound 29 first (the High Scores and the demo keys do not)
                    let (mut want_demo, mut want_scores, mut want_quote) = (false, false, false);
                    if act.is_none() {
                        let k = |key: Key| win.is_key_pressed(key);
                        let pressed = if k(Key::KEY_ONE) || k(Key::KEY_N) || k(Key::KEY_KP_ENTER) { Some(Action::OnePlayer) }
                            else if k(Key::KEY_TWO) { Some(Action::TwoPlayers) }
                            else if k(Key::KEY_P) { Some(Action::Preferences) }
                            else if k(Key::KEY_Q) { Some(Action::Quit) }
                            else if k(Key::KEY_C) { Some(Action::Controls) }
                            else { None };
                        if pressed.is_some() { title_mix.play(29, 3, 0); act = pressed; }
                        else if k(Key::KEY_H) || k(Key::KEY_S) { want_scores = true; }
                        else if k(Key::KEY_D) { want_demo = true; }
                        else if k(Key::KEY_X) { want_quote = true; } // 0x1cef0: sound 65 and the quotation page 400 (the original's shareware reminder screen)
                    }
                    // ten idle seconds post 'D' and 'S' alternately (0x1a878)
                    if std::mem::take(&mut idle_fire) && act.is_none() && !want_demo && !want_scores {
                        if idle_demo_turn { want_demo = true; } else { want_scores = true; }
                        idle_demo_turn = !idle_demo_turn;
                    }
                    // every click, key and hover change restarts the idle timer
                    if t.take_idle_reset() || act.is_some() || ANY_KEYS.iter().any(|k| win.is_key_pressed(*k)) { idle_steps = 0; }
                    for (id, prio, delay) in t.take_sounds() { title_mix.play(id, prio, delay as u8); }
                    if want_quote {
                        if let (Some((art, t)), Some((cdata, cart))) = (title.as_ref(), credits_setup.as_ref()) {
                            draw_title_picture(art, t, &None, &mut title_buf);
                            credits = Some(Credits::single(cdata.clone(), cart, 400, &title_buf));
                            dirty = true;
                        }
                    }
                    if want_demo && !films.is_empty() {
                        // 0x1cdd0: the next recording (resource D.13d4 + 1, then D.13d4 counts on and wraps), level and players from the recording
                        let film = films[demo_next % films.len()].clone();
                        demo_next = (demo_next + 1) % films.len();
                        players = film.players;
                        level_id = film.level.max(1) as u32;
                        demo = Some(Playback::new(film));
                        restart = true;
                        mode = Mode::Game;
                        idle_steps = 0;
                    }
                    if want_scores || act == Some(Action::HighScores) {
                        // 0x1cd5c: the High Scores screen (0x194ac without new rows); it returns to the title, or starts a game when "play" is chosen
                        match (hs_table.as_ref(), title.as_ref(), credits_setup.is_some()) {
                            (Some(table), Some((art, _)), true) => {
                                score_screen = Some((ScoreScreen::new(table, [-1, -1]), ScoreCanvas::new(&art.background)));
                                score_input = None;
                                scores_from_title = true;
                                mode = Mode::Scores;
                                idle_steps = 0;
                                pacer.reset();
                                hm.reset();
                                dirty = true;
                            }
                            _ => if act == Some(Action::HighScores) {
                                notice = Some(("HIGH SCORES NEED THE EXTRACTOR TO BE RUN AGAIN (title_text.txt, default_scores.bin)".to_string(), 180));
                                dirty = true;
                            }
                        }
                    }
                    match act {
                        Some(Action::OnePlayer) | Some(Action::TwoPlayers) if win.is_key_down(Key::KEY_LEFT_ALT) || win.is_key_down(Key::KEY_RIGHT_ALT) => {
                            // the Option key: "Start at which level?" first (0x1cb80 / 0x1cbe8 -> 0x1b254)
                            match (title.as_ref(), prefs_text.as_ref(), credits_setup.is_some()) {
                                (Some((art, t)), Some(text), true) => {
                                    draw_title_picture(art, t, &None, &mut title_buf);
                                    let (ui, ops) = LevelUi::new(LAST_LEVEL as i32, text.clone(), SCREEN_W as i32, SCREEN_H as i32);
                                    for op in &ops { if let mars_rising::levelsel::Op::Snd(id, prio, delay) = op { title_mix.play(*id, *prio, *delay as u8); } }
                                    level_players = if act == Some(Action::TwoPlayers) { 2 } else { 1 };
                                    level_ui = Some(ui);
                                    level_hover = None;
                                    dirty = true;
                                }
                                _ => { notice = Some(("LEVEL SELECT NEEDS THE EXTRACTOR TO BE RUN AGAIN (dialog_text.txt)".to_string(), 180)); dirty = true; }
                            }
                        }
                        Some(Action::OnePlayer) | Some(Action::TwoPlayers) => {
                            players = if act == Some(Action::TwoPlayers) { 2 } else { 1 };
                            level_id = args.level;
                            new_session = true;
                            restart = true;
                            fresh_music = true;
                            mode = Mode::Game;
                        }
                        Some(Action::Quit) => quit = true,
                        Some(Action::Credits) if credits_setup.is_some() => {
                            // Mac keys held when the button is released: Control -> reading, Option -> viewing / playing / listening, Command -> the last pages;
                            // on this keyboard Ctrl, Alt and Shift
                            let ctrl = win.is_key_down(Key::KEY_LEFT_CONTROL) || win.is_key_down(Key::KEY_RIGHT_CONTROL);
                            let opt = win.is_key_down(Key::KEY_LEFT_ALT) || win.is_key_down(Key::KEY_RIGHT_ALT);
                            let cmd = win.is_key_down(Key::KEY_LEFT_SHIFT) || win.is_key_down(Key::KEY_RIGHT_SHIFT);
                            if let (Some((art, t)), Some((cdata, cart))) = (title.as_ref(), credits_setup.as_ref()) {
                                draw_title_picture(art, t, &None, &mut title_buf);
                                credits = Some(Credits::new(cdata.clone(), cart, mars_rising::credits::mode_for_keys(ctrl, opt, cmd), &title_buf));
                                dirty = true;
                            }
                        }
                        Some(Action::Credits) if credits_err.is_some() => {
                            notice = Some(("CREDITS NEED THE EXTRACTOR TO BE RUN AGAIN (title_text.txt)".to_string(), 180));
                            dirty = true;
                        }
                        Some(Action::Preferences) if prefs_text.is_some() && credits_setup.is_some() => {
                            if let (Some((art, t)), Some(text)) = (title.as_ref(), prefs_text.as_ref()) {
                                draw_title_picture(art, t, &None, &mut title_buf);
                                let (ui, ops) = PrefsUi::new(prefs.clone(), library.is_some(), text.clone(), SCREEN_W as i32, SCREEN_H as i32);
                                for op in &ops { if let PrefsOp::Snd(id, prio, delay) = op { title_mix.play(*id, *prio, *delay as u8); } }
                                prefs_ui = Some(ui);
                                prefs_hover = None;
                                dirty = true;
                            }
                        }
                        Some(Action::Preferences) => {
                            notice = Some(("PREFERENCES NEED THE EXTRACTOR TO BE RUN AGAIN (dialog_text.txt)".to_string(), 180));
                            dirty = true;
                        }
                        Some(Action::HighScores) => {}
                        Some(Action::Controls) => {
                            // the original opens Apple's Input Sprocket configuration (an operating system window, `0x8f7c`); the port's keys are fixed (F5 lists them)
                            notice = Some(("CONTROLS: THE KEYS ARE FIXED, F5 IN A GAME LISTS THEM".to_string(), 150));
                            dirty = true;
                        }
                        Some(a) => { notice = Some((format!("{} IS NOT PORTED YET", action_name(a)), 90)); dirty = true; }
                        None => {}
                    }
                }
            }
            Mode::Game if demo.is_some() => {
                // 0xd648: a mouse button or a key going down ends the demo (the event is swallowed)
                let p = merge(pad.input(&win, 0), pad.input(&win, 1));
                if ANY_KEYS.iter().any(|k| win.is_key_pressed(*k)) || win.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_LEFT)
                    || win.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_RIGHT) || start || (p.button1 && !prev_pad.button1) || (p.button2 && !prev_pad.button2) {
                    end_demo = true;
                }
                prev_pad = p;
            }
            Mode::Game => {
                // Esc is the original's quit key (0xd3fc): the session loop reads it once per step (`esc_key`, with the Escape Key Delay preference);
                // while the game is paused no step runs, so a press counts at once
                if std::mem::take(&mut esc_fire) || (paused && win.is_key_pressed(Key::KEY_ESCAPE)) {
                    esc_key = EscKey::new();
                    // the original's key handling (0x1be60) is not ported: Esc ends the game like the original's quit key (the score still counts: name
                    // entry and High Scores) and goes back to the title screen (or quits without one)
                    if hs_table.is_some() {
                        want_end = true;
                    } else if let Some((art, t)) = title.as_mut() {
                        *t = Title::new(art.screen, !args.show_register);
                        title_mix.clear_queue();
                        play_interface();
                        mode = Mode::Title;
                        pacer.reset();
                        hm.reset();
                        dirty = true;
                    } else {
                        quit = true;
                    }
                }
                if win.is_key_pressed(Key::KEY_P) || (start && !game.over) {
                    paused = !paused;
                    { let mut d = deck.lock().unwrap_or_else(|e| e.into_inner()); if paused { d.pause() } else { d.resume() } }
                    pacer.reset(); hm.reset(); dirty = true;
                }
                if win.is_key_pressed(Key::KEY_R) || (start && game.over) { restart = true; fresh_music = true; }
                if win.is_key_pressed(Key::KEY_RIGHT_BRACKET) && level_id < LAST_LEVEL { level_id += 1; restart = true; }
                if win.is_key_pressed(Key::KEY_LEFT_BRACKET) && level_id > 1 { level_id -= 1; restart = true; }
                if win.is_key_pressed(Key::KEY_F1) { game.opts.shadows = !game.opts.shadows; dirty = true; }
                if win.is_key_pressed(Key::KEY_F2) { game.opts.translucent = !game.opts.translucent; dirty = true; }
                if win.is_key_pressed(Key::KEY_F3) { game.targets_enabled = !game.targets_enabled; dirty = true; }
                if win.is_key_pressed(Key::KEY_F5) { info = match info { 1 => 2, 2 => 0, _ => 1 }; info_left = u32::MAX; dirty = true; }
                for (key, kind) in CHEATS {
                    if win.is_key_pressed(key) { game.debug_pickup(kind); session_cheated = true; dirty = true; }
                }
            }
        }
        if restart {
            restart = false;
            let is_demo = demo.is_some();
            if is_demo {
                // a demo is not a session: no high scores, no cheat flag
            } else if new_session { session_start_level = level_id; session_cheated = false; new_session = false; } else { session_cheated = true; }
            flow = None;
            ending_shown = false;
            level = load_level(root, level_id)?;
            // a demo starts from the recording's `randSeed` (0xc7e4)
            let game_seed = match demo.as_ref() { Some(pb) => pb.film.seed, None => { seed = seed.wrapping_add(1); seed } };
            let (opts, sound_on) = (game.opts, game.env.audio.sound_on);
            game = new_game(&level, level_id, players, game_seed, &sounds, &particles);
            game.opts = opts;
            game.env.detail = prefs.extra_detail();
            game.env.audio.sound_on = sound_on;
            if is_demo {
                // every sound entry point returns at once while the demo flag is set, and the music is left alone (0xc6f0 only changes it when not a demo)
                demo_sound = sound_on;
                game.set_demo(true);
                game.env.audio.sound_on = false;
            }
            sync_hud(&mut game, &mut hud_panel, &panel_picture, &palette, &mut hud_seen, &blitter, &mut bank)?;
            info = if is_demo { 0 } else { 1 };
            info_left = 600;
            paused = false;
            if is_demo {
                // (music untouched)
            } else if fresh_music {
                let (s, t) = Sequence::new_game();
                seq = s;
                play_track(t);
                fresh_music = false;
                music_stopped = false;
            } else {
                deck.lock().unwrap_or_else(|e| e.into_inner()).resume();
            }
            pacer.reset();
            hm.reset();
            dirty = true;
        }

        let dt = win.get_frame_time();
        let due = if paused && mode == Mode::Game { 0 } else { pacer.advance(dt as f64) };
        let wd_ms = hb_gap.swap(0, std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0; // longest 1 ms sleep of the watchdog thread since the last refresh
        let slow_before = hm.slow;
        let hitch = if mode == Mode::Game && !paused { hm.frame(dt as f64, due, pacer.measured_frame_time()) } else { None };
        if hm.slow != slow_before {
            // this refresh arrived late: the previous iteration's parts say where the time went
            last_slow = Some(format!("LAST MISS {:.0} MS AT {:.0} S: STEP {:.1} COMPOSE {:.1} (UPLOAD {:.1}) SWAP {:.1} DWM {:.1} WATCHDOG {:.0}", dt * 1000.0, clock.elapsed().as_secs_f32(), prev_times[0], prev_times[1], prev_times[4], prev_times[2], prev_times[3], wd_ms));
        }
        if !warned_vsync && mode == Mode::Game {
            if let DisplayState::NoVsync { hz } = pacer.display_state() {
                warned_vsync = true;
                eprintln!(
                    "WARNING: the window runs at {:.0} frames per second, so vsync is NOT in effect (the graphics driver or the desktop overrides it). The game \
                     steps are then not in step with the display and an uneven frame shows up every few seconds. Turn vertical sync on for this program \
                     (NVIDIA / AMD control panel: vertical sync = on or 'use the 3D application setting'; Linux: vblank_mode=1 or __GL_SYNC_TO_VBLANK=1).",
                    hz
                );
            }
        }
        let t_step = std::time::Instant::now();
        for _ in 0..due {
            if mode != Mode::Game {
                // the title screen's sounds: the same per-frame pump as the game's (0x45a6c)
                title_now = title_now.wrapping_add(1);
                title_mix.frame(title_now);
                title_mix.render(&mut title_audio);
                title_mix.take_events();
                if let Some(a) = &audio { a.push(&title_audio); }
                if mode == Mode::Ending {
                    // two ticks per logic step; 3200 ticks or a click / key, then sound 29 (only for the click) and 56 three times (0xe2a4, 0xe2cc..0xe300)
                    ending_ticks += 2;
                    dirty = true;
                    if ending_click || ending_ticks >= mars_rising::overlay::ENDING_TICKS {
                        if ending_click { title_mix.play(29, 4, 0); }
                        for d in 0..3u8 { title_mix.play(56, 4, d); }
                        ending_click = false;
                        mode = Mode::Game;
                        want_end = true; // the end of game flow (scores) follows
                        pacer.reset();
                        hm.reset();
                        break;
                    }
                    continue;
                }
                if mode == Mode::Entry {
                    // the name entries of the end of a game (`0x1b508`): 15 ticks of waiting, then the entries (the better score first), 40 ticks between
                    // two entries when the first was made; the sound 36 and the High Scores screen (only when a name was entered) or the title follow
                    let mut finished = None;
                    if let (Some(f), Some(table)) = (flow.as_mut(), hs_table.as_mut()) {
                        f.clock = f.clock.wrapping_add(2);
                        dirty = true;
                        if f.entry.is_none() {
                            if f.wait > 0 {
                                f.wait = f.wait.saturating_sub(2);
                            } else if !f.queue.is_empty() {
                                let p = f.queue.remove(0);
                                f.entry = Some(NameEntry::new(table, p, players == 1, f.scores[(p - 1) as usize], level_id as i32));
                            } else {
                                finished = Some(f.entered);
                            }
                        }
                        if let Some(e) = f.entry.as_mut() {
                            for (id, prio, delay) in e.take_sounds() { title_mix.play(id, prio, delay as u8); }
                            match e.done() {
                                Some(true) => {
                                    let (row, snd) = e.commit(table, &hs_strings);
                                    for (id, prio, delay) in snd { title_mix.play(id, prio, delay as u8); }
                                    f.rows[(e.player - 1) as usize] = row as i32;
                                    f.entered = true;
                                    f.wait = if f.queue.is_empty() { 0 } else { 40 };
                                    f.entry = None;
                                    save_scores(&hs_path, table);
                                }
                                Some(false) => { f.entry = None; }
                                None => {}
                            }
                        }
                    } else {
                        finished = Some(false);
                    }
                    if let Some(entered) = finished {
                        title_mix.play(36, 4, 0);
                        let rows = flow.as_ref().map(|f| f.rows).unwrap_or([-1, -1]);
                        flow = None;
                        match (entered, hs_table.as_ref(), credits_setup.as_ref()) {
                            (true, Some(table), Some(_)) => {
                                score_screen = Some((ScoreScreen::new(table, rows), ScoreCanvas::new(&buf)));
                                score_input = None;
                                mode = Mode::Scores;
                            }
                            _ => {
                                if let Some((art, t)) = title.as_mut() { *t = Title::new(art.screen, !args.show_register); }
                                play_interface();
                                mode = Mode::Title;
                            }
                        }
                        pacer.reset();
                        hm.reset();
                        dirty = true;
                    }
                    continue;
                }
                if mode == Mode::Scores && score_wipe.is_some() {
                    // the band wipe (one tick of 1/60 s per `Wipe::next`, two per logic step)
                    let mut over = false;
                    if let Some((w, pic, shown)) = score_wipe.as_mut() {
                        for _ in 0..2 {
                            match w.next() {
                                Some(bands) => {
                                    for (src, dst) in bands {
                                        for r in 0..(src[2] - src[0]) as i32 {
                                            let (sy, dy) = (src[0] as i32 + r, dst[0] as i32 + r);
                                            if sy < 0 || sy >= SCREEN_H as i32 || dy < 0 || dy >= SCREEN_H as i32 { continue; }
                                            let (a, b) = (sy as usize * SCREEN_W, dy as usize * SCREEN_W);
                                            shown[b..b + SCREEN_W].copy_from_slice(&pic[a..a + SCREEN_W]);
                                        }
                                    }
                                    if !w.more() { over = true; break; }
                                }
                                None => { over = true; break; }
                            }
                        }
                        dirty = true;
                    }
                    if over {
                        score_wipe = None;
                        score_screen = None;
                        if let Some((art, t)) = title.as_mut() { *t = Title::new(art.screen, !args.show_register); }
                        // from the title screen's button the interface music was never stopped; after a game it starts again
                        if !std::mem::take(&mut scores_from_title) { play_interface(); }
                        mode = Mode::Title;
                        pacer.reset();
                        hm.reset();
                    }
                    continue;
                }
                if mode == Mode::Scores {
                    // the High Scores screen (`0x194ac`): two ticks per logic step
                    let mut leave: Option<ScoreOutcome> = None;
                    if let (Some((ss, cv)), Some((cdata, cart))) = (score_screen.as_mut(), credits_setup.as_ref()) {
                        for _ in 0..2 {
                            for (_, op) in ss.tick(score_input.take()) {
                                if let ScoreOp::Snd(id, prio, delay) = &op { title_mix.play(*id, *prio, *delay as u8); }
                                cv.apply(&op, cart, cdata);
                            }
                        }
                        dirty = true;
                        leave = ss.outcome();
                    }
                    match leave {
                        Some(ScoreOutcome::NewGame) => {
                            score_screen = None;
                            scores_from_title = false;
                            players = 1;
                            level_id = args.level;
                            new_session = true;
                            restart = true;
                            fresh_music = true;
                            mode = Mode::Game;
                            pacer.reset();
                            hm.reset();
                        }
                        Some(ScoreOutcome::Back) => {
                            // the title window is drawn on the off-screen buffer and shown with the wipe `0x1bafc(12)`
                            match (title.as_ref(), credits_setup.as_ref(), score_screen.as_ref()) {
                                (Some((art, _)), Some((cdata, _)), Some((_, cv))) => {
                                    let t = Title::new(art.screen, !args.show_register);
                                    let mut pic = vec![0u32; SCREEN_W * SCREEN_H];
                                    draw_title_picture(art, &t, &None, &mut pic);
                                    score_wipe = Some((mars_rising::credits::Wipe::new(cdata.wipe_title, (0, 0)), pic, cv.screen().to_vec()));
                                }
                                _ => {
                                    score_screen = None;
                                    if let Some((art, t)) = title.as_mut() { *t = Title::new(art.screen, !args.show_register); }
                                    if !std::mem::take(&mut scores_from_title) { play_interface(); }
                                    mode = Mode::Title;
                                }
                            }
                            pacer.reset();
                            hm.reset();
                        }
                        None => {}
                    }
                    continue;
                }
                if let Some((_, left)) = notice.as_mut() {
                    *left = left.saturating_sub(1);
                    if *left == 0 { notice = None; dirty = true; }
                }
                if let Some(ui) = level_ui.as_mut() {
                    // the dialog's own 30 tick display of the clamped number (ticks of 1/60 s; a logic step is 1/30 s)
                    for _ in 0..2 { ui.tick(); }
                    dirty = true;
                    if let Some(n) = ui.result() {
                        level_ui = None;
                        if n > 0 {
                            players = level_players;
                            level_id = n as u32;
                            new_session = true;
                            restart = true;
                            fresh_music = true;
                            mode = Mode::Game;
                        } else if let Some((_, t)) = title.as_mut() { t.dirty = true; }
                    }
                }
                if let (Some(c), Some((_, cart))) = (credits.as_mut(), credits_setup.as_ref()) {
                    // the slideshow counts ticks of 1/60 s; a logic step is 1/30 s
                    for _ in 0..2 { c.tick(cart, &title_buf); }
                    for (id, prio, delay) in c.take_sounds() { title_mix.play(id, prio, delay as u8); }
                    dirty = true;
                    match c.outcome() {
                        Some(CreditsOutcome::NewGame) => {
                            credits = None;
                            players = 1;
                            level_id = args.level;
                            new_session = true;
                            restart = true;
                            fresh_music = true;
                            mode = Mode::Game;
                        }
                        Some(CreditsOutcome::Back) => {
                            credits = None;
                            if let Some((_, t)) = title.as_mut() { t.dirty = true; }
                        }
                        None => {}
                    }
                }
                if mode == Mode::Title && title.is_some() && credits.is_none() && level_ui.is_none() && prefs_ui.is_none() {
                    // the main loop's idle timer: ten seconds (600 ticks) without a click, a key or a button change
                    idle_steps += 1;
                    if idle_steps > 300 { idle_steps = 0; idle_fire = true; }
                } else {
                    idle_steps = 0;
                }
                continue;
            }
            let inputs = if players == 1 {
                [merge(read_keys(&win, &SOLO), pad.input(&win, 0)), PlayerInput::default()]
            } else {
                [merge(read_keys(&win, &DUO_P1), pad.input(&win, 0)), merge(read_keys(&win, &DUO_P2), pad.input(&win, 1))]
            };
            if demo.is_none() && esc_key.step(win.is_key_down(Key::KEY_ESCAPE), prefs.escape_delay()) { esc_fire = true; }
            let inputs = if let Some(pb) = demo.as_mut() {
                // 0xd68c: the recording has run out (or the game ended): the demo is over; 0x250a0 reads the recorded keys only while a ship is playing
                if pb.finished() || game.over || game.level_complete { end_demo = true; break; }
                let mut i = [PlayerInput::default(); 2];
                for who in 0..players { if game.ships[who].state == mars_rising::ship::STATE_PLAYING { if let Some(k) = pb.next(who) { i[who] = k; } } }
                i
            } else { inputs };
            game.step(inputs);
            sync_hud(&mut game, &mut hud_panel, &panel_picture, &palette, &mut hud_seen, &blitter, &mut bank)?;
            if game.over && !music_stopped { music_stopped = true; stop_music(); }
            if info == 1 {
                info_left = info_left.saturating_sub(1);
                if info_left == 0 { info = 0; }
            }
            if game.level_complete && !game.over && !game.won {
                // the coin bonus is over (D.13e8): the last level ends the game (0xe098(6000)), otherwise the next level starts (0xd7b0)
                if level_id >= LAST_LEVEL {
                    game.finish_game();
                    play_track(seq.game_won());
                } else {
                    level_id += 1;
                    if let Some(t) = seq.level_done() { play_track(t); }
                    level = load_level(root, level_id)?;
                    game.next_level(level.height, level.spawn_trigger_by_row.clone());
                    load_level_into(&mut game, &level);
                    sync_hud(&mut game, &mut hud_panel, &panel_picture, &palette, &mut hud_seen, &blitter, &mut bank)?;
                }
            }
            if game.won && !ending_shown && demo.is_none() {
                // 0xe098(6000): the picture (sound 71), then the end of game flow
                ending_shown = true;
                match load_ending(root) {
                    Ok(pic) => {
                        ending_pic = Some(pic);
                        ending_ticks = 0;
                        ending_click = false;
                        title_mix.clear_queue();
                        title_mix.play(71, 4, 0);
                        mode = Mode::Ending;
                        pacer.reset();
                        hm.reset();
                        dirty = true;
                    }
                    Err(e) => eprintln!("the ending picture is not shown: {} (run the extractor again)", e),
                }
            }
            if mode == Mode::Game && (game.over || game.won) && hs_table.is_some() && demo.is_none() { want_end = true; }
            if want_end { break; }
            if let Some(a) = &audio { a.push(&game.audio_out); }
            dirty = true;
        }
        let step_ms = t_step.elapsed().as_secs_f32() * 1000.0;

        // The end of a demo (`0x1b508` with the demo flag): sound 36, the title window is drawn again; no scores, and the interface music was never stopped
        if end_demo {
            end_demo = false;
            demo = None;
            game.set_demo(false);
            game.env.audio.sound_on = demo_sound;
            title_mix.clear_queue();
            title_mix.play(36, 4, 0);
            if let Some((art, t)) = title.as_mut() { *t = Title::new(art.screen, !args.show_register); }
            idle_steps = 0;
            mode = Mode::Title;
            pacer.reset();
            hm.reset();
            dirty = true;
        }

        // The end of a game (the loop of `0xc6f0` is over: game over, the last level won, or the quit key): the scores of the ships, who qualifies
        // (`0x199dc`: the score must beat the last row; not when the game was started above level 1 or cheated with), and the name entries
        if want_end {
            want_end = false;
            if mode == Mode::Game && flow.is_none() {
                if !music_stopped { music_stopped = true; stop_music(); }
                let sc = [game.ships[0].score, if players == 2 { game.ships[1].score } else { 0 }];
                let mut queue: Vec<u8> = Vec::new();
                if let (false, 1, Some(t)) = (session_cheated, session_start_level, hs_table.as_ref()) {
                    let (q1, q2) = (t.qualifies(sc[0]), players == 2 && t.qualifies(sc[1]));
                    // the higher score enters its name first
                    if q1 && q2 { queue = if sc[0] > sc[1] { vec![1, 2] } else { vec![2, 1] }; } else if q1 { queue = vec![1]; } else if q2 { queue = vec![2]; }
                }
                let wait = if queue.is_empty() { 0 } else { 15 };
                flow = Some(EndFlow { queue, scores: sc, rows: [-1, -1], entered: false, wait, entry: None, base: buf.clone(), clock: 0 });
                title_mix.clear_queue();
                entry_hover = None;
                mode = Mode::Entry;
                pacer.reset();
                hm.reset();
                dirty = true;
            }
        }

        let t_compose = std::time::Instant::now();
        let composed = (dirty && mode == Mode::Game) as u8;
        let mut phase = [0.0f32; 4]; // ms: level tiles, sprites, palette + overlay + HUD text, texture upload
        if mode == Mode::Title {
            if let Some(c) = credits.as_ref() {
                if dirty {
                    dirty = false;
                    buf.copy_from_slice(c.screen());
                    screen.upload(&buf).map_err(Error::Msg)?;
                }
            } else if let (Some(ui), Some((cdata, cart))) = (level_ui.as_ref(), credits_setup.as_ref()) {
                if dirty {
                    dirty = false;
                    buf.copy_from_slice(&title_buf);
                    ui.draw(&mut buf, cart, cdata, level_hover, (title_now / 15) % 2 == 0);
                    screen.upload(&buf).map_err(Error::Msg)?;
                }
            } else if let (Some(ui), Some((cdata, cart))) = (prefs_ui.as_ref(), credits_setup.as_ref()) {
                if dirty {
                    dirty = false;
                    buf.copy_from_slice(&title_buf);
                    ui.draw(&mut buf, cart, cdata, prefs_hover);
                    screen.upload(&buf).map_err(Error::Msg)?;
                }
            } else if let Some((art, t)) = title.as_mut() {
                if t.dirty || dirty {
                    t.dirty = false;
                    dirty = false;
                    draw_title_picture(art, t, &notice, &mut buf);
                    screen.upload(&buf).map_err(Error::Msg)?;
                }
            }
        } else if mode == Mode::Ending {
            if dirty {
                dirty = false;
                if let Some(pic) = ending_pic.as_ref() { screen.upload(pic).map_err(Error::Msg)?; }
            }
        } else if mode == Mode::Entry {
            if dirty {
                dirty = false;
                if let Some(f) = flow.as_ref() {
                    buf.copy_from_slice(&f.base);
                    if let (Some(e), Some((cdata, cart))) = (f.entry.as_ref(), credits_setup.as_ref()) {
                        let panel = EntryPanel::centered((PLAYFIELD_W + 160) as i32, SCREEN_H as i32);
                        let portrait = hs_portraits.as_ref().map(|p| &p[(e.player - 1) as usize]);
                        panel.draw(&mut buf, e, &hs_strings, cart, cdata, portrait, (f.clock / 30) % 2 == 0, entry_hover);
                    }
                    screen.upload(&buf).map_err(Error::Msg)?;
                }
            }
        } else if mode == Mode::Scores {
            if dirty {
                dirty = false;
                if let Some((_, _, shown)) = score_wipe.as_ref() {
                    buf.copy_from_slice(shown);
                    screen.upload(&buf).map_err(Error::Msg)?;
                } else if let Some((_, cv)) = score_screen.as_ref() {
                    buf.copy_from_slice(cv.screen());
                    screen.upload(&buf).map_err(Error::Msg)?;
                }
            }
        } else if dirty {
            dirty = false;
            // world
            frame.clear(0);
            let top = game.scroller.view_top_y(SCREEN_H as i32);
            draw_level(&mut frame, &level, &tiles, &mut bank, top)?;
            phase[0] = t_compose.elapsed().as_secs_f32() * 1000.0;
            blitter.draw_all(&mut frame, &mut bank, &game.env.draws)?;
            if ring.len() >= 60 { ring.pop_front(); }
            ring.push_back((game.now, top, frame.pixels.clone()));
            phase[1] = t_compose.elapsed().as_secs_f32() * 1000.0 - phase[0];

            for y in 0..SCREEN_H {
                for x in 0..PLAYFIELD_W { buf[y * SCREEN_W + x] = palette.argb(frame.pixels[y * PLAYFIELD_W + x]); }
                // the side panel: the original's score bar with everything the HUD drew on it (hud.rs / hud_panel.rs)
                for x in PLAYFIELD_W..SCREEN_W { buf[y * SCREEN_W + x] = palette.argb(hud_panel.panel.pixels[y * hud_panel.panel.width + x]); }
            }

            // debug targets (stand-ins for enemies)
            for t in game.env.targets.iter().filter(|t| t.active) {
                let flash = game.now.wrapping_sub(t.hit_tick) < 3;
                let fill = if flash { 0xFFFF_FFFF } else { 0xFFB0_3030 };
                let (x0, y0) = (t.x as i32, t.y as i32);
                for yy in y0.max(0)..(y0 + t.h).min(SCREEN_H as i32) {
                    for xx in x0.max(0)..(x0 + t.w).min(PLAYFIELD_W as i32) {
                        let edge = xx == x0 || yy == y0 || xx == x0 + t.w - 1 || yy == y0 + t.h - 1;
                        buf[yy as usize * SCREEN_W + xx as usize] = if edge { 0xFFFF_C0C0 } else { fill };
                    }
                }
            }

            // overlay on the playfield (F5): key help, or the diagnostics of the test build
            if info != 0 {
                let mut lines: Vec<(String, u32)> = Vec::new();
                if info == 2 {
                    for (i, label) in ["P1", "P2"].iter().enumerate().take(players) {
                        for l in ship_lines(&game.ships[i], label) { lines.push((l, 0xFFE0_E0E0)); }
                    }
                    for l in [
                        format!("LEVEL {}  ROW {}/{}", level_id, game.scroller.row(), level.height),
                        format!("TARGETS {}  KILLS {}", game.env.active_targets(), game.env.total_kills),
                        match &game.env.ground { Some(g) => format!("GROUND {}/{} DESTROYED", g.destroyed, g.targets), None => String::new() },
                        format!("SHOTS {}  POWERUPS {}", game.shots.count, game.pickups.count),
                        format!("SHADOWS {}  TRANSLUCENT {}", if game.opts.shadows { "ON" } else { "OFF" }, if game.opts.translucent { "ON" } else { "OFF" }),
                        match pacer.display_state() {
                            DisplayState::Measuring => "DISPLAY: MEASURING".to_string(),
                            DisplayState::Locked { n, hz } => format!("DISPLAY {:.1} HZ  LOCKED 1:{}", hz, n),
                            DisplayState::NoVsync { hz } => format!("NO VSYNC! {:.0} FPS", hz),
                            DisplayState::OddRate { hz } => format!("DISPLAY {:.1} HZ  UNEVEN", hz),
                        },
                        format!("HITCHES {}  MISSED REFRESH {}", hm.odd, hm.slow),
                        last_slow.clone().unwrap_or_default(),
                        format!("COMPOSITOR SYNC {}  UPLOAD {}", if dwm_sync { "ON" } else { "OFF" }, if args.lazy_upload { "ON STEPS" } else { "EVERY FRAME" }),
                        format!("SOUND {}  VOICES {}", if game.env.audio.sound_on { "ON" } else { "OFF" }, game.env.audio.voice_count()),
                    ] { lines.push((l, 0xFF80_C0FF)); }
                }
                let esc = if title.is_some() { "ESC TITLE SCREEN" } else { "ESC QUIT" };
                let help: Vec<&str> = if players == 1 {
                    vec!["ARROWS MOVE  Z FIRE", "HOLD Z = CHARGE BEAM", "P PAUSE  R RESTART", "[ ] LEVEL  F1 F2 F3  M SOUND  N MUSIC", "F4 SCALING  F5 THIS TEXT  F6 SAVE PICTURES", "F11 FULLSCREEN", "1-9 0 - = POWER-UPS", esc]
                } else {
                    vec!["P1 WASD SPACE  P2 ARROWS ENTER", "P PAUSE  R RESTART", "[ ] LEVEL  F1 F2 F3  M SOUND  N MUSIC", "F4 SCALING  F5 THIS TEXT  F6 SAVE PICTURES", "F11 FULLSCREEN", "1-9 0 - = POWER-UPS", esc]
                };
                for l in &help { lines.push((l.to_string(), 0xFFD0_D0E0)); }
                let mut y = 6;
                for (l, c) in &lines {
                    draw_text(&mut buf, SCREEN_W, 7, y + 1, l, 1, 0xFF00_0000);
                    draw_text(&mut buf, SCREEN_W, 6, y, l, 1, *c);
                    y += 11;
                }
            }
            // the original's message overlay (overlay.rs, 0x28518): the sprites of GAME OVER are in the draw list, the texts go on top of the picture. A pause
            // shows its own text and gives the old state back afterwards (0xcf78 / 0xdfac).
            if let Some((cdata, cart)) = credits_setup.as_ref() {
                let state = if paused { mars_rising::overlay::PAUSED } else { game.env.msg };
                mars_rising::overlay::draw_text(&mut buf, SCREEN_W, cart, cdata, state, game.level_number());
            }
            // what the port adds: how to go on when there is no title screen to return to
            let banner = if game.over { Some("R TO RESTART") } else if game.won { Some("YOU WIN - R TO RESTART") } else if paused && credits_setup.is_none() { Some("PAUSED") } else { None };
            if let Some(b) = banner {
                let w = mars_rising::font::text_width(b, 1) as i32;
                draw_text(&mut buf, SCREEN_W, (PLAYFIELD_W as i32 - w) / 2, 250, b, 1, 0xFFFF_FFFF);
            }

            let t_up = std::time::Instant::now();
            phase[2] = t_compose.elapsed().as_secs_f32() * 1000.0 - phase[0] - phase[1];
            screen.upload(&buf).map_err(Error::Msg)?;
            phase[3] = t_up.elapsed().as_secs_f32() * 1000.0;
            let total: f32 = phase.iter().sum();
            if total > 30.0 && args.stats {
                report.push(format!("SLOW FRAME at {:.2} s: {:.1} ms = tiles {:.1} + sprites {:.1} + palette/HUD {:.1} + texture upload {:.1} (sprites read from disk so far: {})", clock.elapsed().as_secs_f32(), total, phase[0], phase[1], phase[2], phase[3], bank.late_loads));
            }
        }
        if !args.lazy_upload && mode == Mode::Game && composed == 0 {
            let t_rep = std::time::Instant::now();
            screen.repeat().map_err(Error::Msg)?;
            repeat_ms = t_rep.elapsed().as_secs_f32() * 1000.0;
        }
        if args.stats && mode == Mode::Game {
            let pacer_info = match (pacer.measured_frame_time(), pacer.locked_to()) {
                (Some(m), Some(n)) => format!("display {:.2} Hz, locked: 1 logic step per {} frames", 1.0 / m, n),
                (Some(m), None) => format!("display {:.2} Hz, not lockable: paced by time", 1.0 / m),
                _ => "measuring the display".to_string(),
            };
            let audio_info = match &audio {
                Some(a) => { let (q, u) = a.status(); format!("audio queue {} samples, {} underruns", q, u) }
                None => "no audio".to_string(),
            };
            st.frame(dt, due, step_ms, t_compose.elapsed().as_secs_f32() * 1000.0, bank.late_loads, &pacer_info, &audio_info, &mut report);
            if let Some(g) = hitch {
                report.push(format!("hitch at {:.2} s: {} display frames between logic steps (frame time {:.1} ms, previous compose {:.1} ms, present {:.1} ms)", clock.elapsed().as_secs_f32(), g, dt * 1000.0, st.max_compose, last_present_ms));
            }
        }
        if mode == Mode::Game && !paused {
            if let Some(t) = trace.as_mut() {
                use std::io::Write;
                let state = match pacer.display_state() {
                    DisplayState::Measuring => "measuring".to_string(),
                    DisplayState::Locked { n, hz } => format!("locked-{}@{:.2}", n, hz),
                    DisplayState::NoVsync { hz } => format!("NO-VSYNC@{:.0}", hz),
                    DisplayState::OddRate { hz } => format!("odd-rate@{:.2}", hz),
                };
                let q = audio.as_ref().map(|a| a.status().0).unwrap_or(0);
                let _ = writeln!(t, "{:.4},{:.3},{},{:.3},{:.3},{:.3},{},{},{},{:.3},{:.3},{:.3},{:.3},{},{:.2},{},{},{},{},{},{},{},{},{},{:.3},{:.3}", clock.elapsed().as_secs_f32(), dt * 1000.0, due, step_ms, t_compose.elapsed().as_secs_f32() * 1000.0, last_present_ms, q, state, hitch.unwrap_or(0), phase[0], phase[1], phase[2], phase[3], bank.late_loads, wd_ms, composed, game.now, game.scroller.view_top_y(SCREEN_H as i32), game.env.draws.len(), game.ships[0].x as i32, game.ships[0].y as i32, game.ships[0].shield, game.ships[0].lives, marker, repeat_ms, last_dwm_ms);
            }
        }

        let compose_ms = t_compose.elapsed().as_secs_f32() * 1000.0;
        let (ww, wh) = (win.get_screen_width(), win.get_screen_height());
        let t_present = std::time::Instant::now();
        let mut d = win.begin_drawing(&thread);
        screen.present(&mut d, ww, wh);
        drop(d); // the buffer swap (and with it the wait for the vertical blank) happens here
        last_present_ms = t_present.elapsed().as_secs_f32() * 1000.0;
        if dwm_sync && mode == Mode::Game && !win.is_window_fullscreen() {
            let t_dwm = std::time::Instant::now();
            compositor_sync();
            last_dwm_ms = t_dwm.elapsed().as_secs_f32() * 1000.0;
            // safety: if the swap did wait for the refresh after all, this wait would come on top of it and halve the frame rate; then stop doing it
            dwm_frames += 1;
            dwm_time += dt;
            if dwm_frames == 120 {
                if dwm_time / 120.0 > 0.0225 { dwm_sync = false; eprintln!("compositor sync switched off: the frames took {:.1} ms on average with it", dwm_time * 1000.0 / 120.0); }
                dwm_frames = 0; dwm_time = 0.0;
            }
        } else { last_dwm_ms = 0.0; }
        prev_times = [step_ms, compose_ms, last_present_ms, last_dwm_ms, phase[3]];
    }
    hb_stop.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(t) = trace.as_mut() {
        use std::io::Write;
        let _ = t.flush();
    }
    for l in &report { eprintln!("{}", l); }
    Ok(())
}
