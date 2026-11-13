//! Differential test for pickup::clear_reward (0xea4c) against vectors made by running the original routine (tools/re/gen_clear_vectors.py).
use mars_rising::pickup::clear_reward;

#[test]
fn clear_reward_matches_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/clear_vectors.txt")).expect("vectors");
    let mut n = 0;
    for (ln, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() { continue; }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let kv: Vec<&str> = lhs.split_whitespace().collect();
        let ship = |s: &str| -> Option<u8> {
            let (st, ar) = s.split_once('=').unwrap().1.split_once(',').unwrap();
            if st == "3" { Some(ar.parse().unwrap()) } else { None }
        };
        let dif: u32 = kv[2].split_once('=').unwrap().1.parse().unwrap();
        let (x, y) = (kv[3].split_once('=').unwrap().1, kv[4].split_once('=').unwrap().1);
        let got = match clear_reward(ship(kv[0]), ship(kv[1]), dif) {
            Some((k, v)) => format!("spawn_{}_{}_1_0_{}_{}", k, v, x, y),
            None => "-".to_string(),
        };
        assert_eq!(got, rhs, "line {}", ln + 1);
        n += 1;
    }
    eprintln!("verified {} vectors", n);
}
