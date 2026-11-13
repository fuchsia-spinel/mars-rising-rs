# Mars Rising — Rust port

## Fuchsia's note

This plays and works with keyboard and gamepads on Windows. I have not verified other platforms. I have not played past the second level.

This port exists because the other attempt to port it was both closed source and Mac-only. I wanted to play the game on Windows.

I had Claude build this for myself, so that I could play it again. I will only be maintaining it insofar as it inhibits play as I'm playing it.

There are keys that allow you to "cheat" built in. You can see them when the game loads into the first level or by pressing F5. If you want to play the game like normal, just ignore them.

I will not distribute the assets. You have to get them yourself. Instructions are below. Simply put the executable next to the `assets` folder the script generates to play.

---

A Rust reimplementation of Ambrosia Software's *Mars Rising* (1998, Mac, PowerPC). The port **never reads Mac
resource forks**: you first convert your own copy of the game into ordinary files, and the Rust code loads those.

**Status: early playable test build.** You can fly the ship, shoot (bursts and the charged wave beam), collect power-ups, take damage, die and respawn;
enemies spawn from the level's groups, fly their 22 behaviour routines, shoot back and can be shot down (see "Playing" below); the bosses (special groups, types 2, 4, 5 with their parts and alarm sound) come at their level positions. The logic modules (`ship`, `pickup`, `draw`, `blit`, `ship_draw`, `game`, `scroll`, `input`, `font`)
are compiled and tested with std only. The parts that use external crates (`assets`, `render`, `compose`, `viewer`, `play`) were **type-checked against stub crates but never linked
with the real `png` / `serde` / `raylib` / `hound` or run** (the environment they were written in could not reach crates.io), so expect to fix small build or runtime errors there and please report them.
The Python extractor has been run and verified.

## 1. Extract your own assets (you need your own copy of the game)
Get the installer's data fork: unpack `Mars Rising-1.01.sit` (MD5: `57E3B8FA7B0ED12085AB087CFBA83652`) with `unar` / `stuffit-rs`, giving `Mars Rising Installer`
(+ `.rsrc`). Then, with any Python 3 (standard library only):

    python3 tools/mr_extract.py "Mars Rising Installer" assets/

An installed game folder also works if resource forks are present as `<name>.rsrc`, AppleDouble `._<name>`
or `<name>/..namedfork/rsrc`:

    python3 tools/mr_extract.py "/path/to/Mars Rising 1.0.1 ƒ" assets/

Output (~1,470 files): PNG sprites/tiles/pictures, WAV sounds, JSON levels/strings/menus, raw LUT tables, MADH
music, docs. Both input modes give byte-identical output. See `ASSET_FORMATS.md`.

## 2. Build and run
    cargo test --release                    # unit tests
    MR_ASSETS=assets cargo test --release   # + checks 5 rendered frames against the independent Python reference
    cargo run --release --bin viewer -- [assets] 1     # Up/Down scroll, Left/Right level, Shift = fast, Esc = quit
    # (the test suite also compares level 5 with the original game's screenshot: >88 % pixel match required)

## 3. Playing (work in progress)
    cargo run --release --bin play -- [assets] [--level 1] [--players 1|2] [--seed N] [--scale 1..8] [--mute] [--stats] [--no-menu] [--show-register] [--lazy-upload] [--no-dwm-sync]

The game opens on the **title screen** (the original's main window): hover a button with the mouse (it lights up with the original's sounds), click **1 Player** or
**2 Players** to start at level 1 (`--level N` starts the game at level N instead), **Quit** leaves; Controls is not ported yet and
(it only shows a notice). Keyboard: Up / Down (or W / S) move the highlight, Enter / Space / Z press it; pad: D-pad or stick, A or Start. Esc in the game goes back to the title,
Esc on the title quits. `--no-menu` skips the title and starts the game directly (with `--players`); `--show-register` shows the Register button of an unregistered copy. The picture is sent to the GPU on every refresh, not only on the 30 Hz steps (`--lazy-upload` turns that off); in a window on Windows it also waits for the desktop compositor's refresh after each swap (`--no-dwm-sync` turns that off): every refresh then does the same GPU work, which made the window-system hitching rarer in tests. F6 in a game saves the last 60 pictures (`tools/analyze_capture.py` reads them).

All sprites are decoded up front (a few seconds at start-up, parallel); `--stats` prints a frame-time report to the console every 5 seconds (please include
it when reporting stutter). Gamepads work alongside the keyboard: first connected pad = player 1, second = player 2; left stick or D-pad moves, A / right shoulder
or trigger fires, B / X / left shoulder or trigger = button 2, Start = pause (restart after game over).

The window is a vsynced raylib window (raylib-rs 6.0; the first build compiles raylib itself and needs **CMake and a C compiler**). It can be resized
and goes fullscreen with F11. The picture is scaled by a whole number when it fits (black bars around it; F4 switches to free scaling that keeps the aspect
ratio), which keeps every pixel the same size when things move. `--scale` is only the starting window size (default 2). The logic runs at the original's
30 frames per second in whole steps, independent of the display's refresh rate. Sound plays through `rodio` (cargo feature `audio`, on by default; on Linux it needs the ALSA
development files, e.g. `apt install libasound2-dev`). If that does not build or you have no audio device, use `--no-default-features --features window` (silent `play`)
or `--mute`.

* One player: arrows move, **Z** fires (a tap fires a burst; holding charges the wave beam, releasing fires it; holding too long overheats and destroys the ship, as in the original), X = button 2 (bombs: they fly to the bomb sight and hit ground targets).
  Two players: P1 W A S D + Space / Left Ctrl, P2 arrows + Enter / Right Shift. P pause, R restart, [ ] change level, F1 shadows, F2 translucency, F3 debug targets, F4 integer / free scaling, F11 fullscreen, M sound on/off, N music on/off, Esc back to the title screen (quit when started with `--no-menu`).
* Power-up cheats in front of player 1: 1-9 = spread, weapon level, missiles, extra life, keep-weapons insurance, rear shots, smart bomb, drone, shield; 0 coin; - special charge; = speed-up.
* **Real (verified against the original executable):** ship movement, bursts, shots, charged beam, overheating, shield / damage / destruction / respawn, power-ups and their effects, extra lives from score,
  and the drawing of all of that (ship, muzzle flashes, charge glow, shield ring, blink / hit flash, shadows, beam, shots, power-ups). Logic runs at 30 frames per second like the original.
  **Sound effects** use the original's sound engine (`src/audio.rs`: 6 mixed voices of 16, priorities, delayed sounds, playback rates, 8-bit mono mixing at 22254.5 Hz), checked bit for bit against the original's mixer.
  Every sound the ported code requests is heard (ship, shots, beam, power-ups, enemies, ground objects, explosions and their debris); the menus come with their own part.
* **Enemies (verified against the executable):** the pool and spawn groups, all 22 behaviour routines (types 1-21, 23), damage, contact with the ships, the draw/cull pass, the smart bomb, group-wipe bonuses, and the enemy-shot pool (10 shot kinds, beam, movement, hits on the ships, clear-all, drawing).
  Each of these is checked bit for bit against the original code run in an emulator (`tests/enemy_golden.rs`, `enemy_draw_golden.rs`, `enemy_shot_golden.rs`, `bonus_golden.rs`).
* **Level end (verified against the executable):** five rows before the end: sound, silent smart bomb, then the coin bonus panel (each coin pays level x 100, max 1000), the coins left on the screen explode, and the next level starts with score, lives and weapons kept; after level 28 the ending picture is shown and the game ends. The level number now also sets difficulty and the weapons the ships start with, as in the original.
* **Ground objects (verified against the executable):** turrets, missile towers, tanks and the other ground objects (~7,000 placed in the 28 levels) activate as their row scrolls in, aim and fire at the ships, take damage from bombs (button 2) and the smart bomb only, leave wrecks and scorch marks, drop power-ups, and the last one of a level pays the 0xea4c reward (`tests/ground_golden.rs`, `bomb_golden.rs`, `clear_golden.rs`).
* **Stand-ins:** the red boxes (F3) are optional *debug targets*. Nothing stops enemy shots.
* **Bosses (verified against the executable):** registration, spawn, movement, shots, damage, explosions, pickups and alarm loop (`tests/boss_golden.rs`).
* **HUD (verified against the executable):** the right-hand panel is the original's: score digits, wave-motion power bar, shield bar, lives, weapon / armour icons and the dimmed panel of an absent player (`tests/hud_golden.rs`). F5 shows key help / diagnostics on the playfield.
* **Music (verified against the executable):** the original's PlayerPRO engine (`src/madh.rs`, `src/madplay.rs`) plays the five MADH modules bit for bit like the original driver (`tests/madplay_vectors.rs`); the title screen plays the interface track ("Jingle Bells" on December 24 / 25, `--date MM-DD` to test), new games use level music 1 / 2 alternating every third level, the ending level music 3. N switches the music on/off, P pauses it with the game.
* **Credits (verified against the executable):** the title screen's Credits button shows the original's slideshow (the game's own bitmap font, the 8-line band wipe, 4 s per page, click or key to leave, **N** starts a game); hold Ctrl (reading list), Alt (viewing / playing / listening) or Shift (the last pages, "In memorium") when you click it for the other page sets. It needs `layout/title_text.txt`, which the extractor now makes: run `tools/mr_extract.py` again if the button says so.
* **High scores (verified against the executable):** when a game ends (game over, the last level won, or Esc) a score that beats the 10th row gets the name entry (type, Enter / OK; Esc / Cancel skips; at most 20 characters; the field starts with your last name; two players: the higher score goes first), then the High Scores screen (the new rows blink; a click or key leaves, **N** starts a game; 10 s time-out). Not counted: games started above level 1 (`--level`) and games where the debug keys, R or [ ] were used. The title's High Scores button shows the table. The table is saved in `highscores.bin` (`%APPDATA%\mars_rising_rs` on Windows, else `$XDG_DATA_HOME` or `~/.local/share`); delete the file to reset it. Needs `layout/title_text.txt` and `misc/default_scores.bin` from the extractor (run it again).
* **Preferences (verified against the executable):** the title's Preferences button opens the original's dialog (two sections, Save / Cancel / Factory Settings / Revert; mouse, or arrows + Enter / Space, Tab or PageUp / PageDown for the section, Esc cancels). Working here: Escape Key Delay (hold Esc about a second to end a game), Translucency, Extra Detail, Realtime Shadows, Sound Volume (and "Use System Sound Volume", the default, which leaves the output at full), Music Type (MOD Track / None; there is no CD), Play Interface Music. Greyed (stored, no effect): Resize Screen, Line Skipping, Music Quality / Mode / Surround / Filtering. Saved in `prefs.bin` next to `highscores.bin`. Hovering an option shows the original's help text (new `help` lines in `ui/dialog_text.txt`: run the extractor again). Needs `ui/dialog_text.txt` from the extractor.
* **Level chooser (verified against the executable):** hold **Alt** (the original's Option key) while clicking 1 Player / 2 Players: "Start at which level?" (type a number, Enter / OK; Esc / Cancel; the number is clamped to the last level; high scores are off when starting above level 1). Needs `ui/dialog_text.txt` from the extractor.
* **Attract mode (verified against the executable):** after ten idle seconds the title screen alternates the original's demo (the three recorded games, replayed with the original's `Random()` and the stored seed; any key or click ends it) and the High Scores screen. Keys on the title: 1 / N / Enter one player, 2 two players, P, Q, C, H or S (High Scores), D (demo), X (the quotation page of the original's reminder screen).
* **In-game messages and ending (verified against the executable's code, `src/overlay.rs`):** "Sector N / Get Ready!" at the start of a level, "Sector N Cleared", the GAME OVER sprites (also shown during the attract-mode demo, as in the original), the pause text (the original's pause key is Caps Lock, the port's is P), and after the last level the ending picture (53 s or a click / key) followed by the high scores. Needs `pictures/app/imag_130_*.png` and `layout/title_text.txt` from the extractor.
* **Missing:** the dialog behind the Controls button (a system dialog in the original).
* Assumptions still open: the defaults of the translucency / shadow options (both assumed on), and the 30 fps limiter being on by default (`D.13ca` = 1 in the data section).

The assets are © Ambrosia Software / David Wareing and must not be committed to or redistributed with this repo.

## Known visual bug fixed: double-drawn ground objects
Not a timing problem (the traces showed perfectly even frames): on the step where a ground object becomes part of the background it was drawn twice, so its
drop shadow was darkened twice for one logic step (two display frames). See RE_NOTES.md, "Bake frame".

## Stutter diagnostics
The logic runs at 30.08 steps per second (33.25 ms, like the original) and the window is vsynced. On a 60 Hz display `present::Pacer` locks to exactly one step per two refreshes, which is perfectly even
(simulated in `present.rs`: 0 odd gaps in minutes). **If vsync is not actually in effect** (a driver setting that forces it off, a compositor), the steps and the refreshes drift against each other and
one step lands a refresh early about every 7-13 seconds, which looks like a hitch every ~10 s. To see which case a machine is in:
* the side panel shows `DISPLAY 60.0 HZ LOCKED 1:2` (good), `NO VSYNC! <fps>` (vsync is off) or `UNEVEN`, and `HITCHES n  MISSED REFRESH n` (display frames between steps that were not the usual number / frames that took far longer than one refresh);
* `play assets --stats` collects a frame-time summary every 5 s (with the display state), a `hitch at <time>` line for each odd gap and a `SLOW FRAME` line for every frame whose composing took over 30 ms, and **prints them when the game ends** (a console write while playing can take several ms, so it would cause hitches of its own; the first versions printed live and the frame after each print was ~4 ms late);
* `play assets --trace trace.csv` writes one line per displayed frame (frame time, steps, logic / compose / present time, compose split into tiles / sprites / palette+HUD / texture upload, sprites read from disk, the longest sleep of a 1 ms watchdog thread, audio queue, display state; buffered in memory and written when the game ends, so quit with Esc): send it in if the hitch is still there. 
  The screen texture is now double-buffered (two textures used alternately): with one texture the upload of every new picture blocked ~16 ms inside the graphics driver.
  The pacer now estimates the display period with a trimmed mean (the middle 60 % of the last frame times, ignoring the long frames of start-up) and locks after 16 frames instead of 90: the third trace showed a hitch 1.6 s into the game and another when the old pacer finally locked at 2.1 s, both while it was still in its time-accumulator mode (`present.rs` tests `the_first_second_of_a_real_trace_is_paced_evenly`, `the_start_up_of_a_real_run_...`; note: those tests pass for the new pacer, I could not make the old one fail them in a replay, so they guard the cadence rather than prove the cause).
  First trace from a 60 Hz machine: vsync in effect, locked 1:2, 0 uneven gaps in 71 s, but one 100-117 ms stall at 29.6 s after program start in both of two runs (inside the texture upload in one, the buffer swap in the other, the audio queue ran dry meanwhile): a stall in the graphics driver / system that is tied to wall-clock time, not to the game. Composing a frame itself costs ~1 ms (tiles 0.4, sprites 0.1, palette + HUD 0.4); the 15.8 ms 'compose + upload' of earlier traces was the upload waiting for the vertical blank. The watchdog column says whether the whole process stalled or only the main thread.

## Verified against the original executable
`src/ship.rs` (ship movement, player shots, fire control, charge/wave beam) is checked by `tests/ship_golden.rs`, which compares the Rust code with the
**original PowerPC routines executed in an emulator**: `tests/vectors/ship_vectors.txt` was made by `python3 tools/re/gen_ship_vectors.py MarsRising.pef out.txt <seed> <n>`
(needs the game's executable, `mr_extract.py --with-executable`). `src/blit.rs` (the five sprite blitters, 600 vectors) and the draw routines of `src/ship_draw.rs` / `PickupPool::draw` (840 vectors) are checked by `tests/blit_golden.rs` and `tests/draw_golden.rs` (`tools/re/gen_blit_vectors.py`, `gen_draw_vectors.py`). `src/pickup.rs` (power-up spawner, per-frame update, collection effects) is checked the same way by `tests/pickup_golden.rs` and `tests/vectors/pickup_vectors.txt` (`tools/re/gen_pickup_vectors.py`). `src/audio.rs` (the sound engine, 360 vectors from `tools/re/gen_audio_vectors.py`) is checked by `tests/audio_golden.rs`. `ship.rs`, `pickup.rs`, `audio.rs` and their tests use only the standard library.

## Reverse-engineering helpers
`tools/re/ppcdis.py` (PEF loader + disassembler: `dis|func|callers|refs|imports|data`), `tools/re/lift.py` (disassembly -> pseudo-C), `tools/re/ppcemu.py`
(run single routines with mocks). All take the extracted executable, e.g. `python3 -I tools/re/ppcdis.py assets/executable/MarsRising.pef func 0x25264`.

`python3 tools/mr_extract.py <installer> assets/ --with-executable` also saves the game's PowerPC executable to `assets/executable/`.
`tools/re/enemy_types.py` derives the enemy table (`data/enemy_types.json`, compiled into `src/enemies.rs`) from it; see `RE_NOTES.md`.
