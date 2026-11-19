//! A DJ mixer's tone controls: the three-band isolator EQ (each band cut all the way out) and the
//! one-knob filter (low-pass one way, high-pass the other, flat in the middle).

use std::f64::consts::PI;

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    /// A Butterworth low- or high-pass (RBJ's cookbook).
    fn pass(f: f64, rate: f64, high: bool) -> Biquad {
        let w = 2.0 * PI * f / rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let (b0, b1) = match high {
            true => ((1.0 + cos) / 2.0, -(1.0 + cos)),
            false => ((1.0 - cos) / 2.0, 1.0 - cos),
        };
        Biquad {
            b0: (b0 / a0) as f32,
            b1: (b1 / a0) as f32,
            b2: (b0 / a0) as f32,
            a1: (-2.0 * cos / a0) as f32,
            a2: ((1.0 - alpha) / a0) as f32,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// A Linkwitz-Riley crossover (two Butterworths each way): the two halves add back up flat.
#[derive(Clone, Copy)]
pub(crate) struct Split {
    lo: [Biquad; 2],
    hi: [Biquad; 2],
}

impl Split {
    pub(crate) fn new(f: f64, rate: f64) -> Split {
        let lo = Biquad::pass(f, rate, false);
        let hi = Biquad::pass(f, rate, true);
        Split {
            lo: [lo, lo],
            hi: [hi, hi],
        }
    }

    #[inline]
    pub(crate) fn run(&mut self, x: f32) -> (f32, f32) {
        let lo = self.lo[0].run(x);
        let lo = self.lo[1].run(lo);
        let hi = self.hi[0].run(x);
        let hi = self.hi[1].run(hi);
        (lo, hi)
    }
}

/// Where the isolator splits, Hz: lows, mids, highs.
pub const LOW_MID: f64 = 250.0;
pub const MID_HIGH: f64 = 2500.0;

/// One side of the isolator.
#[derive(Clone, Copy)]
struct Bands {
    a: Split,
    b: Split,
    /// The lows through the mid/high crossover too, so they keep step with the rest.
    align: Split,
}

/// The three-band isolator: each band's gain from 0 (gone) up, unity giving back what went in.
pub struct Isolator {
    sides: [Bands; 2],
    gains: [f32; 3],
}

impl Isolator {
    pub fn new(rate: f64) -> Isolator {
        let bands = Bands {
            a: Split::new(LOW_MID, rate),
            b: Split::new(MID_HIGH, rate),
            align: Split::new(MID_HIGH, rate),
        };
        Isolator {
            sides: [bands, bands],
            gains: [1.0; 3],
        }
    }

    /// A block through it, the gains gliding to `target` across it.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], target: [f32; 3]) {
        let n = l.len().max(1) as f32;
        let from = self.gains;
        for (side, s) in self.sides.iter_mut().zip([&mut *l, &mut *r]) {
            for (k, x) in s.iter_mut().enumerate() {
                let t = (k + 1) as f32 / n;
                let g = |i: usize| from[i] + (target[i] - from[i]) * t;
                let (lo, rest) = side.a.run(*x);
                let (mid, high) = side.b.run(rest);
                let (lo_a, lo_b) = side.align.run(lo);
                *x = (lo_a + lo_b) * g(0) + mid * g(1) + high * g(2);
            }
        }
        self.gains = target;
    }
}

/// An EQ knob's turn (0 to 1, 0.5 the middle) as a gain: the middle is unity, the top +6 dB,
/// the bottom silence (a kill), the lower half falling off as a DJ mixer's does.
pub fn eq_gain(knob: f32) -> f32 {
    let k = knob.clamp(0.0, 1.0);
    match k >= 0.5 {
        true => 10f32.powf((k - 0.5) * 12.0 / 20.0),
        false => (k * 2.0).powi(3),
    }
}

/// The one-knob filter: -1 to 1, low-pass turned left, high-pass right, dry in the middle. Each
/// is a state-variable filter (trapezoidal, so it sweeps without zipper or blow-up), both always
/// running, the one not turned resting wide open, so turning either way starts clean.
pub struct OneKnob {
    rate: f64,
    knob: f64,
    /// Low-pass then high-pass, each side's two integrators.
    ic: [[[f64; 2]; 2]; 2],
}

/// Either side of the middle this much, the filter's not heard.
const DEAD: f64 = 0.02;
/// Coefficients are worked out this often, frames.
const EVERY: usize = 8;

#[derive(Clone, Copy)]
struct Svf {
    k: f64,
    a1: f64,
    a2: f64,
    a3: f64,
}

impl Svf {
    fn new(f: f64, q: f64, rate: f64) -> Svf {
        let g = (PI * f.min(rate * 0.45) / rate).tan();
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        Svf {
            k,
            a1,
            a2: g * a1,
            a3: g * g * a1,
        }
    }

    /// One sample through: (low, high).
    #[inline]
    fn run(&self, ic: &mut [f64; 2], x: f64) -> (f64, f64) {
        let v3 = x - ic[1];
        let v1 = self.a1 * ic[0] + self.a2 * v3;
        let v2 = ic[1] + self.a2 * ic[0] + self.a3 * v3;
        ic[0] = 2.0 * v1 - ic[0];
        ic[1] = 2.0 * v2 - ic[1];
        (v2, x - self.k * v1 - v2)
    }
}

impl OneKnob {
    pub fn new(rate: f64) -> OneKnob {
        OneKnob {
            rate,
            knob: 0.0,
            ic: [[[0.0; 2]; 2]; 2],
        }
    }

    /// A block through it, the knob gliding to `target`; `res` is its resonance, 0 to 1.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], target: f64, res: f64) {
        let n = l.len();
        let from = self.knob;
        let q0 = std::f64::consts::FRAC_1_SQRT_2;
        let mut k = 0;
        while k < n {
            let v = from + (target - from) * ((k + EVERY).min(n) as f64 / n as f64);
            let depth = ((v.abs() - DEAD) / (1.0 - DEAD)).clamp(0.0, 1.0);
            let q = q0 + res.clamp(0.0, 1.0) * 3.0 * depth.sqrt();
            let (lp, hp) = match v > 0.0 {
                true => (
                    Svf::new(20_000.0, q0, self.rate),
                    Svf::new(20.0 * 2f64.powf(depth * 9.0), q, self.rate),
                ),
                false => (
                    Svf::new(20_000.0 * 2f64.powf(-depth * 9.0), q, self.rate),
                    Svf::new(20.0, q0, self.rate),
                ),
            };
            // Into the dead zone and out, faded with the dry sound.
            let wet = ((v.abs() - DEAD) / (2.0 * DEAD)).clamp(0.0, 1.0) as f32;
            for i in k..(k + EVERY).min(n) {
                for (side, s) in [&mut *l, &mut *r].into_iter().enumerate() {
                    let x = s[i] as f64;
                    let (low, _) = lp.run(&mut self.ic[0][side], x);
                    let (_, high) = hp.run(&mut self.ic[1][side], x);
                    let y = if v > 0.0 { high } else { low };
                    s[i] = s[i] * (1.0 - wet) + y as f32 * wet;
                }
            }
            k += EVERY;
        }
        self.knob = target;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(f: f64, rate: f64, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * f * i as f64 / rate).sin() as f32 * 0.5)
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// Flat, the isolator gives back each tone at its level; a band killed, it's gone.
    #[test]
    fn isolator_flat_and_kills() {
        let rate = 48_000.0;
        for f in [60.0, 250.0, 800.0, 2500.0, 8000.0] {
            let x = tone(f, rate, 48_000);
            let (mut l, mut r) = (x.clone(), x.clone());
            let mut eq = Isolator::new(rate);
            eq.process(&mut l, &mut r, [1.0; 3]);
            let db = 20.0 * (rms(&l[24_000..]) / rms(&x[24_000..])).log10();
            assert!(db.abs() < 0.1, "{f} Hz flat: {db} dB");
        }
        let x = tone(60.0, rate, 48_000);
        let (mut l, mut r) = (x.clone(), x.clone());
        let mut eq = Isolator::new(rate);
        // Gains glide across a block: set first, then heard.
        eq.process(&mut [0.0; 64], &mut [0.0; 64], [0.0, 1.0, 1.0]);
        eq.process(&mut l, &mut r, [0.0, 1.0, 1.0]);
        let db = 20.0 * (rms(&l[24_000..]) / rms(&x[24_000..])).log10();
        assert!(db < -20.0, "low killed: {db} dB");
        assert_eq!(eq_gain(0.5), 1.0);
        assert_eq!(eq_gain(0.0), 0.0);
        assert!((20.0 * eq_gain(1.0).log10() - 6.0).abs() < 1e-4);
    }

    /// Turned left, highs go; turned right, lows go; the middle leaves it alone.
    #[test]
    fn one_knob_sweeps() {
        let rate = 48_000.0;
        let level = |f: f64, knob: f64| {
            let x = tone(f, rate, 24_000);
            let (mut l, mut r) = (x.clone(), x.clone());
            let mut flt = OneKnob::new(rate);
            flt.knob = knob;
            flt.process(&mut l, &mut r, knob, 0.0);
            20.0 * (rms(&l[12_000..]) / rms(&x[12_000..])).log10()
        };
        assert!(level(5000.0, 0.0).abs() < 0.01);
        assert!(level(5000.0, -0.7) < -20.0);
        assert!(level(100.0, -0.7).abs() < 1.0);
        assert!(level(100.0, 0.7) < -20.0);
        assert!(level(8000.0, 0.7).abs() < 1.0);
    }
}
