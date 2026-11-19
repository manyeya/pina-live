//! Keylock where Signalsmith Stretch can't go (a web build: it's C++): a time-stretch in plain
//! Rust, by WSOLA. Grains a little over 20 ms long are cut from the track, each laid half over
//! the last, and each cut where it best lines up with how the last one goes on (so they add
//! without phasing); read faster or slower than they're laid, they move the pitch. It has the
//! calls the decks use of Signalsmith's: a block in, a block out, the pitch shift, and a seek.
//!
//! Its latency is fixed, so where a deck is heard is known to the sample: the newest frame fed
//! in is `input_latency() + output_latency() * speed` ahead of the one heard.

/// A grain's length, and how far apart they're laid (half: Hann windows half over each other
/// add to one).
const GRAIN: usize = 1024;
const HOP: usize = GRAIN / 2;
/// How far either way a grain may be moved to line up with the last.
const TOLERANCE: usize = 256;
/// The most the pitch is moved by (two octaves either way).
const PITCH_MAX: f64 = 4.0;

pub struct Stretch {
    channels: usize,
    /// The input kept: `input[i]` is frame `base + i` (each a frame's channels, in turn).
    input: Vec<f32>,
    base: usize,
    fed: usize,
    /// Where in the input the next output frame comes from, and the output frame it is.
    x: f64,
    out_at: usize,
    /// Grains laid but not yet given back: `ola[i]` is output frame `ola_base + i`.
    ola: Vec<f32>,
    ola_base: usize,
    /// The next grain's place in the output, and the last grain's start in the input.
    next_grain: usize,
    last_start: Option<f64>,
    pitch: f64,
    window: Vec<f32>,
}

impl Stretch {
    /// Signalsmith's name for it: `channels` channels at `rate` (the grain's a fixed length).
    pub fn preset_default(channels: u32, _rate: u32) -> Stretch {
        let window = (0..GRAIN)
            .map(|i| {
                let t = i as f32 / GRAIN as f32;
                0.5 - 0.5 * (std::f32::consts::TAU * t).cos()
            })
            .collect();
        Stretch {
            channels: channels.max(1) as usize,
            input: Vec::new(),
            base: 0,
            fed: 0,
            x: 0.0,
            out_at: 0,
            ola: Vec::new(),
            ola_base: 0,
            next_grain: 0,
            last_start: None,
            pitch: 1.0,
            window,
        }
    }

    /// Input frames between the newest fed in and the grain heard: room for the longest grain
    /// read (at the highest pitch) and its search either way.
    pub fn input_latency(&self) -> usize {
        (GRAIN as f64 * PITCH_MAX / 2.0) as usize + TOLERANCE
    }

    /// Output frames a grain's middle is behind its start.
    pub fn output_latency(&self) -> usize {
        GRAIN / 2
    }

    pub fn set_transpose_factor_semitones(&mut self, semis: f32, _tonality: Option<f32>) {
        self.pitch = 2f64
            .powf(semis as f64 / 12.0)
            .clamp(1.0 / PITCH_MAX, PITCH_MAX);
    }

    /// Starts over from `input` (interleaved), heard from `input_latency()` before its end on,
    /// at `_rate` (the speed comes with each block).
    pub fn seek(&mut self, input: &[f32], _rate: f64) {
        self.input = input.to_vec();
        self.base = 0;
        self.fed = input.len() / self.channels;
        self.x = self.fed as f64 - self.input_latency() as f64;
        self.out_at = 0;
        self.ola.clear();
        self.ola_base = 0;
        self.next_grain = 0;
        self.last_start = None;
    }

    /// One block: `input` (interleaved) in, `output` filled; their lengths are the speed (more
    /// in than out plays faster).
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let ch = self.channels;
        let (k, m) = (input.len() / ch, output.len() / ch);
        self.input.extend_from_slice(input);
        self.fed += k;
        if m == 0 {
            return;
        }
        let ratio = k as f64 / m as f64;
        let (start, end) = (self.out_at, self.out_at + m);
        let x0 = self.x;
        // Every grain that reaches into this block, laid.
        while self.next_grain < end {
            let o = self.next_grain;
            let centre = x0 + (o as f64 - start as f64) * ratio;
            let natural = centre - GRAIN as f64 * self.pitch / 2.0;
            let at = self.line_up(natural);
            self.lay(o, at);
            self.last_start = Some(at);
            self.next_grain += HOP;
        }
        // This block's frames, given back; what's before them let go.
        for (i, frame) in output.chunks_mut(ch).enumerate() {
            let j = (start + i - self.ola_base) * ch;
            for (c, v) in frame.iter_mut().enumerate() {
                *v = self.ola.get(j + c).copied().unwrap_or(0.0);
            }
        }
        let gone = (end - self.ola_base).min(self.ola.len() / ch);
        self.ola.drain(..gone * ch);
        self.ola_base += gone;
        self.out_at = end;
        self.x = x0 + m as f64 * ratio;
        // Input no grain can reach back to any more.
        let keep_from =
            (self.x - (GRAIN as f64 * PITCH_MAX) - 2.0 * TOLERANCE as f64).max(0.0) as usize;
        if keep_from > self.base {
            let drop = (keep_from - self.base).min(self.input.len() / ch);
            self.input.drain(..drop * ch);
            self.base += drop;
        }
    }

    /// Input frame `at` (between frames, read between them), channel `c`.
    fn read(&self, at: f64, c: usize) -> f32 {
        if at < self.base as f64 {
            return 0.0;
        }
        let i = at.floor();
        let f = (at - i) as f32;
        let k = i as usize - self.base;
        let get = |k: usize| {
            self.input
                .get(k * self.channels + c)
                .copied()
                .unwrap_or(0.0)
        };
        let (a, b) = (get(k), get(k + 1));
        a + (b - a) * f
    }

    /// The channels summed at `at`: what's lined up.
    fn mono(&self, at: f64) -> f32 {
        (0..self.channels).map(|c| self.read(at, c)).sum()
    }

    /// Where near `natural` the grain starts: where its first half best matches how the last
    /// grain goes on (its second half's continuation), searched either way within `TOLERANCE`.
    fn line_up(&self, natural: f64) -> f64 {
        let Some(last) = self.last_start else {
            return natural;
        };
        let p = self.pitch;
        let goes_on = |i: usize| self.mono(last + (HOP + i) as f64 * p);
        let target: Vec<f32> = (0..HOP).step_by(8).map(goes_on).collect();
        let mut best = (f32::MIN, 0isize);
        let tol = TOLERANCE as isize;
        for d in (-tol..=tol).step_by(4) {
            let at = natural + d as f64;
            let mut sum = 0.0;
            let mut norm = 1e-9;
            for (n, t) in target.iter().enumerate() {
                let v = self.mono(at + (n * 8) as f64 * p);
                sum += v * t;
                norm += v * v;
            }
            let score = sum / norm.sqrt();
            if score > best.0 {
                best = (score, d);
            }
        }
        natural + best.1 as f64
    }

    /// A grain read from input `at` (at the pitch) laid into the output at frame `o`, windowed.
    fn lay(&mut self, o: usize, at: f64) {
        let ch = self.channels;
        let need = (o + GRAIN - self.ola_base) * ch;
        if self.ola.len() < need {
            self.ola.resize(need, 0.0);
        }
        for i in 0..GRAIN {
            let w = self.window[i];
            let from = at + i as f64 * self.pitch;
            let j = (o + i - self.ola_base) * ch;
            for c in 0..ch {
                self.ola[j + c] += self.read(from, c) * w;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tone's frequency, by its upward zero crossings over `x`.
    fn freq(x: &[f32], rate: f64) -> f64 {
        let ups = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        ups as f64 * rate / x.len() as f64
    }

    /// 440 Hz played at `speed` and moved `semis`: what comes out, after it's settled.
    fn run(speed: f64, semis: f32) -> Vec<f32> {
        let rate = 48_000.0;
        let tone = |i: usize| (std::f64::consts::TAU * 440.0 * i as f64 / rate).sin() as f32;
        let mut st = Stretch::preset_default(1, rate as u32);
        st.set_transpose_factor_semitones(semis, None);
        let pre = st.input_latency() * 2 + 64;
        let input: Vec<f32> = (0..pre).map(tone).collect();
        st.seek(&input, speed);
        let mut read = pre;
        let mut acc = 0.0;
        let mut out = Vec::new();
        for _ in 0..1500 {
            let m = 64;
            acc += speed * m as f64;
            let k = acc.floor() as usize;
            acc -= k as f64;
            let block: Vec<f32> = (read..read + k).map(tone).collect();
            read += k;
            let mut o = vec![0.0; m];
            st.process(&block, &mut o);
            out.extend(o);
        }
        out.split_off(out.len() / 2)
    }

    #[test]
    fn a_tone_keeps_its_pitch_at_any_speed() {
        for speed in [0.7, 1.0, 1.3] {
            let f = freq(&run(speed, 0.0), 48_000.0);
            assert!((f - 440.0).abs() < 6.0, "{speed}: {f}");
        }
    }

    #[test]
    fn a_shift_moves_the_pitch_and_nothing_else() {
        let f = freq(&run(1.0, 12.0), 48_000.0);
        assert!((f - 880.0).abs() < 12.0, "{f}");
        // Steady: no grain lands out of step (no level dips between them).
        let out = run(1.15, 0.0);
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let parts: Vec<f32> = out.chunks(2048).map(rms).collect();
        let (lo, hi) = parts
            .iter()
            .fold((f32::MAX, 0f32), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(lo > hi * 0.8, "{lo} {hi}");
    }
}
