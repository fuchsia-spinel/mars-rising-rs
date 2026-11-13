//! Differential test for src/boss.rs: every vector in tests/vectors/boss_vectors.txt was produced by *executing the original PowerPC routines*
//! (registration 0x40dc4, free 0x40d6c, spawn 0x40e7c with the type inits, update 0x4127c with the type routines, draw 0x417a4, damage 0x41c88, sound callback
//! 0x4123c; tools/re/gen_boss_vectors.py) on a random state. After every operation the boss pool (10 x 228 bytes) and counter, the enemy pool (40 x 176
//! bytes), the group table and the exact sequence of calls to the mocked surroundings must be identical.
use mars_rising::boss::*;
use mars_rising::draw::{DrawOpts, Prim};
use mars_rising::enemy::*;
use mars_rising::enemy_draw::EnemyDrawWorld;
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
    opts: DrawOpts,
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
    fn pickup(&mut self, kind: i32, variant: i32, param: i32, announce: i32, x: f32, y: f32) {
        self.log.push(format!("pk_{}_{}_{}_{}_{:08x}_{:08x}", kind, variant, param, announce, x.to_bits(), y.to_bits()));
    }
    fn sound_loop(&mut self, id: i32, prio: i32, boss: i32) -> u32 {
        self.log.push(format!("scb_{}_{}_{}", id, prio, boss));
        0x7100 + boss as u32
    }
}

impl EnemyDrawWorld for Mock {
    fn tick(&self) -> u32 { self.tick }
    fn random(&mut self, lo: i32, hi: i32) -> i32 { EnemyWorld::random(self, lo, hi) }
    fn opts(&self) -> DrawOpts { self.opts }
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.log.push(format!("d{:x}_0_{}_{}_{}_{}_{}_{}", prim.addr(), x, y, w, h, anim, frame));
    }
}

fn digest(e: &Enemies) -> String {
    format!("h{:016x}.{:016x}.{},b{:016x}.{}", fnv64(&e.pool), fnv64(&e.groups), e.live_groups, fnv64(&e.bosses.rec), e.bosses.count)
}

fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{:02x}", x)).collect() }

#[test]
fn bosses_match_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/boss_vectors.txt")).expect("vectors");
    let mut counts: HashMap<String, usize> = HashMap::new();
    for (ln, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() { continue; }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let (head, ops) = lhs.split_once(" ops=").unwrap();
        let kv: HashMap<&str, &str> = head.split_whitespace().map(|t| t.split_once('=').unwrap()).collect();
        let ty: i32 = kv["ty"].parse().unwrap();
        let to68 = |s: &str| -> [u8; 68] { parse_sparse_words(s, 68).try_into().unwrap() };
        let mut mock = Mock {
            tick: kv["tick"].parse().unwrap(),
            np: kv["np"].parse().unwrap(),
            ships: [to68(kv["S1"]), to68(kv["S2"])],
            dif: kv["dif"].parse().unwrap(),
            rq: kv["R"].split(',').map(|v| u32::from_str_radix(v, 16).unwrap()).collect(),
            shots: if kv["Z"].is_empty() { vec![] } else { kv["Z"].split(',').map(|v| v.parse().unwrap()).collect() },
            opts: DrawOpts::default(),
            log: vec![],
        };
        let mut en = Enemies::new();
        en.groups = parse_sparse_words(kv["G"], GROUPS * GROUP_SIZE);
        let want: Vec<&str> = rhs.split(' ').collect();
        for (k, op) in ops.split(';').enumerate() {
            if std::env::var("BOSS_TRACE").is_ok() { eprintln!("line {} op #{} {}", ln + 1, k, op); }
            mock.log.clear();
            let (kind, arg) = op.split_at(1);
            match kind {
                "R" => {
                    let r = en.boss_register(arg.parse().unwrap());
                    mock.log.push(format!("r{}", r));
                }
                "Z" => en.bosses.free_all(),
                "S" => en.boss_spawn(arg.parse().unwrap(), &mut mock),
                "F" => { mock.tick = arg.parse().unwrap(); en.boss_update(&mut mock); }
                "D" => {
                    let a: Vec<i32> = arg.split(',').map(|v| v.parse().unwrap()).collect();
                    mock.tick = a[0] as u32;
                    mock.opts = DrawOpts { translucent: a[1] != 0, shadows: a[2] != 0 };
                    en.boss_draw(&mut mock);
                }
                "H" => {
                    let a: Vec<i32> = arg.split(',').map(|v| v.parse().unwrap()).collect();
                    let r = en.boss_hit(a[0], a[1], a[2], a[3] as u8, &mut mock);
                    mock.log.push(format!("r{}", r));
                }
                "C" => {
                    let a: Vec<i32> = arg.split(',').map(|v| v.parse().unwrap()).collect();
                    en.bosses.sound_ended(a[0], a[1]);
                }
                "U" => {
                    let (n, w) = arg.split_once(':').unwrap();
                    mock.ships[n.parse::<usize>().unwrap() - 1] = to68(w);
                }
                "W" => {
                    let a: Vec<&str> = arg.split(',').collect();
                    en.w32(a[0].parse().unwrap(), a[1].parse().unwrap(), u32::from_str_radix(a[2], 16).unwrap());
                }
                "V" => {
                    let a: Vec<&str> = arg.split(',').collect();
                    en.w8(a[0].parse().unwrap(), a[1].parse().unwrap(), a[2].parse::<u32>().unwrap() as u8);
                }
                "Q" => {
                    let a: Vec<&str> = arg.split(',').collect();
                    en.bosses.w32(a[0].parse().unwrap(), a[1].parse().unwrap(), u32::from_str_radix(a[2], 16).unwrap());
                }
                "q" => {
                    let a: Vec<&str> = arg.split(',').collect();
                    en.bosses.w8(a[0].parse().unwrap(), a[1].parse().unwrap(), a[2].parse::<u32>().unwrap() as u8);
                }
                _ => panic!("op {}", op),
            }
            let got = digest(&en) + &mock.log.iter().map(|l| format!(",{}", l)).collect::<String>();
            if got != want[k] {
                let (gd, wd) = (got.split(',').take(2).collect::<Vec<_>>().join(","), want[k].split(',').take(2).collect::<Vec<_>>().join(","));
                let mut msg = format!("line {} type {} op #{} `{}`\n  digest rust {} want {}", ln + 1, ty, k, op, gd, wd);
                let (gl, wl): (Vec<&str>, Vec<&str>) = (got.split(',').skip(2).collect(), want[k].split(',').skip(2).collect());
                if gl != wl {
                    msg += &format!("\n  calls rust {:?}\n  calls want {:?}", gl, wl);
                }
                if std::env::var("BOSS_DUMP").is_ok() {
                    for bi in 0..BOSSES {
                        let r = &en.bosses.rec[bi * REC..(bi + 1) * REC];
                        let nz: Vec<String> = (0..REC / 4).filter(|k| r[k * 4..k * 4 + 4] != [0; 4]).map(|k| format!("{}={}", k * 4, hex(&r[k * 4..k * 4 + 4]))).collect();
                        if !nz.is_empty() { msg += &format!("\nboss {} {}", bi, nz.join(" ")); }
                    }
                    for g in 0..100 {
                        let r = &en.groups[g * 124..(g + 1) * 124];
                        let nz: Vec<String> = (0..31).filter(|k| r[k * 4..k * 4 + 4] != [0; 4]).map(|k| format!("{}={}", k * 4, hex(&r[k * 4..k * 4 + 4]))).collect();
                        if !nz.is_empty() { msg += &format!("\ngroup {} {}", g, nz.join(" ")); }
                    }
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
