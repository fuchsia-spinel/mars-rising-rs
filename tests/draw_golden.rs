//! Differential test for the draw steps (src/ship_draw.rs, PickupPool::draw): tests/vectors/draw_vectors.txt was produced by running the
//! original routines 0x257b4 (ship), 0x275c8 (beam), 0x2242c (shots) and 0x2c480 (pickups) in the emulator with the blit primitives mocked.
//! The Rust code must emit exactly the same draw calls (primitive, position, size, animation, frame) in the same order and leave the ship,
//! shot pool, pickup pool and counters exactly as the original does.
use mars_rising::draw::*;
use mars_rising::pickup::*;
use mars_rising::ship::*;
use std::collections::HashMap;

mod common;
use common::*;

struct Mock {
    ground: Vec<i32>,
    log: Vec<String>,
}

impl World for Mock {
    fn hit_enemies(&mut self, _x0: i32, _y0: i32, _x1: i32, _y1: i32, _hp: i32, _owner: u8) -> i32 { unreachable!() }
    fn effect(&mut self, _x: i32, _y: i32, _k: i32, _d: i32, _p: i32, _a: i32, _b: i32, _vx: f32, _vy: f32) { unreachable!() }
    fn sound(&mut self, id: i32, priority: i32, arg: i32) { self.log.push(format!("snd_{}_{}_{}", id, priority, arg)); }
    fn sound_pan(&mut self, id: i32, priority: i32, pan: i32) { self.log.push(format!("snp_{}_{}_{}", id, priority, pan)); }
    fn draw(&mut self, d: Draw) { self.log.push(format!("d{:x}_0_{}_{}_{}_{}_{}_{}", d.prim.addr(), d.x, d.y, d.w, d.h, d.anim, d.frame)); }
    fn hud_lives(&mut self, set: u8, lives: i32, flash: i32) { self.log.push(format!("hud_1f848_{}_{}_{}", set, lives, flash)); }
    fn ground_target(&mut self, l: i32, t: i32, r: i32, b: i32, number: u8) -> bool {
        self.log.push(format!("gt_{}_{}_{}_{}_0_{}", l, t, r, b, number));
        (if self.ground.is_empty() { -1 } else { self.ground.remove(0) }) != -1
    }
}

#[test]
fn draw_steps_match_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/draw_vectors.txt")).expect("vectors");
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
        let mut sb = parse_sparse_words(&i["S"], SHIP_SIZE);
        let pb = parse_pool(&i["P"]);
        let mut kb = parse_kpool(&i["K"]);
        let mut ship = ship_from_bytes(&sb);
        let mut shots = pool_from_bytes(&pb);
        shots.count = i["count"].parse().unwrap();
        let mut pickups = PickupPool::new();
        for s in 0..PICKUP_SLOTS { pickups.slots[s] = pickup_from_bytes(&kb[s * REC..(s + 1) * REC]); }
        pickups.count = i["pcount"].parse().unwrap();
        let d: Vec<i32> = i["D"].split(',').map(|v| v.parse().unwrap()).collect();
        let opts = DrawOpts { translucent: d[0] != 0, shadows: d[1] != 0 };
        let g = i["Q"].strip_prefix("g:").unwrap();
        let ground: Vec<i32> = if g == "-" { vec![] } else { g.split(',').map(|v| v.parse().unwrap()).collect() };
        let mut mock = Mock { ground, log: vec![] };
        match kind {
            "sdraw" => ship.draw_step(tick, opts, &mut mock),
            "bdraw" => ship.draw_beam(&mut mock),
            "shdraw" => shots.draw(&mut mock),
            "pdraw" => pickups.draw(opts, &mut mock),
            k => panic!("unknown kind {}", k),
        }
        let expected = &o["E"];
        let elog = if mock.log.is_empty() { "-".to_string() } else { mock.log.join(";") };

        ship_to_bytes(&ship, &mut sb);
        let mut pb2 = pool_to_bytes(&shots);
        let _ = &mut pb2;
        for s in 0..PICKUP_SLOTS { pickup_to_bytes(&pickups.slots[s], &mut kb[s * REC..(s + 1) * REC]); }
        let ctx = format!("line {} kind {}", ln + 1, kind);
        assert_eq!(elog, *expected, "draw calls / hooks, {}", ctx);
        assert_eq!(sparse_words(&sb), o["S2"], "ship struct, {}", ctx);
        assert_eq!(pool_str(&pb2), o["P2"], "shot pool, {}", ctx);
        assert_eq!(kpool_str(&kb), o["K2"], "pickup pool, {}", ctx);
        assert_eq!(shots.count.to_string(), o["count2"], "shot counter, {}", ctx);
        assert_eq!(pickups.count.to_string(), o["pcount2"], "pickup counter, {}", ctx);
        *counts.entry(kind.to_string()).or_default() += 1;
    }
    eprintln!("verified vectors: {:?}", counts);
    assert!(counts.values().all(|&n| n > 0) && counts.len() == 4);
}
