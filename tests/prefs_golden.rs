//! `src/prefs.rs` against the original code (tools/re/gen_prefs_vectors.py, tests/vectors/prefs_vectors.txt): the factory defaults `0xb064` and the whole
//! Preferences dialog `0x6810` (both pages, every item, Save / Cancel / Factory Settings / Revert) with the calls it makes.
use mars_rising::prefs::{Closed, Dialog, Prefs, KEY_RECORD, REGION};

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{:02x}", x)).collect() }

#[test]
fn matches_original() {
    let (mut nf, mut nd) = (0, 0);
    for (ln, line) in include_str!("vectors/prefs_vectors.txt").lines().enumerate() {
        let (head, tail) = line.split_once(" -> ").unwrap();
        let f: Vec<&str> = head.split(' ').collect();
        match f[0] {
            "fact" => {
                let flag: u8 = f[1].parse().unwrap();
                let mut p = Prefs::from_parts(&unhex(f[2]), &unhex(f[3]));
                p.apply_defaults(flag != 0);
                let t: Vec<&str> = tail.split(' ').collect();
                assert_eq!(hex(p.region()), t[0], "line {}: factory block", ln + 1);
                assert_eq!(hex(p.keys()), t[1], "line {}: factory key set", ln + 1);
                nf += 1;
            }
            "prefs" => {
                let (mod_ok, cd_ok) = (f[1] == "1", f[2] == "1");
                let p = Prefs::from_parts(&unhex(f[3]), &unhex(f[4]));
                let (res, ops) = tail.split_once(" | ").unwrap();
                let r: Vec<&str> = res.split(' ').collect();
                let (mut d, mut got) = Dialog::open(p, mod_ok, cd_ok);
                let mut last = (None, None, None);
                for h in f[5].split(',') {
                    let (item, value) = match h.split_once(':') {
                        Some((i, v)) => (i.parse::<i16>().unwrap(), v.parse::<i16>().unwrap()),
                        None => (h.parse::<i16>().unwrap(), 1),
                    };
                    got.extend(d.hit(item, value));
                    if d.closed().is_some() { break; }
                }
                assert!(d.closed().is_some(), "line {}: the dialog did not close", ln + 1);
                let text: Vec<String> = got.iter().map(|o| o.to_string()).collect();
                for o in &got {
                    match o {
                        mars_rising::prefs::Op::Translucent(v) => last.0 = Some(*v),
                        mars_rising::prefs::Op::Shadows(v) => last.1 = Some(*v),
                        mars_rising::prefs::Op::Detail(v) => last.2 = Some(*v),
                        _ => {}
                    }
                }
                assert_eq!(text.join(" "), if ops == "-" { String::new() } else { ops.to_string() }, "line {}: operations", ln + 1);
                assert_eq!(hex(d.prefs().region()), r[0], "line {}: block", ln + 1);
                assert_eq!(hex(d.prefs().keys()), r[1], "line {}: key set", ln + 1);
                for (k, want) in [(last.0, r[2]), (last.1, r[3]), (last.2, r[4])] {
                    if let Some(v) = k { assert_eq!(v.to_string(), want, "line {}: display switch", ln + 1); }
                }
                let _ = (Closed::Save, REGION, KEY_RECORD);
                nd += 1;
            }
            x => panic!("line {}: unknown vector {}", ln + 1, x),
        }
    }
    assert!(nf > 10 && nd > 100, "{} {}", nf, nd);
}

#[test]
fn volume_and_escape() {
    let mut p = Prefs::factory();
    assert_eq!(p.volume_scale(), 128);
    p.set_short(51, 8);
    assert_eq!(p.volume_scale(), 256);
    p.set_short(51, 0);
    assert_eq!(p.volume_scale(), 0);
    p.set_short(51, 9);
    assert_eq!(p.volume_scale(), 128);
    let mut e = mars_rising::prefs::EscKey::new();
    assert!(e.step(true, false));
    assert!(!e.step(false, true));
    for _ in 0..30 { assert!(!e.step(true, true)); }
    assert!(e.step(true, true));
}

#[test]
fn file_round_trip() {
    let p = Prefs::factory();
    assert_eq!(Prefs::from_bytes(&p.to_bytes()).unwrap(), p);
    assert!(Prefs::from_bytes(&[0; 3]).is_none());
}
