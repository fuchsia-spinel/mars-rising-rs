//! `src/levelsel.rs` against the original code (tools/re/gen_levelsel_vectors.py, tests/vectors/levelsel_vectors.txt): the "Start at which level?" dialog `0x1b254`
//! (initial text and sound, OK / Cancel, the clamping, the 30 tick display) and its key filter `0x7e04`.
use mars_rising::levelsel::{filter, LevelSelect, Op};

fn unhex(s: &str) -> Vec<u8> {
    if s == "-" { return Vec::new(); }
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn ops_text(ops: &[Op]) -> String {
    if ops.is_empty() { "-".to_string() } else { ops.iter().map(|o| o.to_string()).collect::<Vec<_>>().join(" ") }
}

#[test]
fn matches_original() {
    let (mut nl, mut nf) = (0, 0);
    for (ln, line) in include_str!("vectors/levelsel_vectors.txt").lines().enumerate() {
        let (head, tail) = line.split_once(" -> ").unwrap();
        let f: Vec<&str> = head.split(' ').collect();
        let (res, ops) = tail.split_once(" | ").unwrap();
        match f[0] {
            "lvl" => {
                let max: i32 = f[1].parse().unwrap();
                let typed = unhex(f[2]);
                let (mut d, mut got) = LevelSelect::open(max);
                for h in f[3].split(',') {
                    got.extend(d.hit(h.parse().unwrap(), &typed));
                    if d.result().is_some() { break; }
                }
                // the original loops until item 1 or 2; the vector's hit lists always end in one of them
                assert_eq!(d.result().unwrap().to_string(), res, "line {}: result", ln + 1);
                assert_eq!(ops_text(&got), ops, "line {}: operations", ln + 1);
                nl += 1;
            }
            "filt" => {
                let ctx = f[1] == "1";
                let what: u16 = f[2].parse().unwrap();
                let msg = u32::from_str_radix(f[3], 16).unwrap();
                let mods = u16::from_str_radix(f[4], 16).unwrap();
                let r = filter(ctx, what, msg, mods);
                let want: Vec<&str> = res.split(' ').collect();
                assert_eq!(r.handled as u8, want[0].parse::<u8>().unwrap(), "line {}: filter result", ln + 1);
                assert_eq!(r.item.map(|i| i.to_string()).unwrap_or("-".into()), want[1], "line {}: item", ln + 1);
                assert_eq!(ops_text(&r.ops), ops, "line {}: filter operations", ln + 1);
                nf += 1;
            }
            x => panic!("line {}: {}", ln + 1, x),
        }
    }
    assert!(nl > 50 && nf > 200, "{} {}", nl, nf);
}
