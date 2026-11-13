//! Replays `tests/vectors/audio_vectors.txt` (original sound code run in the emulator, see tools/re/gen_audio_vectors.py) against `audio.rs`.
use mars_rising::audio::*;

fn lcg_pcm(n: usize, seed: u32) -> Vec<i16> {
    let mut s = seed;
    (0..n).map(|_| { s = s.wrapping_mul(1664525).wrapping_add(1013904223); ((s >> 24) & 255) as i16 - 128 }).collect()
}

fn parse_int(s: &str) -> i64 {
    if let Some(h) = s.strip_prefix("0x") { i64::from_str_radix(h, 16).unwrap() } else { s.parse().unwrap() }
}

fn run_line(line: &str) -> (String, String) {
    let (head, expect) = line.split_once(" -> ").unwrap();
    let mut rate = 0u32; let mut max = 0usize; let mut smp = ""; let mut ops = "";
    for f in head.split(' ') {
        if let Some(v) = f.strip_prefix("rate=") { rate = u32::from_str_radix(v, 16).unwrap(); }
        else if let Some(v) = f.strip_prefix("max=") { max = v.parse().unwrap(); }
        else if let Some(v) = f.strip_prefix("smp=") { smp = v; }
        else if let Some(v) = f.strip_prefix("ops=") { ops = v; }
    }
    let mut m = Mixer::new(rate);
    m.max_mix = max;
    for s in smp.split(',') {
        let p: Vec<&str> = s.split(':').collect();
        let (id, len, r, seed): (usize, usize, u32, u32) = (p[0].parse().unwrap(), p[1].parse().unwrap(), u32::from_str_radix(p[2], 16).unwrap(), p[3].parse().unwrap());
        m.set_sample(id, Sample::new(&lcg_pcm(len, seed), r));
    }
    let mut toks: Vec<String> = Vec::new();
    for op in ops.split(';') {
        let kind = op.as_bytes()[0] as char;
        let a: Vec<i64> = if op.len() > 1 { op[1..].split(',').map(parse_int).collect() } else { vec![] };
        match kind {
            'P' => { m.play(a[0] as i32, a[1] as i32, a[2] as u8); toks.push("p".into()); }
            'T' => { m.frame(a[0] as u32); toks.push("t".into()); }
            'S' => toks.push(format!("h{}", m.start(a[0] as i32, a[1] as i32))),
            'R' => { m.start_rate(a[0] as i32, a[1] as i32, a[2] as u32); toks.push("p".into()); }
            'Q' => toks.push(format!("h{}", m.start_callback(a[0] as i32, a[1] as i32, a[2] as u32, a[3] as u32))),
            'X' => { m.stop(a[0] as u32); toks.push("x".into()); }
            'I' => toks.push(format!("i{}", m.playing(a[0] as u32) & 0xffff)),
            'B' => for _ in 0..a[0] {
                let b = m.mix_buffer();
                let v = m.voice_list();
                let vs = if v.is_empty() { "-".to_string() } else { v.iter().map(|(h, r)| format!("{}.{}", h, r)).collect::<Vec<_>>().join(",") };
                toks.push(format!("b{:016x}/{}", fnv64(&b), vs));
            },
            'M' => { m.suppressed = a[0] != 0; toks.push("m".into()); }
            'E' => { m.sound_on = a[0] != 0; toks.push("e".into()); }
            _ => panic!("op {}", op),
        }
        // the generator collects the callbacks of a whole op (all its buffers) after the op
        for e in m.take_events() { toks.push(format!("c{}.{}.{}", e.sel, e.callback, e.param)); }
    }
    (toks.join(" "), expect.to_string())
}

#[test]
fn audio_matches_original() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/audio_vectors.txt")).unwrap();
    let mut n = 0;
    for (i, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.is_empty() { continue; }
        let (got, want) = run_line(line);
        assert_eq!(got, want, "vector {} ({})", i, &line[..line.find(" -> ").unwrap().min(300)]);
        n += 1;
    }
    assert!(n >= 100, "only {} vectors", n);
}
