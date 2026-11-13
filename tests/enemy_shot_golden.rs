//! Differential test for src/enemy_shot.rs (the enemy-shot pool): the vectors in tests/vectors/enemy_shot_vectors.txt were produced by
//! *executing the original PowerPC routines* (0x37f60, 0x383b8, 0x38754 with its nine kind routines, 0x37fd4, 0x38b0c, 0x38a1c, 0x39020,
//! 0x39338; tools/re/gen_shot_vectors.py) on random states. After every operation the whole 30 x 80 byte pool, the live counter, the gate tick
//! and the exact sequence of calls to the mocked surroundings (ground-object test, effects, sounds, ship damage, blits) must be identical.
use mars_rising::draw::{DrawOpts, Prim};
use mars_rising::enemy::ShipView;
use mars_rising::enemy_shot::{ShotDrawWorld, ShotPool as EnemyShots, ShotWorld};
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
    scroll: i32,
    ships: [[u8; 68]; 2],
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

impl ShotWorld for Mock {
    fn tick(&self) -> u32 { self.tick }
    fn ship(&self, n: u8) -> ShipView { view(&self.ships[n as usize - 1]) }
    fn scroll_speed(&self) -> i32 { self.scroll }
    fn object_hit(&mut self, l: i32, t: i32, r: i32, b: i32) -> bool {
        self.log.push(format!("obj_{}_{}_{}_{}", l, t, r, b));
        l.wrapping_mul(7).wrapping_add(t.wrapping_mul(13)).wrapping_add(r.wrapping_mul(3)).wrapping_add(b).rem_euclid(19) == 0
    }
    fn ship_damage(&mut self, n: u8, amount: i32) { self.log.push(format!("dmg{}_{}", n, amount)); }
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32) {
        self.log.push(format!("fx_{}_{}_{}_{}_{}_{}_{}_{:08x}_{:08x}", x, y, id, a, b, c, d, vx.to_bits(), vy.to_bits()));
    }
    fn sound(&mut self, id: i32, prio: i32, delay: i32) { self.log.push(format!("snd_{}_{}_{}", id, prio, delay)); }
}

impl ShotDrawWorld for Mock {
    fn opts(&self) -> DrawOpts { self.opts }
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.log.push(format!("d{:x}_0_{}_{}_{}_{}_{}_{}", prim.addr(), x, y, w, h, anim, frame));
    }
}

fn bits(s: &str) -> f32 { f32::from_bits(u32::from_str_radix(s, 16).unwrap()) }

#[test]
fn enemy_shots_match_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/enemy_shot_vectors.txt")).expect("vectors");
    let mut nvec = 0usize;
    let mut nops: HashMap<char, usize> = HashMap::new();
    for (ln, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let (head, ops) = lhs.split_once(" ops=").unwrap();
        let kv: HashMap<&str, &str> = head.split_whitespace().map(|t| t.split_once('=').unwrap()).collect();
        let to68 = |s: &str| -> [u8; 68] { parse_sparse_words(s, 68).try_into().unwrap() };
        let mut mock = Mock {
            tick: kv["tick"].parse().unwrap(),
            scroll: 0,
            ships: [to68(kv["S1"]), to68(kv["S2"])],
            opts: DrawOpts::default(),
            log: vec![],
        };
        let mut pool = EnemyShots::new();
        let want: Vec<&str> = rhs.split(' ').collect();
        for (k, op) in ops.split(';').enumerate() {
            mock.log.clear();
            let (kind, arg) = op.split_at(1);
            let kc = kind.chars().next().unwrap();
            *nops.entry(kc).or_default() += 1;
            match kind {
                "I" => pool.init(&mock),
                "A" => {
                    let a: Vec<&str> = arg.split(',').collect();
                    let r = pool.spawn_aimed(&mock, a[0].parse().unwrap(), a[1].parse().unwrap(), a[2].parse().unwrap(), bits(a[3]), bits(a[4]));
                    let _ = r;
                }
                "P" => {
                    let a: Vec<&str> = arg.split(',').collect();
                    let p: Vec<i32> = a[..6].iter().map(|v| v.parse().unwrap()).collect();
                    let r = pool.spawn_param(&mut mock, [p[0], p[1], p[2], p[3], p[4], p[5]], bits(a[6]), bits(a[7]));
                    let _ = r;
                }
                "B" => {
                    let a: Vec<&str> = arg.split(',').collect();
                    pool.spawn_beam(&mut mock, a[0].parse().unwrap(), bits(a[1]), bits(a[2]), bits(a[3]), bits(a[4]), a[5].parse().unwrap(), a[6].parse().unwrap(), a[7].parse().unwrap());
                }
                "F" => {
                    mock.tick = arg.parse().unwrap();
                    pool.update(&mut mock);
                }
                "C" => {
                    mock.tick = arg.parse().unwrap();
                    pool.clear_all(&mut mock);
                }
                "D" => {
                    let (sub, rest) = arg.split_at(1);
                    let (tr, sh) = rest.split_once(',').unwrap();
                    mock.opts = DrawOpts { translucent: tr != "0", shadows: sh != "0" };
                    pool.draw(&mut mock, sub == "b");
                }
                "V" => mock.scroll = arg.parse().unwrap(),
                "U" => {
                    let (n, w) = arg.split_once(':').unwrap();
                    mock.ships[n.parse::<usize>().unwrap() - 1] = to68(w);
                }
                _ => panic!("op {}", op),
            }
            let got = format!("h{:016x}.{}.{}", fnv64(&pool.rec), pool.live, pool.gate) + &mock.log.iter().map(|l| format!(",{}", l)).collect::<String>();
            if got != want[k] {
                let (gd, wd) = (got.split(',').next().unwrap(), want[k].split(',').next().unwrap());
                let mut msg = format!("line {} op #{} `{}`\n  digest rust {} want {}", ln + 1, k, op, gd, wd);
                let (gl, wl): (Vec<&str>, Vec<&str>) = (got.split(',').skip(1).collect(), want[k].split(',').skip(1).collect());
                if gl != wl {
                    msg += &format!("\n  calls rust {:?}\n  calls want {:?}", gl, wl);
                }
                if std::env::var("SHOT_DUMP").is_ok() {
                    for s in 0..30 {
                        let r = &pool.rec[s * 80..(s + 1) * 80];
                        let nz: Vec<String> = (0..20).filter(|k| r[k * 4..k * 4 + 4] != [0; 4]).map(|k| format!("{}={:02x}{:02x}{:02x}{:02x}", k * 4, r[k * 4], r[k * 4 + 1], r[k * 4 + 2], r[k * 4 + 3])).collect();
                        if !nz.is_empty() { msg += &format!("\nrec {} {}", s, nz.join(" ")); }
                    }
                }
                panic!("{}", msg);
            }
        }
        nvec += 1;
    }
    eprintln!("verified vectors: {} ops {:?}", nvec, nops);
}
