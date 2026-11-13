//! Differential test for the sprite blitters (src/blit.rs): tests/vectors/blit_vectors.txt was produced by running the original routines
//! 0x3f08 (anim), 0x4078 (trans), 0x486c (dither), 0x46a8 (shadow) and 0x4a90 (flash) in the emulator on compiled sprite streams.
//! The Rust blitter must produce exactly the same destination bytes (see tools/re/gen_blit_vectors.py for the line format and tables).
use mars_rising::blit::*;
use mars_rising::draw::Prim;
use std::collections::HashMap;

const CW: usize = 40;
const CH: usize = 24;

fn lcg_canvas(seed: u32) -> Vec<u8> {
    let mut s = seed;
    (0..CW * CH).map(|_| { s = s.wrapping_mul(1664525).wrapping_add(1013904223); (s >> 24) as u8 }).collect()
}

fn unhex(h: &str) -> Vec<u8> { (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect() }

#[test]
fn blitters_match_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/blit_vectors.txt")).expect("vectors");
    let flash: Vec<u8> = (0..256usize).map(|i| (5 * i + 17) as u8).collect();
    let shadow: Vec<u8> = (0..256usize).map(|i| (11 * i + 3) as u8).collect();
    let blend: Vec<u8> = (0..65536usize).map(|i| (13 * (i >> 8) + 7 * (i & 255)) as u8).collect();
    let luts = Luts { flash: &flash, shadow: &shadow, blend: &blend };
    let mut counts: HashMap<String, usize> = HashMap::new();
    for (ln, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() { continue; }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let kv: HashMap<&str, &str> = lhs.split_whitespace().map(|t| t.split_once('=').unwrap()).collect();
        let prim = match kv["prim"] {
            "anim" => Prim::Anim, "trans" => Prim::Trans, "dither" => Prim::Dither, "shadow" => Prim::Shadow, "flash" => Prim::Flash, "cast" => Prim::Cast,
            p => panic!("prim {}", p),
        };
        let w: usize = kv["w"].parse().unwrap();
        let h: usize = kv["h"].parse().unwrap();
        let (x, y): (i32, i32) = (kv["x"].parse().unwrap(), kv["y"].parse().unwrap());
        let mut index = vec![0u8; w * h];
        let mut opaque = vec![false; w * h];
        for (r, row) in kv["R"].split('/').enumerate() {
            let mut c = 0;
            for run in row.split(',') {
                if let Some(n) = run.strip_prefix('S') { c += n.parse::<usize>().unwrap(); }
                else {
                    for b in unhex(&run[1..]) { index[r * w + c] = b; opaque[r * w + c] = true; c += 1; }
                }
            }
            assert_eq!(c, w, "row width, line {}", ln + 1);
        }
        let mut canvas = lcg_canvas(kv["seed"].parse().unwrap());
        let s = SpriteView { width: w, height: h, index: &index, opaque: &opaque };
        blit(&mut canvas, CW, CH, &s, x, y, prim, &luts);
        let expected = unhex(rhs.strip_prefix("A=").unwrap());
        assert_eq!(canvas, expected, "line {} prim {:?} {}x{} at ({}, {})", ln + 1, prim, w, h, x, y);
        *counts.entry(kv["prim"].to_string()).or_default() += 1;
    }
    eprintln!("verified vectors: {:?}", counts);
    assert_eq!(counts.len(), 6);
}
