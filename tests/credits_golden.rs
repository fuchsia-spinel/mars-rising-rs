//! `src/credits.rs` against the original code (tools/re/gen_credits_vectors.py, vectors in tests/vectors/credits_vectors.txt): the text layout of `0x29054`
//! (the glyph rectangles come from the table the original's `0x28750` builds; here the committed vectors only carry the table through the extracted
//! `layout/title_text.txt`, so the test needs MR_ASSETS / ./assets) and the band wipe of `0x1bafc`.
use mars_rising::credits::{layout_text, CreditsData, Item, Rect, Wipe};
use std::path::PathBuf;

fn assets() -> Option<PathBuf> {
    let p = std::env::var("MR_ASSETS").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("assets"));
    if p.join("layout/title_text.txt").exists() { Some(p) } else { None }
}

fn rect(s: &str) -> Rect {
    let v: Vec<i16> = s.split('.').map(|x| x.parse().unwrap()).collect();
    [v[0], v[1], v[2], v[3]]
}

fn pairs(s: &str) -> Vec<(Rect, Rect)> {
    if s == "-" { return Vec::new(); }
    s.split(' ').map(|t| { let (a, b) = t.split_once(':').unwrap(); (rect(a), rect(b)) }).collect()
}

fn unhex(s: &str) -> Vec<u8> { (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect() }

#[test]
fn matches_original() {
    let Some(root) = assets() else { eprintln!("no extracted assets (layout/title_text.txt): skipped"); return; };
    let data = CreditsData::parse(&std::fs::read_to_string(root.join("layout/title_text.txt")).unwrap()).unwrap();
    let (mut nt, mut nw, mut np) = (0, 0, 0);
    for (ln, line) in include_str!("vectors/credits_vectors.txt").lines().enumerate() {
        let (head, tail) = line.split_once(" -> ").unwrap();
        let f: Vec<&str> = head.split(' ').collect();
        match f[0] {
            "text" => {
                let (x, y): (i32, i32) = (f[1].parse().unwrap(), f[2].parse().unwrap());
                let (hi, mono) = (f[3] == "1", f[4] == "1");
                let got: Vec<(Rect, Rect)> = layout_text(&data.font, &unhex(f[5]), x, y, hi, mono).into_iter().map(|b| (b.src, b.dst)).collect();
                assert_eq!(got, pairs(tail), "line {}: {}", ln + 1, head);
                nt += 1;
            }
            "page" => {
                // glyph copies and the picture of a whole page, as the original's 0x10dcc draws it
                let n: i32 = f[1].parse().unwrap();
                let items = &data.pages.iter().find(|(p, _)| *p == n).unwrap_or_else(|| panic!("page {} missing", n)).1;
                let mut got: Vec<String> = Vec::new();
                for it in items {
                    match it {
                        Item::Text { text, x, y, hilite, mono } => {
                            for b in layout_text(&data.font, text, *x, *y, *hilite, *mono) {
                                got.push(format!("{}:{}", b.src.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("."), b.dst.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(".")));
                            }
                        }
                        Item::Image { id, rect } => got.push(format!("icon{}.{}.{}.{}.{}", id, rect[0], rect[1], rect[2], rect[3])),
                    }
                }
                let want: Vec<&str> = if tail == "-" { Vec::new() } else { tail.split(' ').collect() };
                assert_eq!(got, want, "page {}", n);
                np += 1;
                continue;
            }
            "wipe" => {
                let (h, ox, oy): (i32, i32, i32) = (f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap());
                let mut w = Wipe::new(h, (ox, oy));
                let mut got = Vec::new();
                while let Some(b) = w.next() { got.extend_from_slice(&b); }
                assert_eq!(got, pairs(tail), "line {}: {}", ln + 1, head);
                nw += 1;
            }
            other => panic!("unknown vector kind {}", other),
        }
    }
    eprintln!("{} text, {} page and {} wipe vectors match", nt, np, nw);
}
