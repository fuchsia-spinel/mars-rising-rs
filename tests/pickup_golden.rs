//! Differential test for src/pickup.rs: every vector in tests/vectors/pickup_vectors.txt was produced by *executing the original
//! PowerPC routines* (spawner 0x2a040, frame update 0x2b884, collection 0x2c134; tools/re/gen_pickup_vectors.py) on a random state.
//! The Rust code must reproduce both player structs (all 408 bytes each), the whole pickup pool (20 x 128 bytes), the pickup counter and the
//! log of calls to the mocked callee routines (Random, effects, sounds, HUD, drones) exactly.
use mars_rising::pickup::*;
use mars_rising::ship::*;
use std::collections::HashMap;

mod common;
use common::*;

/// Compare two sparse pool dumps and report only the differing words (a whole pool is far too long to read).
fn check_pool(got: &str, want: &str, ctx: &str) {
    if got == want { return; }
    let parse = |s: &str| -> HashMap<usize, Vec<String>> {
        let mut m = HashMap::new();
        if s != "-" { for t in s.split(';') { let (a, b) = t.split_once(':').unwrap(); m.insert(a.parse().unwrap(), b.split(',').map(String::from).collect()); } }
        m
    };
    let (g, w) = (parse(got), parse(want));
    let zero = vec!["00000000".to_string(); REC / 4];
    let mut msg = String::new();
    for s in 0..PICKUP_SLOTS {
        let (a, b) = (g.get(&s).unwrap_or(&zero), w.get(&s).unwrap_or(&zero));
        for k in 0..REC / 4 {
            if a[k] != b[k] { msg += &format!("\n  slot {} +{}: rust {} original {}", s, k * 4, a[k], b[k]); }
        }
    }
    panic!("pickup pool differs, {}{}", ctx, msg);
}

struct Mock {
    rnd: Vec<u32>,
    drones: Vec<u32>,
    speed: i32,
    coin_style: bool,
    log: Vec<String>,
}

impl World for Mock {
    fn hit_enemies(&mut self, _x0: i32, _y0: i32, _x1: i32, _y1: i32, _hp: i32, _owner: u8) -> i32 { unreachable!() }
    fn effect(&mut self, x: i32, y: i32, kind: i32, delay: i32, param: i32, a: i32, b: i32, vx: f32, vy: f32) {
        self.log.push(format!("fx_{}_{}_{}_{}_{}_{}_{}_{:08x}_{:08x}", x, y, kind, delay, param, a, b, vx.to_bits(), vy.to_bits()));
    }
    fn effect_aux(&mut self, id: u32, x: i32, y: i32, kind: i32, delay: i32, param: i32) {
        self.log.push(format!("aux_{:x}_{}_{}_{}_{}_{}", id, x, y, kind, delay, param));
    }
    fn hud_lives(&mut self, set: u8, lives: i32, _flash: i32) { self.log.push(format!("hud_1f848_{}_{}", set, lives)); }
    fn hud(&mut self, id: u32, a: i32, b: i32) {
        let argc = match id { 0x1fe28 | 0x1ff94 | 0x1f69c | 0x1f848 => 2, 0x3fbfc | 0x2e6a8 => 1, _ => 0 };
        let mut t = format!("hud_{:x}", id);
        if argc >= 1 { t += &format!("_{}", a); }
        if argc >= 2 { t += &format!("_{}", b); }
        self.log.push(t);
    }
    fn sound(&mut self, id: i32, priority: i32, arg: i32) { self.log.push(format!("snd_{}_{}_{}", id, priority, arg)); }
    fn random(&mut self, lo: i32, hi: i32) -> i32 {
        let raw = if self.rnd.is_empty() { 0 } else { self.rnd.remove(0) };
        let v = lo + (raw % ((hi - lo + 1) as u32)) as i32;
        self.log.push(format!("rnd_{}_{}_{}", lo, hi, v));
        v
    }
    fn coin_style(&mut self) -> bool { self.coin_style }
    fn scroll_speed(&mut self) -> i32 { self.speed }
    fn drone_add(&mut self, number: u8, which: i32) -> bool {
        self.log.push(format!("dadd_{}_{}", number, which));
        if self.drones.is_empty() { false } else { self.drones.remove(0) != 0 }
    }
}

fn parse_q(s: &str) -> (Vec<u32>, Vec<u32>) {
    let mut r = vec![]; let mut d = vec![];
    for part in s.split('/') {
        let (k, v) = part.split_once(':').unwrap();
        if v == "-" { continue; }
        for x in v.split(',') {
            match k { "r" => r.push(x.parse().unwrap()), _ => d.push(x.parse().unwrap()) }
        }
    }
    (r, d)
}

#[test]
fn pickups_match_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/pickup_vectors.txt")).expect("vectors");
    let mut counts: HashMap<String, usize> = HashMap::new();
    for (ln, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() { continue; }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let kv = |s: &str| -> HashMap<String, String> {
            s.split_whitespace().map(|t| { let (k, v) = t.split_once('=').unwrap(); (k.to_string(), v.to_string()) }).collect()
        };
        let i = kv(lhs);
        let o = kv(rhs);
        let kind = i["kind"].as_str();
        let tick: u32 = i["tick"].parse().unwrap();
        let mut ab = parse_sparse_words(&i["A"], SHIP_SIZE);
        let mut bb = parse_sparse_words(&i["B"], SHIP_SIZE);
        let mut kb = parse_kpool(&i["K"]);
        let mut p1 = ship_from_bytes(&ab);
        let mut p2 = ship_from_bytes(&bb);
        let mut pool = PickupPool::new();
        for s in 0..PICKUP_SLOTS { pool.slots[s] = pickup_from_bytes(&kb[s * REC..(s + 1) * REC]); }
        pool.count = i["count"].parse().unwrap();
        let (rnd, drones) = parse_q(&i["Q"]);
        let d: Vec<i32> = i["D"].split(',').map(|v| v.parse().unwrap()).collect();
        let mut mock = Mock { rnd, drones, speed: d[0], coin_style: d[1] != 0, log: vec![] };
        let x = &i["X"];
        match kind {
            "spawn" => {
                let t: Vec<&str> = x.split(',').collect();
                let n = |k: usize| -> i64 { t[k].parse().unwrap() };
                let fx = |k: usize| f32::from_bits(u32::from_str_radix(t[k], 16).unwrap());
                pool.spawn(&p1, &p2, n(0) as u8, n(1) as i32, n(2) as i32, n(3) as i32, fx(4), fx(5), tick, &mut mock);
            }
            "pupd" => pool.update(&mut p1, &mut p2, tick, &mut mock),
            "coll" => {
                let t: Vec<usize> = x.split(',').map(|v| v.parse().unwrap()).collect();
                let pk = pool.slots[t[0]];
                if t[1] == 1 { p1.collect(&pk, tick, &mut mock) } else { p2.collect(&pk, tick, &mut mock) }
            }
            k => panic!("unknown kind {}", k),
        }
        ship_to_bytes(&p1, &mut ab);
        ship_to_bytes(&p2, &mut bb);
        for s in 0..PICKUP_SLOTS { pickup_to_bytes(&pool.slots[s], &mut kb[s * REC..(s + 1) * REC]); }
        let ctx = format!("line {} kind {}", ln + 1, kind);
        check_pool(&kpool_str(&kb), &o["K2"], &ctx);
        assert_eq!(pool.count.to_string(), o["count2"], "pickup counter, {}", ctx);
        assert_eq!(sparse_words(&ab), o["A2"], "player 1, {}", ctx);
        assert_eq!(sparse_words(&bb), o["B2"], "player 2, {}", ctx);
        let elog = if mock.log.is_empty() { "-".to_string() } else { mock.log.join(";") };
        assert_eq!(elog, o["E"], "call log, {}", ctx);
        *counts.entry(kind.to_string()).or_default() += 1;
    }
    eprintln!("verified vectors: {:?}", counts);
    assert!(counts.values().all(|&n| n > 0) && counts.len() == 3);
}
