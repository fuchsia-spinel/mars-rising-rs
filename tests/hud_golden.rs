//! Differential test for src/hud.rs (the side-panel HUD: init 0x1e950, score 0x2633c, power bar 0x1ef5c, shield 0x1f51c / 0x1f5c4 / 0x1f69c, lives 0x1f848,
//! weapon icons 0x1fe28 / 0x1ff94, hit flash 0x1fc50): every vector in tests/vectors/hud_vectors.txt was produced by *executing the original PowerPC routines*
//! (tools/re/gen_hud_vectors.py). After every operation both ship structures, the HUD variables D.154c..D.157c, the rectangle table and the exact sequence of
//! operations requested from the surroundings (surface copies, sprite blits, port switches, QuickDraw calls) must be identical.
use mars_rising::hud::*;
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
    org: [i16; 4],
    log: Vec<String>,
}

fn r(x: Rect) -> String { format!("{}_{}_{}_{}", x.top, x.left, x.bottom, x.right) }

impl HudWorld for Mock {
    fn tick(&self) -> u32 { self.tick }
    fn origin(&self) -> [i16; 4] { self.org }
    fn call(&mut self, c: HudCall) {
        self.log.push(match c {
            HudCall::Restore(a, b) => format!("rs_{}:{}", r(a), r(b)),
            HudCall::Present(a, b) => format!("pr_{}:{}", r(a), r(b)),
            HudCall::Store(a, b) => format!("st_{}:{}", r(a), r(b)),
            HudCall::Blit { addr, x, y, w, h, anim, frame } => format!("d{:x}_0_{}_{}_{}_{}_{}_{}", addr, x, y, w, h, anim, frame),
            HudCall::Port(Port::Panel) => "port_hud".into(),
            HudCall::Port(Port::Screen) => "port_main".into(),
            HudCall::Port(Port::Store) => "port_store".into(),
            HudCall::OpColor(c) => format!("oc_{}_{}_{}", c[0], c[1], c[2]),
            HudCall::PenMode(m) => format!("pm_{}", m),
            HudCall::ForeColor(c) => format!("fc_{}", c),
            HudCall::Paint(x) => format!("pt_{}", r(x)),
        });
    }
}

#[test]
fn hud_matches_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/hud_vectors.txt")).expect("vectors");
    let mut calls_seen: HashMap<String, usize> = HashMap::new();
    let mut n_vec = 0;
    for (ln, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() { continue; }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let (head, ops) = lhs.split_once(" ops=").unwrap();
        let kv: HashMap<&str, &str> = head.split_whitespace().map(|t| t.split_once('=').unwrap()).collect();
        let mut ships = [parse_sparse_words(kv["S1"], SHIP_SIZE), parse_sparse_words(kv["S2"], SHIP_SIZE)];
        let o: Vec<i16> = kv["O"].split(',').map(|v| v.parse().unwrap()).collect();
        let mut mock = Mock { tick: kv["T"].parse().unwrap(), org: [o[0], o[1], o[2], o[3]], log: vec![] };
        let mut hud = Hud::new();
        let want: Vec<&str> = rhs.split(' ').collect();
        assert_eq!(want.len(), ops.split(';').count(), "line {}", ln + 1);
        for (k, op) in ops.split(';').enumerate() {
            mock.log.clear();
            let (kind, arg) = op.split_at(1);
            let a: Vec<&str> = if arg.is_empty() { vec![] } else { arg.split(',').collect() };
            let n = |i: usize| -> i64 { a[i].parse().unwrap() };
            let p = |i: usize| -> u32 { n(i) as u32 };
            match kind {
                "I" => hud.init(&mut mock),
                "S" => {
                    let idx = n(0) as usize - 1;
                    let mut s = ship_from_bytes(&ships[idx]);
                    hud.score(&mut s, p(1), &mut mock);
                    ship_to_bytes(&s, &mut ships[idx]);
                }
                "B" => hud.power(p(0), n(1) as i32, p(2), &mut mock),
                "E" => hud.shield_target(p(0), n(1) as i32, &mock),
                "K" => hud.shield_tick(p(0), &mut mock),
                "H" => hud.shield(p(0), n(1) as i32, &mut mock),
                "L" => hud.lives(p(0), n(1) as i32, p(2), &mut mock),
                "W" => hud.weapon(p(0), p(1), &mut mock),
                "V" => hud.weapon_level(p(0), p(1), &mut mock),
                "F" => hud.flash(p(0), &mut mock),
                "T" => mock.tick = arg.parse().unwrap(),
                "X" => {
                    let idx = n(0) as usize - 1;
                    let off = n(1) as usize;
                    let v = u32::from_str_radix(a[2], 16).unwrap();
                    if n(3) == 1 { ships[idx][off] = v as u8 } else { ships[idx][off..off + 4].copy_from_slice(&v.to_be_bytes()) }
                }
                "Y" => hud.set_var(u32::from_str_radix(a[0], 16).unwrap(), u32::from_str_radix(a[1], 16).unwrap()),
                _ => panic!("op {}", op),
            }
            let mut rb = hud.rect_bytes().to_vec();
            rb.truncate(194);
            let digest = format!("h{:016x}.{:016x}.{:016x}.{:016x}", fnv64(&ships[0]), fnv64(&ships[1]), fnv64(&hud.vars_bytes()), fnv64(&rb));
            let got = digest + &mock.log.iter().map(|l| format!(",{}", l)).collect::<String>();
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
