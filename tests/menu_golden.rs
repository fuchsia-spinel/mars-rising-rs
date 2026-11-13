//! Differential test for src/menu.rs (the title screen's buttons): tests/vectors/menu_vectors.txt was produced by *executing the original
//! PowerPC routines* 0x1b054 (rectangles), 0x1c098 (hit test), 0x1c6f8 / 0x1c5cc (hilite / unhilite), 0x1c998 (click tracking), 0x170
//! and the pointer-tracking block of the main loop 0x1a6f0..0x1a770 (tools/re/gen_menu_vectors.py) on random layouts, window origins and
//! pointer paths. The result of every operation and the exact sequence of drawing / sound calls must be identical.
use mars_rising::menu::{Menu, Op, Rect, HANDLER_TABLE};

fn fmt_rect(r: &Rect) -> String {
    format!("{}.{}.{}.{}", r.top, r.left, r.bottom, r.right)
}

fn fmt_op(o: &Op) -> String {
    match o {
        Op::Blit { src, dst, mode } => format!("cb{}:{}.{}", fmt_rect(src), fmt_rect(dst), mode),
        Op::Refresh => "refresh".to_string(),
        Op::Sound { id, prio, delay } => format!("snd{}.{}.{}", id, prio, delay),
        Op::ResetIdle => "tick".to_string(),
    }
}

fn token(res: i64, ops: &[Op]) -> String {
    format!("r{}", res) + &ops.iter().map(|o| format!(",{}", fmt_op(o))).collect::<String>()
}

fn pt(s: &str) -> (i32, i32) {
    let (h, v) = s.split_once(',').unwrap();
    (h.parse().unwrap(), v.parse().unwrap())
}

#[test]
fn title_screen_buttons_match_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/menu_vectors.txt")).expect("vectors");
    let mut nvec = 0usize;
    let mut nops = std::collections::HashMap::<char, usize>::new();
    for (ln, line) in text.lines().enumerate() {
        if let Some(rest) = line.strip_prefix("# handlers ") {
            let want: Vec<u32> = rest.split_whitespace().take(12).map(|h| u32::from_str_radix(h, 16).unwrap()).collect();
            assert_eq!(want, HANDLER_TABLE.to_vec(), "the click dispatch table");
            continue;
        }
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let (head, ops) = lhs.split_once(" ops=").unwrap();
        let kv: std::collections::HashMap<&str, &str> = head.split_whitespace().map(|t| t.split_once('=').unwrap()).collect();
        let rects: Vec<Rect> = kv["rects"]
            .split('.')
            .map(|r| {
                let v: Vec<i16> = r.split(',').map(|x| x.parse().unwrap()).collect();
                Rect::new(v[0], v[1], v[2], v[3])
            })
            .collect();
        let org = pt(kv["org"]);
        let mut menu = Menu::new(rects.try_into().unwrap(), kv["reg"] != "0", org);
        let want: Vec<&str> = rhs.split(' ').collect();
        for (k, op) in ops.split(';').enumerate() {
            let (kind, arg) = op.split_at(1);
            *nops.entry(kind.chars().next().unwrap()).or_default() += 1;
            let got = match kind {
                "H" => {
                    let (h, v) = pt(arg);
                    token(menu.hit(h, v) as i64, &[])
                }
                "L" => token(0, &menu.hilite(arg.parse().unwrap())),
                "U" => token(0, &menu.unhilite_all()),
                "M" => {
                    let (h, v) = pt(arg);
                    let ops = menu.hover_to(h, v);
                    token(menu.hover as i64, &ops)
                }
                "T" => {
                    let (n, path) = arg.split_once(':').unwrap();
                    let pts: Vec<(i32, i32)> = path.split('/').map(pt).collect();
                    let (mut ops, press) = menu.press(n.parse().unwrap());
                    let mut res = 0;
                    if let Some(p) = press {
                        for (i, &(h, v)) in pts.iter().enumerate() {
                            let last = i + 1 == pts.len();
                            let (o, r) = p.step(&menu, h, v, !last);
                            ops.extend(o);
                            if let Some(inside) = r {
                                res = inside as i64;
                            }
                        }
                    }
                    token(res, &ops)
                }
                _ => panic!("op {}", op),
            };
            assert_eq!(got, want[k], "line {} op #{} `{}`", ln + 1, k, op);
        }
        nvec += 1;
    }
    eprintln!("verified vectors: {} ops {:?}", nvec, nops);
    assert!(nvec >= 100);
}
