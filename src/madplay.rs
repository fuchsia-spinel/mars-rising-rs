//! The music player: the original's PlayerPRO engine (MADDriver) as the game runs it, rendered to 44.1 kHz stereo `i16`.
//!
//! Every routine is a port of the executable's code (addresses are of `MarsRising.pef`); the driver settings are the ones `Music_Init`
//! (0x444f8) leaves with the default preferences: 16 bit, 44100 Hz, 'deluxe' stereo (mode 3), Sound Manager double buffer
//! (732 frames per callback), no oversampling / interpolation / surround, repeat on.
//!   * `Play` 0x56410 -- the callback body: mixes until the next tick boundary, then runs the tick (0x56410..0x56750);
//!   * `read_note` 0x55eb4, `effect_init` 0x52d50, `effect_tick` 0x52904, `volcol_tick` 0x530bc, `envelope` 0x55cc4, `fade` 0x55c78;
//!   * `mix_channel` 0x57a84 (8-bit samples, no interpolation) + the clip pass of 0x5821c, gain tables of 0x54e98;
//!   * `reset_channels` 0x559e0, `get_old_period` 0x55e48, `scan_speed` 0x548c8 (start/resume), `start` 0x449dc, `stop` 0x44ab8.
//! `tests/madplay_vectors.rs` compares the PCM with the original running in the PPC emulator (tools/re/gen_music_vectors.py).
//! Channel/driver field names follow the offsets of the original structs (noted as `[off]`).
use crate::madh::{Cmd, Music};
use std::sync::Arc;

pub const FRAMES: usize = 732;                 // drv[13114]: frames per Sound Manager buffer
const RATE: i32 = 44100;                       // drv[8] >> 16
const PERIODS: [i32; 12] = [27392, 25856, 24384, 23040, 21696, 20480, 19328, 18240, 17216, 16256, 15360, 14512];
const SINE: [i16; 64] = [
    0, 50, 100, 149, 196, 241, 284, 325, 362, 396, 426, 452, 473, 490, 502, 510, 512, 510, 502, 490, 473, 452, 426, 396, 362, 325, 284, 241,
    196, 149, 100, 50, 0, -49, -99, -148, -195, -240, -283, -324, -361, -395, -425, -451, -472, -489, -501, -509, -511, -509, -501, -489,
    -472, -451, -425, -395, -361, -324, -283, -240, -195, -148, -99, -49,
];

/// 0x55e48: period of `note` for a sample with the given c2spd.
pub fn get_old_period(note: i32, c2spd: i32) -> i32 {
    if note == 255 || note == 254 || c2spd == 0 { return 4242; }
    let (q, r) = (note / 12, note - (note / 12) * 12);
    let t = PERIODS.get(r as usize).copied().unwrap_or(0);
    let p = (((t as i64 * 8363) as i32) >> q.clamp(0, 31)) / c2spd;
    if p == 0 { 4242 } else { p }
}

/// One of the 32 channel structs (206 bytes in the original; `[off]` = its offset).
#[derive(Clone, Debug)]
struct Chan {
    id: usize,
    cur: i64,            // [12] position in the sample data (the original holds an address; `[4]` base = 0)
    end: i64,            // [8]  end (loop end when looped)
    loop_size: i32,      // [28]
    amp: i32,            // [20] 8 / 16 bit sample
    ins: i32,            // [32]
    last_ins: i32,       // [36]
    samp: i32,           // [40]
    src: (i32, i32),     // [4] base: the (instrument, sample) whose data the mixer reads -- set only when a note starts a sample (not by tone portamento)
    c2spd: i32,          // [44]
    note: i32,           // [48]
    period: i32,         // [56]
    period_base: i32,    // [60]
    vol: i32,            // [64]
    cmd: i32,            // [68]
    arg: u8,             // [72]
    volcol: u8,          // [73]
    arp: [i32; 3],       // [74] [78] [82]
    arp_idx: i32,        // [86]
    vib_pos: i32,        // [90]
    vib_depth: i32,      // [94]
    vib_speed: i32,      // [98]
    slide_mem: i32,      // [102]
    porta_target: i32,   // [106]
    porta_speed: i32,    // [110]
    vol_slide: i32,      // [114]
    mem: [i32; 16],      // [118 + 4 * cmd] effect memory
    mute: i32,           // [182]
    key_on: u8,          // [186]
    env_cur: i32,        // [188]
    env_next: i32,       // [190]
    env_x: i32,          // [192]
    env_factor: i32,     // [194]
    fade: i32,           // [198]
    frac: i32,           // [202]
}

impl Chan {
    /// 0x559e0 (one channel's part)
    fn reset(id: usize) -> Chan {
        Chan {
            id, cur: 0, end: 0, loop_size: 0, amp: 8, ins: 0, last_ins: 0, samp: 0, src: (0, 0), c2spd: 8363, note: 255,
            period: get_old_period(40, 8363), period_base: get_old_period(40, 8363), vol: 64, cmd: 0, arg: 0, volcol: 0,
            arp: [0; 3], arp_idx: 0, vib_pos: 0, vib_depth: 0, vib_speed: 0, slide_mem: 0, porta_target: 0, porta_speed: 0,
            vol_slide: 0, mem: [0; 16], mute: 0, key_on: 0, env_cur: 0, env_next: 1, env_x: 0, env_factor: 64, fade: 32767, frac: 0,
        }
    }
}

pub struct Player {
    m: Arc<Music>,
    ch: Vec<Chan>,
    num_chn: usize,           // drv[4]
    playing: bool,            // [12880]
    repeat: bool,             // [17]
    speed: i32,               // [12744]
    tempo: i32,               // [12746]
    tick: i32,                // [12960]
    row: i32,                 // [12610]
    pat: i32,                 // [12612]
    pos: i32,                 // [12614]
    mixed: i32,               // [13126] frames mixed so far
    next_tick: i32,           // [13130] frame count at which the next tick starts
    max_period: i32,          // [12956]
    min_period: i32,          // [12952]
    acc: Vec<i32>,
    block: Vec<i16>,
    block_pos: usize,
    queue: Vec<i16>,          // the two primed buffers, played first
    queue_pos: usize,
    /// set when the song reached its end and `repeat` is off
    pub finished: bool,
}

impl Player {
    /// Driver creation + `MADLoad` + `start` (0x53418, 0x540b4, 0x550dc, 0x449dc): ready to play from the first row.
    pub fn new(m: Music) -> Player { Player::shared(Arc::new(m)) }

    /// Same, for a module that is kept loaded and played again and again.
    pub fn shared(m: Arc<Music>) -> Player {
        let n = m.num_chn as usize;
        let mut p = Player {
            ch: (0..32).map(Chan::reset).collect(), num_chn: n, playing: false, repeat: true, speed: 6, tempo: 125, tick: 128, row: 0,
            pat: m.o_pointers[0] as i32, pos: 0, mixed: 0, next_tick: 0, max_period: get_old_period(0, 8363), min_period: get_old_period(95, 8363),
            acc: vec![0; FRAMES * 2], block: vec![0; FRAMES * 2], block_pos: FRAMES * 2, queue: Vec::new(), queue_pos: 0, finished: false, m,
        };
        p.start();
        p
    }

    /// 0x449dc / 0x54c0c + 0x54a84: rewind and play.
    pub fn start(&mut self) {
        self.reset_channels();
        self.pos = 0; self.row = 0; self.tick = 128;
        self.pat = self.m.o_pointers[0] as i32;
        self.speed = 6; self.tempo = 125;
        self.resume();
    }

    /// 0x44b44 / 0x54a84: stop, reset the channels, re-read speed/tempo for the current position, prime BOTH double buffers (two
    /// callbacks, still with `playing` off: they are silent but advance the tick clock), then play.
    pub fn resume(&mut self) {
        self.playing = false;
        self.reset_channels();
        self.scan_speed();
        let mut q = vec![0i16; FRAMES * 4];
        let (a, b) = q.split_at_mut(FRAMES * 2);
        self.play(a);
        self.play(b);
        self.playing = true;
        self.finished = false;
        self.queue = q;
        self.queue_pos = 0;
        self.block_pos = FRAMES * 2;
    }

    /// 0x44ab8 / 0x54b5c: stop (the channels are reset, nothing more is read).
    pub fn stop(&mut self) {
        self.reset_channels();
        self.playing = false;
        self.queue.clear();
        self.queue_pos = 0;
        self.block_pos = FRAMES * 2;
    }

    pub fn is_playing(&self) -> bool { self.playing }
    pub fn music(&self) -> &Music { &self.m }
    pub fn position(&self) -> (i32, i32, i32) { (self.pos, self.pat, self.row) }
    pub fn speed_tempo(&self) -> (i32, i32) { (self.speed, self.tempo) }

    /// 0x559e0
    fn reset_channels(&mut self) {
        for i in 0..32 { self.ch[i] = Chan::reset(i); }
        self.mixed = 0;
        self.next_tick = 0;
    }

    /// 0x548c8: the last speed (F00-F1F) / tempo (F20-FFF) command at or before the current row decides; else the header values.
    fn scan_speed(&mut self) {
        let (mut got_speed, mut got_tempo) = (false, false);
        let mut p = self.pos;
        'outer: while p >= 0 {
            let pat = self.m.o_pointers.get(p as usize).copied().unwrap_or(0) as usize;
            let rows = self.m.patterns.get(pat).map(|x| x.rows as i32).unwrap_or(0);
            let mut r = if p == self.pos { if self.row < rows { self.row } else { self.row - 1 } } else { rows - 1 };
            while r >= 0 {
                for c in (0..self.m.num_chn as i32).rev() {
                    let cmd = self.cmd_at(pat as i32, r, c);
                    if cmd.cmd == 15 {
                        if cmd.arg < 32 {
                            if cmd.arg != 0 { if !got_speed { self.speed = cmd.arg as i32; } got_speed = true; }
                        } else if !got_tempo { self.tempo = cmd.arg as i32; got_tempo = true; }
                    }
                }
                if got_speed && got_tempo { break 'outer; }
                r -= 1;
            }
            p -= 1;
        }
        if !got_speed { self.speed = self.m.speed as i32; }
        if !got_tempo { self.tempo = self.m.tempo as i32; }
    }

    fn cmd_at(&self, pat: i32, row: i32, chan: i32) -> Cmd {
        match self.m.patterns.get(pat as usize) { Some(p) => p.at(row, chan), None => Cmd::default() }
    }

    /// Fill `out` (interleaved L/R) with the next samples of the stream.
    pub fn render(&mut self, out: &mut [i16]) {
        let mut i = 0;
        while i < out.len() {
            if self.queue_pos < self.queue.len() {
                let n = (out.len() - i).min(self.queue.len() - self.queue_pos);
                out[i..i + n].copy_from_slice(&self.queue[self.queue_pos..self.queue_pos + n]);
                i += n; self.queue_pos += n;
                continue;
            }
            if self.block_pos >= self.block.len() {
                let mut b = std::mem::take(&mut self.block);
                self.play(&mut b);
                self.block = b;
                self.block_pos = 0;
            }
            let n = (out.len() - i).min(self.block.len() - self.block_pos);
            out[i..i + n].copy_from_slice(&self.block[self.block_pos..self.block_pos + n]);
            i += n; self.block_pos += n;
        }
    }

    /// 0x56410 `Play`: one callback = FRAMES frames.
    fn play(&mut self, out: &mut [i16]) {
        let mut remaining = FRAMES as i32;
        let mut at = 0usize;
        while remaining as i16 > 0 {
            let mut n = self.next_tick - self.mixed;
            if n < 0 { n = 0; }
            let full = if n > remaining as i16 as i32 { n = remaining as i16 as i32; false } else { true };
            if n > 0 {
                self.mix(n as usize, &mut out[at * 2..(at + n as usize) * 2]);
                at += n as usize;
                self.mixed += n;
                remaining -= n;
            }
            if !full { break; }
            self.tick = (self.tick as i16).wrapping_add(1) as i32;
            if self.tick < self.speed {
                for c in 0..self.num_chn {
                    self.volcol_tick(c, self.tick);
                    self.effect_tick(c, self.tick);
                    self.envelope(c);
                    self.fade(c);
                }
                if self.playing && self.tick == self.speed - 1 && self.pos >= self.m.num_pointers as i32 {
                    self.pos = 0;
                    self.pat = self.m.o_pointers[0] as i32;
                    self.reset_channels();
                    if !self.repeat { self.playing = false; self.finished = true; }
                }
            } else {
                self.tick = 0;
                if self.playing {
                    for c in 0..self.m.num_chn as usize {
                        let cmd = self.cmd_at(self.pat, self.row, c as i32);
                        self.read_note(c, cmd);
                        self.envelope(c);
                        self.fade(c);
                    }
                    self.row += 1;
                    let rows = self.m.patterns.get(self.pat as usize).map(|p| p.rows as i32).unwrap_or(0);
                    if self.row >= rows {
                        self.row = 0;
                        // [13112] (advance through the song) is 1
                        self.pos += 1;
                        self.pat = self.m.o_pointers.get(self.pos as usize).copied().unwrap_or(0) as i32;
                        if self.speed == 1 && self.pos >= self.m.num_pointers as i32 {
                            self.pos = 0;
                            self.pat = self.m.o_pointers[0] as i32;
                            self.reset_channels();
                            if !self.repeat { self.playing = false; self.finished = true; }
                        }
                    }
                }
            }
            let tick_len = ((RATE * 125 / 50) / self.tempo.max(1)) * 80 / 80;
            self.next_tick += tick_len;
        }
    }

    // ---------------------------------------------------------------- 0x55eb4

    fn read_note(&mut self, c: usize, cmd: Cmd) {
        if self.ch[c].mute != 0 { return; }
        let (mut ins_cmd, note, fx, arg, vol) = (cmd.ins as i32, cmd.note as i32, cmd.cmd as i32, cmd.arg, cmd.vol);
        if ins_cmd == 0 && (note == 255 || note == 254) {
            self.ch[c].note = 255;
        } else {
            if ins_cmd == 0 { ins_cmd = self.ch[c].last_ins; } else { self.ch[c].last_ins = ins_cmd; }
            if ins_cmd != 0 && note != 255 && note != 254 {
                let idx = (ins_cmd - 1).min(63);
                let si = self.m.ibyte(idx, 36 + note) as i32;
                if si < self.m.ishort(idx, 34) as i32 {
                    let (ins, samp) = (idx, si);
                    let s = self.m.instr.get(ins as usize).and_then(|i| i.samples.get(samp as usize));
                    self.ch[c].ins = ins;
                    self.ch[c].amp = s.map(|s| s.amp as i32).unwrap_or(0);
                    self.ch[c].samp = samp;
                    if fx != 3 && fx != 5 {
                        let s = s.unwrap();
                        let ch = &mut self.ch[c];
                        ch.src = (ins, samp);
                        ch.cur = 0;
                        ch.end = s.size as i64;
                        ch.frac = 0;
                        if s.loop_size > 2 {
                            ch.loop_size = s.loop_size as i32;
                            ch.end = s.loop_beg as i64 + s.loop_size as i64;
                        } else { ch.loop_size = 0; }
                        ch.vib_pos = 0;
                        ch.vol = (s.vol as i32).min(64);
                        ch.fade = 32767;
                        ch.env_x = 0; ch.env_cur = 0; ch.env_next = 1;       // 0x55e30
                    }
                }
            } else if ins_cmd != 0 && note == 255 {
                let (ins, samp) = (self.ch[c].ins, self.ch[c].samp);
                if samp < self.m.ishort(ins, 34) as i32 {
                    if let Some(s) = self.m.instr.get(ins as usize).and_then(|i| i.samples.get(samp as usize)) {
                        let v = (s.vol as i32).min(64);
                        let ch = &mut self.ch[c];
                        ch.vol = v; ch.fade = 32767;
                    }
                }
            }
            // pitch
            if note != 255 && note != 254 {
                let ins = self.ch[c].ins;
                let si = self.m.ibyte(ins, 36 + note) as i32;
                if si < self.m.ishort(ins, 34) as i32 {
                    if let Some(s) = self.m.instr.get(ins as usize).and_then(|i| i.samples.get(si as usize)) {
                        let (rel, c2) = (s.rel_note as i32, s.c2spd as i32);
                        let ch = &mut self.ch[c];
                        ch.note = note + rel;
                        ch.c2spd = c2;
                        ch.key_on = 1;
                        if fx != 3 && fx != 5 {
                            let p = get_old_period(ch.note, ch.c2spd);
                            ch.period = p * 80 / 80;
                            ch.period_base = ch.period;
                        }
                    }
                }
            }
        }
        // volume column
        {
            let ch = &mut self.ch[c];
            if vol == 255 { ch.volcol = 0; }
            else if (16..=80).contains(&vol) { ch.vol = (vol as i32 - 16).clamp(0, 64); ch.volcol = 0; }
            else { ch.volcol = vol; }
            ch.cmd = fx;
            ch.arg = arg;
        }
        self.effect_init(c);
        if note == 254 { self.ch[c].key_on = 0; }
    }

    // ---------------------------------------------------------------- 0x52d50 (tick 0 of a row)

    fn effect_init(&mut self, c: usize) {
        let ch = &mut self.ch[c];
        let fx = ch.cmd;
        if ch.arg == 0 {
            let load = match fx { 7 | 0 | 11..=15 => false, 1..=6 | 8..=10 => true, _ => false };
            if load { ch.arg = ch.mem[fx as usize] as u8; }
        } else if (0..16).contains(&fx) {
            ch.mem[fx as usize] = ch.arg as i32;
        }
        let arg = ch.arg as i32;
        if fx == 15 {
            if arg < 32 { if arg != 0 { self.speed = arg; } } else { self.tempo = arg; }
            return;
        }
        match fx {
            1 | 2 => { if arg != 0 { ch.slide_mem = arg; } }
            4 => {
                if arg >> 4 != 0 { ch.vib_speed = arg >> 4; }
                if arg & 15 != 0 { ch.vib_depth = arg & 15; }
                ch.period_base = ch.period;
            }
            0 => {
                if arg == 0 { ch.arp[0] = 0; }
                else if ch.note != 255 {
                    let a = ch.note + (arg >> 4);
                    if a < 96 { ch.arp[1] = get_old_period(a, 8363); }
                    let b = ch.note + (arg & 15);
                    if b < 96 { ch.arp[2] = get_old_period(b, 8363); }
                    ch.arp_idx = 0;
                    ch.arp[0] = ch.period;
                }
            }
            10 => vol_slide_setup(ch, arg),
            14 => match arg >> 4 {
                1 => ch.period -= (arg & 15) << 2,
                2 => ch.period += (arg & 15) << 2,
                10 => ch.vol = (ch.vol + (arg & 15)).clamp(0, 64),
                11 => ch.vol = (ch.vol - (arg & 15)).clamp(0, 64),
                _ => {}
            },
            3 => {
                ch.porta_speed = arg;
                if ch.note != 255 { ch.porta_target = get_old_period(ch.note, ch.c2spd); }
            }
            5 => {
                if ch.note != 255 { ch.porta_target = get_old_period(ch.note, ch.c2spd); }
                vol_slide_setup(ch, arg);
            }
            6 => { ch.period_base = ch.period; vol_slide_setup(ch, arg); }
            9 => { ch.cur = (arg as i64) << 8; }
            12 => { ch.vol = arg.clamp(0, 64); }
            _ => {}
        }
    }

    // ---------------------------------------------------------------- 0x530bc volume column, ticks >= 1

    fn volcol_tick(&mut self, c: usize, tick: i32) {
        let ch = &mut self.ch[c];
        let v = ch.volcol as i32;
        let low = v & 15;
        match (v >> 4) - 6 {
            0 => ch.vol = (ch.vol - low).clamp(0, 64),
            1 => ch.vol = (ch.vol + low).clamp(0, 64),
            2 => if tick == 1 { ch.vol = (ch.vol - low).clamp(0, 64) },
            3 => if tick == 1 { ch.vol = (ch.vol + low).clamp(0, 64) },
            _ => {}
        }
    }

    // ---------------------------------------------------------------- 0x52904 (ticks >= 1)

    fn effect_tick(&mut self, c: usize, tick: i32) {
        match self.ch[c].cmd {
            0 => {
                let ch = &mut self.ch[c];
                if ch.arg != 0 && ch.arp[0] != 0 {
                    ch.arp_idx += 1;
                    if ch.arp_idx >= 3 { ch.arp_idx = 0; }
                    ch.period = ch.arp[ch.arp_idx as usize];
                }
            }
            1 => { let (mn, ch) = (self.min_period, &mut self.ch[c]); if ch.period > mn { ch.period -= ch.slide_mem << 2; } }
            2 => { let (mx, ch) = (self.max_period, &mut self.ch[c]); if ch.period < mx { ch.period += ch.slide_mem << 2; } }
            3 => porta_tick(&mut self.ch[c]),
            4 => vib_tick(&mut self.ch[c]),
            5 => { porta_tick(&mut self.ch[c]); vslide_tick(&mut self.ch[c]); self.ch[c].cmd = 5; }
            6 => { vib_tick(&mut self.ch[c]); vslide_tick(&mut self.ch[c]); self.ch[c].cmd = 6; }
            10 => vslide_tick(&mut self.ch[c]),
            11 => if tick == self.speed - 1 { self.pos_jump(c) },
            13 => if tick == self.speed - 1 { self.pattern_break(c) },
            14 => {
                let ch = &mut self.ch[c];
                if (ch.arg >> 4) == 12 && tick >= (ch.arg & 15) as i32 { ch.vol = 0; }
            }
            fx if fx > 14 || matches!(fx, 7 | 8 | 9 | 12) => {
                let ch = &mut self.ch[c];
                ch.cmd = 0; ch.arg = 0;
                return;                         // 0x5294c: no end-of-row clear needed
            }
            _ => {}
        }
        if tick == self.speed - 1 { self.ch[c].arg = 0; self.ch[c].cmd = 0; }
    }

    fn pos_jump(&mut self, c: usize) {
        self.pos = self.ch[c].arg as i32;
        self.pat = self.m.o_pointers.get(self.pos as usize).copied().unwrap_or(0) as i32;
        if self.pos >= self.m.num_pointers as i32 {
            self.pos = 0;
            self.pat = self.m.o_pointers[0] as i32;
            self.reset_channels();
            if !self.repeat { self.playing = false; self.finished = true; }
        }
        self.row = 0;
        self.ch[c].cmd = 0; self.ch[c].arg = 0;
    }

    fn pattern_break(&mut self, c: usize) {
        if self.row != 0 {
            self.pos += 1;
            self.pat = self.m.o_pointers.get(self.pos as usize).copied().unwrap_or(0) as i32;
        }
        let a = self.ch[c].arg as i32;
        self.row = (a & 15) + (a >> 4) * 10;
        if self.pos >= self.m.num_pointers as i32 {
            self.pos = 0;
            self.pat = self.m.o_pointers[0] as i32;
            self.reset_channels();
            if !self.repeat { self.playing = false; self.finished = true; }
        }
        // the channels were reset when the song wrapped: `c` still names the same slot
        self.ch[c].cmd = 0; self.ch[c].arg = 0;
    }

    // ---------------------------------------------------------------- 0x55cc4 volume envelope, 0x55c78 fade-out

    fn envelope(&mut self, c: usize) {
        self.ch[c].env_factor = 64;
        let ins = self.ch[c].ins;
        if ins < 0 || self.ch[c].mute != 0 { return; }
        let size = self.m.ibyte(ins, 228) as i32;
        let flags = self.m.ibyte(ins, 236);
        if size == 0 || flags & 1 == 0 { return; }
        let ex = |i: i32| self.m.ishort(ins, 132 + 4 * (i & 255)) as i32;
        let ey = |i: i32| self.m.ishort(ins, 134 + 4 * (i & 255)) as i32;
        let ch = &self.ch[c];
        let (mut cur, mut nxt, mut x) = (ch.env_cur & 255, ch.env_next & 255, ch.env_x as i16 as i32);
        // 0x55c30: the envelope value
        let val = if x < ex(cur) { ey(cur) } else {
            let (ax, bx, ay, by) = (ex(cur), ex(nxt), ey(cur), ey(nxt));
            if ax == bx { ay } else { ay + ((x - ax) * (by - ay)) / (bx - ax) }
        };
        let key_on = ch.key_on != 0;
        let hold = flags & 2 != 0 && key_on && cur == self.m.ibyte(ins, 230) as i32 && x == ex(cur);
        if !hold {
            x = (x + 1) as i16 as i32;
            if x >= ex(nxt) {
                cur = nxt;
                nxt = (nxt + 1) & 255;
                if flags & 4 != 0 {
                    let end = self.m.ibyte(ins, 232) as i32;
                    if nxt > end {
                        let beg = self.m.ibyte(ins, 231) as i32;
                        cur = beg;
                        x = ex(beg);
                        nxt = (beg + 1) & 255;
                    }
                } else if nxt >= size {
                    nxt = (nxt - 1) & 255;
                    x -= 1;
                }
            }
        }
        let ch = &mut self.ch[c];
        ch.env_cur = cur & 255; ch.env_next = nxt & 255; ch.env_x = x as i16 as i32; ch.env_factor = val;
    }

    fn fade(&mut self, c: usize) {
        if self.ch[c].key_on != 0 { return; }
        let f = self.m.ibyte(self.ch[c].ins, 238) as i32 * 256 + self.m.ibyte(self.ch[c].ins, 239) as i32;
        let ch = &mut self.ch[c];
        ch.fade -= f;
        if ch.fade < 0 { ch.fade = 0; ch.loop_size = 0; }
    }

    // ---------------------------------------------------------------- mixer

    /// 0x5821c: mix `n` frames of all channels, clip into `out`.
    fn mix(&mut self, n: usize, out: &mut [i16]) {
        for v in self.acc[..n * 2].iter_mut() { *v = 0; }
        for c in 0..self.num_chn {
            if self.ch[c].amp == 8 { self.mix_channel8(c, n); }
        }
        for i in 0..n * 2 {
            let v = self.acc[i];
            out[i] = if v > 32767 { 32767 } else if v < -32767 { -32767 } else { v as i16 };
        }
    }

    /// 0x57a84 (non-interpolating path).
    fn mix_channel8(&mut self, c: usize, n: usize) {
        let (ins, samp) = self.ch[c].src;
        let ch = &self.ch[c];
        let step: u32 = 0xda77_9000u32.checked_div((ch.period as u32).wrapping_mul(RATE as u32)).unwrap_or(0);
        let mut pos = ch.frac as u32;
        if ch.cur >= ch.end && ch.loop_size == 0 { return; }
        let Some(s) = self.m.instr.get(ins as usize).and_then(|i| i.samples.get(samp as usize)) else { return };
        let data = &s.data;
        let vf = ((ch.fade as i64 * (ch.vol as i64 * ch.env_factor as i64)) / 0x1fffc0) as i32;
        let pan = 30 + 40 * (ch.id as i32 & 1);
        let cv = 64;                                           // drv[12616 + 4 * id]
        let g_r = (pan * (cv * vf)) / 8000;
        let g_l = ((100 - pan) * (cv * vf)) / 8000;
        let tab = |g: i32, s: u8| -> i32 {
            let x = if s < 128 { s as i32 } else { s as i32 - 255 };
            ((x * g) << 2) as i16 as i32
        };
        let byte = |i: i64| -> u8 { if i < 0 { 0 } else { data.get(i as usize).copied().unwrap_or(0) } };
        let mut cur = ch.cur;
        let end = ch.end;
        let loop_size = ch.loop_size as i64;
        let total = (pos as i32).wrapping_add((step as i32).wrapping_mul(n as i32));
        let fast = cur + ((total >> 8) as i64 + 1) < end;
        let acc = &mut self.acc;
        let mut o = 0usize;
        let mut stopped = false;
        if fast {
            for _ in 0..n {
                let b = byte(cur + (pos as i32 >> 8) as i64);
                acc[o] += tab(g_l, b);
                acc[o + 1] += tab(g_r, b);
                o += 2;
                pos = pos.wrapping_add(step);
            }
            let ch = &mut self.ch[c];
            ch.cur = cur + (pos as i32 >> 8) as i64;
        } else {
            for _ in 0..n {
                let r6 = (pos as i32 >> 8) as i64;
                if cur + r6 >= end {
                    if loop_size <= 0 { stopped = true; break; }
                    cur -= loop_size;
                    if cur + r6 < 0 { cur = -r6; }
                }
                let b = byte(cur + r6);
                acc[o] += tab(g_l, b);
                acc[o + 1] += tab(g_r, b);
                o += 2;
                pos = pos.wrapping_add(step);
            }
            let ch = &mut self.ch[c];
            if !stopped && loop_size > 0 {
                ch.cur = cur + (pos as i32 >> 8) as i64;
            } else {
                ch.mute = 0;
                ch.cur = end;
            }
        }
        self.ch[c].frac = (pos & 255) as i32;
    }
}

fn vol_slide_setup(ch: &mut Chan, arg: i32) {      // 0x528dc
    if arg & 15 == 0 { ch.vol_slide = arg >> 4; } else { ch.vol_slide = -(arg & 15); }
}

fn vslide_tick(ch: &mut Chan) {                    // case 10 of 0x52904
    ch.vol = (ch.vol + ch.vol_slide).clamp(0, 64);
}

fn vib_tick(ch: &mut Chan) {
    ch.vib_pos = (ch.vib_pos + ch.vib_speed) & 0x3f;
    ch.period = ch.period_base + (((ch.vib_depth * SINE[ch.vib_pos as usize] as i32) / 512) << 2);
}

fn porta_tick(ch: &mut Chan) {
    if ch.period == ch.porta_target { return; }
    if ch.period < ch.porta_target {
        ch.period += ch.porta_speed << 2;
        if ch.period > ch.porta_target { ch.cmd = 0; ch.arg = 0; ch.period = ch.porta_target; }
    } else {
        ch.period -= ch.porta_speed << 2;
        if ch.period < ch.porta_target { ch.cmd = 0; ch.arg = 0; ch.period = ch.porta_target; }
    }
}
