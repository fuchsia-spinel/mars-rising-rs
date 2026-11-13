//! The Credits slideshow end to end on the real pictures: timing of the wipes and pages, the modes, the keys, and the picture it draws
//! (needs the extracted assets, MR_ASSETS / ./assets; skipped without).
use mars_rising::credits::*;
use mars_rising::assets::read_png;
use mars_rising::title::{load_credits, Art};
use std::path::PathBuf;

fn root() -> Option<PathBuf> {
    let p = std::env::var("MR_ASSETS").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("assets"));
    if p.join("layout/title_text.txt").exists() { Some(p) } else { None }
}

fn setup() -> Option<(std::rc::Rc<CreditsData>, CreditsArt, Vec<u32>)> {
    let root = root()?;
    let bg = Art::from_decoded(&read_png(&root.join("pictures/titles/9000_background.png")).unwrap(), &[]).unwrap();
    let background: Vec<u32> = bg.px.iter().map(|p| p | 0xFF00_0000).collect();
    let (data, cart) = load_credits(&root, &background).unwrap();
    // stand-in for the title picture (the real one has the logo and the buttons on the same background): the background with a mark on it
    let mut title = background.clone();
    for y in 100..200 { for x in 100..200 { title[y * W + x] = 0xFF11_2233; } }
    Some((data, cart, title))
}

fn run_ticks(c: &mut Credits, art: &CreditsArt, title: &[u32], n: u32) { for _ in 0..n { c.tick(art, title); } }

#[test]
fn the_first_page_is_wiped_in_and_pages_follow_every_240_ticks() {
    let Some((data, art, title)) = setup() else { return; };
    let mut c = Credits::new(data.clone(), &art, 0, &title);
    assert_eq!(c.screen(), &title[..]);
    // 58 wipe ticks (the bands meet after 29, the rest is the original's idle loop), then the page stays until tick 241 counted from the start
    run_ticks(&mut c, &art, &title, 29);
    assert_ne!(c.screen(), &title[..]);
    run_ticks(&mut c, &art, &title, 29);
    let page0 = c.screen().to_vec();
    run_ticks(&mut c, &art, &title, 241 - 58 - 1);
    assert_eq!(c.screen(), &page0[..], "the first page is still shown");
    run_ticks(&mut c, &art, &title, 1);   // tick 241: the second page starts its wipe
    run_ticks(&mut c, &art, &title, 58);
    assert_ne!(c.screen(), &page0[..]);
    assert_eq!(c.outcome(), None);
}

#[test]
fn a_click_ends_it_with_sound_29_and_the_title_comes_back() {
    let Some((data, art, title)) = setup() else { return; };
    let mut c = Credits::new(data, &art, 0, &title);
    run_ticks(&mut c, &art, &title, 60);
    c.push(Event::Click);
    run_ticks(&mut c, &art, &title, 1);
    assert_eq!(c.take_sounds(), vec![(29, 3, 0)]);
    assert_eq!(c.outcome(), None, "the return wipe is running");
    run_ticks(&mut c, &art, &title, 60);
    assert_eq!(c.outcome(), Some(Outcome::Back));
    // the rows the wipe never copies (the first and last 12) were not title rows to begin with: the rest is the title
    assert_eq!(&c.screen()[12 * W..468 * W], &title[12 * W..468 * W]);
}

#[test]
fn n_starts_a_game_and_other_keys_and_events_during_a_wipe_wait() {
    let Some((data, art, title)) = setup() else { return; };
    let mut c = Credits::new(data.clone(), &art, 0, &title);
    c.push(Event::Key { n: true });             // pressed during the first wipe: handled when it is over
    run_ticks(&mut c, &art, &title, 57);
    assert_eq!(c.outcome(), None);
    run_ticks(&mut c, &art, &title, 1);
    assert_eq!(c.outcome(), Some(Outcome::NewGame));
    assert_eq!(c.take_sounds(), vec![(29, 3, 0)]);
    let mut c = Credits::new(data, &art, 0, &title);
    run_ticks(&mut c, &art, &title, 100);
    c.push(Event::Key { n: false });
    run_ticks(&mut c, &art, &title, 1);
    assert_eq!(c.take_sounds(), vec![(29, 3, 0)]);
    run_ticks(&mut c, &art, &title, 40);
    assert_eq!(c.outcome(), Some(Outcome::Back));
}

#[test]
fn running_out_of_pages_ends_silently() {
    let Some((data, art, title)) = setup() else { return; };
    // mode 3 has three pages (300..302): 58 + 3 x ... ticks
    let mut c = Credits::new(data, &art, 3, &title);
    let mut ticks = 0;
    while c.outcome().is_none() && ticks < 5000 { run_ticks(&mut c, &art, &title, 1); ticks += 1; }
    assert_eq!(c.outcome(), Some(Outcome::Back));
    assert!(c.take_sounds().is_empty());
    // first page: 241 ticks from the start; pages 2 and 3: 58 + 241 each; then the return wipe (38 ticks)
    assert_eq!(ticks, 241 + 2 * (58 + 241) + 38 - 0 + 0, "ticks until the end");
}

#[test]
fn the_text_is_drawn_in_the_font_colours() {
    let Some((data, art, title)) = setup() else { return; };
    let mut c = Credits::new(data, &art, 0, &title);
    run_ticks(&mut c, &art, &title, 58);
    let s = c.screen();
    // "Programming and Design" is highlighted text centred at y = 130: pixels differ from the plain background there
    let diff = (130..156).flat_map(|y| (0..W).map(move |x| y * W + x)).filter(|&i| s[i] != art.background[i]).count();
    assert!(diff > 300, "{} differing pixels", diff);
}

#[test]
fn the_quotation_page_of_the_x_key_has_no_timer_and_ends_like_the_slideshow() {
    // 0x1cef0 / 0x1e808: sound 65, the page 400 wiped in, then it waits for ever; a click plays 29 and the title comes back; N starts a game
    let Some((data, art, title)) = setup() else { return; };
    let mut c = Credits::single(data.clone(), &art, 400, &title);
    assert_eq!(c.take_sounds(), vec![(65, 4, 0)]);
    run_ticks(&mut c, &art, &title, 58);
    let page = c.screen().to_vec();
    assert_ne!(page, title);
    run_ticks(&mut c, &art, &title, 2000);
    assert_eq!(c.screen(), &page[..], "no page timer: the quotation stays");
    assert_eq!(c.outcome(), None);
    c.push(Event::Click);
    run_ticks(&mut c, &art, &title, 1);
    assert_eq!(c.take_sounds(), vec![(29, 3, 0)]);
    run_ticks(&mut c, &art, &title, 60);
    assert_eq!(c.outcome(), Some(Outcome::Back));
    let mut c = Credits::single(data, &art, 400, &title);
    run_ticks(&mut c, &art, &title, 70);
    c.push(Event::Key { n: true });
    run_ticks(&mut c, &art, &title, 1);
    assert_eq!(c.outcome(), Some(Outcome::NewGame));
}
