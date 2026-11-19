//! A track's audio, decoded whole and kept at the engine's rate, as the decks read it.

use std::path::Path;

pub struct Audio {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

impl std::fmt::Debug for Audio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Audio({} frames)", self.len())
    }
}

impl Audio {
    pub fn new(l: Vec<f32>, r: Vec<f32>) -> Audio {
        debug_assert_eq!(l.len(), r.len());
        Audio { l, r }
    }

    /// A file, decoded and converted to `rate`: Pina Studio's decoder (WAV, AIFF, FLAC, MP3, and
    /// with this crate's Symphonia features M4A and Ogg too). A mono file plays on both sides.
    pub fn load(path: &Path, rate: f64) -> Result<Audio, String> {
        // Held in memory (a browser's file): decoded here, converted as Pina Studio would.
        let mut sides = match crate::memfs::get(path) {
            Some(bytes) => {
                let (sides, from) = crate::memfs::decode(path, &bytes)?;
                sides
                    .iter()
                    .map(|s| pina_engine::audio::engine::resample(s, from, rate))
                    .collect()
            }
            None => pina_engine::audio::engine::load_audio(path, rate)?
                .channels()
                .to_vec(),
        };
        let l = sides.first().cloned().ok_or("no audio in the file")?;
        let r = match sides.len() {
            1 => l.clone(),
            _ => sides.swap_remove(1),
        };
        Ok(Audio::new(l, r))
    }

    /// Where the sound starts: the first frame above -40 dB (0 if it's silent throughout).
    pub fn first_sound(&self) -> f64 {
        self.l
            .iter()
            .zip(&self.r)
            .position(|(a, b)| a.abs().max(b.abs()) > 0.01)
            .unwrap_or(0) as f64
    }

    pub fn len(&self) -> usize {
        self.l.len()
    }

    pub fn is_empty(&self) -> bool {
        self.l.is_empty()
    }

    /// A frame; silence past either end.
    #[inline]
    pub fn at(&self, i: isize) -> (f32, f32) {
        match usize::try_from(i) {
            Ok(i) if i < self.l.len() => (self.l[i], self.r[i]),
            _ => (0.0, 0.0),
        }
    }

    /// Between frames, by a 4-point Hermite curve: what a platter turned by hand reads.
    #[inline]
    pub fn read(&self, pos: f64) -> (f32, f32) {
        let i = pos.floor();
        let t = (pos - i) as f32;
        let i = i as isize;
        let (a, b, c, d) = (self.at(i - 1), self.at(i), self.at(i + 1), self.at(i + 2));
        (
            hermite(a.0, b.0, c.0, d.0, t),
            hermite(a.1, b.1, c.1, d.1, t),
        )
    }
}

#[inline]
pub(crate) fn hermite(x0: f32, x1: f32, x2: f32, x3: f32, t: f32) -> f32 {
    let c1 = 0.5 * (x2 - x0);
    let c2 = x0 - 2.5 * x1 + 2.0 * x2 - 0.5 * x3;
    let c3 = 0.5 * (x3 - x0) + 1.5 * (x1 - x2);
    ((c3 * t + c2) * t + c1) * t + x1
}
