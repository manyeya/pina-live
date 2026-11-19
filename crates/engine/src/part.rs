//! A track parted into stems on several cores at once. One ONNX Runtime run keeps only two or
//! three of an M4's ten cores busy, so the track's cut into overlapping pieces, each parted by
//! its own model session on its own thread, and the pieces crossfaded back together in the
//! middle of where they overlap: far enough from each piece's ends that its edges aren't heard.

use std::{
    path::Path,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use pina_engine::separate::{self, Model};

use crate::audio::Audio;

/// How far each piece runs past its share, each side.
const MARGIN_S: f64 = 6.0;
/// The crossfade between two pieces, centred on where one's share ends and the next's starts.
const FADE_S: f64 = 4.0;

/// How many pieces to cut a track into at once on this machine.
// shortcut: one, until more is shown to pay. On an M4 with the cores busy elsewhere, two pieces of
// HTDemucs took 142 s to one's 145 s and 1.9 GB more memory (examples/part.rs); measure on an
// idle machine before raising it.
pub fn pieces() -> usize {
    1
}

/// `file` parted by `model` into `out`: a WAV for each stem named in `keep` that the model gives
/// (at the model's rate; a deck reads it at its own). `pieces` runs at once; `progress` told how
/// far it's got (0 to 1), from any of them; `stop` stops them all.
pub fn part(
    file: &Path,
    model: &Model,
    out: &Path,
    keep: &[&str],
    pieces: usize,
    progress: impl Fn(f32) + Sync,
    stop: &AtomicBool,
) -> Result<(), String> {
    let rate = model.about.rate as f64;
    let audio = Audio::load(file, rate)?;
    let len = audio.len();
    let margin = (MARGIN_S * rate) as usize;
    let fade = (FADE_S * rate) as usize;
    // A short track isn't worth cutting: each piece gets at least four margins' worth.
    let n = pieces.clamp(1, (len / (4 * margin)).max(1));
    let share = len.div_ceil(n);
    let work = out.with_extension("pieces");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let spans: Vec<(usize, usize)> = (0..n)
        .map(|k| {
            let (a, b) = (k * share, ((k + 1) * share).min(len));
            (a.saturating_sub(margin), (b + margin).min(len))
        })
        .collect();
    for (k, &(from, to)) in spans.iter().enumerate() {
        write(
            &work.join(format!("{k}.wav")),
            &audio.l[from..to],
            &audio.r[from..to],
            rate as u32,
        )?;
    }
    let done: Vec<AtomicU32> = (0..n).map(|_| AtomicU32::new(0)).collect();
    let tell = |k: usize, p: f32| {
        done[k].store(p.to_bits(), Ordering::Relaxed);
        let sum: f32 = (0..n)
            .map(|j| {
                f32::from_bits(done[j].load(Ordering::Relaxed)) * (spans[j].1 - spans[j].0) as f32
            })
            .sum();
        let total: usize = spans.iter().map(|(a, b)| b - a).sum();
        progress(sum / total as f32);
    };
    let ran: Vec<Result<(), String>> = std::thread::scope(|s| {
        let runs: Vec<_> = (0..n)
            .map(|k| {
                let (work, tell) = (&work, &tell);
                s.spawn(move || {
                    let r = separate::separate(
                        &work.join(format!("{k}.wav")),
                        model,
                        &work.join(k.to_string()),
                        |p| tell(k, p),
                        stop,
                    );
                    // One failing stops the rest: there's no track without it.
                    if r.is_err() {
                        stop.store(true, Ordering::Relaxed);
                    }
                    r.map(|_| ())
                })
            })
            .collect();
        runs.into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("a separation thread panicked".into()))
            })
            .collect()
    });
    if let Some(e) = ran.into_iter().find_map(Result::err) {
        let _ = std::fs::remove_dir_all(&work);
        return Err(e);
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let given = &model.about.stems;
    for name in keep
        .iter()
        .filter(|k| given.iter().any(|g| g.eq_ignore_ascii_case(k)))
    {
        let mut whole = [vec![0f32; len], vec![0f32; len]];
        for (k, &(from, _)) in spans.iter().enumerate() {
            let a = Audio::load(&work.join(k.to_string()).join(format!("{name}.wav")), rate)?;
            for (i, (l, r)) in a.l.iter().zip(&a.r).enumerate() {
                let at = from + i;
                if at >= len {
                    break;
                }
                let w = weight(k, n, share, fade, at);
                whole[0][at] += l * w;
                whole[1][at] += r * w;
            }
        }
        write(
            &out.join(format!("{name}.wav")),
            &whole[0],
            &whole[1],
            rate as u32,
        )?;
    }
    let _ = std::fs::remove_dir_all(&work);
    Ok(())
}

/// Piece `k`'s (of `n`, each `share` long) say at frame `at`: all of it inside its share, fading
/// to and from its neighbours over `fade` frames centred on each edge it shares with one. A
/// piece and its neighbour's add to one everywhere.
pub fn weight(k: usize, n: usize, share: usize, fade: usize, at: usize) -> f32 {
    let half = fade as f64 / 2.0;
    let ramp = |edge: usize| ((at as f64 - edge as f64 + half) / fade as f64).clamp(0.0, 1.0);
    let rise = if k == 0 { 1.0 } else { ramp(k * share) };
    let fall = if k + 1 == n {
        1.0
    } else {
        1.0 - ramp((k + 1) * share)
    };
    (rise * fall) as f32
}

/// A two-channel 32-bit float WAV (written aside, then moved into place).
fn write(path: &Path, l: &[f32], r: &[f32], rate: u32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let part = path.with_extension("wav.part");
    let err = |e: hound::Error| format!("{}: {e}", path.display());
    let mut w = hound::WavWriter::create(&part, spec).map_err(err)?;
    for (a, b) in l.iter().zip(r) {
        w.write_sample(*a).map_err(err)?;
        w.write_sample(*b).map_err(err)?;
    }
    w.finalize().map_err(err)?;
    std::fs::rename(&part, path).map_err(|e| format!("{}: {e}", path.display()))
}
