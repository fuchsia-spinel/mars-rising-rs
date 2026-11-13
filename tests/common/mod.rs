//! Helpers shared by the golden tests: sparse big-endian word dumps and the byte <-> Ship conversions.
#![allow(dead_code)]
use mars_rising::pickup::*;
use mars_rising::ship::*;

pub const REC: usize = 128;

pub const SHIP_SIZE: usize = 408;
pub const SLOT: usize = 48;

pub fn parse_sparse_words(s: &str, len: usize) -> Vec<u8> {
    let mut b = vec![0u8; len];
    if s != "-" {
        for t in s.split(',') {
            let (i, h) = t.split_once('=').unwrap();
            let i: usize = i.parse().unwrap();
            b[i * 4..i * 4 + 4].copy_from_slice(&u32::from_str_radix(h, 16).unwrap().to_be_bytes());
        }
    }
    b
}

pub fn sparse_words(b: &[u8]) -> String {
    let mut v = Vec::new();
    for i in 0..b.len() / 4 {
        let w = u32::from_be_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
        if w != 0 { v.push(format!("{}={:08x}", i, w)); }
    }
    if v.is_empty() { "-".into() } else { v.join(",") }
}

pub fn u32at(b: &[u8], o: usize) -> u32 { u32::from_be_bytes(b[o..o + 4].try_into().unwrap()) }
pub fn i32at(b: &[u8], o: usize) -> i32 { u32at(b, o) as i32 }
pub fn f32at(b: &[u8], o: usize) -> f32 { f32::from_bits(u32at(b, o)) }
pub fn put_u32(b: &mut [u8], o: usize, v: u32) { b[o..o + 4].copy_from_slice(&v.to_be_bytes()); }
pub fn put_i32(b: &mut [u8], o: usize, v: i32) { put_u32(b, o, v as u32) }
pub fn put_f32(b: &mut [u8], o: usize, v: f32) { put_u32(b, o, v.to_bits()) }

pub fn ship_from_bytes(b: &[u8]) -> Ship {
    let mut s = Ship::new(1, true, 1);
    s.active = b[0] != 0;
    s.state = b[1];
    s.number = b[8];
    s.input_set = b[9];
    s.solo = b[10];
    s.x = f32at(b, 20); s.y = f32at(b, 24); s.vx = f32at(b, 28); s.vy = f32at(b, 32);
    s.w = i32at(b, 36); s.h = i32at(b, 40); s.hit_w = i32at(b, 44); s.hit_h = i32at(b, 48);
    s.vmax = f32at(b, 52);
    s.frame = i32at(b, 60);
    s.input = ShipInput { up: b[96] != 0, down: b[97] != 0, left: b[98] != 0, right: b[99] != 0, fire: b[100] != 0, alt: b[101] != 0 };
    s.latch = b[108] != 0;
    s.charge = i32at(b, 208);
    s.beam = Beam {
        active: b[212] != 0, x: i32at(b, 216), y: i32at(b, 220), vx: i32at(b, 224), vy: i32at(b, 228), w: i32at(b, 232),
        h: i32at(b, 236), anim: i32at(b, 240), pat: i32at(b, 244), power: i32at(b, 248), delay: i32at(b, 252),
    };
    s.charge_fx = b[256] != 0;
    s.fx_tick_a = u32at(b, 260); s.fx_a = i32at(b, 264);
    s.fx_b = b[268] != 0;
    s.fx_tick_b = u32at(b, 272); s.fx_c = i32at(b, 276);
    s.held = i32at(b, 280); s.heat = i32at(b, 284);
    s.heat_flash = b[288] != 0;
    s.heat_stage = i32at(b, 292);
    s.level = i32at(b, 296); s.burst = i32at(b, 300); s.drones = i32at(b, 304);
    s.trail_x = i32at(b, 312); s.trail_y = i32at(b, 316);
    s.weapon_kind = i32at(b, 324); s.spread = i32at(b, 328); s.missiles = i32at(b, 332);
    s.last_shot = u32at(b, 336);
    s.rear = b[341] != 0;
    s.pattern = i32at(b, 344);
    s.missile_tick = u32at(b, 348);
    s.charge_sound = u32at(b, 356);
    s.stream_ready = b[360] != 0;
    s.stream = u32at(b, 364);
    s.state_time = u32at(b, 4); s.lives = i32at(b, 12); s.sprite_id = i32at(b, 56);
    s.invuln = b[64] != 0; s.blink = b[65] != 0; s.blink_tick = u32at(b, 68);
    s.flag_72 = b[72] != 0; s.tick_76 = u32at(b, 76);
    s.special_max = i32at(b, 80); s.special_flag = b[84] != 0; s.special_tick = u32at(b, 88); s.special_left = i32at(b, 92);
    s.misc_104 = i32at(b, 104);
    s.shield = i32at(b, 112); s.shield_fx = b[116] != 0; s.shield_dir = b[117] != 0; s.shield_tick = u32at(b, 120);
    s.shield_frame = i32at(b, 124); s.warn = b[128] != 0; s.warn_tick = u32at(b, 132); s.warn_blink = b[136] != 0;
    s.warn_n = i32at(b, 140); s.hit_flash = b[144] != 0; s.hit_flash_tick = u32at(b, 148);
    s.hud_level = b[172]; s.drone_a = b[308] != 0; s.drone_b = b[309] != 0; s.coins = i32at(b, 320); s.keep = b[340] != 0;
    s.score = i32at(b, 152); s.next_life = i32at(b, 156); s.score_flag = b[160]; s.score_flash = b[161];
    s.score_tick = u32at(b, 164); s.lives_awarded = i32at(b, 168);
    s
}

/// Write the modelled fields of `s` over `b` (everything else is left as it was, so untouched words are checked too).
pub fn ship_to_bytes(s: &Ship, b: &mut [u8]) {
    b[0] = s.active as u8; b[1] = s.state; b[8] = s.number; b[9] = s.input_set; b[10] = s.solo;
    put_f32(b, 20, s.x); put_f32(b, 24, s.y); put_f32(b, 28, s.vx); put_f32(b, 32, s.vy);
    put_i32(b, 36, s.w); put_i32(b, 40, s.h); put_i32(b, 44, s.hit_w); put_i32(b, 48, s.hit_h);
    put_f32(b, 52, s.vmax);
    put_i32(b, 60, s.frame);
    b[96] = s.input.up as u8; b[97] = s.input.down as u8; b[98] = s.input.left as u8; b[99] = s.input.right as u8;
    b[100] = s.input.fire as u8; b[101] = s.input.alt as u8;
    b[108] = s.latch as u8;
    put_i32(b, 208, s.charge);
    let bm = &s.beam;
    b[212] = bm.active as u8;
    put_i32(b, 216, bm.x); put_i32(b, 220, bm.y); put_i32(b, 224, bm.vx); put_i32(b, 228, bm.vy);
    put_i32(b, 232, bm.w); put_i32(b, 236, bm.h); put_i32(b, 240, bm.anim); put_i32(b, 244, bm.pat);
    put_i32(b, 248, bm.power); put_i32(b, 252, bm.delay);
    b[256] = s.charge_fx as u8;
    put_u32(b, 260, s.fx_tick_a); put_i32(b, 264, s.fx_a);
    b[268] = s.fx_b as u8;
    put_u32(b, 272, s.fx_tick_b); put_i32(b, 276, s.fx_c);
    put_i32(b, 280, s.held); put_i32(b, 284, s.heat);
    b[288] = s.heat_flash as u8;
    put_i32(b, 292, s.heat_stage);
    put_i32(b, 296, s.level); put_i32(b, 300, s.burst); put_i32(b, 304, s.drones);
    put_i32(b, 312, s.trail_x); put_i32(b, 316, s.trail_y);
    put_i32(b, 324, s.weapon_kind); put_i32(b, 328, s.spread); put_i32(b, 332, s.missiles);
    put_u32(b, 336, s.last_shot);
    b[341] = s.rear as u8;
    put_i32(b, 344, s.pattern);
    put_u32(b, 348, s.missile_tick);
    put_u32(b, 356, s.charge_sound);
    b[360] = s.stream_ready as u8;
    put_u32(b, 364, s.stream);
    put_u32(b, 4, s.state_time); put_i32(b, 12, s.lives); put_i32(b, 56, s.sprite_id);
    b[64] = s.invuln as u8; b[65] = s.blink as u8; put_u32(b, 68, s.blink_tick);
    b[72] = s.flag_72 as u8; put_u32(b, 76, s.tick_76);
    put_i32(b, 80, s.special_max); b[84] = s.special_flag as u8; put_u32(b, 88, s.special_tick); put_i32(b, 92, s.special_left);
    put_i32(b, 104, s.misc_104);
    put_i32(b, 112, s.shield); b[116] = s.shield_fx as u8; b[117] = s.shield_dir as u8; put_u32(b, 120, s.shield_tick);
    put_i32(b, 124, s.shield_frame); b[128] = s.warn as u8; put_u32(b, 132, s.warn_tick); b[136] = s.warn_blink as u8;
    put_i32(b, 140, s.warn_n); b[144] = s.hit_flash as u8; put_u32(b, 148, s.hit_flash_tick);
    b[172] = s.hud_level; b[308] = s.drone_a as u8; b[309] = s.drone_b as u8; put_i32(b, 320, s.coins); b[340] = s.keep as u8;
    put_i32(b, 152, s.score); put_i32(b, 156, s.next_life); b[160] = s.score_flag; b[161] = s.score_flash;
    put_u32(b, 164, s.score_tick); put_i32(b, 168, s.lives_awarded);
}


pub fn shot_from_bytes(r: &[u8]) -> Shot {
    Shot {
        active: r[0] != 0, piercing: r[1], owner: r[2], x: f32at(r, 4), y: f32at(r, 8), vx: f32at(r, 12), vy: f32at(r, 16),
        kind: i32at(r, 20), w: i32at(r, 24), h: i32at(r, 28), sprite: i32at(r, 32), frame: i32at(r, 36), hp: i32at(r, 40),
        start_tick: u32at(r, 44),
    }
}

pub fn shot_to_bytes(s: &Shot) -> [u8; SLOT] {
    let mut r = [0u8; SLOT];
    r[0] = s.active as u8; r[1] = s.piercing; r[2] = s.owner;
    put_f32(&mut r, 4, s.x); put_f32(&mut r, 8, s.y); put_f32(&mut r, 12, s.vx); put_f32(&mut r, 16, s.vy);
    put_i32(&mut r, 20, s.kind); put_i32(&mut r, 24, s.w); put_i32(&mut r, 28, s.h); put_i32(&mut r, 32, s.sprite);
    put_i32(&mut r, 36, s.frame); put_i32(&mut r, 40, s.hp); put_u32(&mut r, 44, s.start_tick);
    r
}

pub fn parse_pool(s: &str) -> [u8; SHOT_SLOTS * SLOT] {
    let mut p = [0u8; SHOT_SLOTS * SLOT];
    if s != "-" {
        for t in s.split(';') {
            let (slot, words) = t.split_once(':').unwrap();
            let slot: usize = slot.parse().unwrap();
            for (i, w) in words.split(',').enumerate() {
                let o = slot * SLOT + i * 4;
                p[o..o + 4].copy_from_slice(&u32::from_str_radix(w, 16).unwrap().to_be_bytes());
            }
        }
    }
    p
}

pub fn pool_str(p: &[u8]) -> String {
    let mut v = Vec::new();
    for s in 0..SHOT_SLOTS {
        let rec = &p[s * SLOT..(s + 1) * SLOT];
        if rec.iter().any(|&b| b != 0) {
            let w: Vec<String> = (0..SLOT / 4).map(|i| format!("{:08x}", u32at(rec, i * 4))).collect();
            v.push(format!("{}:{}", s, w.join(",")));
        }
    }
    if v.is_empty() { "-".into() } else { v.join(";") }
}

pub fn pool_from_bytes(p: &[u8]) -> ShotPool {
    let mut pool = ShotPool::new();
    for i in 0..SHOT_SLOTS { pool.shots[i] = shot_from_bytes(&p[i * SLOT..(i + 1) * SLOT]); }
    pool
}

pub fn pool_to_bytes(pool: &ShotPool) -> [u8; SHOT_SLOTS * SLOT] {
    let mut p = [0u8; SHOT_SLOTS * SLOT];
    for i in 0..SHOT_SLOTS { p[i * SLOT..(i + 1) * SLOT].copy_from_slice(&shot_to_bytes(&pool.shots[i])); }
    p
}


pub fn pickup_from_bytes(r: &[u8]) -> Pickup {
    Pickup {
        active: r[0] != 0, spawn_tick: u32at(r, 4), kind: u32at(r, 8), f12: i32at(r, 12),
        x: f32at(r, 16), y: f32at(r, 20), vx: f32at(r, 24), vy: f32at(r, 28), ax: f32at(r, 32), ay: f32at(r, 36),
        w: i32at(r, 40), h: i32at(r, 44), half_w: i32at(r, 48), half_h: i32at(r, 52), variant: i32at(r, 56), scatter: r[60],
        spr_a: i32at(r, 64), spr_b: i32at(r, 68), spr_w: i32at(r, 72), spr_h: i32at(r, 76), off_x: i32at(r, 80), off_y: i32at(r, 84),
        blink: r[88] != 0, dir: r[89] != 0, sprite: i32at(r, 92), frame: i32at(r, 96), anim_tick: u32at(r, 100), anim_max: i32at(r, 104),
        action: i32at(r, 108), f112: i32at(r, 112), f116: i32at(r, 116), f120: i32at(r, 120), f124: i32at(r, 124),
    }
}

/// Overwrite the modelled bytes of `r` (padding bytes and any unmodelled garbage stay as they were).
pub fn pickup_to_bytes(p: &Pickup, r: &mut [u8]) {
    r[0] = p.active as u8; put_u32(r, 4, p.spawn_tick); put_u32(r, 8, p.kind); put_i32(r, 12, p.f12);
    put_f32(r, 16, p.x); put_f32(r, 20, p.y); put_f32(r, 24, p.vx); put_f32(r, 28, p.vy); put_f32(r, 32, p.ax); put_f32(r, 36, p.ay);
    put_i32(r, 40, p.w); put_i32(r, 44, p.h); put_i32(r, 48, p.half_w); put_i32(r, 52, p.half_h); put_i32(r, 56, p.variant);
    r[60] = p.scatter;
    put_i32(r, 64, p.spr_a); put_i32(r, 68, p.spr_b); put_i32(r, 72, p.spr_w); put_i32(r, 76, p.spr_h);
    put_i32(r, 80, p.off_x); put_i32(r, 84, p.off_y);
    r[88] = p.blink as u8; r[89] = p.dir as u8;
    put_i32(r, 92, p.sprite); put_i32(r, 96, p.frame); put_u32(r, 100, p.anim_tick); put_i32(r, 104, p.anim_max);
    put_i32(r, 108, p.action); put_i32(r, 112, p.f112); put_i32(r, 116, p.f116); put_i32(r, 120, p.f120); put_i32(r, 124, p.f124);
}

pub fn parse_kpool(s: &str) -> Vec<u8> {
    let mut p = vec![0u8; PICKUP_SLOTS * REC];
    if s != "-" {
        for t in s.split(';') {
            let (slot, words) = t.split_once(':').unwrap();
            let slot: usize = slot.parse().unwrap();
            for (i, w) in words.split(',').enumerate() {
                let o = slot * REC + i * 4;
                p[o..o + 4].copy_from_slice(&u32::from_str_radix(w, 16).unwrap().to_be_bytes());
            }
        }
    }
    p
}

pub fn kpool_str(p: &[u8]) -> String {
    let mut v = Vec::new();
    for s in 0..PICKUP_SLOTS {
        let rec = &p[s * REC..(s + 1) * REC];
        if rec.iter().any(|&b| b != 0) {
            let w: Vec<String> = (0..REC / 4).map(|i| format!("{:08x}", u32at(rec, i * 4))).collect();
            v.push(format!("{}:{}", s, w.join(",")));
        }
    }
    if v.is_empty() { "-".into() } else { v.join(";") }
}

