//! Presentation helpers that do not depend on any windowing library (so they are unit-tested everywhere): the fixed-timestep pacer that turns
//! display refreshes into logic frames, the destination rectangle of the picture inside the window, and the pixel format conversion.
//!
//! The game logic runs at `FRAME_MICROS` (33.25 ms: 2 Mac ticks of 1/60.15 s). A vsynced display delivers a picture every 1/60 s (or
//! 1/144 s ...), so the pacer decides *per displayed frame* how many logic frames are due. The picture is drawn on every refresh; the
//! logic only advances when it is due, which never causes a partial or uneven step.

/// How far (relative) a whole number of display frames per logic frame may be from the logic frame time for the pacer to lock to it.
const LOCK_TOLERANCE: f64 = 0.05;
/// Display frames in the window the refresh rate is measured over, and how often the lock is re-evaluated.
const WINDOW: usize = 90;
const EVAL_EVERY: u32 = 30;
/// Samples needed for the first estimate, and how often to try while not locked.
const MIN_SAMPLES: usize = 16;
const EVAL_EVERY_UNLOCKED: u32 = 8;
/// Longest whole-number cadence the pacer locks to (a display that runs much faster than 300 Hz, or a window that is not vsynced, is paced by time).
const MAX_LOCK: u32 = 10;

/// Turns displayed frames into logic frames.
///
/// The logic runs at `step` seconds per frame (33.25 ms: 2 Mac ticks). A vsynced display delivers pictures at a fixed rate, so the pacer first
/// *measures* that rate (mean of the last 90 frame times: timer jitter cancels out in a mean) and, when a whole number N of display frames
/// is within 5% of one logic frame (60 Hz: N = 2, 120 Hz: 4, 90 Hz: 3, 240 Hz: 8, 144 Hz: 5 - the game then runs 4% slow), it runs one logic
/// frame on every N-th displayed frame by *counting frames*, not time. A timer-based decision drifts against the display (the logic is 0.25% faster
/// than 2 refreshes of a 60 Hz display, which gave a double step every 13 seconds) and is thrown off by any scheduling jitter; counting cannot be.
/// On other refresh rates (75, 100, 165 Hz ...) it falls back to a time accumulator, which keeps the speed right and the cadence uneven.
#[derive(Debug, Clone)]
pub struct Pacer {
    step: f64,
    acc: f64,
    max_steps: u32,
    times: Vec<f64>,
    since_eval: u32,
    /// Display frames per logic frame when locked.
    lock: Option<u32>,
    /// A different lock seen by the last evaluation (needs to be seen twice before it is taken).
    candidate: Option<Option<u32>>,
    frames_since_step: u32,
}

impl Pacer {
    /// `step_seconds`: length of one logic frame. In accumulator mode at most `max_steps` logic frames are run for one displayed frame (a longer
    /// stall is forgiven instead of fast-forwarded).
    pub fn new(step_seconds: f64, max_steps: u32) -> Pacer {
        Pacer { step: step_seconds, acc: 0.0, max_steps: max_steps.max(1), times: Vec::new(), since_eval: 0, lock: None, candidate: None, frames_since_step: 0 }
    }

    /// Forget accumulated time (after a pause or a level change). The measured refresh rate is kept.
    pub fn reset(&mut self) {
        self.acc = 0.0;
        self.frames_since_step = 0;
    }

    /// Display frames per logic frame the pacer is locked to, if it is.
    pub fn locked_to(&self) -> Option<u32> {
        self.lock
    }

    /// The measured display frame time in seconds (mean over the last frames), once there are enough of them.
    pub fn measured_frame_time(&self) -> Option<f64> {
        if self.times.len() >= WINDOW / 2 { Some(self.times.iter().sum::<f64>() / self.times.len() as f64) } else { None }
    }

    /// The display period estimated by a trimmed mean (the middle 60 %) of the recent frame times, once there are enough of them (16): unlike the mean it is not thrown
    /// off by the long frames of start-up (window creation, the title screen, level load) or by a stall, so the pacer can lock within a quarter of
    /// a second instead of after a window of 90 frames (while it is not locked the cadence depends on timer jitter and can hitch).
    pub fn robust_frame_time(&self) -> Option<f64> {
        if self.times.len() < MIN_SAMPLES { return None; }
        let mut v = self.times.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // trimmed mean: the middle 60 %, which ignores outliers like a median but keeps the noise-averaging of a mean
        let cut = v.len() / 5;
        let mid = &v[cut..v.len() - cut];
        Some(mid.iter().sum::<f64>() / mid.len() as f64)
    }

    fn evaluate(&mut self) {
        let Some(m) = self.robust_frame_time() else { return };
        let n = (self.step / m).round();
        let want = if n >= 1.0 && n <= MAX_LOCK as f64 && (n * m - self.step).abs() <= LOCK_TOLERANCE * self.step { Some(n as u32) } else { None };
        if want == self.lock {
            self.candidate = None;
        } else if self.candidate == Some(want) || (self.lock.is_none() && want.is_some()) {
            self.lock = want;
            self.candidate = None;
            self.acc = 0.0;
        } else {
            self.candidate = Some(want);
        }
    }

    /// Time since the previous displayed frame, in seconds -> number of logic frames to run now.
    pub fn advance(&mut self, dt: f64) -> u32 {
        let mut dt = if dt.is_finite() && dt > 0.0 { dt } else { 0.0 };
        let stalled = dt > 0.25;
        if stalled {
            dt = self.step; // window drag, debugger, suspended laptop: do not fast-forward
            self.frames_since_step = 0;
            self.times.clear();
            self.since_eval = 0;
        } else if dt > 0.0 {
            if self.times.len() >= WINDOW {
                self.times.remove(0);
            }
            self.times.push(dt);
            self.since_eval += 1;
            if self.since_eval >= if self.lock.is_none() { EVAL_EVERY_UNLOCKED } else { EVAL_EVERY } {
                self.since_eval = 0;
                self.evaluate();
            }
        }
        if let (Some(n), false) = (self.lock, stalled) {
            self.frames_since_step += 1;
            if self.frames_since_step >= n {
                self.frames_since_step = 0;
                return 1;
            }
            return 0;
        }
        // time accumulator. The measured time is snapped to a multiple of half a logic frame when it is within 3% of one: a 60 Hz display shows a
        // frame every 16.67 ms while half a logic frame is 16.625 ms, and without snapping the 0.25% difference would make the logic run one
        // display frame early about every 13 seconds (only used until the lock is found, and on displays that cannot be locked to).
        let half = self.step * 0.5;
        let m = (dt / half).round();
        if m >= 1.0 && (dt - m * half).abs() < half * 0.08 {
            dt = m * half;
        }
        self.acc += dt;
        let mut n = 0;
        while self.acc >= self.step - 1e-9 {
            self.acc -= self.step;
            n += 1;
            if n >= self.max_steps {
                self.acc = 0.0;
                break;
            }
        }
        if self.acc < 0.0 {
            self.acc = 0.0;
        }
        if n > 0 {
            self.frames_since_step = 0;
        } else {
            self.frames_since_step += 1;
        }
        n
    }
}

/// Samples to queue per logic frame so that the audio queue neither runs dry nor grows: `nominal` samples are produced per logic frame, the device
/// consumes at its own pace (the logic frame is 33.25 ms, 33.33 ms on a locked 60 Hz display, 34.7 ms on a locked 144 Hz one), and the queue holds
/// `queued` samples now against a `target`. A proportional correction of at most +-6% of a frame, stretched over the frame by `resample_linear`
/// (a pitch change far below what can be heard).
pub fn audio_frame_len(nominal: usize, queued: usize, target: usize) -> usize {
    let err = target as i64 - queued as i64;
    let max = (nominal as i64 * 6) / 100;
    let adj = (err / 12).clamp(-max, max);
    (nominal as i64 + adj).max(1) as usize
}

/// `src` stretched or squeezed to `out_len` samples by linear interpolation (unsigned 8 bit samples in, `f32` -1..1 out).
pub fn resample_linear(src: &[u8], out_len: usize, out: &mut Vec<f32>) {
    if src.is_empty() || out_len == 0 {
        return;
    }
    let conv = |b: u8| (b as f32 - 128.0) / 128.0;
    if out_len == src.len() {
        out.extend(src.iter().map(|&b| conv(b)));
        return;
    }
    let ratio = src.len() as f64 / out_len as f64;
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let k = pos.floor() as usize;
        let frac = (pos - k as f64) as f32;
        let a = conv(src[k.min(src.len() - 1)]);
        let b = conv(src[(k + 1).min(src.len() - 1)]);
        out.push(a + (b - a) * frac);
    }
}

/// Where the `w` x `h` picture goes inside a `win_w` x `win_h` window: `(x, y, width, height)` in whole pixels, centred.
/// `integer`: use the largest whole-number scale that fits (every picture pixel becomes the same number of screen pixels: no shimmer when
/// things move); a window smaller than the picture falls back to fitting. Otherwise the largest scale that fits with the aspect ratio kept.
pub fn dest_rect(win_w: i32, win_h: i32, w: i32, h: i32, integer: bool) -> (i32, i32, i32, i32) {
    let (win_w, win_h) = (win_w.max(1), win_h.max(1));
    let s = (win_w / w).min(win_h / h);
    let (dw, dh) = if integer && s >= 1 {
        (w * s, h * s)
    } else {
        let f = (win_w as f64 / w as f64).min(win_h as f64 / h as f64);
        (((w as f64 * f).floor() as i32).max(1), ((h as f64 * f).floor() as i32).max(1))
    };
    ((win_w - dw) / 2, (win_h - dh) / 2, dw, dh)
}

/// Window position (pixels, as the mouse reports it) -> picture coordinates, the inverse of `dest_rect`. The result may lie outside the picture
/// (the black bars); coordinates are rounded down.
pub fn window_to_picture(win_w: i32, win_h: i32, w: i32, h: i32, integer: bool, x: f32, y: f32) -> (i32, i32) {
    let (dx, dy, dw, dh) = dest_rect(win_w, win_h, w, h, integer);
    let px = ((x as f64 - dx as f64) * w as f64 / dw as f64).floor();
    let py = ((y as f64 - dy as f64) * h as f64 / dh as f64).floor();
    (px.clamp(-32768.0, 32767.0) as i32, py.clamp(-32768.0, 32767.0) as i32)
}

/// 0xAARRGGBB pixels -> RGBA bytes (the texture format). Alpha is forced opaque.
pub fn argb_to_rgba(src: &[u32], dst: &mut [u8]) {
    for (p, o) in src.iter().zip(dst.chunks_exact_mut(4)) {
        o[0] = (p >> 16) as u8;
        o[1] = (p >> 8) as u8;
        o[2] = *p as u8;
        o[3] = 0xFF;
    }
}

/// What the frame loop learns about the display, for the diagnostics (`play --stats`, the F5 overlay, `--trace`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayState {
    /// Fewer than half a window of frames measured.
    Measuring,
    /// Vsync works and the pacer is locked: one logic step per `n` display frames (the display runs at `hz`).
    Locked { n: u32, hz: f64 },
    /// The frames come much faster than any display refreshes (`hz` is what the loop achieves): **vsync is not in effect** (driver setting, compositor, ...).
    NoVsync { hz: f64 },
    /// Vsync seems to work but the rate is not a whole fraction of the logic rate (75 / 100 / 165 Hz ...): paced by time, uneven by nature.
    OddRate { hz: f64 },
}

impl Pacer {
    /// Classify the measured display. A window that runs faster than ~250 Hz is not waiting for a display (no display refreshes that fast at the
    /// sizes the game uses): vsync is off.
    pub fn display_state(&self) -> DisplayState {
        let Some(m) = self.robust_frame_time() else { return DisplayState::Measuring };
        let hz = 1.0 / m;
        match self.lock {
            Some(n) => DisplayState::Locked { n, hz },
            None if hz > 250.0 => DisplayState::NoVsync { hz },
            None => DisplayState::OddRate { hz },
        }
    }
}

/// Watches the *displayed* cadence: how many display frames lie between two logic steps and when that was not the usual number. A smooth game at
/// 60 Hz has exactly 2 every time; a gap of 1 or 3 is a visible hitch (the picture advanced early or late). Also counts frames that took far longer
/// than the display period (a missed refresh).
#[derive(Debug, Clone, Default)]
pub struct HitchMeter {
    /// Display frames since the last logic step.
    gap: u32,
    /// The gap that counts as normal (the mode of the last gaps).
    usual: u32,
    recent: [u32; 8],
    n_recent: usize,
    /// Seconds since the meter was created.
    clock: f64,
    /// Odd gaps so far and the (time, gap) of the last few.
    pub odd: u32,
    pub log: Vec<(f64, u32)>,
    /// Frames longer than 1.5 display periods (the display missed a refresh because something took too long or the system stalled).
    pub slow: u32,
    pub steps: u32,
    pub frames: u32,
}

impl HitchMeter {
    pub fn new() -> HitchMeter { HitchMeter::default() }

    /// After a pause or a level change the next gap is not a hitch.
    pub fn reset(&mut self) { self.gap = 0; self.n_recent = 0; }

    /// One displayed frame: `dt` since the previous one, `due` logic steps run on it, `period` = measured display period if known.
    /// Returns `Some(gap)` when this frame ended an odd gap.
    pub fn frame(&mut self, dt: f64, due: u32, period: Option<f64>) -> Option<u32> {
        self.frames += 1;
        self.clock += dt;
        self.gap += 1;
        if let Some(p) = period {
            if dt > 1.5 * p && dt < 0.25 { self.slow += 1; }
        }
        let mut out = None;
        if due > 0 {
            self.steps += due;
            let g = self.gap;
            self.gap = 0;
            if self.n_recent < self.recent.len() {
                self.recent[self.n_recent] = g;
                self.n_recent += 1;
            } else {
                self.recent.rotate_left(1);
                self.recent[7] = g;
            }
            // the usual gap = the most frequent of the last 8
            let mut best = (0, 0);
            for k in 0..self.n_recent {
                let c = self.recent[..self.n_recent].iter().filter(|&&v| v == self.recent[k]).count();
                if c > best.0 { best = (c, self.recent[k]); }
            }
            self.usual = best.1;
            if self.n_recent >= 8 && g != self.usual || due > 1 {
                self.odd += 1;
                if self.log.len() >= 16 { self.log.remove(0); }
                self.log.push((self.clock, g));
                out = Some(g);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: f64 = 0.03325;

    fn run(refresh_hz: f64, seconds: f64) -> Vec<u32> {
        let mut p = Pacer::new(STEP, 5);
        let dt = 1.0 / refresh_hz;
        (0..(seconds * refresh_hz) as usize).map(|_| p.advance(dt)).collect()
    }

    #[test]
    fn sixty_hertz_gives_one_step_every_second_refresh_forever() {
        let v = run(60.0, 120.0);
        assert!(v.iter().all(|&n| n <= 1));
        // after the first frame the pattern is strictly alternating: no early or late step
        for w in v.windows(2).skip(2) {
            assert_eq!(w[0] + w[1], 1, "{:?}", w);
        }
        let total: u32 = v.iter().sum();
        assert_eq!(total, 3600);
    }

    #[test]
    fn sixty_hertz_with_small_timer_noise_stays_even() {
        let mut p = Pacer::new(STEP, 5);
        let mut x = 12345u32;
        let mut steps = Vec::new();
        for _ in 0..3600 {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = ((x >> 16) as f64 / 65536.0 - 0.5) * 0.0004; // +-0.2 ms
            steps.push(p.advance(1.0 / 60.0 + noise));
        }
        for w in steps.windows(2).skip(2) {
            assert_eq!(w[0] + w[1], 1);
        }
    }

    /// Feed a display of `hz` whose frames are timestamped with `jitter` seconds of uniform noise (a noisy clock: the display itself is exact).
    fn jittery(hz: f64, frames: usize, jitter: f64, seed: u32) -> Vec<u32> {
        let mut p = Pacer::new(STEP, 5);
        let mut x = seed;
        let mut prev = 0.0;
        let mut out = Vec::new();
        for k in 1..=frames {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = ((x >> 8) as f64 / (1u32 << 24) as f64 - 0.5) * 2.0 * jitter;
            let t = k as f64 / hz + noise;
            out.push(p.advance(t - prev));
            prev = t;
        }
        out
    }

    fn cadence_is(v: &[u32], from: usize, n: usize) -> bool {
        let steps: Vec<usize> = v.iter().enumerate().skip(from).filter(|(_, &s)| s > 0).map(|(i, _)| i).collect();
        steps.len() > 10 && steps.windows(2).all(|w| w[1] - w[0] == n) && v.iter().skip(from).all(|&s| s <= 1)
    }

    #[test]
    fn a_noisy_clock_cannot_disturb_a_locked_sixty_hertz_cadence() {
        for seed in [1u32, 2, 3, 77] {
            for jitter in [0.0, 0.0005, 0.002, 0.004] {
                let v = jittery(60.0, 6000, jitter, seed);
                assert!(cadence_is(&v, 150, 2), "seed {} jitter {}", seed, jitter);
            }
        }
    }

    #[test]
    fn locks_to_whole_numbers_of_display_frames() {
        for (hz, n) in [(60.0, 2usize), (59.94, 2), (120.0, 4), (90.0, 3), (240.0, 8), (144.0, 5), (30.0, 1)] {
            let v = jittery(hz, 4000, 0.0003, 5);
            assert!(cadence_is(&v, 250, n), "{} Hz", hz);
        }
        let mut p = Pacer::new(STEP, 5);
        for _ in 0..200 {
            p.advance(1.0 / 60.0);
        }
        assert_eq!(p.locked_to(), Some(2));
        assert!((p.measured_frame_time().unwrap() - 1.0 / 60.0).abs() < 1e-9);
    }

    #[test]
    fn displays_that_do_not_divide_evenly_are_paced_by_time() {
        for hz in [75.0, 100.0, 165.0, 1000.0] {
            let mut p = Pacer::new(STEP, 5);
            let mut total = 0u32;
            let frames = (hz * 60.0) as usize;
            for _ in 0..frames {
                total += p.advance(1.0 / hz);
            }
            assert_eq!(p.locked_to(), None, "{} Hz", hz);
            let want = 60.0 / STEP;
            assert!((total as f64 - want).abs() <= want * 0.01 + 1.0, "{} Hz: {} steps, want {}", hz, total, want);
        }
    }

    #[test]
    fn a_new_refresh_rate_is_picked_up_and_a_stall_is_forgiven() {
        let mut p = Pacer::new(STEP, 5);
        let mut v = Vec::new();
        for _ in 0..300 {
            v.push(p.advance(1.0 / 60.0));
        }
        for _ in 0..600 {
            v.push(p.advance(1.0 / 120.0));
        }
        assert!(cadence_is(&v, 300 + 250, 4), "120 Hz after the switch");
        assert_eq!(p.locked_to(), Some(4));
        // a stall in the middle: one step, no fast-forward, the lock survives
        assert_eq!(p.advance(2.0), 1);
        let after: Vec<u32> = (0..400).map(|_| p.advance(1.0 / 120.0)).collect();
        assert!(after.iter().all(|&s| s <= 1));
        assert_eq!(after.iter().sum::<u32>(), 100);
    }

    #[test]
    fn audio_queue_control_keeps_the_queue_near_its_target() {
        // a device that consumes 0.25%, 4% and -2% more than a frame produces
        for (consume, label) in [(1.0025, "60 Hz lock"), (1.042, "144 Hz lock"), (0.98, "fast logic")] {
            let nominal = 740usize;
            let target = 3 * nominal;
            let mut queued = target as f64;
            let mut lo = f64::MAX;
            let mut hi = 0f64;
            for k in 0..20000 {
                let n = audio_frame_len(nominal, queued as usize, target);
                queued += n as f64;
                queued -= nominal as f64 * consume;
                if k > 2000 {
                    lo = lo.min(queued);
                    hi = hi.max(queued);
                }
            }
            assert!(lo > nominal as f64 * 0.8 && hi < nominal as f64 * 6.0, "{}: queue {}..{}", label, lo, hi);
        }
        assert_eq!(audio_frame_len(740, 2220, 2220), 740);
        assert!(audio_frame_len(740, 0, 2220) <= 740 + 44);
        assert!(audio_frame_len(740, 100000, 2220) >= 740 - 44);
    }

    #[test]
    fn linear_resampling_keeps_the_shape_and_the_length() {
        let src: Vec<u8> = (0..100).map(|i| (i * 2) as u8).collect();
        for n in [90usize, 100, 106] {
            let mut out = Vec::new();
            resample_linear(&src, n, &mut out);
            assert_eq!(out.len(), n);
            assert!(out.windows(2).all(|w| w[1] >= w[0] - 1e-6), "monotonic");
            assert!((out[0] - (0.0 - 128.0) / 128.0).abs() < 1e-6);
            assert!(out[n - 1] <= (198.0 - 128.0) / 128.0 + 1e-6);
        }
        let mut out = Vec::new();
        resample_linear(&[], 10, &mut out);
        resample_linear(&src, 0, &mut out);
        assert!(out.is_empty());
        resample_linear(&[128, 255], 4, &mut out);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0], 0.0);
    }

    #[test]
    fn other_refresh_rates_average_the_logic_rate() {
        for hz in [30.0, 50.0, 75.0, 120.0, 144.0, 240.0] {
            let total: u32 = run(hz, 60.0).iter().sum();
            let want = 60.0 / STEP;
            // 144 Hz locks to 5 display frames per logic frame: 4.4% slower, evenly paced
            let tol = if hz == 144.0 { 0.05 } else { 0.01 };
            assert!((total as f64 - want).abs() <= want * tol + 1.0, "{} Hz: {} steps, want {}", hz, total, want);
        }
    }

    #[test]
    fn stalls_are_forgiven() {
        let mut p = Pacer::new(STEP, 5);
        assert_eq!(p.advance(10.0), 1);
        assert_eq!(p.advance(0.2), 5);
        assert_eq!(p.advance(0.0), 0);
        assert_eq!(p.advance(f64::NAN), 0);
        assert_eq!(p.advance(-1.0), 0);
        p.reset();
        assert_eq!(p.advance(0.016), 0);
    }

    #[test]
    fn rect_integer_and_fit() {
        assert_eq!(dest_rect(1280, 960, 640, 480, true), (0, 0, 1280, 960));
        assert_eq!(dest_rect(1920, 1080, 640, 480, true), (320, 60, 1280, 960)); // 2x, letterboxed
        assert_eq!(dest_rect(1920, 1080, 640, 480, false), (240, 0, 1440, 1080));
        assert_eq!(dest_rect(500, 400, 640, 480, true), (0, 12, 500, 375)); // too small: fit
        assert_eq!(dest_rect(500, 400, 640, 480, false), (0, 12, 500, 375));
        assert_eq!(dest_rect(0, 0, 640, 480, true).2 >= 1, true);
        let (x, y, w, h) = dest_rect(1921, 1081, 640, 480, true);
        assert_eq!((x, y, w, h), (320, 60, 1280, 960));
        assert_eq!(x * 2 + w, 1920); // centred to the pixel (odd leftover goes to the right/bottom)
    }

    #[test]
    fn mouse_positions_map_back_into_the_picture() {
        // 2x in a 1920 x 1080 window: the picture starts at (320, 60)
        assert_eq!(window_to_picture(1920, 1080, 640, 480, true, 320.0, 60.0), (0, 0));
        assert_eq!(window_to_picture(1920, 1080, 640, 480, true, 321.9, 61.9), (0, 0));
        assert_eq!(window_to_picture(1920, 1080, 640, 480, true, 322.0, 62.0), (1, 1));
        assert_eq!(window_to_picture(1920, 1080, 640, 480, true, 1599.0, 1019.0), (639, 479));
        assert_eq!(window_to_picture(1920, 1080, 640, 480, true, 100.0, 10.0), (-110, -25)); // in the black bars: outside
        // free scaling: 1440 x 1080 at (240, 0), 2.25x
        assert_eq!(window_to_picture(1920, 1080, 640, 480, false, 240.0 + 2.25 * 100.0, 2.25 * 50.0 + 0.5), (100, 50));
        // dest_rect and window_to_picture agree on every picture pixel (centre of its block)
        for &(ww, wh, int) in &[(640, 480, true), (1280, 960, true), (1000, 700, false), (777, 555, true)] {
            let (dx, dy, dw, dh) = dest_rect(ww, wh, 640, 480, int);
            for &(px, py) in &[(0, 0), (1, 1), (320, 240), (638, 478), (639, 479), (428, 71)] {
                let fx = dx as f32 + (px as f32 + 0.5) * dw as f32 / 640.0;
                let fy = dy as f32 + (py as f32 + 0.5) * dh as f32 / 480.0;
                assert_eq!(window_to_picture(ww, wh, 640, 480, int, fx, fy), (px, py), "{}x{} {} {:?}", ww, wh, int, (px, py));
            }
        }
    }

    #[test]
    fn pixel_conversion() {
        let src = [0xFF11_2233u32, 0x0000_00FF, 0xFFFF_FFFF];
        let mut dst = [0u8; 12];
        argb_to_rgba(&src, &mut dst);
        assert_eq!(dst, [0x11, 0x22, 0x33, 0xFF, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn the_hitch_meter_sees_odd_gaps_only() {
        let mut m = HitchMeter::new();
        for k in 0..400 {
            let due = if k % 2 == 1 { 1 } else { 0 };
            assert_eq!(m.frame(1.0 / 60.0, due, Some(1.0 / 60.0)), None, "frame {}", k);
        }
        assert_eq!((m.odd, m.slow), (0, 0));
        // a step one display frame early
        assert_eq!(m.frame(1.0 / 60.0, 1, Some(1.0 / 60.0)), Some(1));
        assert_eq!(m.odd, 1);
        // a missed refresh
        m.frame(0.034, 0, Some(1.0 / 60.0));
        assert_eq!(m.slow, 1);
    }

    #[test]
    fn display_states_are_told_apart() {
        let mut p = Pacer::new(0.03325, 4);
        assert_eq!(p.display_state(), DisplayState::Measuring);
        for _ in 0..200 { p.advance(1.0 / 60.0); }
        assert!(matches!(p.display_state(), DisplayState::Locked { n: 2, .. }));
        let mut q = Pacer::new(0.03325, 4);
        for _ in 0..200 { q.advance(0.001); }
        assert!(matches!(q.display_state(), DisplayState::NoVsync { .. }));
        let mut r = Pacer::new(0.03325, 4);
        for _ in 0..400 { r.advance(1.0 / 75.0); }
        assert!(matches!(r.display_state(), DisplayState::OddRate { .. }));
    }

    /// The beat that shows up when vsync is NOT in effect: the loop runs freely, the logic steps every 33.25 ms of wall time, and the display shows them
    /// at its own 16.67 ms refreshes, so every ~13 s the step lands one refresh early (a gap of 1 instead of 2). With a working vsync the pacer counts
    /// frames and the gap is 2 forever. (This is what the "stutter about every 10 seconds" looks like; see README, "Stutter".)
    #[test]
    fn without_vsync_the_cadence_beats_with_it_it_does_not() {
        let refresh = 1.0 / 60.0;
        let run = |vsync: bool| -> (u32, u32) {
            let mut p = Pacer::new(0.03325, 4);
            let mut m = HitchMeter::new();
            let (mut t, mut prev, mut shown_last) = (0.0f64, 0.0f64, 0i64);
            let mut odd_shown = 0;
            let mut frame = 0u32;
            while t < 60.0 {
                frame += 1;
                t = if vsync { frame as f64 * refresh } else { t + 0.001 };
                let dt = t - prev;
                prev = t;
                let due = p.advance(dt);
                m.frame(dt, due, p.measured_frame_time());
                if due > 0 {
                    // when the display shows it: the next refresh at or after `t`
                    let shown = (t / refresh - 1e-9).ceil() as i64;
                    if t > 3.0 && shown - shown_last != 2 { odd_shown += 1; }
                    shown_last = shown;
                }
            }
            (odd_shown, m.odd)
        };
        assert_eq!(run(true), (0, 0));
        assert!(run(false).0 >= 4, "{:?}", run(false)); // ~ one per 6.7 s: a "1" and a "3" around each crossing
    }

    /// The start-up of a real run (traces of a 60 Hz machine): a few long frames from loading, then 60 Hz whose frame times alternate 16.4 / 16.9 ms
    /// with one late frame of 17.4 ms somewhere in the first second. The first version of the pacer took ~90 frames to lock and ran in the
    /// time-accumulator mode meanwhile, where a single frame time more than 3 % off a multiple of half a logic frame made a step come one refresh
    /// early (a hitch 1.6 s after the start of the run). Now it locks within ~25 frames and the snapping tolerance is wider.
    #[test]
    fn the_start_up_of_a_real_run_is_even_after_a_quarter_of_a_second() {
        for late_at in 10..70usize {
            let mut p = Pacer::new(STEP, 4);
            for dt in [0.12, 0.09, 0.05, 0.033] { p.advance(dt); }
            let mut steps = Vec::new();
            for k in 0..400usize {
                let dt = if k == late_at { 0.01738 } else if k % 2 == 0 { 0.0164 } else { 0.0169 };
                steps.push(p.advance(dt));
            }
            assert_eq!(p.locked_to(), Some(2));
            assert!(steps.iter().all(|&n| n <= 1));
            let idx: Vec<usize> = steps.iter().enumerate().filter(|(_, &n)| n > 0).map(|(i, _)| i).filter(|&i| i >= 40).collect();
            for w in idx.windows(2) { assert_eq!(w[1] - w[0], 2, "late frame at {}: {:?}", late_at, w); }
        }
    }

    /// The first 120 frame times (ms) of a trace from a real 60 Hz machine (start of a run, after a few long loading frames): one of them (17.381 at
    /// index 33) made the first pacer step twice in a row, a hitch 1.6 s into the game.
    #[test]
    fn the_first_second_of_a_real_trace_is_paced_evenly() {
        const DT_MS: [f64; 120] = [16.686, 16.943, 18.179, 14.885, 16.666, 16.826, 16.516, 16.849, 16.544, 16.946, 16.314, 16.860, 16.545, 16.823, 16.452, 16.861, 16.450, 16.908, 16.514, 16.952, 16.372, 17.073, 16.280, 16.896, 16.397, 16.872, 16.494, 16.876, 16.377, 16.994, 16.384, 16.940, 16.491, 17.381, 16.178, 16.440, 16.899, 16.314, 16.995, 16.398, 16.941, 16.305, 16.969, 16.376, 16.934, 16.513, 16.744, 16.441, 16.964, 16.374, 16.867, 16.469, 16.967, 16.375, 17.011, 16.359, 16.930, 16.480, 16.746, 16.466, 16.911, 16.415, 16.885, 16.492, 16.864, 16.474, 16.911, 16.379, 17.005, 16.326, 16.925, 16.425, 16.911, 16.446, 16.877, 16.406, 16.889, 16.475, 16.951, 16.399, 16.901, 16.495, 17.081, 16.356, 17.483, 15.643, 17.257, 16.142, 17.116, 16.166, 17.783, 15.612, 16.989, 16.294, 16.939, 16.403, 17.090, 16.221, 16.954, 16.414, 16.889, 16.420, 16.977, 16.371, 16.896, 16.446, 16.863, 16.473, 16.916, 16.404, 16.897, 16.477, 16.875, 16.441, 16.943, 16.402, 16.813, 16.543, 16.900, 16.375];
        let mut p = Pacer::new(STEP, 4);
        for dt in [0.12, 0.09, 0.05] { p.advance(dt); }
        let mut steps = Vec::new();
        for ms in DT_MS { steps.push(p.advance(ms / 1000.0)); }
        assert_eq!(p.locked_to(), Some(2));
        let idx: Vec<usize> = steps.iter().enumerate().filter(|(_, &n)| n > 0).map(|(i, _)| i).collect();
        assert!(idx.len() >= 55, "{}", idx.len());
        for w in idx.windows(2) { assert_eq!(w[1] - w[0], 2, "{:?}", w); }
    }
}
