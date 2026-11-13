//! Differential test for src/bomb.rs (button-2 control 0x26b58, launch 0x201c0, update 0x203a4, draw 0x205ec): every vector in tests/vectors/bomb_vectors.txt was
//! produced by *executing the original PowerPC routines* (tools/re/gen_bomb_vectors.py). After every operation the bomb pool (30 x 44 bytes), the counter,
//! both complete ship structures (408 bytes) and the exact sequence of calls to the mocked surroundings must be identical.
use mars_rising::bomb::*;
use mars_rising::draw::Prim;
use std::collections::HashMap;

mod common;
use common::{parse_sparse_words, ship_from_bytes, ship_to_bytes, SHIP_SIZE};

fn fnv64(b: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &x in b {
        h = (h ^ x as u64).wrapping_mul(0x100000001b3);
    }
    h
}

struct Mock {
    tick: u32,
    scroll: i32,
    shadows: bool,
    trans: bool,
    fancy: bool,
    rq: Vec<u32>,
    gq: Vec<i32>,
    log: Vec<String>,
}

impl BombWorld for Mock {
    fn tick(&self) -> u32 { self.tick }
    fn scroll_step(&self) -> i32 { self.scroll }
    fn random(&mut self, lo: i32, hi: i32) -> i32 {
        let raw = if self.rq.is_empty() { 0 } else { self.rq.remove(0) };
        let v = lo + (raw % ((hi - lo + 1) as u32)) as i32;
        self.log.push(format!("rnd_{}_{}_{}", lo, hi, v));
        v
    }
    fn sound(&mut self, id: i32, prio: i32, arg: i32) { self.log.push(format!("snd_{}_{}_{}", id, prio, arg)); }
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32) {
        self.log.push(format!("fx_{}_{}_{}_{}_{}_{}_{}_{:08x}_{:08x}", x, y, id, a, b, c, d, vx.to_bits(), vy.to_bits()));
    }
    fn ground_hit(&mut self, l: i32, t: i32, r: i32, b: i32, amount: i32, owner: u8) -> i32 {
        self.log.push(format!("gh_{}_{}_{}_{}_{}_{}", l, t, r, b, amount, owner));
        if self.gq.is_empty() { -1 } else { self.gq.remove(0) }
    }
    fn add_decal(&mut self, a: [i32; 8], p9: i32, p10: i32) {
        self.log.push(format!("decal_{}_{}_{}_{}_{}_{}_{}_{}_{}_{}", a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], p9, p10));
    }
    fn shadows(&self) -> bool { self.shadows }
    fn translucent(&self) -> bool { self.trans }
    fn fancy(&self) -> bool { self.fancy }
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.log.push(format!("d{:x}_0_{}_{}_{}_{}_{}_{}", prim.addr(), x, y, w, h, anim, frame));
    }
}

fn digest(b: &Bombs, ships: &[Vec<u8>; 2]) -> String {
    format!("h{:016x}.{}.{:016x}.{:016x}", fnv64(&b.rec), b.count, fnv64(&ships[0]), fnv64(&ships[1]))
}

#[test]
fn bombs_match_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/bomb_vectors.txt")).expect("vectors");
    let mut calls_seen: HashMap<String, usize> = HashMap::new();
    let mut n_vec = 0;
    for (ln, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() { continue; }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let (head, ops) = lhs.split_once(" ops=").unwrap();
        let kv: HashMap<&str, &str> = head.split_whitespace().map(|t| t.split_once('=').unwrap()).collect();
        let mut ships = [parse_sparse_words(kv["S1"], SHIP_SIZE), parse_sparse_words(kv["S2"], SHIP_SIZE)];
        let mut mock = Mock {
            tick: kv["T"].parse().unwrap(),
            scroll: 1,
            shadows: true,
            trans: true,
            fancy: true,
            rq: kv["R"].split(',').map(|v| u32::from_str_radix(v, 16).unwrap()).collect(),
            gq: if kv["Q"].is_empty() { vec![] } else { kv["Q"].split(',').map(|v| v.parse().unwrap()).collect() },
            log: vec![],
        };
        let mut bombs = Bombs::new();
        let want: Vec<&str> = rhs.split(' ').collect();
        assert_eq!(want.len(), ops.split(';').count(), "line {}", ln + 1);
        for (k, op) in ops.split(';').enumerate() {
            mock.log.clear();
            let (kind, arg) = op.split_at(1);
            let a: Vec<&str> = if arg.is_empty() { vec![] } else { arg.split(',').collect() };
            let n = |i: usize| -> i32 { a[i].parse().unwrap() };
            match kind {
                "S" => {
                    let idx = arg.parse::<usize>().unwrap() - 1;
                    let mut s = ship_from_bytes(&ships[idx]);
                    special_step(&mut s, &mut bombs, &mut mock);
                    ship_to_bytes(&s, &mut ships[idx]);
                }
                "U" => {
                    mock.tick = n(0) as u32;
                    mock.scroll = n(1);
                    bombs.update(&mut mock);
                }
                "D" => {
                    mock.tick = n(0) as u32;
                    mock.shadows = n(1) != 0;
                    mock.trans = n(2) != 0;
                    mock.fancy = n(3) != 0;
                    bombs.draw(&mut mock);
                }
                "Z" => bombs.clear(),
                "W" => bombs.w32(n(0) as usize, n(1) as usize, u32::from_str_radix(a[2], 16).unwrap()),
                "X" => {
                    let idx = n(0) as usize - 1;
                    let off = n(1) as usize;
                    let v = u32::from_str_radix(a[2], 16).unwrap();
                    if n(3) == 1 { ships[idx][off] = v as u8 } else { ships[idx][off..off + 4].copy_from_slice(&v.to_be_bytes()) }
                }
                "K" => bombs.count = arg.parse::<i64>().unwrap() as i32,
                "T" => mock.tick = arg.parse().unwrap(),
                _ => panic!("op {}", op),
            }
            let got = digest(&bombs, &ships) + &mock.log.iter().map(|l| format!(",{}", l)).collect::<String>();
            for l in &mock.log {
                *calls_seen.entry(l.split('_').next().unwrap().to_string()).or_default() += 1;
            }
            if got != want[k] {
                let (gl, wl): (Vec<&str>, Vec<&str>) = (got.split(',').skip(1).collect(), want[k].split(',').skip(1).collect());
                let mut msg = format!("line {} op #{} `{}`\n  digest rust {} want {}", ln + 1, k, op, got.split(',').next().unwrap(), want[k].split(',').next().unwrap());
                if gl != wl { msg += &format!("\n  calls rust {:?}\n  calls want {:?}", gl, wl); }
                panic!("{}", msg);
            }
        }
        n_vec += 1;
    }
    let mut calls: Vec<_> = calls_seen.into_iter().collect();
    calls.sort();
    eprintln!("verified {} vectors; calls {:?}", n_vec, calls);
}
