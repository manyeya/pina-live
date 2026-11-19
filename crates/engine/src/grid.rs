//! A track's beat grid: a steady tempo and where one of its downbeats falls. Everything that
//! snaps (cues, loops, jumps) and everything that locks (sync, the phase meter) reads it.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub bpm: f64,
    /// A downbeat, seconds into the track.
    pub anchor: f64,
    /// Beats to a bar.
    pub per_bar: u32,
}

impl Grid {
    pub fn new(bpm: f64, anchor: f64) -> Grid {
        Grid {
            bpm,
            anchor,
            per_bar: 4,
        }
    }

    /// A beat's length, in frames at `rate`.
    pub fn period(&self, rate: f64) -> f64 {
        60.0 * rate / self.bpm
    }

    /// Beats from the anchor to `frame` (fractional; negative before it).
    pub fn beat_at(&self, frame: f64, rate: f64) -> f64 {
        (frame - self.anchor * rate) / self.period(rate)
    }

    pub fn frame_of(&self, beat: f64, rate: f64) -> f64 {
        self.anchor * rate + beat * self.period(rate)
    }

    /// How far through its beat `frame` is, 0 to 1.
    pub fn phase(&self, frame: f64, rate: f64) -> f64 {
        self.beat_at(frame, rate).rem_euclid(1.0)
    }

    /// How far through its bar `frame` is, in beats (0 to `per_bar`).
    pub fn bar_phase(&self, frame: f64, rate: f64) -> f64 {
        self.beat_at(frame, rate)
            .rem_euclid(self.per_bar.max(1) as f64)
    }

    /// The nearest point on a grid of `step` beats (1 = every beat, 0.25 = sixteenths).
    pub fn snap(&self, frame: f64, rate: f64, step: f64) -> f64 {
        self.frame_of((self.beat_at(frame, rate) / step).round() * step, rate)
    }

    /// The point on a grid of `step` beats at or before `frame`.
    pub fn floor(&self, frame: f64, rate: f64, step: f64) -> f64 {
        // A hair of slack: a frame a rounding error short of a beat is on it.
        self.frame_of(
            (self.beat_at(frame, rate) / step + 1e-6).floor() * step,
            rate,
        )
    }
}

/// `x` wrapped into -0.5..0.5: the shortest way round a beat.
pub fn wrap_half(x: f64) -> f64 {
    x - x.round()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beats_and_snaps() {
        let g = Grid::new(120.0, 0.25);
        let rate = 48_000.0;
        assert_eq!(g.period(rate), 24_000.0);
        assert_eq!(g.frame_of(0.0, rate), 12_000.0);
        assert!((g.beat_at(36_000.0, rate) - 1.0).abs() < 1e-9);
        assert!((g.phase(42_000.0, rate) - 0.25).abs() < 1e-9);
        assert_eq!(g.snap(47_000.0, rate, 1.0), 36_000.0);
        assert_eq!(g.snap(49_000.0, rate, 1.0), 60_000.0);
        assert_eq!(g.floor(59_999.0, rate, 1.0), 36_000.0);
        assert_eq!(g.floor(60_000.0, rate, 1.0), 60_000.0);
        assert_eq!(g.floor(13_000.0, rate, 0.25), 12_000.0);
        // Before the anchor, beats count back.
        assert_eq!(g.floor(11_000.0, rate, 1.0), -12_000.0);
        assert!((wrap_half(0.75) + 0.25).abs() < 1e-12);
        assert!((wrap_half(-0.6) - 0.4).abs() < 1e-12);
    }
}
