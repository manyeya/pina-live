//! `cargo run --release --example part -- <file> <seconds> <model> <pieces> <out> [<ref dir>]`:
//! the first seconds of a file parted in that many pieces at once, timed; with a reference
//! (the same parted in one piece), how far each stem is from it, in dB under the stem.

use std::{path::Path, sync::atomic::AtomicBool, time::Instant};

use pina_live_engine::audio::Audio;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (file, secs, name, pieces, out) = (
        Path::new(&a[0]),
        a[1].parse::<f64>().unwrap(),
        a[2].as_str(),
        a[3].parse::<usize>().unwrap(),
        Path::new(&a[4]),
    );
    std::fs::create_dir_all(out).unwrap();
    let audio = Audio::load(file, 44_100.0).unwrap();
    let n = ((secs * 44_100.0) as usize).min(audio.len());
    let cut = out.join("excerpt.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 44_100,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(&cut, spec).unwrap();
    for i in 0..n {
        w.write_sample(audio.l[i]).unwrap();
        w.write_sample(audio.r[i]).unwrap();
    }
    w.finalize().unwrap();
    let model = pina_engine::separate::find(&pina_engine::separate::folder(), Some(name)).unwrap();
    let t = Instant::now();
    let stems = out.join("stems");
    pina_live_engine::part::part(
        &cut,
        &model,
        &stems,
        &["vocals", "drums", "bass"],
        pieces,
        |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    let took = t.elapsed().as_secs_f64();
    println!(
        "{} in {pieces} pieces: {secs} s parted in {took:.1} s ({:.2}x real time)",
        model.about.name,
        took / secs
    );
    if let Some(r) = a.get(5) {
        for s in ["vocals", "drums", "bass"] {
            let x = Audio::load(&stems.join(format!("{s}.wav")), 44_100.0).unwrap();
            let y = Audio::load(&Path::new(r).join(format!("{s}.wav")), 44_100.0).unwrap();
            let (mut e, mut d) = (0f64, 0f64);
            for i in 0..x.len().min(y.len()) {
                e += (y.l[i] as f64).powi(2) + (y.r[i] as f64).powi(2);
                d += ((x.l[i] - y.l[i]) as f64).powi(2) + ((x.r[i] - y.r[i]) as f64).powi(2);
            }
            println!(
                "  {s}: {:.1} dB under the stem",
                10.0 * (e / d.max(1e-12)).log10()
            );
        }
    }
}
