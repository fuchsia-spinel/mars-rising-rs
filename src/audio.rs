//! The original's sound engine, modelled sample for sample (std-only, no audio device).
//!
//! Verified against the original code run in the emulator: `tests/audio_golden.rs` replays `tests/vectors/audio_vectors.txt`
//! (see `tools/re/gen_audio_vectors.py`), which drives the original routines 0x45a38/0x45a6c/0x45bac/0x45ca4/0x45d54/0x45e24
//! (the game-level API), 0x51b4c/0x51d2c (stop / is-playing) and the buffer callback 0x52030 -> 0x52294 (the mono mixer).
//!
//! How the original works (all of it is reproduced here):
//! * 72 sound effects, `snd ` resources 10000..10071, widened to signed 16 bit (`byte - 128`) and zero padded to a multiple of 1024
//!   samples ("chunks"). The header holds the chunk count and the sample rate as a 16.16 fixed point number.
//! * The engine is initialised mono with 6 mixed voices (`0xba7c`: `init(6)`), 16 voice slots, 1024-sample buffers and a default output
//!   rate of 0x56EE8BA3 (22254.545 Hz) -- the rate the sounds are recorded at, so they play 1:1.
//! * A request (`0x51844`) becomes a voice. A voice's position is a 16.8 accumulator advanced by `step >> 8` per output sample, with
//!   `step = FixMul(FixDiv(sound rate >> 8, output rate >> 8), playback rate)`; the sample index is `acc >> 8`. When the position has not
//!   moved since the previous output sample (slowed-down playback) the sample is averaged with the next one. The voice lasts
//!   `max(1, floor(chunks / step))` buffers.
//! * New voices are inserted by priority: a voice is placed before the first existing voice that its priority *and* its volume sum are
//!   not lower than; a full table (16) drops the request, or the last voice when the new one outranks one of them. The handle is the
//!   request's own (never set by the game) or a counter that starts at 1 and grows by 2.
//! * Only the first 6 voices are mixed (the others still advance, silently). Each contributes `(sample * volume) >> 7` (volume 128 =
//!   full), the sum is clamped to +-127 and 128 added: unsigned 8 bit output.
//! * The game-level `play(id, priority, delay)` clamps the priority to 1..5, queues delayed sounds (5 slots, played by the per-frame
//!   pump once `due <= tick`) and plays them at once when the queue is full. Everything is refused when the sound option is off
//!   (`D.1be4`, set by the sound menu item; init sets it) or while the display mode byte `D.13e3` is 1.
//! * A request may carry a callback id and a parameter (the over-charge stream of 0x2766c, the enemy streams of 0x4123c); the original
//!   calls it with 1 when the voice ends and 2 when it is stopped. Here these are collected in [`Mixer::events`].
//!
//! Toolbox arithmetic (the emulator's mocks in `gen_audio_vectors.py` use the same definitions, so the golden test cannot check them):
//! `FixMul` rounds to nearest with ties up -- documented for the PowerPC routines in Apple Technical Q&A OPS 20 -- and `FixDiv` is
//! assumed to truncate toward zero (no source found). The assumption only matters for the five sounds recorded at 11127 Hz (half
//! the output rate, where a one-unit difference changes the playback speed by 1/128) and is inaudible.
//! Not modelled: stereo output (the original runs mono), the Sound Manager's double-buffer latency and the master volume (the original
//! only passes it to `SetDefaultOutputVolume`; it is not part of the mix).
use std::sync::Arc;

/// Output samples per buffer.
pub const BUFFER: usize = 1024;
/// Voice slots.
pub const MAX_VOICES: usize = 16;
/// Sound effects (`snd ` 10000..10071).
pub const SOUND_COUNT: usize = 72;
/// Voices that are actually mixed (`init(6)`).
pub const MIX_VOICES: usize = 6;
/// Default output rate, 16.16 fixed (22254.545 Hz).
pub const DEFAULT_OUT_RATE: u32 = 0x56EE_8BA3;
/// 1.0 in 16.16 fixed point.
pub const UNIT: u32 = 0x1_0000;
/// Output samples per game frame (2 ticks of 1/60.147 s at 22254.545 Hz = exactly 740).
pub const SAMPLES_PER_FRAME: usize = 740;
/// Callback id the over-charge stream (`0x2766c`) is registered with; the parameter is the player number.
pub const CB_OVERCHARGE: u32 = 0x1d60;
/// Callback id of the enemy streams (`0x4123c`); the parameter is the enemy record index.
pub const CB_ENEMY: u32 = 0x1e18;

/// Toolbox `FixDiv` (signed 16.16): truncates toward zero, saturates.
pub fn fix_div(a: u32, b: u32) -> u32 {
    let (a, b) = (a as i32 as i64, b as i32 as i64);
    if b == 0 { return if a >= 0 { 0x7fff_ffff } else { 0x8000_0000 }; }
    let q = (a << 16) / b;
    q.clamp(i32::MIN as i64, i32::MAX as i64) as i32 as u32
}

/// Toolbox `FixMul` (signed 16.16): rounds to nearest.
pub fn fix_mul(a: u32, b: u32) -> u32 {
    let p = (a as i32 as i64) * (b as i32 as i64);
    ((p + 0x8000) >> 16) as i32 as u32
}

/// One loaded sound: the samples and their rate.
#[derive(Debug, Clone)]
pub struct Sample {
    /// Buffer count, `ceil(len / 1024)`, at least 1.
    pub chunks: u32,
    /// Sample rate, 16.16 fixed.
    pub rate: u32,
    data: Vec<i16>,
}

impl Sample {
    /// `pcm` are the widened samples (`byte - 128`), `rate` the 16.16 sample rate.
    pub fn new(pcm: &[i16], rate: u32) -> Sample {
        let chunks = (((pcm.len() + 1023) >> 10) as u32).max(1);
        let mut data = pcm.to_vec();
        data.resize(chunks as usize * BUFFER + 2, 0); // the original's allocation is zero filled
        Sample { chunks, rate, data }
    }
    pub fn len(&self) -> usize { self.data.len() }
    pub fn is_empty(&self) -> bool { self.data.is_empty() }
    fn at(&self, i: u32) -> i32 { self.data.get(i as usize).copied().unwrap_or(0) as i32 }
}

#[derive(Debug, Clone)]
struct Voice {
    handle: u32,
    sample: Arc<Sample>,
    remaining: u32,
    step: u32,
    acc: u32,
    cur: u32,
    prev: Option<u32>,
    callback: u32,
    param: u32,
    vol_a: u16, // voice +44: the volume the mono mixer uses
    vol_b: u16, // voice +46
    prio: u16,
}

/// A callback the original would have made: `sel` 1 = the voice ended, 2 = it was stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub sel: u32,
    pub callback: u32,
    pub param: u32,
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    id: i8, // -1 = free
    due: u32,
    prio: u8,
}

/// The sound engine.
#[derive(Debug, Clone)]
pub struct Mixer {
    samples: Vec<Option<Arc<Sample>>>,
    voices: Vec<Voice>,
    out_rate: u32,
    counter: u32,
    queue: [Slot; 5],
    tick: u32,
    /// The sound option (`D.1be4`): sounds are only started while it is on.
    pub sound_on: bool,
    /// The display mode byte `D.13e3` equals 1: the game refuses to start sounds.
    pub suppressed: bool,
    /// Voices mixed per buffer.
    pub max_mix: usize,
    /// Callbacks made since the last [`Mixer::take_events`].
    pub events: Vec<Event>,
    /// The parameter block of the original's callback glue (`D.11c8`, +16) keeps the parameter of the last voice that had a callback;
    /// the "stopped" callback (selector 2) is made without refreshing it, so it sees that stale value.
    cb_param: u32,
    buf: [u8; BUFFER],
    buf_pos: usize,
}

impl Mixer {
    /// A mixer with the game's settings: 6 mixed voices, output rate `out_rate` (usually [`DEFAULT_OUT_RATE`]), sound on.
    pub fn new(out_rate: u32) -> Mixer {
        Mixer {
            samples: vec![None; SOUND_COUNT], voices: Vec::new(), out_rate, counter: 1,
            queue: [Slot { id: -1, due: 0, prio: 0 }; 5], tick: 0, sound_on: true, suppressed: false, max_mix: MIX_VOICES,
            events: Vec::new(), cb_param: 0, buf: [128; BUFFER], buf_pos: BUFFER,
        }
    }

    /// Install sound effect `id` (0..71).
    pub fn set_sample(&mut self, id: usize, s: Sample) { if id < SOUND_COUNT { self.samples[id] = Some(Arc::new(s)); } }
    /// Like [`Mixer::set_sample`] for an already shared sample (a restarted game reuses the loaded sounds).
    pub fn set_sample_arc(&mut self, id: usize, s: Arc<Sample>) { if id < SOUND_COUNT { self.samples[id] = Some(s); } }
    pub fn has_sample(&self, id: usize) -> bool { id < SOUND_COUNT && self.samples[id].is_some() }
    pub fn voice_count(&self) -> usize { self.voices.len() }
    /// `(handle, remaining buffers)` of every voice, in slot order.
    pub fn voice_list(&self) -> Vec<(u32, u32)> { self.voices.iter().map(|v| (v.handle, v.remaining)).collect() }
    pub fn take_events(&mut self) -> Vec<Event> { std::mem::take(&mut self.events) }

    fn gated(&self) -> bool { self.suppressed || !self.sound_on }

    fn sample_for(&self, id: i32) -> Option<Arc<Sample>> {
        let id = id as u8 as i8 as i32; // the original sign-extends the low byte
        if id < 0 || id >= SOUND_COUNT as i32 { return None; }
        self.samples[id as usize].clone()
    }

    fn id_ok(id: i32) -> bool { let id = id as u8 as i8 as i32; id >= 0 && id < SOUND_COUNT as i32 }

    fn clamp_prio(p: i32) -> u16 { let p = p as u8; if (1..=5).contains(&p) { p as u16 } else { 1 } }

    /// 0x45a38: empty the delay queue.
    pub fn clear_queue(&mut self) { self.queue = [Slot { id: -1, due: 0, prio: 0 }; 5]; }

    /// 0x45a6c, once per frame: advance the tick and play the queued sounds that are due.
    pub fn frame(&mut self, now: u32) {
        self.tick = now;
        for i in 0..5 {
            let s = self.queue[i];
            if s.id == -1 || s.due > now { continue; }
            self.queue[i].id = -1;
            if s.id >= 0 && (s.id as i32) < SOUND_COUNT as i32 {
                let sample = self.samples[s.id as usize].clone();
                self.start_voice(sample, 0, UNIT, 0, 0, s.prio as u16, 256, 256);
            }
        }
    }

    /// 0x45bac: play a sound effect; `delay` frames later when non-zero (and a queue slot is free).
    pub fn play(&mut self, id: i32, prio: i32, delay: u8) {
        if self.gated() { return; }
        let prio = Self::clamp_prio(prio);
        if !Self::id_ok(id) { return; }
        if delay != 0 {
            for s in self.queue.iter_mut() {
                if s.id == -1 {
                    *s = Slot { id: id as u8 as i8, due: (delay as u32).wrapping_add(self.tick), prio: prio as u8 };
                    return;
                }
            }
        }
        let sample = self.sample_for(id);
        self.start_voice(sample, 0, UNIT, 0, 0, prio, 256, 256);
    }

    /// 0x45ca4: start a sound and return its handle (0 when refused).
    pub fn start(&mut self, id: i32, prio: i32) -> u32 {
        if self.gated() || !Self::id_ok(id) { return 0; }
        let prio = Self::clamp_prio(prio);
        let sample = self.sample_for(id);
        self.start_voice(sample, 0, UNIT, 0, 0, prio, 256, 256)
    }

    /// 0x45d54: start a sound at a playback-rate multiplier (16.16; 0x10000 = normal, the game uses 0x9000 for player 2's warning).
    pub fn start_rate(&mut self, id: i32, prio: i32, rate: u32) {
        if self.gated() || !Self::id_ok(id) { return; }
        let prio = Self::clamp_prio(prio);
        let sample = self.sample_for(id);
        self.start_voice(sample, 0, rate, 0, 0, prio, 256, 256);
    }

    /// 0x45e24: start a sound with a callback (`callback` != 0, e.g. [`CB_OVERCHARGE`]) and its parameter; returns the handle.
    pub fn start_callback(&mut self, id: i32, prio: i32, callback: u32, param: u32) -> u32 {
        if self.gated() || !Self::id_ok(id) { return 0; }
        let prio = Self::clamp_prio(prio);
        let sample = self.sample_for(id);
        self.start_voice(sample, 0, UNIT, callback, param, prio, 256, 256)
    }

    /// 0x51844: turn a request into a voice. Returns the handle, 0 when the request is refused.
    #[allow(clippy::too_many_arguments)]
    fn start_voice(&mut self, sample: Option<Arc<Sample>>, req_handle: u32, rate: u32, callback: u32, param: u32, prio: u16, vol_r: u16, vol_l: u16) -> u32 {
        let sample = match sample { Some(s) => s, None => return 0 };
        let prio = prio.max(1);
        let vol_l = vol_l.min(128);
        let vol_r = vol_r.min(128);
        // priority insertion
        let mut j = 0;
        while j < self.voices.len() && j < MAX_VOICES {
            let v = &self.voices[j];
            if prio >= v.prio && (vol_l as i32 + vol_r as i32) >= (v.vol_a as i32 + v.vol_b as i32) { break; }
            j += 1;
        }
        if j >= MAX_VOICES { return 0; }
        if self.voices.len() >= MAX_VOICES { self.voices.truncate(MAX_VOICES - 1); }
        let ratio = fix_div(sample.rate >> 8, self.out_rate >> 8);
        let step = fix_mul(ratio, rate);
        let remaining = (fix_div(sample.chunks << 16, step) >> 16).max(1);
        let handle = if req_handle != 0 { req_handle } else { let h = self.counter; self.counter = self.counter.wrapping_add(2); h };
        let vol_a = if vol_l > vol_r { vol_l } else { vol_r }; // mono: the louder side
        self.voices.insert(j.min(self.voices.len()), Voice {
            handle, sample, remaining, step, acc: 0, cur: 0, prev: None, callback, param, vol_a, vol_b: vol_l, prio,
        });
        handle
    }

    /// 0x51d2c: how many voices have this handle (0 = all voices).
    pub fn playing(&self, handle: u32) -> u32 {
        self.voices.iter().filter(|v| v.handle == handle || handle == 0).count() as u32
    }

    /// 0x51b4c: stop the voice(s) with this handle (0 = all).
    pub fn stop(&mut self, handle: u32) {
        let mut i = 0;
        while i < self.voices.len() {
            if self.voices[i].handle == handle || handle == 0 {
                let v = self.voices.remove(i);
                if v.callback != 0 { self.events.push(Event { sel: 2, callback: v.callback, param: self.cb_param }); }
            } else {
                i += 1;
            }
        }
    }

    /// 0x52030: mix one buffer (mono, unsigned 8 bit) and retire the voices whose time is up.
    pub fn mix_buffer(&mut self) -> [u8; BUFFER] {
        let mut out = [128u8; BUFFER];
        let max_mix = self.max_mix;
        for o in out.iter_mut() {
            let mut sum: i32 = 0;
            for (i, v) in self.voices.iter_mut().enumerate() {
                if i < max_mix {
                    let mut s = v.sample.at(v.cur);
                    if v.prev == Some(v.cur) { s = (s + v.sample.at(v.cur + 1)) >> 1; }
                    v.prev = Some(v.cur);
                    sum += (s * v.vol_a as i32) >> 7;
                }
                v.acc = v.acc.wrapping_add(v.step >> 8);
                v.cur = (v.acc >> 8) & 0x00ff_ffff;
            }
            *o = (sum.clamp(-127, 127) + 128) as u8;
        }
        let mut i = 0;
        while i < self.voices.len() {
            if self.voices[i].callback != 0 { self.cb_param = self.voices[i].param; }
            self.voices[i].remaining -= 1;
            if self.voices[i].remaining == 0 {
                let v = self.voices.remove(i);
                if v.callback != 0 { self.events.push(Event { sel: 1, callback: v.callback, param: v.param }); }
            } else {
                i += 1;
            }
        }
        out
    }

    /// Stream `out.len()` samples (unsigned 8 bit), mixing a new buffer whenever the previous one is used up.
    pub fn render(&mut self, out: &mut [u8]) {
        for o in out.iter_mut() {
            if self.buf_pos >= BUFFER {
                self.buf = self.mix_buffer();
                self.buf_pos = 0;
            }
            *o = self.buf[self.buf_pos];
            self.buf_pos += 1;
        }
    }
}

/// FNV-1a 64 over bytes (used by the golden vectors).
pub fn fnv64(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &x in b { h = (h ^ x as u64).wrapping_mul(0x100_0000_01b3); }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(n: usize) -> Vec<i16> { (0..n).map(|i| ((i % 100) as i16) - 50).collect() }

    fn mixer() -> Mixer {
        let mut m = Mixer::new(DEFAULT_OUT_RATE);
        m.set_sample(3, Sample::new(&ramp(3000), DEFAULT_OUT_RATE));
        m.set_sample(5, Sample::new(&ramp(1500), DEFAULT_OUT_RATE));
        m
    }

    #[test]
    fn fixed_point_helpers() {
        assert_eq!(fix_mul(UNIT, 0x9000), 0x9000);
        assert_eq!(fix_mul(0x8000, 0x8000), 0x4000);
        assert_eq!(fix_div(UNIT, 0x2_0000), 0x8000);
        assert_eq!(fix_div(3 << 16, 0x2_0000), 0x1_8000);
        assert_eq!(fix_div(UNIT, 0), 0x7fff_ffff);
    }

    #[test]
    fn a_sound_lasts_its_chunk_count_and_plays_one_to_one() {
        let mut m = mixer();
        let h = m.start(3, 2);
        assert_eq!(h, 1);
        assert_eq!(m.voice_list(), vec![(1, 3)]); // 3000 samples -> 3 chunks
        let b = m.mix_buffer();
        for i in 0..BUFFER { assert_eq!(b[i] as i32 - 128, ramp(3000)[i] as i32); }
        m.mix_buffer();
        assert_eq!(m.playing(h), 1);
        m.mix_buffer();
        assert_eq!(m.playing(h), 0);
        assert_eq!(m.start(5, 1), 3); // handles grow by 2
    }

    #[test]
    fn half_speed_averages_repeated_samples_and_lasts_twice_as_long() {
        let mut m = mixer();
        m.start_rate(3, 2, 0x8000);
        assert_eq!(m.voice_list(), vec![(1, 6)]);
        let b = m.mix_buffer();
        let r = ramp(3000);
        assert_eq!(b[0] as i32 - 128, r[0] as i32); // first sample is not averaged
        assert_eq!(b[1] as i32 - 128, (r[0] as i32 + r[1] as i32) >> 1);
        assert_eq!(b[2] as i32 - 128, r[1] as i32);
    }

    #[test]
    fn delayed_sounds_wait_for_their_tick_and_a_full_queue_plays_at_once() {
        let mut m = mixer();
        m.frame(10);
        m.play(3, 3, 5);
        m.frame(14);
        assert_eq!(m.voice_count(), 0);
        m.frame(15);
        assert_eq!(m.voice_count(), 1);
        for _ in 0..6 { m.play(5, 3, 9); }
        assert_eq!(m.voice_count(), 2); // five queued, the sixth started at once
    }

    #[test]
    fn refusals() {
        let mut m = mixer();
        m.sound_on = false;
        assert_eq!(m.start(3, 1), 0);
        m.sound_on = true;
        m.suppressed = true;
        assert_eq!(m.start(3, 1), 0);
        m.suppressed = false;
        assert_eq!(m.start(72, 1), 0);
        assert_eq!(m.start(-1, 1), 0);
        assert_eq!(m.start(4, 1), 0); // no sample loaded
        assert_eq!(m.voice_count(), 0);
    }

    #[test]
    fn priority_insertion_and_overflow() {
        let mut m = mixer();
        for _ in 0..MAX_VOICES { assert_ne!(m.start(3, 3), 0); }
        assert_eq!(m.voice_count(), 16);
        assert_eq!(m.start(5, 2), 0); // lower priority than all 16: dropped
        let h = m.start(5, 4);
        assert_ne!(h, 0);
        assert_eq!(m.voice_count(), 16);
        assert_eq!(m.voice_list()[0].0, h); // placed first, the last voice fell off
    }

    #[test]
    fn callbacks_on_end_and_stop() {
        let mut m = mixer();
        let a = m.start_callback(5, 2, CB_OVERCHARGE, 1);
        let b = m.start_callback(3, 2, CB_OVERCHARGE, 2);
        // the "stopped" callback sees the parameter block's stale value (nothing has been mixed yet: 0), not the voice's own parameter
        m.stop(b);
        assert_eq!(m.take_events(), vec![Event { sel: 2, callback: CB_OVERCHARGE, param: 0 }]);
        m.mix_buffer();
        m.mix_buffer();
        assert_eq!(m.playing(a), 0);
        assert_eq!(m.take_events(), vec![Event { sel: 1, callback: CB_OVERCHARGE, param: 1 }]);
        // after a buffer was mixed the stale value is the last callback voice's parameter
        let c = m.start_callback(3, 2, CB_OVERCHARGE, 2);
        m.mix_buffer();
        m.stop(c);
        assert_eq!(m.take_events(), vec![Event { sel: 2, callback: CB_OVERCHARGE, param: 2 }]);
    }

    #[test]
    fn mixing_clamps_and_only_six_voices_count() {
        let mut m = Mixer::new(DEFAULT_OUT_RATE);
        m.set_sample(1, Sample::new(&vec![100i16; 1024], DEFAULT_OUT_RATE));
        for _ in 0..8 { m.start(1, 3); }
        let b = m.mix_buffer();
        assert_eq!(b[0], 255); // clamped to 127 + 128
        let mut m = Mixer::new(DEFAULT_OUT_RATE);
        m.set_sample(1, Sample::new(&vec![10i16; 1024], DEFAULT_OUT_RATE));
        for _ in 0..8 { m.start(1, 3); }
        assert_eq!(m.mix_buffer()[0], 128 + 60); // 6 of the 8 voices
    }

    #[test]
    fn render_streams_across_buffers() {
        let mut m = mixer();
        m.start(3, 2);
        let mut out = vec![0u8; 3 * BUFFER + 5];
        m.render(&mut out);
        assert_eq!(out[3 * BUFFER + 1], 128); // sound over: silence
        assert_eq!(out[10] as i32 - 128, ramp(3000)[10] as i32);
        assert_eq!(m.voice_count(), 0);
    }
}
