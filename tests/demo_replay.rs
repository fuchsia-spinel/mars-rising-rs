//! The three attract-mode recordings (`misc/film_N.bin`) replayed on the real levels (needs the extracted assets, MR_ASSETS / ./assets; skipped without).
//! The recordings were made on the original with its `Random()` seeded by the stored value, so a port that is in step with the original keeps the ship alive
//! and playing sensibly to the end of the recording; a desynchronised one flies blind and dies early. The test prints a trace with `--nocapture`.
use mars_rising::assets::{load_level, load_particle_colours};
use mars_rising::demo::{Film, Playback};
use mars_rising::game::Game;
use mars_rising::input::PlayerInput;
use std::path::PathBuf;

fn root() -> Option<PathBuf> {
    let r = std::env::var("MR_ASSETS").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("assets"));
    if r.join("misc/film_1.bin").exists() { Some(r) } else { None }
}

fn replay(root: &std::path::Path, n: u32, seed_delta: u32) -> (usize, i32, i32, bool, usize) {
    let film = Film::parse(&std::fs::read(root.join(format!("misc/film_{}.bin", n))).unwrap()).unwrap();
    let level = load_level(root, film.level as u32).unwrap();
    let palette = mars_rising::assets::Palette::load_system(root).unwrap();
    let particles = load_particle_colours(root, &palette).unwrap();
    let mut g = Game::new_at(film.players, level.height, level.spawn_trigger_by_row.clone(), film.seed.wrapping_add(seed_delta), film.level);
    g.set_particle_colours(&particles);
    {
        let groups: Vec<(usize, Vec<u32>)> = level.groups.iter().map(|gr| (gr.index as usize, gr.fields.clone())).collect();
        g.load_groups(&groups);
        g.load_ground(&level.ground_resource(), level.width, level.height, level.ground_marker.clone());
    }
    let mut pb = Playback::new(film.clone());
    let mut frames = 0usize;
    let mut first_death = None;
    while !pb.finished() {
        let mut inp = [PlayerInput::default(); 2];
        // 0x250a0 reads (or records) the keys only while the ship is playing (state 3)
        for s in 0..film.players { if g.ships[s].state == 3 { if let Some(i) = pb.next(s) { inp[s] = i; } } }
        g.step(inp);
        frames += 1;
        if first_death.is_none() && g.ships[0].lives < 3 { first_death = Some(frames); }
        if frames % 300 == 0 {
            println!("film {} frame {:5}: x {:6.1} y {:6.1} state {} lives {} shield {} score {} level_finished {}", n, frames, g.ships[0].x, g.ships[0].y, g.ships[0].state, g.ships[0].lives, g.ships[0].shield, g.ships[0].score, g.level_finished);
        }
        if g.over { break; }
    }
    println!("film {} done after {} frames (recorded {}): over {} score {} lives {} first_death {:?}", n, frames, film.end + 1, g.over, g.ships[0].score, g.ships[0].lives, first_death);
    (frames, g.ships[0].score, g.ships[0].lives, g.over, first_death.unwrap_or(0))
}

#[test]
fn films_replay_to_the_end() {
    let Some(root) = root() else { return; };
    for n in 1..=3 {
        let (frames, score, lives, over, _) = replay(&root, n, 0);
        let recorded = Film::parse(&std::fs::read(root.join(format!("misc/film_{}.bin", n))).unwrap()).unwrap().end as usize + 1;
        assert!(!over, "film {}: game over after {} frames", n, frames);
        assert!(frames >= recorded && lives >= 1 && score > 5000, "film {}: {} frames (recorded {}), score {}, lives {}", n, frames, recorded, score, lives);
    }
}

/// The check means something: with the seed off by one the same inputs end in a game over (the ship flies blind), so the replay above is only possible
/// when the random number sequence (`Random()`, call order, call count) is the original's.
#[test]
fn a_wrong_seed_does_not_survive() {
    let Some(root) = root() else { return; };
    let dead = (1..=3u32).filter(|&n| replay(&root, n, 1).3).count();
    assert!(dead >= 2, "only {} of 3 recordings died with a wrong seed", dead);
}
