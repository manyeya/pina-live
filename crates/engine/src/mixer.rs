//! The mixer: a channel per deck (trim and auto gain, the isolator, the filter, the fader, its
//! side of the crossfader, cue), the crossfader, the master through Pina Studio's limiter, and
//! the headphones' mix of what's cued with the master.

use pina_engine::{
    dsp::{Clock, limit::Limiting},
    song::project::Limiter,
};
use serde::{Deserialize, Serialize};

use crate::{
    dsp::{Isolator, OneKnob, eq_gain},
    fx::FxUnit,
};

/// Which side of the crossfader a channel is on; `Thru` passes it by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    A,
    #[default]
    Thru,
    B,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MixCmd {
    Trim {
        ch: usize,
        db: f32,
    },
    /// The track's own gain, to bring it to the others' loudness.
    AutoGain {
        ch: usize,
        db: f32,
    },
    /// A band's knob, 0 to 1 (0.5 flat, 0 a kill).
    Eq {
        ch: usize,
        band: usize,
        knob: f32,
    },
    /// -1 (low-pass) to 1 (high-pass), 0 off.
    Filter {
        ch: usize,
        knob: f32,
    },
    Fader {
        ch: usize,
        v: f32,
    },
    Side {
        ch: usize,
        side: Side,
    },
    Cue {
        ch: usize,
        on: bool,
    },
    /// -1 (all A) to 1 (all B).
    Crossfader(f32),
    /// 0 smooth (constant power) to 1 a sharp cut, for scratching.
    CrossfaderCurve(f32),
    /// 0 a fast cut-in, 0.5 even, 1 slow.
    FaderCurve(f32),
    MasterDb(f32),
    /// 0 only what's cued in the headphones, 1 only the master.
    CueMix(f32),
    PhonesDb(f32),
    /// The filter's resonance, 0 to 1.
    FilterRes(f32),
}

pub(crate) struct Channel {
    trim: f32,
    auto: f32,
    eq: [f32; 3],
    filter: f32,
    fader: f32,
    side: Side,
    cue: bool,
    iso: Isolator,
    flt: OneKnob,
    /// The gain into the channel and out of it, as last heard (glided between blocks).
    gain_in: f32,
    gain_out: f32,
    /// The loudest sample since the status last went out, each side, before the fader.
    pub(crate) peak: [f32; 2],
}

impl Channel {
    fn new(rate: f64) -> Channel {
        Channel {
            trim: 1.0,
            auto: 1.0,
            eq: [0.5; 3],
            filter: 0.0,
            fader: 1.0,
            side: Side::Thru,
            cue: false,
            iso: Isolator::new(rate),
            flt: OneKnob::new(rate),
            gain_in: 1.0,
            gain_out: 1.0,
            peak: [0.0; 2],
        }
    }
}

pub(crate) struct Mixer {
    pub(crate) chans: Vec<Channel>,
    xf: f32,
    xf_curve: f32,
    fader_curve: f32,
    master: f32,
    master_now: f32,
    cue_mix: f32,
    phones: f32,
    res: f32,
    limiting: Limiting,
    limit: Limiter,
    pub(crate) master_peak: [f32; 2],
    pub(crate) phones_peak: [f32; 2],
    pub(crate) reduction: f32,
    // A block's worth: what's cued, and the master before it's limited.
    cl: Vec<f32>,
    cr: Vec<f32>,
}

impl Mixer {
    pub(crate) fn new(rate: f64, channels: usize, block: usize) -> Mixer {
        Mixer {
            chans: (0..channels).map(|_| Channel::new(rate)).collect(),
            xf: 0.0,
            xf_curve: 0.0,
            fader_curve: 0.6,
            master: 1.0,
            master_now: 1.0,
            cue_mix: 0.0,
            phones: 1.0,
            res: 0.3,
            limiting: Limiting::new(rate),
            // A master that never clips: clean, quick, a hair under full scale.
            limit: Limiter {
                ceiling_db: -0.3,
                lookahead_ms: 1.0,
                attack_ms: 0.8,
                release_ms: 80.0,
                ..Limiter::default()
            },
            master_peak: [0.0; 2],
            phones_peak: [0.0; 2],
            reduction: 0.0,
            cl: vec![0.0; block],
            cr: vec![0.0; block],
        }
    }

    pub(crate) fn command(&mut self, cmd: MixCmd) {
        let db = |d: f32| 10f32.powf(d / 20.0);
        let n = self.chans.len();
        match cmd {
            MixCmd::Trim { ch, db: d } if ch < n => self.chans[ch].trim = db(d.clamp(-24.0, 12.0)),
            MixCmd::AutoGain { ch, db: d } if ch < n => {
                self.chans[ch].auto = db(d.clamp(-24.0, 12.0))
            }
            MixCmd::Eq { ch, band, knob } if ch < n && band < 3 => {
                self.chans[ch].eq[band] = knob.clamp(0.0, 1.0)
            }
            MixCmd::Filter { ch, knob } if ch < n => self.chans[ch].filter = knob.clamp(-1.0, 1.0),
            MixCmd::Fader { ch, v } if ch < n => self.chans[ch].fader = v.clamp(0.0, 1.0),
            MixCmd::Side { ch, side } if ch < n => self.chans[ch].side = side,
            MixCmd::Cue { ch, on } if ch < n => self.chans[ch].cue = on,
            MixCmd::Crossfader(x) => self.xf = x.clamp(-1.0, 1.0),
            MixCmd::CrossfaderCurve(c) => self.xf_curve = c.clamp(0.0, 1.0),
            MixCmd::FaderCurve(c) => self.fader_curve = c.clamp(0.0, 1.0),
            MixCmd::MasterDb(d) => self.master = db(d.clamp(-60.0, 6.0)),
            MixCmd::CueMix(m) => self.cue_mix = m.clamp(0.0, 1.0),
            MixCmd::PhonesDb(d) => self.phones = db(d.clamp(-60.0, 6.0)),
            MixCmd::FilterRes(r) => self.res = r.clamp(0.0, 1.0),
            _ => {}
        }
    }

    /// The channels' sound (`ins`, a pair each) mixed: the master into `ml`/`mr`, the headphones
    /// into `pl`/`pr`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn process(
        &mut self,
        ins: &mut [[Vec<f32>; 2]],
        fx: &mut [FxUnit],
        clock: Clock,
        ml: &mut [f32],
        mr: &mut [f32],
        pl: &mut [f32],
        pr: &mut [f32],
    ) {
        let n = ml.len();
        ml.fill(0.0);
        mr.fill(0.0);
        let (cl, cr) = (&mut self.cl[..n], &mut self.cr[..n]);
        cl.fill(0.0);
        cr.fill(0.0);
        let mut cued = false;
        for (ch, (c, [l, r])) in self.chans.iter_mut().zip(ins.iter_mut()).enumerate() {
            let (l, r) = (&mut l[..n], &mut r[..n]);
            let gain_in = c.trim * c.auto;
            glide(l, r, c.gain_in, gain_in);
            c.gain_in = gain_in;
            c.iso.process(l, r, c.eq.map(eq_gain));
            c.flt.process(l, r, c.filter as f64, self.res as f64);
            for (s, x) in [&*l, &*r].into_iter().enumerate() {
                c.peak[s] = x.iter().fold(c.peak[s], |m, v| m.max(v.abs()));
            }
            if c.cue {
                cued = true;
                for k in 0..n {
                    cl[k] += l[k];
                    cr[k] += r[k];
                }
            }
            let gain_out =
                fader_gain(c.fader, self.fader_curve) * side_gain(c.side, self.xf, self.xf_curve);
            glide(l, r, c.gain_out, gain_out);
            c.gain_out = gain_out;
            // The FX after the fader: an echo rings on as the fader comes down.
            for unit in fx.iter_mut() {
                let s = match unit.on[ch] {
                    true => unit.settings,
                    false => crate::fx::FxSettings {
                        on: false,
                        ..unit.settings
                    },
                };
                unit.inserts[ch].process(&s, clock, l, r);
            }
            for k in 0..n {
                ml[k] += l[k];
                mr[k] += r[k];
            }
        }
        glide(ml, mr, self.master_now, self.master);
        self.master_now = self.master;
        self.reduction = self
            .reduction
            .max(self.limiting.process(&self.limit, ml, mr, true));
        // The headphones: what's cued, against the master by the cue mix (all master with
        // nothing cued).
        let m = if cued { self.cue_mix } else { 1.0 };
        let (gc, gm) = ((1.0 - m) * self.phones, m * self.phones);
        for k in 0..n {
            pl[k] = cl[k] * gc + ml[k] * gm;
            pr[k] = cr[k] * gc + mr[k] * gm;
        }
        for (s, x) in [&*ml, &*mr].into_iter().enumerate() {
            self.master_peak[s] = x.iter().fold(self.master_peak[s], |m, v| m.max(v.abs()));
        }
        for (s, x) in [&*pl, &*pr].into_iter().enumerate() {
            self.phones_peak[s] = x.iter().fold(self.phones_peak[s], |m, v| m.max(v.abs()));
        }
    }
}

fn glide(l: &mut [f32], r: &mut [f32], from: f32, to: f32) {
    if from == to && to == 1.0 {
        return;
    }
    let n = l.len() as f32;
    for (k, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
        let g = from + (to - from) * ((k + 1) as f32 / n);
        *a *= g;
        *b *= g;
    }
}

/// A channel fader's position as a gain: `curve` 0.5 an even taper, toward 0 cutting in fast
/// (a scratch fader), toward 1 slow (most of its travel in the top).
pub fn fader_gain(v: f32, curve: f32) -> f32 {
    let exp = 4f32.powf(curve * 2.0 - 1.0) * 1.5;
    v.clamp(0.0, 1.0).powf(exp)
}

/// A channel's gain by the crossfader at `x` (-1 all A, 1 all B): `curve` 0 is constant power,
/// 1 a sharp cut a hair from the far end.
pub fn side_gain(side: Side, x: f32, curve: f32) -> f32 {
    let t = (x.clamp(-1.0, 1.0) + 1.0) / 2.0;
    let away = match side {
        Side::Thru => return 1.0,
        Side::A => t,
        Side::B => 1.0 - t,
    };
    let smooth = (away * std::f32::consts::FRAC_PI_2).cos();
    let sharp = ((1.0 - away) / 0.04).clamp(0.0, 1.0);
    smooth * (1.0 - curve) + sharp * curve
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossfader_curves() {
        // Constant power: full at its own end, gone at the other, -3 dB in the middle.
        assert!((side_gain(Side::A, -1.0, 0.0) - 1.0).abs() < 1e-6);
        assert!(side_gain(Side::A, 1.0, 0.0).abs() < 1e-6);
        let mid = 20.0 * side_gain(Side::B, 0.0, 0.0).log10();
        assert!((mid + 3.01).abs() < 0.05, "{mid}");
        // Sharp: full until the very end.
        assert_eq!(side_gain(Side::A, 0.9, 1.0), 1.0);
        assert_eq!(side_gain(Side::A, 1.0, 1.0), 0.0);
        assert_eq!(side_gain(Side::Thru, 1.0, 1.0), 1.0);
        assert_eq!(fader_gain(1.0, 0.3), 1.0);
        assert_eq!(fader_gain(0.0, 0.3), 0.0);
        assert!(fader_gain(0.5, 0.0) > fader_gain(0.5, 1.0));
    }
}
