//! PlayerPRO "MADH" module loader (the music format of the original; the game links the PlayerPRO library statically).
//!
//! Ported from the original's loader `0x540b4` (read header, patterns, instruments) and `0x551f0` (MAD1 pattern decompression):
//!   * header, 16044 bytes = 556 (the MADSpec: magic `MADH`, 32-byte name, 240-byte info, `numPat` @292, `numChn` @293, `numPointers` @294,
//!     `oPointers[128]` @295, `speed` @552, `tempo` @554, all big-endian) + 64 instruments x 242 bytes;
//!   * per pattern: a 48-byte header (`rows` u32 @0, compression tag u32 @4 -- `MAD1` = 0x4d414431 --, `patBytes` u32 @40), then either
//!     `rows * numChn * 6` bytes of commands or, for `MAD1`, `patBytes` bytes of compressed data;
//!   * then for every instrument (0..64) its `numSamples` samples: a 56-byte sample header and `size` bytes of 8-bit data.
//! A command is 6 bytes (`ins note cmd arg vol unused`); the patterns are stored track after track (`cmd = chan * rows + row`).
//! `MAD1`: one flag byte per command (bit0 ins, bit1 note, bit2 cmd, bit3 arg, bit4 vol) followed by the flagged bytes; absent fields
//! keep the defaults of `0x548a4` (`ins 0, note 255, cmd 0, arg 0, vol 255`).
use crate::assets::Error;

pub const HEADER: usize = 16044;
pub const INSTR_OFS: usize = 556;
pub const INSTR_SIZE: usize = 242;
pub const MAD1: u32 = 0x4d41_4431;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cmd { pub ins: u8, pub note: u8, pub cmd: u8, pub arg: u8, pub vol: u8 }
impl Default for Cmd { fn default() -> Cmd { Cmd { ins: 0, note: 255, cmd: 0, arg: 0, vol: 255 } } }

#[derive(Clone, Debug)]
pub struct Pattern { pub rows: u32, pub cmds: Vec<Cmd> }
impl Pattern {
    /// `0x54de8`: the command of (row, channel); the row is clamped into the pattern.
    pub fn at(&self, row: i32, chan: i32) -> Cmd {
        let rows = self.rows as i32;
        let r = if row < 0 { 0 } else if row >= rows { rows - 1 } else { row };
        self.cmds.get((r + rows * chan) as usize).copied().unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub size: u32, pub loop_beg: u32, pub loop_size: u32,
    pub vol: u8, pub c2spd: u16, pub loop_type: u8, pub amp: u8, pub rel_note: i8,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Instr {
    pub num_samples: i16,
    pub what: [u8; 96],
    pub vol_env: [(i16, i16); 12],
    pub vol_size: u8, pub vol_sus: u8, pub vol_beg: u8, pub vol_end: u8, pub vol_type: u8,
    pub vol_fade: u16,
    pub samples: Vec<Sample>,
}
impl Default for Instr {
    fn default() -> Instr {
        Instr { num_samples: 0, what: [0; 96], vol_env: [(0, 0); 12], vol_size: 0, vol_sus: 0, vol_beg: 0, vol_end: 0, vol_type: 0, vol_fade: 0, samples: Vec::new() }
    }
}

#[derive(Clone, Debug)]
pub struct Music {
    pub name: String,
    pub num_pat: u8, pub num_chn: u8, pub num_pointers: u8,
    pub o_pointers: [u8; 128],
    pub speed: i16, pub tempo: i16,
    pub patterns: Vec<Pattern>,
    pub instr: Vec<Instr>,
    /// the raw 16044-byte header: the player reads instrument fields (and, out of range, their neighbours) straight from it
    pub hdr: Vec<u8>,
}
impl Music {
    /// byte `off` of instrument `ins` of the header block (instruments are contiguous: out-of-range offsets read the next ones)
    pub fn ibyte(&self, ins: i32, off: i32) -> u8 {
        let a = INSTR_OFS as i64 + ins as i64 * INSTR_SIZE as i64 + off as i64;
        if a >= 0 { self.hdr.get(a as usize).copied().unwrap_or(0) } else { 0 }
    }
    pub fn ishort(&self, ins: i32, off: i32) -> i16 { i16::from_be_bytes([self.ibyte(ins, off), self.ibyte(ins, off + 1)]) }
}

fn be16(b: &[u8], o: usize) -> u16 { u16::from_be_bytes([b[o], b[o + 1]]) }
fn be32(b: &[u8], o: usize) -> u32 { u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) }

fn short(what: &str) -> Error { Error::Msg(format!("MADH file is truncated ({})", what)) }

/// `0x551f0`: expand a `MAD1` pattern body (`comp` = the bytes after the 48-byte header) into `rows * num_chn` commands.
pub fn decompress(rows: u32, num_chn: u8, comp: &[u8]) -> Result<Vec<Cmd>, Error> {
    // the original counts with a 16-bit register
    let n = ((rows as i32).wrapping_mul(num_chn as i32) as i16) as usize;
    let mut out = Vec::with_capacity(n);
    let mut p = 0usize;
    for _ in 0..n {
        let f = *comp.get(p).ok_or_else(|| short("MAD1 pattern"))?;
        p += 1;
        let mut c = Cmd::default();
        let mut take = |bit: u8, dst: &mut u8| -> Result<(), Error> {
            if f & bit != 0 { *dst = *comp.get(p).ok_or_else(|| short("MAD1 pattern"))?; p += 1; }
            Ok(())
        };
        take(1, &mut c.ins)?; take(2, &mut c.note)?; take(4, &mut c.cmd)?; take(8, &mut c.arg)?; take(16, &mut c.vol)?;
        out.push(c);
    }
    Ok(out)
}

impl Music {
    pub fn parse(b: &[u8]) -> Result<Music, Error> {
        if b.len() < HEADER { return Err(short("header")); }
        if be32(b, 0) != 0x4d41_4448 { return Err(Error::Msg("not a MADH file".into())); }
        let name = String::from_utf8_lossy(&b[4..36]).trim_end_matches('\0').to_string();
        let (num_pat, num_chn, num_pointers) = (b[292], b[293], b[294]);
        let mut o_pointers = [0u8; 128];
        o_pointers.copy_from_slice(&b[295..295 + 128]);
        let (speed, tempo) = (be16(b, 552) as i16, be16(b, 554) as i16);
        let mut pos = HEADER;
        let mut patterns = Vec::new();
        for _ in 0..num_pat {
            if b.len() < pos + 48 { return Err(short("pattern header")); }
            let rows = be32(b, pos);
            let tag = be32(b, pos + 4);
            let total = if tag == MAD1 { be32(b, pos + 40) as usize + 48 } else { rows as usize * num_chn as usize * 6 + 48 };
            if b.len() < pos + total { return Err(short("pattern")); }
            let body = &b[pos + 48..pos + total];
            let cmds = if tag == MAD1 { decompress(rows, num_chn, body)? } else {
                body.chunks_exact(6).map(|c| Cmd { ins: c[0], note: c[1], cmd: c[2], arg: c[3], vol: c[4] }).collect()
            };
            patterns.push(Pattern { rows, cmds });
            pos += total;
        }
        let mut instr = Vec::with_capacity(64);
        for i in 0..64 {
            let o = INSTR_OFS + i * INSTR_SIZE;
            let mut ins = Instr::default();
            ins.num_samples = be16(b, o + 34) as i16;
            ins.what.copy_from_slice(&b[o + 36..o + 132]);
            for k in 0..12 { ins.vol_env[k] = (be16(b, o + 132 + 4 * k) as i16, be16(b, o + 134 + 4 * k) as i16); }
            ins.vol_size = b[o + 228]; ins.vol_sus = b[o + 230]; ins.vol_beg = b[o + 231]; ins.vol_end = b[o + 232];
            ins.vol_type = b[o + 236];
            ins.vol_fade = be16(b, o + 238);
            instr.push(ins);
        }
        for ins in instr.iter_mut() {
            for _ in 0..ins.num_samples.max(0) {
                if b.len() < pos + 56 { return Err(short("sample header")); }
                let h = &b[pos..pos + 56];
                let mut s = Sample {
                    size: be32(h, 0), loop_beg: be32(h, 4), loop_size: be32(h, 8),
                    vol: h[12], c2spd: be16(h, 14), loop_type: h[16], amp: h[17], rel_note: h[19] as i8, data: Vec::new(),
                };
                pos += 56;
                if b.len() < pos + s.size as usize { return Err(short("sample data")); }
                s.data = b[pos..pos + s.size as usize].to_vec();
                pos += s.size as usize;
                ins.samples.push(s);
            }
        }
        Ok(Music { name, num_pat, num_chn, num_pointers, o_pointers, speed, tempo, patterns, instr, hdr: b[..HEADER].to_vec() })
    }
}
