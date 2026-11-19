//! Beat FX: two units, each an effect in time with the leading deck (its length in beats), a
//! level and one more control, switched on for whichever channels it's assigned to. They sit
//! after a channel's fader, so an echo or reverb carries on as the fader comes down, and goes on
//! ringing out (its trails) when the effect's switched off. The echoes, reverbs, flanger,
//! phaser, filter sweep and pitch shifts are Pina Studio's own effects; trans, crush and noise
//! are DJ tools made here.

use pina_engine::{
    dsp::{
        Clock, Running,
        delay::Delaying,
        pitch::{Pitching, Stopping},
        reverb::Reverbing,
        tools::{Filtering, Moving},
    },
    song::project::{
        Delay, DelayStyle, FilterFx, FilterMode, Motion, MotionStyle, PitchShift, Reverb,
        ReverbStyle, TapeStop, Unit, Wave,
    },
};
use serde::{Deserialize, Serialize};

pub const UNITS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FxKind {
    #[default]
    Echo,
    TapeEcho,
    DubEcho,
    PingPong,
    Reverb,
    Shimmer,
    Flanger,
    Phaser,
    Sweep,
    Trans,
    Pitch,
    PitchEcho,
    Crush,
    TapeStop,
    Noise,
}

impl FxKind {
    pub const ALL: [FxKind; 15] = [
        FxKind::Echo,
        FxKind::TapeEcho,
        FxKind::DubEcho,
        FxKind::PingPong,
        FxKind::Reverb,
        FxKind::Shimmer,
        FxKind::Flanger,
        FxKind::Phaser,
        FxKind::Sweep,
        FxKind::Trans,
        FxKind::Pitch,
        FxKind::PitchEcho,
        FxKind::Crush,
        FxKind::TapeStop,
        FxKind::Noise,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FxKind::Echo => "Echo",
            FxKind::TapeEcho => "Tape Echo",
            FxKind::DubEcho => "Dub Echo",
            FxKind::PingPong => "Ping Pong",
            FxKind::Reverb => "Reverb",
            FxKind::Shimmer => "Shimmer",
            FxKind::Flanger => "Flanger",
            FxKind::Phaser => "Phaser",
            FxKind::Sweep => "Filter Sweep",
            FxKind::Trans => "Trans",
            FxKind::Pitch => "Pitch",
            FxKind::PitchEcho => "Pitch Echo",
            FxKind::Crush => "Crush",
            FxKind::TapeStop => "Tape Stop",
            FxKind::Noise => "Noise",
        }
    }

    /// What its second knob does.
    pub fn param_name(self) -> &'static str {
        match self {
            FxKind::Echo | FxKind::TapeEcho | FxKind::DubEcho | FxKind::PingPong => "Feedback",
            FxKind::Reverb | FxKind::Shimmer => "Size",
            FxKind::Flanger | FxKind::Phaser => "Feedback",
            FxKind::Sweep => "Cutoff",
            FxKind::Trans => "Gap",
            FxKind::Pitch | FxKind::PitchEcho => "Pitch",
            FxKind::Crush => "Bits",
            FxKind::TapeStop => "Curve",
            FxKind::Noise => "Sweep",
        }
    }

    /// Where its second knob rests.
    pub fn param_rest(self) -> f32 {
        match self {
            // A fifth up: the stacks rise in a chord.
            FxKind::PitchEcho => 0.5 + 7.0 / 24.0,
            FxKind::Trans => 0.5,
            FxKind::Crush => 0.6,
            _ => 0.5,
        }
    }

    /// Rings on when switched off, nothing new going in: echoes and reverbs.
    pub fn trails(self) -> bool {
        matches!(
            self,
            FxKind::Echo
                | FxKind::TapeEcho
                | FxKind::DubEcho
                | FxKind::PingPong
                | FxKind::Reverb
                | FxKind::Shimmer
                | FxKind::PitchEcho
        )
    }

    pub fn default_beats(self) -> f64 {
        match self {
            FxKind::Echo | FxKind::TapeEcho | FxKind::PingPong => 0.75,
            FxKind::DubEcho => 1.5,
            FxKind::Reverb | FxKind::Shimmer => 1.0,
            FxKind::Flanger | FxKind::Phaser | FxKind::Sweep => 8.0,
            FxKind::Trans => 0.25,
            FxKind::Pitch | FxKind::PitchEcho | FxKind::Crush | FxKind::Noise => 1.0,
            FxKind::TapeStop => 2.0,
        }
    }

    /// A pitch knob's semitones: none in the middle, an octave down or up at its ends.
    fn semitones(p: f32) -> f32 {
        ((p.clamp(0.0, 1.0) - 0.5) * 24.0).round()
    }
}

/// A unit as it's set.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FxSettings {
    pub kind: FxKind,
    pub on: bool,
    pub beats: f64,
    /// How much is heard, 0 to 1.
    pub level: f32,
    /// Its second knob, 0 to 1.
    pub param: f32,
}

impl Default for FxSettings {
    fn default() -> Self {
        FxSettings {
            kind: FxKind::Echo,
            on: false,
            beats: FxKind::Echo.default_beats(),
            level: 0.5,
            param: 0.5,
        }
    }
}

enum Proc {
    Pina(Box<dyn Running>),
    Trans { open: f32 },
    Crush { held: [f32; 2], count: f64 },
    Noise { seed: u32, ic: [[f64; 2]; 2] },
}

/// One unit's effect on one channel, running. Made off the audio thread (an effect's memory is
/// allocated) and handed in.
pub struct Insert {
    kind: FxKind,
    proc: Proc,
    wet: [Vec<f32>; 2],
    level: f32,
    was_on: bool,
    /// Frames it's been quiet, ringing out: past a second, it stops.
    quiet: usize,
    rate: f64,
}

impl std::fmt::Debug for Insert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Insert({:?})", self.kind)
    }
}

impl Insert {
    pub fn new(kind: FxKind, rate: f64, block: usize) -> Insert {
        let proc = match kind {
            FxKind::Echo | FxKind::TapeEcho | FxKind::DubEcho | FxKind::PingPong => {
                Proc::Pina(Box::new(Delaying::new(rate)))
            }
            FxKind::Reverb | FxKind::Shimmer => Proc::Pina(Box::new(Reverbing::new(rate))),
            FxKind::Flanger | FxKind::Phaser => Proc::Pina(Box::new(Moving::new(rate))),
            FxKind::Sweep => Proc::Pina(Box::new(Filtering::new(rate))),
            FxKind::Pitch | FxKind::PitchEcho => Proc::Pina(Box::new(Pitching::new())),
            FxKind::TapeStop => Proc::Pina(Box::new(Stopping::new(rate))),
            FxKind::Trans => Proc::Trans { open: 1.0 },
            FxKind::Crush => Proc::Crush {
                held: [0.0; 2],
                count: 0.0,
            },
            FxKind::Noise => Proc::Noise {
                seed: 0x9e37_79b9,
                ic: [[0.0; 2]; 2],
            },
        };
        Insert {
            kind,
            proc,
            wet: [vec![0.0; block], vec![0.0; block]],
            level: 0.0,
            was_on: false,
            quiet: usize::MAX,
            rate,
        }
    }

    pub fn kind(&self) -> FxKind {
        self.kind
    }

    /// Pina Studio's effect for this kind, set: its time in beats, its second knob, and how much
    /// of it is heard (`mix`).
    fn unit(&self, s: &FxSettings, mix: f32) -> Unit {
        let b = s.beats as f32;
        let p = s.param.clamp(0.0, 1.0);
        let delay = |style: DelayStyle| {
            Unit::Delay(Delay {
                style,
                sync: true,
                beats: [b, b],
                feedback: p * 0.95,
                mix,
                ..Delay::default()
            })
        };
        match self.kind {
            FxKind::Echo => delay(DelayStyle::Digital),
            FxKind::TapeEcho => delay(DelayStyle::Tape),
            FxKind::DubEcho => delay(DelayStyle::Dub),
            FxKind::PingPong => delay(DelayStyle::PingPong),
            FxKind::Reverb | FxKind::Shimmer => Unit::Reverb(Reverb {
                style: if self.kind == FxKind::Shimmer {
                    ReverbStyle::Shimmer
                } else {
                    ReverbStyle::Hall
                },
                predelay_beats: Some(b / 16.0),
                size: 0.3 + p * 0.7,
                decay_s: 0.8 + p * 7.0,
                mix,
                ..Reverb::default()
            }),
            FxKind::Flanger | FxKind::Phaser => Unit::Motion(Motion {
                style: if self.kind == FxKind::Flanger {
                    MotionStyle::Flanger
                } else {
                    MotionStyle::Phaser
                },
                beats: b,
                depth: 0.9,
                feedback: p * 0.9,
                mix,
                ..Motion::default()
            }),
            FxKind::Sweep => Unit::Filter(FilterFx {
                mode: FilterMode::LowPass,
                cutoff_hz: 200.0 * 2f32.powf(p * 6.0),
                resonance: 0.55,
                lfo_depth: 3.0,
                lfo_wave: Wave::Triangle,
                lfo_beats: b,
                mix,
                ..FilterFx::default()
            }),
            FxKind::Pitch | FxKind::PitchEcho => Unit::Pitch(PitchShift {
                semitones: FxKind::semitones(p),
                feedback: if self.kind == FxKind::PitchEcho {
                    0.6
                } else {
                    0.0
                },
                mix,
                ..PitchShift::default()
            }),
            FxKind::TapeStop => Unit::TapeStop(TapeStop {
                stop: if s.on { 1.0 } else { 0.0 },
                stop_beats: b,
                start_beats: (b / 2.0).max(0.25),
                curve: p,
                mix: 1.0,
                ..TapeStop::default()
            }),
            _ => Unit::Pitch(PitchShift::default()),
        }
    }

    /// A block of a channel through it, in place.
    pub fn process(&mut self, s: &FxSettings, clock: Clock, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        let on = s.on && s.kind == self.kind;
        let trails = self.kind.trails();
        let ringing = trails && self.quiet < self.rate as usize;
        // A tape stop runs on after it's let go, to come back up to speed.
        let running =
            on || ringing || self.level > 0.0 || (self.kind == FxKind::TapeStop && self.was_on);
        if !running {
            self.was_on = false;
            return;
        }
        if on && !self.was_on {
            self.reset();
        }
        let target = if on { s.level.clamp(0.0, 1.0) } else { 0.0 };
        let (from, to) = (self.level, target);
        match trails {
            // Echoes and reverbs: what goes in, wet only, added to the sound; switched off,
            // nothing goes in but what's ringing rings out (at the level it had).
            true => {
                let lvl = if on { to } else { from.max(to) };
                let unit = self.unit(s, 1.0);
                let [wl, wr] = &mut self.wet;
                for k in 0..n {
                    (wl[k], wr[k]) = if on { (l[k], r[k]) } else { (0.0, 0.0) };
                }
                if let Proc::Pina(p) = &mut self.proc {
                    p.run(&unit, clock, &mut wl[..n], &mut wr[..n]);
                }
                let mut peak = 0f32;
                for k in 0..n {
                    let g = from + (lvl - from) * ((k + 1) as f32 / n as f32);
                    l[k] += wl[k] * g;
                    r[k] += wr[k] * g;
                    peak = peak.max(wl[k].abs()).max(wr[k].abs());
                }
                self.level = if on { lvl } else { from };
                self.quiet = match (on, peak < 1e-4) {
                    (true, _) | (_, false) => 0,
                    (false, true) => self.quiet.saturating_add(n),
                };
                if !on && self.quiet >= self.rate as usize {
                    self.level = 0.0;
                }
            }
            // The rest act on the sound itself, faded in and out by their level.
            false => {
                self.level = to;
                let mix = if self.kind == FxKind::TapeStop {
                    1.0
                } else {
                    to.max(from)
                };
                let unit = self.unit(s, mix);
                match &mut self.proc {
                    Proc::Pina(p) => {
                        p.run(&unit, clock, l, r);
                    }
                    Proc::Trans { open } => {
                        let beats = s.beats.max(1.0 / 64.0);
                        let per_frame = clock.bpm / 60.0 / self.rate;
                        let gap = 0.15 + s.param.clamp(0.0, 1.0) as f64 * 0.7;
                        for k in 0..n {
                            let at = ((clock.beats + k as f64 * per_frame) / beats).rem_euclid(1.0);
                            let want = if at < 1.0 - gap { 1.0 } else { 0.0 };
                            // 2 ms edges: a gate, not a click.
                            *open +=
                                (want - *open) * (1.0 - (-1.0 / (0.002 * self.rate as f32)).exp());
                            let g = 1.0 - to * (1.0 - *open);
                            l[k] *= g;
                            r[k] *= g;
                        }
                    }
                    Proc::Crush { held, count } => {
                        let bits = 3.0 + s.param.clamp(0.0, 1.0) * 9.0;
                        let q = 2f32.powf(bits);
                        // Held for longer at shorter "beats": 1 beat keeps every 4th sample.
                        let hold = (4.0 / s.beats.max(0.0625)).clamp(1.0, 32.0);
                        for k in 0..n {
                            *count += 1.0;
                            if *count >= hold {
                                *count -= hold;
                                *held = [(l[k] * q).round() / q, (r[k] * q).round() / q];
                            }
                            l[k] = l[k] * (1.0 - to) + held[0] * to;
                            r[k] = r[k] * (1.0 - to) + held[1] * to;
                        }
                    }
                    Proc::Noise { seed, ic } => {
                        // White noise up through a high-pass the knob sweeps: a riser.
                        let f = 200.0 * 2f64.powf(s.param.clamp(0.0, 1.0) as f64 * 6.0);
                        let g = (std::f64::consts::PI * f / self.rate).tan();
                        let kq = 1.0 / 0.9;
                        let a1 = 1.0 / (1.0 + g * (g + kq));
                        let (a2, a3) = (g * a1, g * g * a1);
                        for k in 0..n {
                            for (side, x) in [&mut l[k], &mut r[k]].into_iter().enumerate() {
                                *seed ^= *seed << 13;
                                *seed ^= *seed >> 17;
                                *seed ^= *seed << 5;
                                let w = (*seed as f64 / u32::MAX as f64) * 2.0 - 1.0;
                                let [c1, c2] = &mut ic[side];
                                let v3 = w - *c2;
                                let v1 = a1 * *c1 + a2 * v3;
                                let v2 = *c2 + a2 * *c1 + a3 * v3;
                                *c1 = 2.0 * v1 - *c1;
                                *c2 = 2.0 * v2 - *c2;
                                let high = w - kq * v1 - v2;
                                *x += (high * 0.25) as f32 * to;
                            }
                        }
                    }
                }
            }
        }
        self.was_on = on;
    }

    fn reset(&mut self) {
        if let Proc::Pina(p) = &mut self.proc {
            p.reset();
        }
        self.quiet = 0;
    }
}

/// A unit: as it's set, which channels it's on, and its effect on each.
pub struct FxUnit {
    pub settings: FxSettings,
    pub on: Vec<bool>,
    pub inserts: Vec<Box<Insert>>,
}

impl FxUnit {
    pub fn new(channels: usize, rate: f64, block: usize) -> FxUnit {
        let settings = FxSettings::default();
        FxUnit {
            settings,
            on: vec![false; channels],
            inserts: (0..channels)
                .map(|_| Box::new(Insert::new(settings.kind, rate, block)))
                .collect(),
        }
    }
}

/// What the FX are told.
#[derive(Debug)]
pub enum FxCmd {
    Settings {
        unit: usize,
        settings: FxSettings,
    },
    /// A new effect for a unit: one insert per channel, made off the audio thread.
    Kind {
        unit: usize,
        inserts: Vec<Box<Insert>>,
    },
    Assign {
        unit: usize,
        ch: usize,
        on: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect()
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|v| v * v).sum::<f32>()
    }

    /// An echo, switched off, rings out; another kind of effect, off, leaves the sound alone.
    #[test]
    fn echoes_trail_and_the_rest_get_out_of_the_way() {
        let rate = 48_000.0;
        let clock = Clock {
            bpm: 120.0,
            beats: 0.0,
        };
        let mut echo = Insert::new(FxKind::Echo, rate, 512);
        let mut s = FxSettings {
            kind: FxKind::Echo,
            on: true,
            beats: 0.25,
            level: 1.0,
            param: 0.6,
        };
        for _ in 0..40 {
            let (mut l, mut r) = (tone(512), tone(512));
            echo.process(&s, clock, &mut l, &mut r);
        }
        s.on = false;
        // Silence in: the echoes still come out.
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        echo.process(&s, clock, &mut l, &mut r);
        assert!(energy(&l) > 1.0, "trails: {}", energy(&l));
        // In time they're gone, and it stops running.
        for _ in 0..400 {
            let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
            echo.process(&s, clock, &mut l, &mut r);
        }
        let (mut l, mut r) = (tone(512), tone(512));
        echo.process(&s, clock, &mut l, &mut r);
        assert_eq!(l, tone(512));

        let mut trans = Insert::new(FxKind::Trans, rate, 512);
        let t = FxSettings {
            kind: FxKind::Trans,
            on: true,
            beats: 0.25,
            level: 1.0,
            param: 0.5,
        };
        let (mut l, mut r) = (tone(48_000), tone(48_000));
        for k in (0..48_000).step_by(512) {
            let e = (k + 512).min(48_000);
            let c = Clock {
                bpm: 120.0,
                beats: k as f64 / 24_000.0,
            };
            trans.process(&t, c, &mut l[k..e], &mut r[k..e]);
        }
        // Gated about half the time.
        let kept = energy(&l) / energy(&tone(48_000));
        assert!((0.3..0.7).contains(&kept), "{kept}");
        let off = FxSettings { on: false, ..t };
        let (mut l, mut r) = (tone(512), tone(512));
        trans.process(
            &off,
            Clock {
                bpm: 120.0,
                beats: 0.0,
            },
            &mut l,
            &mut r,
        );
        trans.process(
            &off,
            Clock {
                bpm: 120.0,
                beats: 0.0,
            },
            &mut l,
            &mut r,
        );
        let (mut l, mut r) = (tone(512), tone(512));
        trans.process(
            &off,
            Clock {
                bpm: 120.0,
                beats: 0.0,
            },
            &mut l,
            &mut r,
        );
        assert_eq!(l, tone(512));
    }

    #[test]
    fn every_kind_runs() {
        for kind in FxKind::ALL {
            let mut i = Insert::new(kind, 48_000.0, 512);
            let s = FxSettings {
                kind,
                on: true,
                beats: kind.default_beats(),
                level: 0.8,
                param: 0.5,
            };
            for b in 0..20 {
                let (mut l, mut r) = (tone(512), tone(512));
                i.process(
                    &s,
                    Clock {
                        bpm: 124.0,
                        beats: b as f64 * 0.1,
                    },
                    &mut l,
                    &mut r,
                );
                assert!(l.iter().chain(&r).all(|v| v.is_finite()), "{kind:?}");
            }
        }
    }
}
