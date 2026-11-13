//! The preferences: the original's preference block and the logic of its Preferences dialog (`0x6810`, `DLOG`/`DITL` 190 with the two pages `DITL` 191 / 192).
//!
//! The block (10240 bytes at `*D.932c`, accessors `0xae6c` / `0xa9ac` for the byte preferences 1..=100 at offset `index + 1`, `0xaed4` / `0xaa20`
//! for the short preferences 1..=100 at `102 + 2 * (index - 1)`, big endian) also holds the key sets (record `n` at `702 + 36 * (n - 1)`: a 12 byte name
//! and twelve key codes). Only the part that matters is kept here: bytes `0..304` and the first key set record.
//!
//! Byte preferences used by the dialog:  55 resize the screen, 56 line skipping, 57 interface music, 58 system sound volume, 59 escape key delay,
//! 60 translucency (`D.12be`), 61 extra detail (`D.13c8`), 62 realtime shadows (`D.12bc`), 63 surround music, 64 music filtering.
//! Short preferences:  51 sound volume (0..=8, `0x45f40` scales it to `v * 32`, 128 when out of range), 55 music type (1 MOD, 2 CD, 3 none),
//! 56 music quality (1..=7), 57 music mode (1 mono, 2 stereo, 3 deluxe stereo), 53 / 54 the selected key set of the players.
//!
//! [`Dialog`] is the item handling of `0x6810` as a state machine: `hit(item, value)` is one `ModalDialog` result (`value` = what the control
//! reads, `0x83e4`), the returned [`Op`]s are the calls the original makes (control values, enabling, sounds and the side effects). It is verified
//! against the original (`tools/re/gen_prefs_vectors.py`, `tests/vectors/prefs_vectors.txt`).

use std::fmt;
use std::path::Path;

/// Bytes of the block that are kept: the preference region.
pub const REGION: usize = 304;
/// The first key set record (`0xab10(1, ...)`).
pub const KEY_RECORD: usize = 36;

/// Item numbers of the dialog (the appended `DITL`s continue after the 17 items of `DITL` 190).
pub mod item {
    pub const SAVE: i16 = 1;
    pub const CANCEL: i16 = 2;
    pub const FACTORY: i16 = 3;
    pub const REVERT: i16 = 4;
    pub const PAGE_MENU: i16 = 11;
    // page 1 (DITL 191)
    pub const RESIZE: i16 = 18;
    pub const ESCAPE_DELAY: i16 = 19;
    pub const LINE_SKIPPING: i16 = 20;
    pub const TRANSLUCENCY: i16 = 21;
    pub const EXTRA_DETAIL: i16 = 22;
    pub const SHADOWS: i16 = 23;
    // page 2 (DITL 192)
    pub const VOLUME: i16 = 19;
    pub const SYSTEM_VOLUME: i16 = 20;
    pub const MUSIC_TYPE: i16 = 22;
    pub const MUSIC_QUALITY: i16 = 24;
    pub const MUSIC_MODE: i16 = 26;
    pub const SURROUND: i16 = 27;
    pub const FILTERING: i16 = 28;
    pub const INTERFACE_MUSIC: i16 = 29;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    region: [u8; REGION],
    keys: [u8; KEY_RECORD],
}

impl Default for Prefs {
    /// A block of zeros with the factory settings applied (`0xb064(0)`).
    fn default() -> Prefs { Prefs::factory() }
}

impl Prefs {
    pub fn zeroed() -> Prefs { Prefs { region: [0; REGION], keys: [0; KEY_RECORD] } }

    pub fn from_parts(region: &[u8], keys: &[u8]) -> Prefs {
        let mut p = Prefs::zeroed();
        p.region.copy_from_slice(&region[..REGION]);
        p.keys.copy_from_slice(&keys[..KEY_RECORD]);
        p
    }

    pub fn region(&self) -> &[u8] { &self.region }
    pub fn keys(&self) -> &[u8] { &self.keys }

    /// `0xae6c(index)`: byte preference 1..=100 (`index + 1` into the block).
    pub fn byte(&self, index: usize) -> u8 { self.region[index + 1] }
    /// `0xa9ac(index, value)`.
    pub fn set_byte(&mut self, index: usize, v: u8) { self.region[index + 1] = v; }
    /// `0xaed4(index)`: short preference 1..=100.
    pub fn short(&self, index: usize) -> i16 {
        let o = 102 + 2 * (index - 1);
        i16::from_be_bytes([self.region[o], self.region[o + 1]])
    }
    /// `0xaa20(index, value)`.
    pub fn set_short(&mut self, index: usize, v: i16) {
        let o = 102 + 2 * (index - 1);
        self.region[o..o + 2].copy_from_slice(&v.to_be_bytes());
    }

    /// The factory settings on a fresh block (what the original has before any file exists is `first_run`).
    pub fn factory() -> Prefs {
        let mut p = Prefs::zeroed();
        p.apply_defaults(false);
        p
    }

    /// `0xb064(first_run)`: the first run also clears the byte preferences 51..=54 and 59 before the defaults
    /// (`0xb0d0` sound, `0xb14c` key set 1 and its selection, `0xb1e4` display).
    pub fn apply_defaults(&mut self, first_run: bool) {
        if first_run {
            for i in [51, 52, 53, 54, 59] { self.set_byte(i, 0); }
        }
        // 0xb0d0
        self.set_byte(58, 1);
        self.set_byte(63, 0);
        self.set_byte(64, 0);
        self.set_short(51, 4);
        self.set_short(52, 4);
        self.set_short(56, 7);
        self.set_short(57, 3);
        self.set_short(55, 1);
        // 0xb14c: key set 1 = "1 Player Def" (the first 12 bytes of the string at D.325a) and the codes below
        self.set_short(53, 1);
        self.set_short(54, 1);
        const NAME: &[u8; 12] = b"1 Player Def";
        const CODES: [i16; 12] = [123, 124, 126, 125, 55, 58, 86, 88, 91, 87, 121, 119];
        self.keys[..12].copy_from_slice(NAME);
        for (i, c) in CODES.iter().enumerate() { self.keys[12 + 2 * i..14 + 2 * i].copy_from_slice(&c.to_be_bytes()); }
        // 0xb1e4
        self.set_byte(56, 0);
        self.set_byte(55, 1);
        self.set_byte(57, 1);
        self.set_byte(59, 0);
        self.set_byte(60, 1);
        self.set_byte(61, 1);
        self.set_byte(62, 1);
    }

    /// `0x45f40` of the volume preference: 0..=8 -> `v * 32` (0..=256, the Sound Manager output volume where 256 is full), else 128.
    pub fn volume_scale(&self) -> u32 {
        let v = self.short(51);
        if (0..=8).contains(&v) { (v as u32) << 5 } else { 128 }
    }

    // --- the named preferences
    pub fn resize_screen(&self) -> bool { self.byte(55) != 0 }
    pub fn line_skipping(&self) -> bool { self.byte(56) != 0 }
    pub fn interface_music(&self) -> bool { self.byte(57) != 0 }
    pub fn system_volume(&self) -> bool { self.byte(58) != 0 }
    /// The Esc key has to be held for about a second (`0xd3fc`, see [`EscKey`]).
    pub fn escape_delay(&self) -> bool { self.byte(59) != 0 }
    pub fn translucency(&self) -> bool { self.byte(60) != 0 }
    pub fn extra_detail(&self) -> bool { self.byte(61) != 0 }
    pub fn shadows(&self) -> bool { self.byte(62) != 0 }
    pub fn surround(&self) -> bool { self.byte(63) != 0 }
    pub fn filtering(&self) -> bool { self.byte(64) != 0 }
    pub fn volume(&self) -> i16 { self.short(51) }
    pub fn music_type(&self) -> i16 { self.short(55) }
    pub fn music_quality(&self) -> i16 { self.short(56) }
    pub fn music_mode(&self) -> i16 { self.short(57) }

    // --- the port's file: the region and the key record
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = self.region.to_vec();
        v.extend_from_slice(&self.keys);
        v
    }

    pub fn from_bytes(b: &[u8]) -> Option<Prefs> {
        if b.len() != REGION + KEY_RECORD { return None; }
        Some(Prefs::from_parts(&b[..REGION], &b[REGION..]))
    }

    pub fn load(path: &Path) -> Prefs {
        std::fs::read(path).ok().and_then(|b| Prefs::from_bytes(&b)).unwrap_or_else(|| {
            let mut p = Prefs::zeroed();
            p.apply_defaults(true);
            p
        })
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() { std::fs::create_dir_all(d)?; }
        std::fs::write(path, self.to_bytes())
    }
}

/// What the dialog asks the caller to do (the original's calls, in order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// `0x45bac(id, priority, delay)`.
    Snd(i32, i32, i32),
    /// `0x752c(id)`: append the item list of `DITL` 191 / 192 (after removing the previous page, `Clear`).
    Append(i16),
    Clear,
    /// `0x8420(item, value)`: set a control.
    Set(i16, i16),
    /// `0x82a0` / `0x82f0`: enable / disable an item.
    Enable(i16),
    Disable(i16),
    /// The Music Type menu: `EnableItem` / `DisableItem` of the menu items 1 (MOD track) and 2 (CD).
    MenuEnable(i16),
    MenuDisable(i16),
    /// `0x45f6c`: apply the volume.
    Volume,
    /// `0x45848`: apply the music type.
    Music,
    /// `0x1d180`: rebuild the key set menu.
    Keys,
    /// `0xa4d4`: write the preference file.
    Save,
    /// `0x29ac` / `0x299c` / `0xe35c`: the display switches.
    Translucent(u8),
    Shadows(u8),
    Detail(u8),
}

impl fmt::Display for Op {
    /// The token of the golden vectors.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Op::Snd(a, b, c) => write!(f, "snd:{}:{}:{}", a, b, c),
            Op::Append(i) => write!(f, "app:{}", i),
            Op::Clear => write!(f, "clr"),
            Op::Set(i, v) => write!(f, "set:{}:{}", i, v),
            Op::Enable(i) => write!(f, "en:{}", i),
            Op::Disable(i) => write!(f, "dis:{}", i),
            Op::MenuEnable(i) => write!(f, "men:{}", i),
            Op::MenuDisable(i) => write!(f, "mdis:{}", i),
            Op::Volume => write!(f, "vol"),
            Op::Music => write!(f, "mus"),
            Op::Keys => write!(f, "keys"),
            Op::Save => write!(f, "save"),
            Op::Translucent(v) => write!(f, "trans:{}", v),
            Op::Shadows(v) => write!(f, "shad:{}", v),
            Op::Detail(v) => write!(f, "det:{}", v),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed { Save, Cancel }

/// `0x6810`.
#[derive(Debug, Clone)]
pub struct Dialog {
    prefs: Prefs,
    backup: Prefs,
    /// `D.1314`: 1 = the display page, 2 = the audio page.
    page: i16,
    /// `0x45840` (`D.1bd0`): the MOD player can be used; `0x45838` (`D.1bd2`): a CD can be played.
    mod_ok: bool,
    cd_ok: bool,
    closed: Option<Closed>,
}

impl Dialog {
    /// The start of `0x6810`: the backup (`0x7868`), the sound (30, 3, 0) and the first page.
    pub fn open(prefs: Prefs, mod_ok: bool, cd_ok: bool) -> (Dialog, Vec<Op>) {
        let d = Dialog { backup: prefs.clone(), prefs, page: 1, mod_ok, cd_ok, closed: None };
        let mut ops = vec![Op::Snd(30, 3, 0), Op::Append(191)];
        d.fill_page1(&mut ops);
        (d, ops)
    }

    pub fn prefs(&self) -> &Prefs { &self.prefs }
    pub fn page(&self) -> i16 { self.page }
    pub fn closed(&self) -> Option<Closed> { self.closed }

    /// `0x789c`.
    fn fill_page1(&self, ops: &mut Vec<Op>) {
        let p = &self.prefs;
        for (it, b) in [(item::RESIZE, 55), (item::ESCAPE_DELAY, 59), (item::LINE_SKIPPING, 56), (item::TRANSLUCENCY, 60), (item::EXTRA_DETAIL, 61), (item::SHADOWS, 62)] {
            ops.push(Op::Set(it, p.byte(b) as i16));
        }
    }

    /// `0x7984`: the quality, mode, surround and filtering items follow the music type.
    fn music_items(&self, disable: bool, ops: &mut Vec<Op>) {
        for it in [item::MUSIC_QUALITY, item::MUSIC_MODE, item::SURROUND, item::FILTERING] {
            ops.push(if disable { Op::Disable(it) } else { Op::Enable(it) });
        }
    }

    /// `0x7a38`.
    fn fill_page2(&self, ops: &mut Vec<Op>) {
        let p = &self.prefs;
        ops.push(Op::Set(item::VOLUME, p.short(51).wrapping_add(1)));
        ops.push(Op::Set(item::MUSIC_TYPE, p.short(55)));
        ops.push(Op::Set(item::MUSIC_QUALITY, p.short(56)));
        ops.push(Op::Set(item::MUSIC_MODE, p.short(57)));
        ops.push(Op::Set(item::SYSTEM_VOLUME, p.byte(58) as i16));
        if p.system_volume() { ops.push(Op::Disable(item::VOLUME)); }
        if self.mod_ok { ops.push(Op::Set(item::INTERFACE_MUSIC, p.byte(57) as i16)); } else { ops.push(Op::Disable(item::INTERFACE_MUSIC)); }
        ops.push(Op::Set(item::SURROUND, p.byte(63) as i16));
        ops.push(Op::Set(item::FILTERING, p.byte(64) as i16));
        self.music_items(p.short(55) != 1, ops);
        ops.push(Op::MenuDisable(1));
        ops.push(Op::MenuDisable(2));
        if self.mod_ok { ops.push(Op::MenuEnable(1)); }
        if self.cd_ok { ops.push(Op::MenuEnable(2)); }
    }

    fn fill_page(&self, ops: &mut Vec<Op>) {
        match self.page { 1 => self.fill_page1(ops), 2 => self.fill_page2(ops), _ => {} }
    }

    /// `0x7660`.
    fn switch(&mut self, page: i16, ops: &mut Vec<Op>) {
        self.page = page;
        ops.push(Op::Clear);
        ops.push(Op::Append(190 + page));
        self.fill_page(ops);
    }

    /// Flip a byte preference; the control shows the new state (`0x8420(item, 0 / 1)`). Returns the new state.
    fn toggle(&mut self, index: usize, item: i16, ops: &mut Vec<Op>) -> u8 {
        let v = if self.prefs.byte(index) == 0 { 1 } else { 0 };
        self.prefs.set_byte(index, v);
        ops.push(Op::Set(item, v as i16));
        v
    }

    /// One `ModalDialog` result: `item` is the item hit, `value` the value of its control (`GetControlValue`, only read for pop-up menus).
    pub fn hit(&mut self, item: i16, value: i16) -> Vec<Op> {
        let mut ops = Vec::new();
        if self.closed.is_some() { return ops; }
        match item {
            item::FACTORY => {
                self.prefs.apply_defaults(false);
                self.fill_page(&mut ops);
            }
            item::REVERT => {
                self.prefs = self.backup.clone();
                ops.push(Op::Volume);
                self.fill_page(&mut ops);
            }
            item::PAGE_MENU => match value {
                1 if self.page != 1 => self.switch(1, &mut ops),
                2 if self.page != 2 => self.switch(2, &mut ops),
                _ => {}
            },
            _ => {}
        }
        if item == item::FACTORY || item == item::REVERT || item == item::PAGE_MENU { return ops; }
        match self.page {
            1 => match item {
                item::RESIZE => { self.toggle(55, item, &mut ops); }
                item::ESCAPE_DELAY => { self.toggle(59, item, &mut ops); }
                item::LINE_SKIPPING => { self.toggle(56, item, &mut ops); }
                item::TRANSLUCENCY => { let v = self.toggle(60, item, &mut ops); ops.push(Op::Translucent(v)); }
                item::EXTRA_DETAIL => { let v = self.toggle(61, item, &mut ops); ops.push(Op::Detail(v)); }
                item::SHADOWS => { let v = self.toggle(62, item, &mut ops); ops.push(Op::Shadows(v)); }
                _ => {}
            },
            2 => match item {
                item::VOLUME => {
                    self.prefs.set_short(51, value.wrapping_sub(1));
                    ops.push(Op::Volume);
                    ops.push(Op::Snd(30, 3, 0));
                }
                item::SYSTEM_VOLUME => {
                    self.toggle(58, item, &mut ops);
                    ops.push(if self.prefs.system_volume() { Op::Disable(item::VOLUME) } else { Op::Enable(item::VOLUME) });
                }
                item::MUSIC_TYPE => {
                    self.prefs.set_short(55, value);
                    ops.push(Op::Volume);
                    let mut tail = Vec::new();
                    self.music_items(self.prefs.short(55) != 1, &mut tail);
                    ops.extend(tail);
                }
                item::MUSIC_QUALITY => { self.prefs.set_short(56, value); ops.push(Op::Volume); }
                item::MUSIC_MODE => { self.prefs.set_short(57, value); ops.push(Op::Volume); }
                item::SURROUND => { self.toggle(63, item, &mut ops); }
                item::FILTERING => { self.toggle(64, item, &mut ops); }
                item::INTERFACE_MUSIC => { self.toggle(57, item, &mut ops); }
                _ => {}
            },
            _ => {}
        }
        if item == item::SAVE || item == item::CANCEL {
            if item == item::CANCEL { self.prefs = self.backup.clone(); }
            self.closed = Some(if item == item::SAVE { Closed::Save } else { Closed::Cancel });
            // 0x6f00..: the file is written, the volume and the music applied, the key menu rebuilt
            ops.extend([Op::Save, Op::Volume, Op::Music, Op::Keys]);
        }
        ops
    }
}

/// `0xd3fc`: the Esc key as the session loop reads it (`GetKeys`, key 53), once per loop. Without the delay the game ends at once; with it
/// (`D.13dc` counts the loops the key is held) when the count passes 30.
#[derive(Debug, Clone, Copy, Default)]
pub struct EscKey { held: u32 }

impl EscKey {
    pub fn new() -> EscKey { EscKey::default() }
    /// Returns true when the session must end.
    pub fn step(&mut self, down: bool, delay: bool) -> bool {
        if !down { self.held = 0; return false; }
        if !delay { return true; }
        self.held += 1;
        self.held > 30
    }
}
