//! Level scrolling, ported from the original's scroll step (PowerPC `0xC3E0`).
//!
//! Facts recovered from the original (see RE_NOTES.md): scroll speed is only ever 0 or 1 pixel per step; every 32 pixels a new
//! row starts; at that moment the ground objects of row `counter + 1` are activated (0x3b450) and the spawn trigger `ASGM[counter]` fires (0x2d9bc),
//! both BEFORE the row counter advances; the level "near end" event fires when the row counter reaches `height - 5`, and the level finishes
//! (scrolling stops) at `height`. **The row counter starts at 16**, not 0: the 16 rows below it are the screen the level starts on (the original keeps
//! a 512-pixel background buffer), so a level of `height` rows scrolls for `height - 16` rows and the triggers of rows 0..15 never fire.
//! Row 0 is the bottom of the level. The step is called once per game frame; the original runs 30 frames per second (frame limiter at 0xd3b4 waits 2 `TickCount` ticks, the read-me says 30 FPS), i.e. the level scrolls 30 px/s.

pub const ROW_PX: i32 = 32;
/// Rows before the end at which the original raises its end-of-level event.
pub const END_ROWS: usize = 5;
/// The value of the row counter at the start of a level.
pub const START_ROW: usize = 16;
/// Spawn groups are indices below this (the original ignores 255 and anything >= 100).
pub const MAX_GROUPS: u32 = 100;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TickResult {
    /// Index of the row that was just completed (its spawn trigger fired), if a 32-pixel boundary was crossed.
    pub row_completed: Option<usize>,
    /// Spawn group triggered by `row_completed`, if any.
    pub spawn_group: Option<u32>,
    /// True on the step where the row counter reaches `height - 5`.
    pub near_end: bool,
    /// True on the step where the row counter reaches `height`; scrolling stops.
    pub finished: bool,
}

#[derive(Debug, Clone)]
pub struct Scroller {
    height: usize,
    row: usize,
    sub: i32,
    speed: i32,
}

impl Scroller {
    pub fn new(height: usize) -> Scroller {
        Scroller { height, row: START_ROW, sub: 0, speed: 1 }
    }
    /// Current row counter (starts at 16; the row whose objects were activated last is `row() + 1`).
    pub fn row(&self) -> usize { self.row }
    pub fn stopped(&self) -> bool { self.speed == 0 }
    /// 0xc3e0 returns 1 or 2 (the level is in its last five rows or over): the end-of-level tally runs every frame.
    pub fn ending(&self) -> bool { self.speed == 0 || self.row >= self.height.saturating_sub(END_ROWS) }
    /// Pixels scrolled since the level start (0 at the beginning).
    pub fn scroll_px(&self) -> i32 { (self.row as i32 - START_ROW as i32) * ROW_PX + self.sub }
    /// Value to pass to `render::draw_level` as `scroll_y` for a frame of `frame_height` pixels.
    pub fn view_top_y(&self, frame_height: i32) -> i32 {
        (self.height as i32 * ROW_PX - frame_height - self.scroll_px()).max(0)
    }
    /// One scroll step. `spawn_by_row` is the level's `spawn_trigger_by_row`.
    pub fn tick(&mut self, spawn_by_row: &[u32]) -> TickResult {
        let mut out = TickResult::default();
        if self.speed == 0 { return out; }
        self.sub += self.speed;
        if self.sub < ROW_PX { return out; }
        self.sub = 0;
        let cur = self.row;
        out.row_completed = Some(cur);
        out.spawn_group = spawn_by_row.get(cur).copied().filter(|&g| g < MAX_GROUPS);
        self.row += 1;
        if self.row == self.height.saturating_sub(END_ROWS) { out.near_end = true; }
        if self.row >= self.height {
            out.finished = true;
            self.speed = 0;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_completes_every_32_steps() {
        let mut s = Scroller::new(40);
        let asgm = vec![255u32; 40];
        for _ in 0..31 { assert_eq!(s.tick(&asgm), TickResult::default()); }
        let r = s.tick(&asgm);
        assert_eq!(r.row_completed, Some(16));
        assert_eq!(s.row(), 17);
        assert_eq!(s.scroll_px(), 32);
    }

    #[test]
    fn spawn_fires_with_the_row_being_left_and_ignores_invalid_groups() {
        let mut asgm = vec![255u32; 40];
        asgm[16] = 7;
        asgm[17] = 100; // out of range -> ignored
        asgm[18] = 0;   // group 0 is valid
        asgm[3] = 9;    // below the start row: never fires
        let mut s = Scroller::new(40);
        let mut got = Vec::new();
        for _ in 0..(3 * 32) { let r = s.tick(&asgm); if r.row_completed.is_some() { got.push(r.spawn_group); } }
        assert_eq!(got, vec![Some(7), None, Some(0)]);
    }

    #[test]
    fn end_events_and_stop() {
        let mut s = Scroller::new(26);
        let asgm = vec![255u32; 26];
        let (mut near, mut fin, mut steps_after) = (None, None, 0);
        for step in 1..=(12 * 32) {
            let r = s.tick(&asgm);
            if r.near_end { near = Some(step); }
            if r.finished { fin = Some(step); }
            if fin.is_some() && r.row_completed.is_some() && Some(step) != fin { steps_after += 1; }
        }
        assert_eq!(near, Some(5 * 32));   // row counter 16 -> height-5 = 21
        assert_eq!(fin, Some(10 * 32));   // row counter reaches height = 26, scrolling stops
        assert!(s.stopped());
        assert_eq!(steps_after, 0);
        assert_eq!(s.row(), 26);
    }

    #[test]
    fn view_position_matches_the_original_screenshots() {
        // (level height in rows, scroll_y of the original screenshot) from the pixel-exact alignments in ASSET_FORMATS.md;
        // the original was therefore at (height*32 - 480 - scroll_y) pixels from the start.
        for &(height, scroll_y) in &[(150usize, 2755i32), (150, 2019), (150, 3738), (150, 3644)] {
            let px = height as i32 * ROW_PX - 480 - scroll_y;
            let mut s = Scroller::new(height);
            let asgm = vec![255u32; height];
            for _ in 0..px { s.tick(&asgm); }
            assert_eq!(s.scroll_px(), px);
            assert_eq!(s.view_top_y(480), scroll_y);
        }
    }
}
