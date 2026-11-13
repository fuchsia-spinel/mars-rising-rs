//! Differential test for src/ship.rs: every vector in tests/vectors/ship_vectors.txt was produced by *executing the original
//! PowerPC routine* (tools/re/gen_ship_vectors.py + ppcemu.py) on the same random state. The Rust code must reproduce the
//! resulting player struct (all 408 bytes), shot pool (all 60 records), shot counter, return value and the log of calls to
//! the mocked callee routines (enemy hit tests, effects, sounds ...) exactly.
use mars_rising::ship::*;
use std::collections::HashMap;

mod common;
use common::*;

/// Mock of everything outside the ship code; mirrors the Python mocks in gen_ship_vectors.py.
struct Mock {
    start_level: i32,
    level_bonus: i32,
    hits: Vec<i32>,
    msl: Vec<u32>,
    playing: Vec<u32>,
    log: Vec<String>,
}

impl Mock {
    fn pop<T: Default>(v: &mut Vec<T>) -> T { if v.is_empty() { T::default() } else { v.remove(0) } }
}

impl World for Mock {
    fn hit_enemies(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, hp: i32, owner: u8) -> i32 {
        self.log.push(format!("hit_{}_{}_{}_{}_{}_{}", x0, y0, x1, y1, hp, owner));
        Mock::pop(&mut self.hits)
    }
    fn effect(&mut self, x: i32, y: i32, kind: i32, delay: i32, param: i32, a: i32, b: i32, _vx: f32, _vy: f32) {
        self.log.push(format!("fx_{}_{}_{}_{}_{}_{}_{}", x, y, kind, delay, param, a, b));
    }
    fn effect_aux(&mut self, id: u32, x: i32, y: i32, kind: i32, delay: i32, param: i32) {
        self.log.push(format!("aux_{:x}_{}_{}_{}_{}_{}", id, x, y, kind, delay, param));
    }
    fn drop_pickup(&mut self, kind: i32, x: f32, y: f32) { self.log.push(format!("drop_{}_{}_{}", kind, x as i32, y as i32)); }
    fn start_level(&mut self) -> i32 { self.start_level }
    fn level_bonus(&mut self) -> i32 { self.level_bonus }
    fn hud_lives(&mut self, set: u8, lives: i32, _flash: i32) { self.log.push(format!("hud_1f848_{}_{}", set, lives)); }
    fn hud(&mut self, id: u32, a: i32, b: i32) {
        let argc = match id { 0x1fe28 | 0x1ff94 | 0x1f69c | 0x1f848 | 0x1f51c => 2, 0x1f5c4 | 0x28510 | 0x4560c => 1, _ => 0 };
        let mut t = format!("hud_{:x}", id);
        if argc >= 1 { t += &format!("_{}", a); }
        if argc >= 2 { t += &format!("_{}", b); }
        self.log.push(t);
    }
    fn drones_reset(&mut self, n: u8) { self.log.push(format!("dres_{}", n)); }
    fn demo_check(&mut self) -> bool { self.log.push("demo".into()); Mock::pop(&mut self.playing) != 0 }
    fn sound(&mut self, id: i32, priority: i32, arg: i32) { self.log.push(format!("snd_{}_{}_{}", id, priority, arg)); }
    fn launch_missiles(&mut self, _ship: &Ship) -> bool { self.log.push("msl".into()); Mock::pop(&mut self.msl) != 0 }
    fn drones_fire(&mut self, _ship: &Ship, _shots: &mut ShotPool) { self.log.push("drn".into()); }
    fn ship_overheated(&mut self, _ship: &mut Ship) { self.log.push("ovh".into()); }
    fn charge_loop_start(&mut self) -> u32 { self.log.push("chg".into()); 0x77 }
    fn sound_playing(&mut self, h: u32) -> bool { self.log.push(format!("playing_{}", h)); Mock::pop(&mut self.playing) != 0 }
    fn sound_stop(&mut self, h: u32) { self.log.push(format!("stop_{}", h)); }
    fn overcharge_stream_start(&mut self, n: u8) -> u32 { self.log.push(format!("stream_{}", n)); 0x55 }
}

fn parse_q(s: &str) -> (Vec<i32>, Vec<u32>, Vec<u32>) {
    let mut h = vec![]; let mut m = vec![]; let mut p = vec![];
    for part in s.split('/') {
        let (k, v) = part.split_once('=').unwrap();
        if v == "-" { continue; }
        for x in v.split(',') {
            match k { "h" => h.push(x.parse().unwrap()), "m" => m.push(x.parse().unwrap()), _ => p.push(x.parse().unwrap()) }
        }
    }
    (h, m, p)
}

#[test]
fn ship_matches_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/ship_vectors.txt")).expect("vectors");
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
        let count: i32 = i["count"].parse().unwrap();
        let mut sb = parse_sparse_words(&i["S"], SHIP_SIZE);
        let pb = parse_pool(&i["P"]);
        let (h, m, p) = parse_q(&i["Q"]);
        let mut ship = ship_from_bytes(&sb);
        let mut pool = pool_from_bytes(&pb);
        pool.count = count;
        let ex = |k: &str| -> i32 { i.get(k).map(|v| v.parse().unwrap()).unwrap_or(0) };
        let mut mock = Mock { start_level: i.get("sl").map(|v| v.parse().unwrap()).unwrap_or(1), level_bonus: ex("lb"), hits: h, msl: m, playing: p, log: vec![] };
        let mut tail: Option<(u32, u32)> = None;
        let mut ret: Option<u32> = None;
        match kind {
            "mv" => ship.step_movement(),
            "front" => ret = Some(pool.spawn_front(&mut ship) as u32),
            "beam" => ret = Some(pool.spawn_beam(&mut ship, tick) as u32),
            "rear" => ret = Some(pool.spawn_rear(&mut ship) as u32),
            "upd" => pool.update(tick, &mut mock),
            "fire" => ship.fire_step(tick, &mut pool, &mut mock),
            "bupd" => ship.update_beam(&mut mock),
            "dmg" => ship.damage(i["A"].parse::<i32>().unwrap(), tick, &mut mock),
            "dstr" => ship.destroy(ex("A") != 0, tick, &mut mock),
            "rst" => ship.reset_weapons(ex("A") != 0, tick, &mut mock),
            "init" => ship.init(ex("A") != 0, tick, &mut mock),
            "sess" => {
                let x: Vec<i32> = i["X"].split(',').map(|v| v.parse().unwrap()).collect();
                let ctx = SessionCtx { solo: x[0] != 0, other_in_game: x[1] != 0, demo: x[2] != 0, mode: x[3] as u8 };
                let out = ship.session_step(tick, &ctx, x[4] != 0, &mut mock);
                tail = Some((out.game_over as u32, out.in_game as u32));
            }
            k => panic!("unknown kind {}", k),
        }
        ship_to_bytes(&ship, &mut sb);
        let ctx = format!("line {} kind {}", ln + 1, kind);
        assert_eq!(sparse_words(&sb), o["S2"], "ship struct, {}", ctx);
        assert_eq!(pool_str(&pool_to_bytes(&pool)), o["P2"], "shot pool, {}", ctx);
        assert_eq!(pool.count.to_string(), o["count2"], "shot counter, {}", ctx);
        let elog = if mock.log.is_empty() { "-".to_string() } else { mock.log.join(";") };
        assert_eq!(elog, o["E"], "call log, {}", ctx);
        if let Some((go, ig)) = tail {
            assert_eq!(go.to_string(), o["go"], "game over flag, {}", ctx);
            assert_eq!(ig.to_string(), o["ig"], "in-game flag, {}", ctx);
        }
        if let Some(r) = ret {
            assert_eq!(r.to_string(), o["ret"], "return value, {}", ctx);
        }
        *counts.entry(kind.to_string()).or_default() += 1;
    }
    eprintln!("verified vectors: {:?}", counts);
    assert!(counts.values().all(|&n| n > 0) && counts.len() == 12);
}

#[test]
fn hit_box_walk_matches_2ead4_rules() {
    // two overlapping enemies, attacker hp 3: highest slot is hit first, loop ends when hp is used up
    let mut en = vec![EnemyBox::default(); 40];
    for s in [3usize, 10] {
        en[s] = EnemyBox { active: true, state: 0, left: 0.0, top: 0.0, right: 100.0, bottom: 100.0 };
    }
    let mut order = vec![];
    let total = hit_enemy_boxes(&en, 10, 10, 20, 20, 3, 1, |slot, hp, _o| { order.push(slot); hp.min(2) });
    assert_eq!(order, vec![10, 3]);
    assert_eq!(total, 3);
}
