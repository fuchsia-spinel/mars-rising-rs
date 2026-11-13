//! Differential test for src/enemy.rs / src/enemy_ai.rs: every vector in tests/vectors/enemy_vectors.txt was produced by *executing the original
//! PowerPC routines* (group spawn 0x2da30, frame 0x2dc20 with all its type routines; tools/re/gen_enemy_vectors.py) on a random state. After every
//! operation the whole enemy pool (40 x 176 bytes), the group table (100 x 124 bytes), the live-group counter and the exact sequence of calls made
//! to the mocked surroundings (random numbers, score, ship damage, effects, sounds, enemy shots) must be identical.
use mars_rising::enemy::*;
use std::collections::HashMap;

mod common;
use common::parse_sparse_words;

fn fnv64(b: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &x in b {
        h = (h ^ x as u64).wrapping_mul(0x100000001b3);
    }
    h
}

struct Mock {
    tick: u32,
    np: u8,
    ships: [[u8; 68]; 2],
    dif: u32,
    rq: Vec<u32>,
    shots: Vec<i32>,
    log: Vec<String>,
}

fn view(b: &[u8; 68]) -> ShipView {
    ShipView {
        alive: b[1] == 3,
        vulnerable: b[64] == 0,
        x: f32::from_be_bytes(b[20..24].try_into().unwrap()),
        y: f32::from_be_bytes(b[24..28].try_into().unwrap()),
        w: i32::from_be_bytes(b[36..40].try_into().unwrap()),
        h: i32::from_be_bytes(b[40..44].try_into().unwrap()),
        hw: i32::from_be_bytes(b[44..48].try_into().unwrap()),
        hh: i32::from_be_bytes(b[48..52].try_into().unwrap()),
    }
}

impl EnemyWorld for Mock {
    fn tick(&self) -> u32 { self.tick }
    fn players(&self) -> u8 { self.np }
    fn ship(&self, n: u8) -> ShipView { view(&self.ships[n as usize - 1]) }
    fn random(&mut self, lo: i32, hi: i32) -> i32 {
        let raw = if self.rq.is_empty() { 0 } else { self.rq.remove(0) };
        let v = lo + (raw % ((hi - lo + 1) as u32)) as i32;
        self.log.push(format!("rnd_{}_{}_{}", lo, hi, v));
        v
    }
    fn difficulty(&self) -> u32 { self.dif }
    fn score(&mut self, owner: u8, points: i32) { self.log.push(format!("score{}_{}_0", owner, points)); }
    fn ship_damage(&mut self, n: u8, amount: i32) { self.log.push(format!("dmg{}_{}", n, amount)); }
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32) {
        self.log.push(format!("fx_{}_{}_{}_{}_{}_{}_{}_{:08x}_{:08x}", x, y, id, a, b, c, d, vx.to_bits(), vy.to_bits()));
    }
    fn aux(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32) { self.log.push(format!("aux_{}_{}_{}_{}_{}_{}", x, y, id, a, b, c)); }
    fn particles(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32) { self.log.push(format!("part_{}_{}_{}_{}_{}_{}", x, y, id, a, b, c)); }
    fn group_bonus(&mut self, owner: u8, x: f32, y: f32) { self.log.push(format!("bonus_{}_{:08x}_{:08x}", owner, x.to_bits(), y.to_bits())); }
    fn sound(&mut self, id: i32, prio: i32, delay: i32) { self.log.push(format!("snd_{}_{}_{}", id, prio, delay)); }
    fn sound_rate(&mut self, id: i32, prio: i32, rate: i32) { self.log.push(format!("sndr_{}_{}_{}", id, prio, rate)); }
    fn shot_aimed(&mut self, kind: i32, target: i32, flag: i32, x: f32, y: f32) -> i32 {
        self.log.push(format!("shot383b8_{}_{}_{}_{:08x}_{:08x}", kind, target, flag, x.to_bits(), y.to_bits()));
        if self.shots.is_empty() { 0 } else { self.shots.remove(0) }
    }
    fn shot_param(&mut self, a: [i32; 6], x: f32, y: f32) -> i32 {
        self.log.push(format!("shot38754_{}_{}_{}_{}_{}_{}_{:08x}_{:08x}", a[0], a[1], a[2], a[3], a[4], a[5], x.to_bits(), y.to_bits()));
        if self.shots.is_empty() { 0 } else { self.shots.remove(0) }
    }
}

fn digest(e: &Enemies) -> String { format!("h{:016x}.{:016x}.{}", fnv64(&e.pool), fnv64(&e.groups), e.live_groups) }

#[allow(dead_code)]
fn dump_diff(a: &[u8], b: &[u8], size: usize) -> String {
    let mut s = String::new();
    for k in 0..a.len() / 4 {
        if a[k * 4..k * 4 + 4] != b[k * 4..k * 4 + 4] {
            s += &format!("\n    rec {} +{}: rust {} want {}", (k * 4) / size, (k * 4) % size, hex(&a[k * 4..k * 4 + 4]), hex(&b[k * 4..k * 4 + 4]));
        }
    }
    s
}
fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{:02x}", x)).collect() }

#[test]
fn enemies_match_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/enemy_vectors.txt")).expect("vectors");
    let mut counts: HashMap<String, usize> = HashMap::new();
    let only: Option<u32> = std::env::var("ENEMY_TYPE").ok().and_then(|v| v.parse().ok());
    for (ln, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() { continue; }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let (head, ops) = lhs.split_once(" ops=").unwrap();
        let kv: HashMap<&str, &str> = head.split_whitespace().map(|t| t.split_once('=').unwrap()).collect();
        let ty: u32 = kv["ty"].parse().unwrap();
        if only.map_or(false, |o| o != ty) { continue; }
        let to68 = |s: &str| -> [u8; 68] { parse_sparse_words(s, 68).try_into().unwrap() };
        let mut mock = Mock {
            tick: kv["tick"].parse().unwrap(),
            np: kv["np"].parse().unwrap(),
            ships: [to68(kv["S1"]), to68(kv["S2"])],
            dif: kv["dif"].parse().unwrap(),
            rq: kv["R"].split(',').map(|v| u32::from_str_radix(v, 16).unwrap()).collect(),
            shots: if kv["Q"].is_empty() { vec![] } else { kv["Q"].split(',').map(|v| v.parse().unwrap()).collect() },
            log: vec![],
        };
        let mut en = Enemies::new();
        en.groups = parse_sparse_words(kv["G"], GROUPS * GROUP_SIZE);
        let want: Vec<&str> = rhs.split(' ').collect();
        for (k, op) in ops.split(';').enumerate() {
            mock.log.clear();
            let (kind, arg) = op.split_at(1);
            match kind {
                "S" => en.spawn_group(arg.parse().unwrap(), &mut mock),
                "F" => { mock.tick = arg.parse().unwrap(); en.frame(&mut mock) }
                "H" => {
                    let a: Vec<i32> = arg.split(',').map(|v| v.parse().unwrap()).collect();
                    let r = en.damage(a[0] as usize, a[1], a[2] as u8, &mut mock);
                    mock.log.push(format!("ret{}", r));
                }
                "K" => {
                    let (sl, mode) = arg.split_once(',').unwrap();
                    let (sl, mode): (usize, usize) = (sl.parse().unwrap(), mode.parse().unwrap());
                    let mut b = [100.0f32, 100.0, 200.0, 200.0];
                    let (ei, ev) = [(2, -64.0f32), (2, -64.5), (0, 480.0), (0, 479.5), (3, -64.0), (3, -64.5), (1, 544.0), (1, 543.5)][mode];
                    b[ei] = ev;
                    for (k, v) in b.iter().enumerate() { en.wf(sl, 56 + 4 * k, *v); }
                }
                "W" => {
                    let a: Vec<&str> = arg.split(',').collect();
                    en.w32(a[0].parse().unwrap(), a[1].parse().unwrap(), u32::from_str_radix(a[2], 16).unwrap());
                }
                "V" => {
                    let a: Vec<&str> = arg.split(',').collect();
                    en.w8(a[0].parse().unwrap(), a[1].parse().unwrap(), a[2].parse::<u32>().unwrap() as u8);
                }
                "X" => en.bomb(arg.parse().unwrap(), &mut mock),
                "U" => {
                    let (n, w) = arg.split_once(':').unwrap();
                    mock.ships[n.parse::<usize>().unwrap() - 1] = to68(w);
                }
                _ => panic!("op {}", op),
            }
            let got = digest(&en) + &mock.log.iter().map(|l| format!(",{}", l)).collect::<String>();
            if got != want[k] {
                let (gd, wd) = (got.split(',').next().unwrap(), want[k].split(',').next().unwrap());
                let mut msg = format!("line {} type {} op #{} `{}`\n  digest rust {} want {}", ln + 1, ty, k, op, gd, wd);
                let (gl, wl): (Vec<&str>, Vec<&str>) = (got.split(',').skip(1).collect(), want[k].split(',').skip(1).collect());
                if gl != wl {
                    msg += &format!("\n  calls rust {:?}\n  calls want {:?}", gl, wl);
                }
                if std::env::var("ENEMY_DUMP").is_ok() {
                    for sl in 0..SLOTS {
                        let r = &en.pool[sl * SLOT_SIZE..(sl + 1) * SLOT_SIZE];
                        let nz: Vec<String> = (0..SLOT_SIZE / 4).filter(|k| r[k * 4..k * 4 + 4] != [0; 4]).map(|k| format!("{}={}", k * 4, hex(&r[k * 4..k * 4 + 4]))).collect();
                        if !nz.is_empty() { msg += &format!("\nslot {} {}", sl, nz.join(" ")); }
                    }
                }
                panic!("{}", msg);
            }
        }
        *counts.entry(format!("type{}", ty)).or_default() += 1;
    }
    eprintln!("verified vectors: {:?}", counts);
}
