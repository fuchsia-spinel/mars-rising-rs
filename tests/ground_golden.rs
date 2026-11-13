//! Differential test for src/ground.rs (turrets, missile towers and the other ground objects): every vector in tests/vectors/ground_vectors.txt was
//! produced by *executing the original PowerPC routines* (level load 0x3ade0, activation 0x3b450, update 0x3d0fc, move/draw 0x3f554, damage 0x40040, ...;
//! tools/re/gen_ground_vectors.py) on a random synthetic level. After every operation the whole object pool (250 x 168 bytes), the active list, the decal
//! list, the three counters and the exact sequence of calls made to the mocked surroundings must be identical.
use mars_rising::enemy::ShipView;
use mars_rising::ground::*;
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
    scroll: i32,
    c08c: i32,
    shadows: bool,
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

impl GroundWorld for Mock {
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
    fn scroll_step(&self) -> i32 { self.scroll }
    fn buffer_offset(&self) -> i32 { self.c08c }
    fn shadows(&self) -> bool { self.shadows }
    fn score(&mut self, owner: u8, points: i32) { self.log.push(format!("score{}_{}_0", owner, points)); }
    fn effect(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32, d: i32, vx: f32, vy: f32) {
        self.log.push(format!("fx_{}_{}_{}_{}_{}_{}_{}_{:08x}_{:08x}", x, y, id, a, b, c, d, vx.to_bits(), vy.to_bits()));
    }
    fn aux(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32) { self.log.push(format!("aux_{}_{}_{}_{}_{}_{}", x, y, id, a, b, c)); }
    fn particles(&mut self, x: i32, y: i32, id: i32, a: i32, b: i32, c: i32) { self.log.push(format!("part_{}_{}_{}_{}_{}_{}", x, y, id, a, b, c)); }
    fn sound(&mut self, id: i32, prio: i32, delay: i32) { self.log.push(format!("snd_{}_{}_{}", id, prio, delay)); }
    fn pickup(&mut self, kind: i32, variant: i32, param: i32, announce: i32, x: f32, y: f32) {
        self.log.push(format!("pick_{}_{}_{}_{}_{:08x}_{:08x}", kind, variant, param, announce, x.to_bits(), y.to_bits()));
    }
    fn beam(&mut self, kind: i32, snd: i32, target: i32, flag: i32, x: f32, y: f32, mx: f32, my: f32) {
        self.log.push(format!("beam_{}_{}_{}_{}_{:08x}_{:08x}_{:08x}_{:08x}", kind, snd, target, flag, x.to_bits(), y.to_bits(), mx.to_bits(), my.to_bits()));
    }
    fn shot_aimed(&mut self, kind: i32, target: i32, flag: i32, x: f32, y: f32) -> i32 {
        self.log.push(format!("shot383b8_{}_{}_{}_{:08x}_{:08x}", kind, target, flag, x.to_bits(), y.to_bits()));
        if self.shots.is_empty() { 0 } else { self.shots.remove(0) }
    }
    fn clear_bonus(&mut self, x: f32, y: f32) { self.log.push(format!("clear_{:08x}_{:08x}", x.to_bits(), y.to_bits())); }
    fn prim(&mut self, which: u32, a: [i32; 7]) {
        self.log.push(format!("d{:x}_{}", which, a.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("_")));
    }
}

fn digest(g: &Ground) -> String {
    let list: Vec<u8> = g.list.iter().flat_map(|v| v.to_be_bytes()).collect();
    format!("h{:016x}.{:016x}.{:016x}.{}.{}.{}", fnv64(&g.pool), fnv64(&list), fnv64(&g.decals), g.targets, g.destroyed, g.activated)
}

fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{:02x}", x)).collect() }

fn dump_diff(a: &[u8], b: &[u8], size: usize) -> String {
    let mut s = String::new();
    for k in 0..a.len() / 4 {
        if a[k * 4..k * 4 + 4] != b[k * 4..k * 4 + 4] {
            s += &format!("\n    rec {} +{}: rust {} want {}", (k * 4) / size, (k * 4) % size, hex(&a[k * 4..k * 4 + 4]), hex(&b[k * 4..k * 4 + 4]));
        }
    }
    s
}

#[test]
fn ground_objects_match_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/ground_vectors.txt")).expect("vectors");
    let mut ops_seen: HashMap<char, usize> = HashMap::new();
    let mut calls_seen: HashMap<String, usize> = HashMap::new();
    let mut n_vec = 0;
    for (ln, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() { continue; }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let (head, ops) = lhs.split_once(" ops=").unwrap();
        let kv: HashMap<&str, &str> = head.split_whitespace().map(|t| t.split_once('=').unwrap()).collect();
        let to68 = |s: &str| -> [u8; 68] { parse_sparse_words(s, 68).try_into().unwrap() };
        let (width, height): (usize, usize) = (kv["W"].parse().unwrap(), kv["H"].parse().unwrap());
        let marker: Vec<u8> = (0..width * height).map(|i| u8::from_str_radix(&kv["M"][i * 2..i * 2 + 2], 16).unwrap()).collect();
        let res = parse_sparse_words(kv["G"], 250 * RES_REC);
        let mut mock = Mock {
            tick: kv["tick"].parse().unwrap(),
            np: kv["np"].parse().unwrap(),
            ships: [to68(kv["S1"]), to68(kv["S2"])],
            dif: kv["dif"].parse().unwrap(),
            scroll: 2,
            c08c: 0,
            shadows: true,
            rq: kv["R"].split(',').map(|v| u32::from_str_radix(v, 16).unwrap()).collect(),
            shots: if kv["Q"].is_empty() { vec![] } else { kv["Q"].split(',').map(|v| v.parse().unwrap()).collect() },
            log: vec![],
        };
        let mut g = Ground::new();
        let mut row_counter = 0i32;
        let want: Vec<&str> = rhs.split(' ').collect();
        assert_eq!(want.len(), ops.split(';').count(), "line {}", ln + 1);
        for (k, op) in ops.split(';').enumerate() {
            mock.log.clear();
            let (kind, arg) = op.split_at(1);
            let a: Vec<&str> = if arg.is_empty() { vec![] } else { arg.split(',').collect() };
            let n = |i: usize| -> i32 { a[i].parse().unwrap() };
            match kind {
                "L" => g.load(&res, &mock),
                "N" => g.count_targets(width, height, &marker),
                "A" => {
                    row_counter = arg.parse().unwrap();
                    g.activate_row(row_counter, width, height, &marker, &mock)
                }
                "F" => {
                    mock.tick = n(0) as u32;
                    mock.scroll = n(1);
                    g.update(&mut mock)
                }
                "G" | "Q" => {
                    mock.tick = n(0) as u32;
                    mock.scroll = n(1);
                    mock.c08c = n(2);
                    mock.shadows = n(3) != 0;
                    if kind == "G" { g.move_draw(&mut mock) } else { g.draw_baked(&mut mock) }
                }
                "D" => {
                    mock.scroll = arg.parse().unwrap();
                    g.scroll_decals(&mock)
                }
                "E" => {
                    mock.tick = n(0) as u32;
                    mock.scroll = n(1);
                    mock.c08c = n(2);
                    mock.shadows = n(3) != 0;
                    g.draw_decals(&mut mock)
                }
                "X" => g.clear_decals(),
                "H" => g.damage(n(0) as usize, n(1), n(2) as u8, &mut mock),
                "T" => {
                    let r = g.hit_box(n(0), n(1), n(2), n(3), n(4), n(5) as u8, &mut mock);
                    mock.log.push(format!("ret{}", r));
                }
                "B" => g.bomb_all(arg.parse().unwrap(), &mut mock),
                "K" => {
                    let r = g.blocked(n(0), n(1), n(2), n(3));
                    mock.log.push(format!("ret{}", r as u8));
                }
                "Y" => g.add_decal(&mut mock, [n(0), n(1), n(2), n(3), n(4), n(5), n(6), n(7)], n(8), n(9)),
                "W" => g.w32(n(0) as usize, n(1) as usize, u32::from_str_radix(a[2], 16).unwrap()),
                "V" => g.w8(n(0) as usize, n(1) as usize, n(2) as u8),
                "U" => {
                    let (s, w) = arg.split_once(':').unwrap();
                    mock.ships[s.parse::<usize>().unwrap() - 1] = to68(w);
                }
                "C" => mock.scroll = arg.parse().unwrap(),
                _ => panic!("op {}", op),
            }
            let _ = row_counter;
            let got = digest(&g) + &mock.log.iter().map(|l| format!(",{}", l)).collect::<String>();
            *ops_seen.entry(kind.chars().next().unwrap()).or_default() += 1;
            for l in &mock.log {
                let key: String = l.split('_').next().unwrap().to_string();
                *calls_seen.entry(key).or_default() += 1;
            }
            if got != want[k] {
                let (gd, wd) = (got.split(',').next().unwrap(), want[k].split(',').next().unwrap());
                let mut msg = format!("line {} op #{} `{}`\n  digest rust {} want {}", ln + 1, k, op, gd, wd);
                let (gl, wl): (Vec<&str>, Vec<&str>) = (got.split(',').skip(1).collect(), want[k].split(',').skip(1).collect());
                if gl != wl {
                    msg += &format!("\n  calls rust {:?}\n  calls want {:?}", gl, wl);
                }
                if let Ok(prefix) = std::env::var("GROUND_PREFIX") {
                    // the original's pool after this op, written by tools/re/ground_dump.py
                    if let Ok(want_pool) = std::fs::read(format!("{}.pool", prefix)) {
                        msg += &dump_diff(&g.pool, &want_pool, REC);
                    }
                }
                panic!("{}", msg);
            }
        }
        n_vec += 1;
    }
    let mut ops: Vec<_> = ops_seen.into_iter().collect();
    ops.sort();
    let mut calls: Vec<_> = calls_seen.into_iter().collect();
    calls.sort();
    eprintln!("verified {} vectors; ops {:?}; calls {:?}", n_vec, ops, calls);
}
