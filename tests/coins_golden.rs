//! Differential test for `PickupPool::convert_coins` (0x2b73c, the coins left on the screen turn into explosions when the coin bonus ends): every vector in
//! tests/vectors/coins_vectors.txt was produced by *executing the original PowerPC routine* (tools/re/gen_coins_vectors.py). The pool afterwards (only the
//! "active" byte of a record can change), the counter and every effect request (including the float arguments, bit for bit) must be identical.
use mars_rising::pickup::*;

fn fnv64(b: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &x in b {
        h = (h ^ x as u64).wrapping_mul(0x100000001b3);
    }
    h
}

fn u32at(r: &[u8], o: usize) -> u32 { u32::from_be_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]) }

#[test]
fn convert_coins_matches_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/coins_vectors.txt")).expect("vectors");
    let (mut n_vec, mut n_fx) = (0, 0);
    for (ln, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() { continue; }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let kv: Vec<&str> = lhs.split_whitespace().collect();
        let hex = kv[1].strip_prefix("pool=").unwrap();
        let count: i32 = kv[2].strip_prefix("count=").unwrap().parse().unwrap();
        let mut rec: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()).collect();
        let mut pool = PickupPool::default();
        pool.count = count;
        for i in 0..20 {
            let r = &rec[i * 128..(i + 1) * 128];
            let s = &mut pool.slots[i];
            s.active = r[0] != 0;
            s.kind = u32at(r, 8);
            s.x = f32::from_bits(u32at(r, 16));
            s.y = f32::from_bits(u32at(r, 20));
            s.vx = f32::from_bits(u32at(r, 24));
            s.vy = f32::from_bits(u32at(r, 28));
            s.w = u32at(r, 40) as i32;
            s.h = u32at(r, 44) as i32;
            s.scatter = r[60];
        }
        let mut log = Vec::new();
        pool.convert_coins(|x, y, k, d, snd, sc, lp, vx, vy| log.push(format!("fx_{}_{}_{}_{}_{}_{}_{}_{:08x}_{:08x}", x, y, k, d, snd, sc, lp, (vx as f64).to_bits(), (vy as f64).to_bits())));
        for i in 0..20 {
            if !pool.slots[i].active { rec[i * 128] = 0; }
        }
        let mut got = vec![format!("h{:016x}.{}", fnv64(&rec), pool.count)];
        got.extend(log.iter().cloned());
        let want: Vec<&str> = rhs.split(' ').collect();
        assert_eq!(got.join(" "), want.join(" "), "line {}", ln + 1);
        n_fx += log.len();
        n_vec += 1;
    }
    eprintln!("verified {} vectors, {} effects", n_vec, n_fx);
}
