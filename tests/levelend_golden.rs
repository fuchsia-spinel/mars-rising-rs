//! Differential test for src/levelend.rs (the end-of-level coin bonus: setup 0x20764, per-frame state machine 0x208fc, drawing 0x20be0): every vector in
//! tests/vectors/levelend_vectors.txt was produced by *executing the original PowerPC routines* (tools/re/gen_levelend_vectors.py). After every call the
//! state block and the exact sequence of calls to the mocked surroundings (sounds, score, sprite blits) must be identical.
use mars_rising::draw::Prim;
use mars_rising::levelend::*;
use std::collections::HashMap;

fn fnv64(b: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &x in b {
        h = (h ^ x as u64).wrapping_mul(0x100000001b3);
    }
    h
}

#[derive(Default)]
struct Mock {
    tick: u32,
    players: u8,
    level: i32,
    style: bool,
    trans: bool,
    ships: [(bool, i32, i32); 2],
    log: Vec<String>,
}

impl TallyWorld for Mock {
    fn tick(&self) -> u32 { self.tick }
    fn players(&self) -> u8 { self.players }
    fn level(&self) -> i32 { self.level }
    fn coin_style(&self) -> bool { self.style }
    fn translucent(&self) -> bool { self.trans }
    fn ship_active(&self, n: u8) -> bool { self.ships[n as usize - 1].0 }
    fn ship_lives(&self, n: u8) -> i32 { self.ships[n as usize - 1].1 }
    fn ship_coins(&self, n: u8) -> i32 { self.ships[n as usize - 1].2 }
    fn add_score(&mut self, n: u8, points: i32, flash: i32) { self.log.push(format!("sc{}_{}_{}", n, points, flash)); }
    fn convert_coins(&mut self) { self.log.push("conv_".to_string()); }
    fn sound(&mut self, id: i32, prio: i32, arg: i32) { self.log.push(format!("snd_{}_{}_{}", id, prio, arg)); }
    fn blit(&mut self, prim: Prim, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.log.push(format!("d{:x}_0_{}_{}_{}_{}_{}_{}", prim.addr(), x, y, w, h, anim, frame));
    }
    fn blit_clipped(&mut self, x: i32, y: i32, w: i32, h: i32, anim: i32, frame: i32) {
        self.log.push(format!("d307c_0_{}_{}_{}_{}_{}_{}", x, y, w, h, anim, frame));
    }
}

#[test]
fn levelend_matches_original_executable() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/levelend_vectors.txt")).expect("vectors");
    let mut calls_seen: HashMap<String, usize> = HashMap::new();
    let mut n_vec = 0;
    for (ln, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() { continue; }
        let (lhs, rhs) = line.split_once(" -> ").unwrap();
        let (_, ops) = lhs.split_once(" ops=").unwrap();
        let want: Vec<&str> = rhs.split(' ').collect();
        assert_eq!(want.len(), ops.split(';').count(), "line {}", ln + 1);
        let mut m = Mock::default();
        let mut t = Tally::default();
        for (k, op) in ops.split(';').enumerate() {
            m.log.clear();
            let (kind, arg) = op.split_at(1);
            let a: Vec<i64> = if arg.is_empty() { vec![] } else { arg.split(',').map(|v| v.parse().unwrap()).collect() };
            let mut ret = None;
            match kind {
                "P" => { m.players = a[0] as u8; m.level = a[1] as i32; m.style = a[2] != 0; m.trans = a[3] != 0; }
                "H" => m.ships[a[0] as usize - 1] = (a[1] != 0, a[2] as i32, a[3] as i32),
                "T" => m.tick = a[0] as u32,
                "S" => t.setup(&m),
                "A" => ret = Some(t.step(&mut m) as u8),
                "D" => t.draw(&mut m, a[0] as i32),
                _ => panic!("op {}", op),
            }
            let mut got = format!("h{:016x}", fnv64(&t.field_bytes()));
            for l in &m.log { got += &format!(",{}", l); }
            if let Some(r) = ret { got += &format!(",r{}", r); }
            for l in &m.log { *calls_seen.entry(l.split('_').next().unwrap().to_string()).or_default() += 1; }
            if got != want[k] {
                let (gl, wl): (Vec<&str>, Vec<&str>) = (got.split(',').skip(1).collect(), want[k].split(',').skip(1).collect());
                let mut msg = format!("line {} op #{} `{}`\n  digest rust {} want {}", ln + 1, k, op, got.split(',').next().unwrap(), want[k].split(',').next().unwrap());
                if gl != wl { msg += &format!("\n  rest rust {:?}\n  rest want {:?}", gl, wl); }
                msg += &format!("\n  state {:?}", t);
                panic!("{}", msg);
            }
        }
        n_vec += 1;
    }
    let mut calls: Vec<_> = calls_seen.into_iter().collect();
    calls.sort();
    eprintln!("verified {} vectors; calls {:?}", n_vec, calls);
}
