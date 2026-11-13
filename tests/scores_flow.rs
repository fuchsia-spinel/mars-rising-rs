//! The High Scores screen end to end on the real pictures (needs the extracted assets, MR_ASSETS / ./assets; skipped without): the blinking rows
//! hide and show again, the keys, and the name entry's panel draws.
use mars_rising::assets::read_png;
use mars_rising::credits::{CreditsArt, CreditsData, W};
use mars_rising::scores::{Button, EntryPanel, Input, NameEntry, Op, Outcome, ScoreCanvas, ScoreScreen, Strings, Table};
use mars_rising::title::{load_credits, Art};
use std::path::PathBuf;

fn setup() -> Option<(std::rc::Rc<CreditsData>, CreditsArt, Vec<u32>, Table)> {
    let root = std::env::var("MR_ASSETS").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("assets"));
    if !root.join("layout/title_text.txt").exists() || !root.join("misc/default_scores.bin").exists() { return None; }
    let bg = Art::from_decoded(&read_png(&root.join("pictures/titles/9000_background.png")).unwrap(), &[]).unwrap();
    let background: Vec<u32> = bg.px.iter().map(|p| p | 0xFF00_0000).collect();
    let (data, art) = load_credits(&root, &background).unwrap();
    let table = Table::from_bytes(&std::fs::read(root.join("misc/default_scores.bin")).unwrap()).unwrap();
    Some((data, art, background, table))
}

fn run(ss: &mut ScoreScreen, cv: &mut ScoreCanvas, art: &CreditsArt, data: &CreditsData, from: u32, to: u32, input: impl Fn(u32) -> Option<Input>) {
    for t in from..to {
        for (_, op) in ss.tick(input(t)) { cv.apply(&op, art, data); }
    }
}

fn row_pixels(px: &[u32], row: usize) -> Vec<u32> {
    let y = 70 + 35 * row;
    (y..y + 26).flat_map(|r| px[r * W + 100..r * W + 580].to_vec()).collect()
}

#[test]
fn new_rows_blink_five_times_and_the_screen_times_out() {
    let Some((data, art, background, table)) = setup() else { return; };
    let mut ss = ScoreScreen::new(&table, [2, -1]);
    let mut cv = ScoreCanvas::new(&background);
    run(&mut ss, &mut cv, &art, &data, 0, 1, |_| None);
    let shown = row_pixels(cv.screen(), 2);
    let bg_row = row_pixels(&background, 2);
    assert_ne!(shown, bg_row, "the row is drawn");
    // ticks 20 .. 28 the row is shown (the screen has not changed), at 28 it is hidden (background), at 36 shown again
    run(&mut ss, &mut cv, &art, &data, 1, 29, |_| None);
    assert_eq!(row_pixels(cv.screen(), 2), bg_row, "hidden after the first 8 ticks");
    run(&mut ss, &mut cv, &art, &data, 29, 37, |_| None);
    assert_eq!(row_pixels(cv.screen(), 2), shown, "drawn again");
    // other rows never change
    assert_eq!(row_pixels(cv.screen(), 5), row_pixels(&{ let mut c = ScoreCanvas::new(&background); let mut s = ScoreScreen::new(&table, [-1, -1]); run(&mut s, &mut c, &art, &data, 0, 1, |_| None); c.screen().to_vec() }, 5));
    run(&mut ss, &mut cv, &art, &data, 37, 700, |_| None);
    assert_eq!(ss.outcome(), Some(Outcome::Back));
    assert_eq!(ss.end(), 602);
}

#[test]
fn keys_and_clicks() {
    let Some((data, art, background, table)) = setup() else { return; };
    for (input, want, sounds) in [(Input::Click, Outcome::Back, 0), (Input::Key(b'x'), Outcome::Back, 0), (Input::Key(b'N'), Outcome::NewGame, 1), (Input::Key(b'n'), Outcome::NewGame, 1)] {
        let mut ss = ScoreScreen::new(&table, [-1, -1]);
        let mut cv = ScoreCanvas::new(&background);
        let mut snd = 0;
        for t in 0..10 {
            for (_, op) in ss.tick(if t == 5 { Some(input) } else { None }) {
                if let Op::Snd(30, 3, 0) = op { snd += 1; }
                cv.apply(&op, &art, &data);
            }
        }
        assert_eq!(ss.outcome(), Some(want));
        assert_eq!(snd, sounds);
    }
    // input while the blinking runs is dropped
    let mut ss = ScoreScreen::new(&table, [0, 1]);
    for t in 0..50 { ss.tick(if t == 10 { Some(Input::Click) } else { None }); }
    assert_eq!(ss.outcome(), None);
}

#[test]
fn the_panel_draws_and_has_two_buttons() {
    let Some((data, art, background, table)) = setup() else { return; };
    let strings = Strings::from_data(&data);
    let panel = EntryPanel::centered(576, 480);
    let e = NameEntry::new(&table, 2, false, 5000, 1);
    let mut buf = background.clone();
    panel.draw(&mut buf, &e, &strings, &art, &data, None, true, Some(Button::Ok));
    assert_ne!(buf, background);
    assert_eq!(panel.hit(panel.x + 400, panel.y + 190), Some(Button::Ok));
    assert_eq!(panel.hit(panel.x + 250, panel.y + 190), Some(Button::Cancel));
    assert_eq!(panel.hit(panel.x + 5, panel.y + 5), None);
}
