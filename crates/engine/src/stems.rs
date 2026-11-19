//! A track's stems, for a deck: its vocals, drums and bass as a separation model heard them, kept
//! at 16 bits (half a float's memory, far under what's heard), and the rest of it (the melody)
//! whatever the mix has left once they're taken out, so all four together are the track exactly.

use std::path::Path;

use crate::audio::Audio;

pub const STEMS: usize = 4;
pub const NAMES: [&str; STEMS] = ["Vocals", "Drums", "Bass", "Melody"];
/// What the models call the three kept apart.
const MODEL_NAMES: [&str; 3] = ["vocals", "drums", "bass"];

pub struct Stems {
    /// Vocals, drums, bass: each side, 16-bit.
    parts: [[Vec<i16>; 2]; 3],
    /// Which of the three the model gave (a drums-only model gives one).
    pub has: [bool; 3],
}

impl std::fmt::Debug for Stems {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stems({:?})", self.has)
    }
}

const SCALE: f32 = 32767.0;

impl Stems {
    /// The stems a separation wrote into `dir` (a WAV each, by the model's names for them), at
    /// `rate`, as long as the track (`len` frames).
    pub fn load(dir: &Path, rate: f64, len: usize) -> Result<Stems, String> {
        let mut parts: [[Vec<i16>; 2]; 3] = Default::default();
        let mut has = [false; 3];
        for (k, name) in MODEL_NAMES.iter().enumerate() {
            let file = dir.join(format!("{name}.wav"));
            if !file.is_file() {
                parts[k] = [vec![0; len], vec![0; len]];
                continue;
            }
            let a = Audio::load(&file, rate)?;
            let side = |x: &[f32]| {
                let mut v: Vec<i16> = x
                    .iter()
                    .map(|s| (s.clamp(-1.0, 1.0) * SCALE).round() as i16)
                    .collect();
                v.resize(len, 0);
                v
            };
            parts[k] = [side(&a.l), side(&a.r)];
            has[k] = true;
        }
        if !has.iter().any(|h| *h) {
            return Err(format!("no vocals, drums or bass in {}", dir.display()));
        }
        Ok(Stems { parts, has })
    }

    /// From samples (tests): each part's two sides.
    pub fn from_parts(parts: [[Vec<f32>; 2]; 3]) -> Stems {
        let q = |x: &Vec<f32>| x.iter().map(|s| (s * SCALE).round() as i16).collect();
        Stems {
            parts: parts.map(|[l, r]| [q(&l), q(&r)]),
            has: [true; 3],
        }
    }

    /// Frame `i` of `audio` with each stem at its gain (vocals, drums, bass, melody).
    #[inline]
    pub fn frame(&self, audio: &Audio, i: isize, g: &[f32; STEMS]) -> (f32, f32) {
        let (ml, mr) = audio.at(i);
        let Ok(i) = usize::try_from(i) else {
            return (0.0, 0.0);
        };
        if i >= self.parts[0][0].len() {
            return (ml, mr);
        }
        // The melody (what's left) at its gain, each part at its own: the mix less what's
        // turned down of each.
        let (mut l, mut r) = (ml * g[3], mr * g[3]);
        for k in 0..3 {
            let (pl, pr) = (
                self.parts[k][0][i] as f32 / SCALE,
                self.parts[k][1][i] as f32 / SCALE,
            );
            l += pl * (g[k] - g[3]);
            r += pr * (g[k] - g[3]);
        }
        (l, r)
    }
}
