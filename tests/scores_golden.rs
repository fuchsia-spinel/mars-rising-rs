//! `src/scores.rs` against the original code (tools/re/gen_scores_vectors.py, tests/vectors/scores_vectors.txt): the qualifying test `0x199dc`, the name entry
//! `0x19b28` (insertion, default / last / author's name, sounds) and the whole High Scores screen `0x194ac` (drawing operations with their ticks, blinking,
//! 600 tick time-out, click / N key exit). The strings of the name entry come from `layout/title_text.txt` (needs MR_ASSETS / ./assets).
use mars_rising::credits::CreditsData;
use mars_rising::scores::{Input, NameEntry, ScoreScreen, Strings, Table, TABLE_LEN};
use std::path::PathBuf;

fn unhex(s: &str) -> Vec<u8> {
    if s == "-" { return Vec::new(); }
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{:02x}", x)).collect() }

/// The name fields keep stack garbage behind the Pascal string in the original; only the string counts.
fn mask(t: &[u8]) -> Vec<u8> {
    let mut t = t.to_vec();
    for i in 0..12 {
        let n = (t[i * 21] as usize).min(20);
        for k in 1 + n..21 { t[i * 21 + k] = 0; }
    }
    t
}

fn strings() -> Option<Strings> {
    let p = std::env::var("MR_ASSETS").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("assets")).join("layout/title_text.txt");
    let text = std::fs::read_to_string(p).ok()?;
    Some(Strings::from_data(&CreditsData::parse(&text).unwrap()))
}

#[test]
fn matches_original() {
    let Some(strings) = strings() else { eprintln!("no extracted assets (layout/title_text.txt): skipped"); return; };
    let (mut nq, mut ne, mut ns) = (0, 0, 0);
    for (ln, line) in include_str!("vectors/scores_vectors.txt").lines().enumerate() {
        let (head, tail) = line.split_once(" -> ").unwrap();
        let f: Vec<&str> = head.split(' ').collect();
        match f[0] {
            "qual" => {
                let (p, s1, s2): (u8, i32, i32) = (f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap());
                let t = Table::from_bytes(&unhex(f[4])).unwrap();
                let got = t.qualifies(if p == 1 { s1 } else { s2 });
                assert_eq!(got as u8, tail.parse::<u8>().unwrap(), "line {}", ln + 1);
                nq += 1;
            }
            "entry" => {
                let p: u8 = f[1].parse().unwrap();
                let single = f[2] == "1";
                let (s1, s2, lvl): (i32, i32, i32) = (f[3].parse().unwrap(), f[4].parse().unwrap(), f[5].parse().unwrap());
                let typed = unhex(f[6]);
                let hits: Vec<i32> = f[7].split(',').map(|x| x.parse().unwrap()).collect();
                let mut t = Table::from_bytes(&unhex(f[8])).unwrap();
                let score = if p == 1 { s1 } else { s2 };
                let mut ops: Vec<String> = Vec::new();
                let (mut ret, mut idx) = (0, [-1i32; 2]);
                if t.wants(score) {
                    let mut e = NameEntry::new(&t, p, single, score, lvl);
                    ops.push("flush@0".into());
                    ops.push(format!("settext:3:{}", hex(strings.prompt(single, p))));
                    ops.push(format!("settext:2:{}", hex(t.last_name(p))));
                    for (a, b, c) in e.take_sounds() { ops.push(format!("snd:{}:{}:{}@0", a, b, c)); }
                    ops.push(format!("portrait:{}", e.portrait()));
                    e.text = typed.clone();
                    for h in hits.iter().copied().chain(std::iter::once(6)) {
                        if h == 1 { e.ok(); break; }
                        if h == 6 { e.cancel(); break; }
                    }
                    if e.done() == Some(true) {
                        let (row, sounds) = e.commit(&mut t, &strings);
                        for (a, b, c) in sounds { ops.push(format!("snd:{}:{}:{}@0", a, b, c)); }
                        idx[(p - 1) as usize] = row as i32;
                        ret = 1;
                    }
                }
                let want = tail.split(" | ").nth(1).unwrap();
                let head_want: Vec<&str> = tail.split(" | ").next().unwrap().split(' ').collect();
                assert_eq!(ret.to_string(), head_want[0], "line {} ret", ln + 1);
                assert_eq!(idx[0].to_string(), head_want[1], "line {} idx1", ln + 1);
                assert_eq!(idx[1].to_string(), head_want[2], "line {} idx2", ln + 1);
                assert_eq!(hex(&mask(t.to_bytes())), head_want[3], "line {} table", ln + 1);
                let want: Vec<&str> = if want == "-" { Vec::new() } else { want.split(' ').collect() };
                assert_eq!(ops, want, "line {}", ln + 1);
                ne += 1;
            }
            "screen" => {
                let idx = [f[1].parse::<i32>().unwrap(), f[2].parse::<i32>().unwrap()];
                let mut events: Vec<(u32, Input)> = Vec::new();
                if f[3] != "-" {
                    for e in f[3].split(',') {
                        let v: Vec<u32> = e.split(':').map(|x| x.parse().unwrap()).collect();
                        match v[1] { 1 => events.push((v[0], Input::Click)), 3 => events.push((v[0], Input::Key((v[2] & 0xff) as u8))), _ => {} }
                    }
                }
                let t = Table::from_bytes(&unhex(f[4])).unwrap();
                let mut ss = ScoreScreen::new(&t, idx);
                let mut got: Vec<String> = Vec::new();
                let mut now = 0u32;
                while ss.outcome().is_none() {
                    let input = events.iter().find(|(t, _)| *t == now).map(|(_, i)| *i);
                    for (at, op) in ss.tick(input) { got.push(op.token(at)); }
                    now += 1;
                    assert!(now < 2000);
                }
                let ret = (ss.outcome() == Some(mars_rising::scores::Outcome::NewGame)) as u8;
                got.push(format!("end:{}", ss.end()));
                got.push(format!("ret:{}", ret));
                let want: Vec<&str> = tail.split(' ').collect();
                assert_eq!(got.len(), want.len(), "line {}: operation count", ln + 1);
                for (i, (a, b)) in got.iter().zip(want.iter()).enumerate() {
                    assert_eq!(a, b, "line {} operation {}", ln + 1, i);
                }
                ns += 1;
            }
            _ => panic!("line {}", ln + 1),
        }
    }
    eprintln!("{} qualify, {} name entry, {} screen vectors", nq, ne, ns);
    assert!(nq > 50 && ne > 80 && ns > 30);
    let _ = TABLE_LEN;
}
