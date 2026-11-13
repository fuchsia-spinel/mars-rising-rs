use mars_rising::madh::Music;
fn assets() -> Option<std::path::PathBuf> { std::env::var("MR_ASSETS").ok().map(Into::into) }
#[test]
fn parses_all_tracks() {
    let Some(a) = assets() else { return };
    for n in ["interface", "level_1", "level_2", "level_3", "special"] {
        let b = std::fs::read(a.join("music").join(format!("{}_music.madh", n))).unwrap();
        let m = Music::parse(&b).unwrap();
        let samples: usize = m.instr.iter().map(|i| i.samples.len()).sum();
        let bytes: usize = m.instr.iter().flat_map(|i| i.samples.iter()).map(|s| s.data.len()).sum();
        println!("{} name={:?} pat={} chn={} ptr={} speed={} tempo={} samples={} bytes={} amp16={}", n, m.name, m.num_pat, m.num_chn, m.num_pointers, m.speed, m.tempo, samples, bytes,
            m.instr.iter().flat_map(|i| i.samples.iter()).filter(|s| s.amp == 16).count());
        assert!(m.patterns.iter().all(|p| p.cmds.len() == (p.rows as usize) * m.num_chn as usize));
    }
}
