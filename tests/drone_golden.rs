//! Differential test for src/drone.rs (drones: add 0x2c760, remove 0x2c87c, fire 0x2c960 + shot spawner 0x21288, update 0x2cb40, draw 0x2d1d8; side missiles: launch 0x277e4,
//! update 0x27dfc, draw 0x28260, clears): every vector in tests/vectors/drone_vectors.txt was produced by *executing the original PowerPC routines*
//! (tools/re/gen_drone_vectors.py). After every operation the drone pool, the missile pool, the player-shot pool, the three counters, both complete ship structures
//! (408 bytes) and the exact sequence of calls to the mocked surroundings must be identical.
use mars_rising::draw::Prim;
use mars_rising::drone::*;
use std::collections::HashMap;

mod common;
use common::{parse_sparse_words, pool_from_bytes, pool_to_bytes, ship_from_bytes, ship_to_bytes, SHIP_SIZE};

fn fnv64(b: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &x in b {
        h = (h ^ x as u64).wrapping_mul(0x100000001b3);
    }
    h
}

struct Mock {
    tick: u32,
    shadows: bool,
    trans: bool,
    fancy: bool,
    hq: Vec<i32>,
    log: Vec<String>,
}

impl DroneWorld for Mock {
    fn tick(&self) -> u32 { self.tick }
    fn hit_enemies(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, hp: i32, owner: u8) -> i32 {
        self.log.push(format!("he_{}_{}_{}_{}_{}_{}", x0, y0, x1, y1, hp, owner));
        if self.hq.is_empty() { 0 } else { self.hq.remove(0) }
    }
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32) {
        self.log.push(format!("fx_{}_{}_{}_{}_{}_{}_{}_{:08x}_{:08x}", x, y, id, a, b, c, d, vx.to_bits(), vy.to_bits()));
    }
    fn sound(&mut self, id: i32, prio: i32, arg: i32) { self.log.push(format!("snd_{}_{}_{}", id, prio, arg)); }
    fn shadows(&self) -> bool { self.shadows }
    fn translucent(&self) -> bool { self.trans }
    fn fancy(&self) -> bool { self.fancy }
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.log.push(format!("d{:x}_0_{}_{}_{}_{}_{}_{}", prim.addr(), x, y, w, h, anim, frame));
    }
}

#[test]
fn drones_and_missiles_match_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/drone_vectors.txt")).expect("vectors");
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
            shadows: true,
            trans: true,
            fancy: true,
            hq: kv["Q"].split(',').filter(|v| !v.is_empty()).map(|v| v.parse().unwrap()).collect(),
            log: vec![],
        };
        let mut drones = Drones::new();
        let mut missiles = Missiles::new();
        let mut shots = pool_from_bytes(&[0u8; 60 * 48]);
        let want: Vec<&str> = rhs.split(' ').collect();
        assert_eq!(want.len(), ops.split(';').count(), "line {}", ln + 1);
        for (k, op) in ops.split(';').enumerate() {
            mock.log.clear();
            let (kind, arg) = op.split_at(1);
            let a: Vec<&str> = if arg.is_empty() { vec![] } else { arg.split(',').collect() };
            let n = |i: usize| -> i32 { a[i].parse().unwrap() };
            let hex = |i: usize| -> u32 { u32::from_str_radix(a[i], 16).unwrap() };
            let mut ret: Option<i32> = None;
            match kind {
                "A" => ret = Some(drones.add(n(0) as u8, n(1), &mock) as i32),
                "R" => drones.remove(arg.parse::<i32>().unwrap() as u8, &mut mock),
                "F" => {
                    let idx = arg.parse::<usize>().unwrap() - 1;
                    let s = ship_from_bytes(&ships[idx]);
                    drones.fire(&s, &mut shots);
                }
                "U" => {
                    mock.tick = arg.parse().unwrap();
                    let (s1, s2) = (ship_from_bytes(&ships[0]), ship_from_bytes(&ships[1]));
                    drones.update(&s1, &s2, &mut mock);
                }
                "D" => {
                    mock.tick = n(0) as u32;
                    mock.shadows = n(1) != 0;
                    mock.trans = n(2) != 0;
                    mock.fancy = n(3) != 0;
                    let (s1, s2) = (ship_from_bytes(&ships[0]), ship_from_bytes(&ships[1]));
                    drones.draw(&s1, &s2, &mut mock);
                }
                "Z" => drones.clear(),
                "L" => {
                    let idx = arg.parse::<usize>().unwrap() - 1;
                    let s = ship_from_bytes(&ships[idx]);
                    ret = Some(missiles.launch(&s, &mock) as i32);
                }
                "M" => {
                    mock.tick = arg.parse().unwrap();
                    missiles.update(&mut mock);
                }
                "N" => {
                    mock.tick = n(0) as u32;
                    mock.shadows = n(1) != 0;
                    mock.trans = n(2) != 0;
                    mock.fancy = n(3) != 0;
                    missiles.draw(&mut mock);
                }
                "C" => missiles.clear(),
                "P" => drones.set_word(n(0) as usize, n(1) as usize, hex(2)),
                "p" => drones.set_byte(n(0) as usize, n(1) as usize, hex(2) as u8),
                "Q" => missiles.set_word(n(0) as usize, n(1) as usize, hex(2)),
                "q" => missiles.set_byte(n(0) as usize, n(1) as usize, hex(2) as u8),
                "G" => shots.shots[n(0) as usize].active = n(1) != 0,
                "H" => shots.count = arg.parse::<i64>().unwrap() as i32,
                "K" => drones.count = arg.parse::<i64>().unwrap() as i32,
                "J" => missiles.count = arg.parse::<i64>().unwrap() as i32,
                "X" => {
                    let idx = n(0) as usize - 1;
                    let off = n(1) as usize;
                    let v = hex(2);
                    if n(3) == 1 { ships[idx][off] = v as u8 } else { ships[idx][off..off + 4].copy_from_slice(&v.to_be_bytes()) }
                }
                "T" => mock.tick = arg.parse().unwrap(),
                _ => panic!("op {}", op),
            }
            let digest = format!(
                "h{:016x}.{}.{:016x}.{}.{:016x}.{}.{:016x}.{:016x}",
                fnv64(drones.bytes()), drones.count, fnv64(missiles.bytes()), missiles.count, fnv64(&pool_to_bytes(&shots)), shots.count, fnv64(&ships[0]), fnv64(&ships[1])
            );
            let mut got = digest + &mock.log.iter().map(|l| format!(",{}", l)).collect::<String>();
            if let Some(r) = ret { got += &format!(",r{}", r); }
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
