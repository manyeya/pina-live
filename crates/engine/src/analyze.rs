//! A track studied for the decks: its beat grid, key (and its Camelot number, for mixing in
//! key), loudness (for auto gain), phrases (intro, build, drop, outro) and a waveform in three
//! bands. Pina Studio's song study does the listening; the file is decoded once for both.

use std::path::Path;

use pina_engine::study::{self, nature};
use serde::{Deserialize, Serialize};

use crate::{dsp::Split, grid::Grid};

/// Bumped when what's worked out changes, so old analyses are done again.
pub const VERSION: u32 = 3;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Tags {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub year: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Phrase {
    pub name: String,
    /// The same letter for the same music.
    pub label: String,
    pub start: f32,
    pub end: f32,
    /// How hard it plays against the rest, 0 to 1.
    pub energy: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Analysis {
    pub version: u32,
    pub seconds: f32,
    pub grid: Option<Grid>,
    /// How steady the beat is, 0 to 1.
    pub steady: f32,
    /// "A minor"; empty when there's no clear key.
    pub key: String,
    /// "8A".
    pub camelot: String,
    pub lufs: f32,
    pub peak_db: f32,
    pub phrases: Vec<Phrase>,
    pub genre: Option<String>,
}

impl Analysis {
    /// The gain that brings it to `target` LUFS, kept within reason.
    pub fn gain_db(&self, target: f32) -> f32 {
        match self.lufs > -70.0 {
            true => (target - self.lufs).clamp(-18.0, 12.0),
            false => 0.0,
        }
    }
}

/// A file studied, its tags and its waveform.
pub fn analyze(path: &Path) -> Result<(Analysis, Tags, Wave), String> {
    let (mut sides, rate) = crate::memfs::read(path)?;
    if sides.is_empty() {
        return Err("no audio in the file".into());
    }
    let l = sides.remove(0);
    let r = sides.pop().unwrap_or_else(|| l.clone());
    let wave = Wave::of(&l, &r, rate);
    let what = nature::of(&l, &r, rate, path);
    let mut s = study::study_given(&l, &r, rate as f32, &nature::given(path, &what))?;
    nature::settle(&mut s, what);
    let id = study::identity(path);
    let tags = Tags {
        title: id.title,
        artist: id.artist,
        album: id.album,
        genre: id.genre,
        year: id.year,
    };
    let per_bar = s.tempo.time_signature[0].max(1);
    let grid = (s.tempo.bpm > 30.0)
        .then(|| fit_grid(s.tempo.bpm as f64, &s.tempo.beats, &s.tempo.bars, per_bar));
    let key = nature::named_key(&s.key.name)
        .filter(|_| s.key.fit > 0.3)
        .map(|k| (s.key.name.clone(), camelot(k.root, k.minor)));
    let (key, camelot) = key.unwrap_or_default();
    let peak = s.mix.overall.peak_db[0].max(s.mix.overall.peak_db[1]);
    Ok((
        Analysis {
            version: VERSION,
            seconds: l.len() as f32 / rate as f32,
            grid,
            steady: s.tempo.confidence,
            key,
            camelot,
            lufs: s.mix.overall.lufs,
            peak_db: peak,
            phrases: s
                .sections
                .iter()
                .map(|p| Phrase {
                    name: p.name.clone(),
                    label: p.label.clone(),
                    start: p.start,
                    end: p.end,
                    energy: p.energy,
                })
                .collect(),
            genre: s.genres.first().map(|g| g.name.clone()),
        },
        tags,
        wave,
    ))
}

/// A steady grid through the beats heard: their tempo by a straight line through them (to a
/// whole BPM when it's that close, as most tracks are made), lined up on them on average, and
/// anchored on the first downbeat.
pub fn fit_grid(bpm: f64, beats: &[f32], bars: &[f32], per_bar: u32) -> Grid {
    let mut period = 60.0 / bpm;
    if beats.len() >= 16 {
        let n = beats.len() as f64;
        let mx = (n - 1.0) / 2.0;
        let my = beats.iter().map(|&b| b as f64).sum::<f64>() / n;
        let (mut sxy, mut sxx) = (0.0, 0.0);
        for (i, &b) in beats.iter().enumerate() {
            let dx = i as f64 - mx;
            sxy += dx * (b as f64 - my);
            sxx += dx * dx;
        }
        let fitted = sxy / sxx;
        if (fitted / period - 1.0).abs() < 0.03 {
            period = fitted;
        }
    }
    let mut bpm = 60.0 / period;
    if (bpm - bpm.round()).abs() < 0.03 {
        bpm = bpm.round();
    }
    let period = 60.0 / bpm;
    // Where in the beat the beats fall, on average round the circle.
    let (mut c, mut s) = (0.0, 0.0);
    for &b in beats {
        let a = std::f64::consts::TAU * (b as f64 / period);
        c += a.cos();
        s += a.sin();
    }
    let offset = match beats.is_empty() {
        true => 0.0,
        false => (s.atan2(c) / std::f64::consts::TAU).rem_euclid(1.0) * period,
    };
    let first = bars.first().or(beats.first()).copied().unwrap_or(0.0) as f64;
    let anchor = offset + ((first - offset) / period).round() * period;
    Grid {
        bpm,
        anchor,
        per_bar,
    }
}

/// A key's place on the Camelot wheel: "8A" for A minor, "8B" for C major.
pub fn camelot(root: u8, minor: bool) -> String {
    let major_root = if minor { (root + 3) % 12 } else { root % 12 };
    let n = (major_root as u32 * 7 + 7) % 12 + 1;
    format!("{n}{}", if minor { 'A' } else { 'B' })
}

/// Keys that mix: the same, one either way round the wheel, and its relative.
pub fn compatible(a: &str, b: &str) -> bool {
    let parse = |k: &str| -> Option<(u32, char)> {
        let (n, l) = k.split_at(k.len().checked_sub(1)?);
        Some((n.parse().ok()?, l.chars().next()?))
    };
    match (parse(a), parse(b)) {
        (Some((n, x)), Some((m, y))) => {
            let d = (n as i32 - m as i32).rem_euclid(12);
            (x == y && (d == 0 || d == 1 || d == 11)) || (n == m)
        }
        _ => false,
    }
}

/// How many waveform columns a second.
pub const WAVE_PER_SECOND: f64 = 150.0;

/// A track's waveform: a column every 1/150 s, each its peak and its lows', mids' and highs'
/// loudness (RMS), stored as square roots (0 to 255) so quiet detail keeps its steps. How it's
/// drawn (each band's weight, the track brought up to fill its lane) is the drawing's business,
/// so it can change without analysing again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wave {
    pub cols: Vec<[u8; 4]>,
}

impl Wave {
    /// A stored value back to a level, 0 to 1.
    pub fn level(v: u8) -> f32 {
        let x = v as f32 / 255.0;
        x * x
    }

    /// What brings the track up to fill its lane: one over the loudest a band gets anywhere in
    /// it (weighted as drawn), within reason.
    pub fn fill(&self, weights: [f32; 3]) -> f32 {
        let most = self
            .cols
            .iter()
            .map(|c| {
                (1..4)
                    .map(|b| Wave::level(c[b]) * weights[b - 1])
                    .fold(0f32, f32::max)
            })
            .fold(0f32, f32::max);
        (1.0 / most.max(1e-3)).min(12.0)
    }

    pub fn of(l: &[f32], r: &[f32], rate: f64) -> Wave {
        let mut a = Split::new(crate::dsp::LOW_MID, rate);
        let mut b = Split::new(crate::dsp::MID_HIGH, rate);
        let per = rate / WAVE_PER_SECOND;
        let count = (l.len() as f64 / per).ceil() as usize;
        let mut cols = Vec::with_capacity(count);
        let mut i = 0usize;
        for c in 0..count {
            let end = (((c + 1) as f64 * per) as usize).min(l.len());
            let (mut peak, mut lo, mut mid, mut hi) = (0f32, 0f32, 0f32, 0f32);
            let n = (end - i).max(1) as f32;
            while i < end {
                let x = 0.5 * (l[i] + r[i]);
                let (low, rest) = a.run(x);
                let (m, h) = b.run(rest);
                peak = peak.max(l[i].abs().max(r[i].abs()));
                lo += low * low;
                mid += m * m;
                hi += h * h;
                i += 1;
            }
            let q = |v: f32| (v.clamp(0.0, 1.0).sqrt() * 255.0).round() as u8;
            cols.push([
                q(peak),
                q((lo / n).sqrt()),
                q((mid / n).sqrt()),
                q((hi / n).sqrt()),
            ]);
        }
        Wave { cols }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = b"PLW1".to_vec();
        out.extend((self.cols.len() as u32).to_le_bytes());
        out.extend(self.cols.iter().flatten());
        out
    }

    pub fn from_bytes(b: &[u8]) -> Option<Wave> {
        let rest = b.strip_prefix(b"PLW1")?;
        let n = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
        let data = rest.get(4..4 + n * 4)?;
        Some(Wave {
            cols: data
                .chunks_exact(4)
                .map(|c| [c[0], c[1], c[2], c[3]])
                .collect(),
        })
    }
}

/// What `analyze` found, as bytes: the analysis and tags as JSON (as the library keeps them), then
/// the waveform (as it's cached). A web build's worker hands it to the page this way.
pub fn pack((analysis, tags, wave): &(Analysis, Tags, Wave)) -> Vec<u8> {
    let json = serde_json::to_vec(&(analysis, tags)).unwrap_or_default();
    let mut out = (json.len() as u32).to_le_bytes().to_vec();
    out.extend(json);
    out.extend(wave.to_bytes());
    out
}

pub fn unpack(b: &[u8]) -> Result<(Analysis, Tags, Wave), String> {
    let n = b
        .get(..4)
        .and_then(|n| n.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or("nothing found")? as usize;
    let json = b.get(4..4 + n).ok_or("cut short")?;
    let (analysis, tags) = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    let wave = Wave::from_bytes(&b[4 + n..]).ok_or("no waveform")?;
    Ok((analysis, tags, wave))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camelot_wheel() {
        assert_eq!(camelot(9, true), "8A");
        assert_eq!(camelot(0, false), "8B");
        assert_eq!(camelot(7, false), "9B");
        assert_eq!(camelot(11, false), "1B");
        assert_eq!(camelot(4, true), "9A");
        assert_eq!(camelot(0, true), "5A");
        assert!(compatible("8A", "9A") && compatible("12B", "1B") && compatible("8A", "8B"));
        assert!(!compatible("8A", "10A") && !compatible("8A", "9B"));
    }

    /// Beats a little off a steady 125: the grid finds 125 and lines up on them.
    #[test]
    fn grid_through_the_beats() {
        let beats: Vec<f32> = (0..200)
            .map(|i| 0.31 + i as f32 * 0.48 + if i % 2 == 0 { 0.004 } else { -0.004 })
            .collect();
        let g = fit_grid(124.6, &beats, &[0.31 + 4.0 * 0.48], 4);
        assert_eq!(g.bpm, 125.0);
        assert!(
            (g.anchor - (0.31 + 4.0 * 0.48)).abs() < 0.002,
            "{}",
            g.anchor
        );
    }

    #[test]
    fn wave_round_trip() {
        let rate = 48_000.0;
        let x: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
        let w = Wave::of(&x, &x, rate);
        assert_eq!(w.cols.len(), 150);
        assert!(w.cols.iter().all(|c| c[0] > 100 && c[1] > c[3]));
        // Quiet or loud, it's brought up to fill its lane.
        let f = w.fill([1.0; 3]);
        let top = w
            .cols
            .iter()
            .map(|c| Wave::level(c[1]) * f)
            .fold(0f32, f32::max);
        assert!((top - 1.0).abs() < 1e-3);
        assert_eq!(Wave::from_bytes(&w.to_bytes()), Some(w));
    }

    #[test]
    fn what_is_found_survives_being_handed_over() {
        let x: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
        let found = (
            Analysis {
                version: VERSION,
                seconds: 1.0,
                grid: None,
                steady: 0.8,
                key: "A minor".into(),
                camelot: "8A".into(),
                lufs: -9.5,
                peak_db: -0.3,
                phrases: vec![Phrase {
                    name: "Drop".into(),
                    label: "A".into(),
                    start: 0.0,
                    end: 1.0,
                    energy: 0.9,
                }],
                genre: Some("House".into()),
            },
            Tags {
                title: "T".into(),
                ..Tags::default()
            },
            Wave::of(&x, &x, 48_000.0),
        );
        assert_eq!(unpack(&pack(&found)), Ok(found));
        assert!(unpack(&[1, 0]).is_err());
    }
}
