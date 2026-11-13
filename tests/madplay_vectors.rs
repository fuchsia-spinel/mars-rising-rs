//! Golden vectors for src/madh.rs + src/madplay.rs, produced by running the original PlayerPRO engine of the executable in the PPC
//! emulator (tools/re/gen_music_vectors.py; format described there). `real` lines need the extracted assets (MR_ASSETS or ./assets);
//! `synth` lines are self-contained: the module is generated from its seed by `synth_module` (same algorithm as the Python generator).
use mars_rising::{madh::Music, madplay::{Player, FRAMES}};

struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u32 { self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223); self.0 >> 8 }
    fn r(&mut self, n: u32) -> u32 { self.next() % n }
}

fn synth_module(seed: u64) -> Vec<u8> {
    let mut g = Lcg((seed.wrapping_mul(7919).wrapping_add(17)) as u32);
    let num_chn = [4usize, 8, 8, 16][g.r(4) as usize];
    let num_pat = 1 + g.r(3) as usize;
    let rows: Vec<usize> = (0..num_pat).map(|_| 8 + g.r(40) as usize).collect();
    let num_ptr = 1 + g.r(5) as usize;
    let mut optr: Vec<u8> = (0..num_ptr).map(|_| g.r(num_pat as u32) as u8).collect();
    optr.resize(128, 0);
    let speed = 2 + g.r(7) as u16;
    let tempo = 90 + g.r(100) as u16;
    let num_ins = 1 + g.r(4) as usize;
    let mut hdr = vec![0u8; 16044];
    hdr[0..4].copy_from_slice(b"MADH");
    hdr[4..15].copy_from_slice(b"synthetic  ");
    hdr[292] = num_pat as u8; hdr[293] = num_chn as u8; hdr[294] = num_ptr as u8;
    hdr[295..295 + 128].copy_from_slice(&optr);
    hdr[552..554].copy_from_slice(&speed.to_be_bytes());
    hdr[554..556].copy_from_slice(&tempo.to_be_bytes());
    let mut samples: Vec<u8> = Vec::new();
    for i in 0..num_ins {
        let o = 556 + 242 * i;
        let ns = 1 + g.r(3) as usize;
        hdr[o + 34..o + 36].copy_from_slice(&(ns as i16).to_be_bytes());
        for k in 0..96 { hdr[o + 36 + k] = g.r(ns as u32) as u8; }
        let vs = g.r(7) as usize;
        let mut x = 0i16;
        for k in 0..vs {
            if k > 0 { x += 1 + g.r(30) as i16; }
            hdr[o + 132 + 4 * k..o + 134 + 4 * k].copy_from_slice(&x.to_be_bytes());
            let y = g.r(70) as i16;
            hdr[o + 134 + 4 * k..o + 136 + 4 * k].copy_from_slice(&y.to_be_bytes());
        }
        hdr[o + 228] = vs as u8;
        let m = vs.max(1) as u32;
        hdr[o + 230] = g.r(m) as u8;
        hdr[o + 231] = g.r(m) as u8;
        hdr[o + 232] = g.r(m) as u8;
        hdr[o + 236] = g.r(8) as u8;
        let fade: u16 = if g.r(4) == 0 { 0 } else { g.r(3000) as u16 };
        hdr[o + 238..o + 240].copy_from_slice(&fade.to_be_bytes());
        for _ in 0..ns {
            let size = 100 + g.r(3000);
            let data: Vec<u8> = (0..size).map(|_| (g.next() & 255) as u8).collect();
            let kind = g.r(4);
            let (lb, ls) = if kind == 0 { (0, 0) } else if kind == 1 { (g.r(size / 2), 1 + g.r(2)) } else {
                let lb = g.r(size / 2);
                (lb, 3 + g.r(size - lb - 3))
            };
            let vol = g.r(70) as u8;
            let c2 = (3000 + g.r(14000)) as u16;
            let rel = g.r(13) as u8;
            let mut sh = vec![0u8; 56];
            sh[0..4].copy_from_slice(&size.to_be_bytes());
            sh[4..8].copy_from_slice(&lb.to_be_bytes());
            sh[8..12].copy_from_slice(&ls.to_be_bytes());
            sh[12] = vol;
            sh[14..16].copy_from_slice(&c2.to_be_bytes());
            sh[17] = 8;
            sh[19] = rel;
            samples.extend_from_slice(&sh);
            samples.extend_from_slice(&data);
        }
    }
    let mut pats: Vec<u8> = Vec::new();
    for p in 0..num_pat {
        let mut ph = vec![0u8; 48];
        ph[0..4].copy_from_slice(&(rows[p] as u32).to_be_bytes());
        let mut body = vec![0u8; rows[p] * num_chn * 6];
        for ch in 0..num_chn {
            for row in 0..rows[p] {
                let ins = if g.r(3) == 0 { g.r(num_ins as u32 + 2) } else { 0 };
                let note = if g.r(3) == 0 { if g.r(4) == 0 { 254 } else { g.r(96) } } else { 255 };
                let mut cmd = if g.r(3) == 0 { g.r(16) } else { 0 };
                let mut arg = if cmd == 0 { if g.r(6) == 0 { g.r(256) } else { 0 } } else if g.r(5) == 0 { 0 } else { g.r(256) };
                if cmd == 11 { arg = g.r(num_ptr as u32 + 1); }
                else if cmd == 13 { arg = (g.r(6) << 4) | g.r(10); }
                else if cmd == 15 { arg = if g.r(2) == 0 { 1 + g.r(8) } else { 32 + g.r(130) }; }
                let vol = if g.r(4) == 0 { if g.r(3) == 0 { 96 + g.r(64) } else { 16 + g.r(65) } } else { 255 };
                let _ = &mut cmd;
                let o = (row + rows[p] * ch) * 6;
                body[o..o + 5].copy_from_slice(&[ins as u8, note as u8, cmd as u8, arg as u8, vol as u8]);
            }
        }
        pats.extend_from_slice(&ph);
        pats.extend_from_slice(&body);
    }
    let mut out = hdr;
    out.extend_from_slice(&pats);
    out.extend_from_slice(&samples);
    out
}

fn fnv(b: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &x in b { h = (h ^ x as u64).wrapping_mul(0x100000001b3); }
    h
}

/// run the ops, returning the tokens
fn run(p: &mut Player, ops: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut group: Vec<u8> = Vec::new();
    let mut blocks_in_group = 0;
    let mut flush = |group: &mut Vec<u8>, n: &mut i32, toks: &mut Vec<String>| {
        if *n > 0 { toks.push(format!("h{:016x}", fnv(group))); group.clear(); *n = 0; }
    };
    for op in ops.split(';') {
        let (k, n) = (op.as_bytes()[0], op[1..].parse::<usize>().unwrap_or(0));
        match k {
            b'B' => {
                for _ in 0..n {
                    let mut buf = vec![0i16; FRAMES * 2];
                    p.render(&mut buf);
                    for s in buf { group.extend_from_slice(&s.to_le_bytes()); }
                    blocks_in_group += 1;
                    if blocks_in_group == 16 { flush(&mut group, &mut blocks_in_group, &mut toks); }
                }
                flush(&mut group, &mut blocks_in_group, &mut toks);
            }
            b'S' => { p.stop(); toks.push("s".into()); }
            b'R' => { p.resume(); toks.push("r".into()); }
            b'T' => { p.start(); toks.push("t".into()); }
            _ => panic!("op {}", op),
        }
    }
    toks
}

fn assets() -> Option<std::path::PathBuf> {
    std::env::var("MR_ASSETS").ok().map(Into::into).or_else(|| Some(std::path::PathBuf::from("assets")).filter(|p| p.join("music").exists()))
}

#[test]
fn matches_original_engine() {
    let text = include_str!("vectors/music_vectors.txt");
    let (mut real, mut synth, mut bad) = (0, 0, Vec::new());
    for line in text.lines() {
        let (head, toks) = line.split_once(" -> ").unwrap();
        let mut it = head.split_whitespace();
        let kind = it.next().unwrap();
        let who = it.next().unwrap();
        let ops = it.next().unwrap().strip_prefix("ops=").unwrap();
        let bytes = match kind {
            "real" => match assets() {
                Some(a) => std::fs::read(a.join("music").join(format!("{}_music.madh", who))).unwrap(),
                None => continue,
            },
            _ => synth_module(who.parse().unwrap()),
        };
        let mut p = Player::new(Music::parse(&bytes).unwrap());
        let got = run(&mut p, ops).join(" ");
        if got != toks {
            let (g, w): (Vec<&str>, Vec<&str>) = (got.split(' ').collect(), toks.split(' ').collect());
            let i = g.iter().zip(&w).position(|(a, b)| a != b).unwrap_or(g.len().min(w.len()));
            bad.push(format!("{} {}: first differing token #{} (of {})", kind, who, i, w.len()));
        }
        if kind == "real" { real += 1 } else { synth += 1 }
    }
    println!("{} real tracks, {} synthetic modules compared", real, synth);
    assert!(bad.is_empty(), "{} mismatches:\n{}", bad.len(), bad.join("\n"));
}
