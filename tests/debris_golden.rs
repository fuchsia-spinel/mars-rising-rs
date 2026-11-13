//! Differential test for src/debris.rs (initialisation 0x2942c, spawners 0x29714 / 0x298ec, update / draw 0x29b10): every vector in tests/vectors/debris_vectors.txt
//! was produced by *executing the original PowerPC routines* (tools/re/gen_debris_vectors.py). After every operation the pool (10 x 976 bytes), the direction
//! table, its index, every pixel written to the off-screen buffer and the exact sequence of calls to the mocked surroundings must be identical.
use mars_rising::debris::*;
use std::collections::HashMap;

const ROW: usize = 704;
const ROWS: usize = 608;

fn fnv64(b: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &x in b {
        h = (h ^ x as u64).wrapping_mul(0x100000001b3);
    }
    h
}

struct Mock {
    scroll: i32,
    rq: Vec<u32>,
    log: Vec<String>,
    buf: Vec<u8>,
}

impl DebrisWorld for Mock {
    fn scroll_step(&self) -> i32 { self.scroll }
    fn random(&mut self, lo: i32, hi: i32) -> i32 {
        let raw = if self.rq.is_empty() { 0 } else { self.rq.remove(0) };
        let v = lo + (raw % ((hi - lo + 1) as u32)) as i32;
        self.log.push(format!("rnd_{}_{}_{}", lo, hi, v));
        v
    }
    fn sound(&mut self, id: i32, prio: i32, arg: i32) { self.log.push(format!("snd_{}_{}_{}", id, prio, arg)); }
    fn plot(&mut self, x: i32, y: i32, colour: u8) {
        for dy in 0..3 {
            for dx in 0..3 {
                let (px, py) = (x + dx, y + dy);
                if px >= 0 && py >= 0 && (px as usize) < ROW && (py as usize) < ROWS { self.buf[py as usize * ROW + px as usize] = colour; }
            }
        }
    }
}

fn digest(d: &Debris) -> String {
    format!("h{:016x}.{:016x}.{}", fnv64(&d.rec), fnv64(&d.star_bytes()), d.next_star as i32)
}

/// `,px<hash>.<count>` over every nonzero byte of the buffer (then cleared), like the generator.
fn pixels(buf: &mut [u8]) -> String {
    let (mut h, mut n) = (0xcbf29ce484222325u64, 0);
    for (pos, v) in buf.iter().enumerate() {
        if *v != 0 {
            for b in (pos as u32).to_be_bytes().into_iter().chain([*v]) {
                h = (h ^ b as u64).wrapping_mul(0x100000001b3);
            }
            n += 1;
        }
    }
    if n == 0 { return String::new(); }
    buf.iter_mut().for_each(|b| *b = 0);
    format!(",px{:016x}.{}", h, n)
}

#[test]
fn debris_matches_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/debris_vectors.txt")).expect("vectors");
    let mut calls_seen: HashMap<String, usize> = HashMap::new();
    let mut n_vec = 0;
    for (ln, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() { continue; }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let (head, ops) = lhs.split_once(" ops=").unwrap();
        let kv: HashMap<&str, &str> = head.split_whitespace().map(|t| t.split_once('=').unwrap()).collect();
        let colours: Vec<u8> = (0..130).map(|i| u8::from_str_radix(&kv["C"][i * 2..i * 2 + 2], 16).unwrap()).collect();
        let mut mock = Mock { scroll: 1, rq: kv["R"].split(',').map(|v| u32::from_str_radix(v, 16).unwrap()).collect(), log: vec![], buf: vec![0; ROW * ROWS] };
        let mut d = Debris::new();
        let want: Vec<&str> = rhs.split(' ').collect();
        assert_eq!(want.len(), ops.split(';').count(), "line {}", ln + 1);
        for (k, op) in ops.split(';').enumerate() {
            mock.log.clear();
            let (kind, arg) = op.split_at(1);
            let a: Vec<&str> = if arg.is_empty() { vec![] } else { arg.split(',').collect() };
            let n = |i: usize| -> i32 { a[i].parse().unwrap() };
            let mut px = String::new();
            match kind {
                "I" => d.init(&mut mock, &colours),
                "A" => d.spawn(&mut mock, n(0), n(1), n(2), n(3), n(4), n(5)),
                "B" => d.spawn_in(&mut mock, n(0), n(1), n(2), n(3), n(4), n(5)),
                "U" => {
                    mock.scroll = n(0);
                    d.update(&mut mock);
                    px = pixels(&mut mock.buf);
                }
                "W" => d.w32(n(0) as usize, u32::from_str_radix(a[1], 16).unwrap()),
                "V" => d.w8(n(0) as usize, u32::from_str_radix(a[1], 16).unwrap() as u8),
                _ => panic!("op {}", op),
            }
            let got = digest(&d) + &px + &mock.log.iter().map(|l| format!(",{}", l)).collect::<String>();
            for l in &mock.log {
                *calls_seen.entry(l.split('_').next().unwrap().to_string()).or_default() += 1;
            }
            if got != want[k] {
                let (gl, wl): (Vec<&str>, Vec<&str>) = (got.split(',').skip(1).collect(), want[k].split(',').skip(1).collect());
                let mut msg = format!("line {} op #{} `{}`\n  digest rust {} want {}", ln + 1, k, op, got.split(',').next().unwrap(), want[k].split(',').next().unwrap());
                if gl != wl { msg += &format!("\n  rest rust {:?}\n  rest want {:?}", gl, wl); }
                panic!("{}", msg);
            }
        }
        n_vec += 1;
    }
    let mut calls: Vec<_> = calls_seen.into_iter().collect();
    calls.sort();
    eprintln!("verified {} vectors; calls {:?}", n_vec, calls);
}
