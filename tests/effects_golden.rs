//! Differential test for src/effects.rs (pool initialisation 0x22618, effect spawner 0x22670, particle burst 0x23440, update / draw 0x236b4): every vector in
//! tests/vectors/effects_vectors.txt was produced by *executing the original PowerPC routines* (tools/re/gen_effects_vectors.py). After every operation the
//! effect pool (60 x 180 bytes), the direction table, its index and the exact sequence of calls to the mocked surroundings (random numbers, sounds, blits)
//! must be identical.
use mars_rising::draw::Prim;
use mars_rising::effects::*;
use std::collections::HashMap;

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
    trans: bool,
    fancy: bool,
    rq: Vec<u32>,
    log: Vec<String>,
}

impl EffectWorld for Mock {
    fn tick(&self) -> u32 { self.tick }
    fn scroll_step(&self) -> i32 { self.scroll }
    fn translucent(&self) -> bool { self.trans }
    fn fancy(&self) -> bool { self.fancy }
    fn random(&mut self, lo: i32, hi: i32) -> i32 {
        let raw = if self.rq.is_empty() { 0 } else { self.rq.remove(0) };
        let v = lo + (raw % ((hi - lo + 1) as u32)) as i32;
        self.log.push(format!("rnd_{}_{}_{}", lo, hi, v));
        v
    }
    fn sound(&mut self, id: i32, prio: i32, arg: i32) { self.log.push(format!("snd_{}_{}_{}", id, prio, arg)); }
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.log.push(format!("d{:x}_0_{}_{}_{}_{}_{}_{}", prim.addr(), x, y, w, h, anim, frame));
    }
}

fn digest(e: &Effects) -> String {
    format!("h{:016x}.{:016x}.{}", fnv64(&e.rec), fnv64(&e.star_bytes()), e.next_star as i32)
}

#[test]
fn effects_match_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/effects_vectors.txt")).expect("vectors");
    let mut calls_seen: HashMap<String, usize> = HashMap::new();
    let mut n_vec = 0;
    for (ln, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() { continue; }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let (head, ops) = lhs.split_once(" ops=").unwrap();
        let kv: HashMap<&str, &str> = head.split_whitespace().map(|t| t.split_once('=').unwrap()).collect();
        let mut mock = Mock {
            tick: kv["T"].parse().unwrap(),
            scroll: 1,
            trans: true,
            fancy: true,
            rq: kv["R"].split(',').map(|v| u32::from_str_radix(v, 16).unwrap()).collect(),
            log: vec![],
        };
        let mut fx = Effects::new();
        let want: Vec<&str> = rhs.split(' ').collect();
        assert_eq!(want.len(), ops.split(';').count(), "line {}", ln + 1);
        for (k, op) in ops.split(';').enumerate() {
            mock.log.clear();
            let (kind, arg) = op.split_at(1);
            let a: Vec<&str> = if arg.is_empty() { vec![] } else { arg.split(',').collect() };
            let n = |i: usize| -> i32 { a[i].parse().unwrap() };
            let f = |i: usize| -> f32 { f32::from_bits(u32::from_str_radix(a[i], 16).unwrap()) };
            match kind {
                "I" => fx.init(&mut mock),
                "E" => fx.spawn(&mut mock, n(1), n(2), n(0), n(3), n(4), n(5), n(6), f(7), f(8)),
                "P" => fx.particles(&mut mock, n(0), n(1), n(2), n(3), n(4), n(5)),
                "U" => {
                    mock.tick = n(0) as u32;
                    mock.scroll = n(1);
                    mock.trans = n(2) != 0;
                    mock.fancy = n(3) != 0;
                    fx.update(&mut mock);
                }
                "W" => fx.w32(n(0) as usize, n(1) as usize, u32::from_str_radix(a[2], 16).unwrap()),
                "B" => fx.w8(n(0) as usize, n(1) as usize, u32::from_str_radix(a[2], 16).unwrap() as u8),
                "T" => mock.tick = arg.parse().unwrap(),
                _ => panic!("op {}", op),
            }
            let got = digest(&fx) + &mock.log.iter().map(|l| format!(",{}", l)).collect::<String>();
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
