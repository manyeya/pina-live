//! `cargo run --release --example stems -- <file> <seconds> <out dir>`: the first seconds of a
//! file parted by the first separation model added to Pina Studio, timed; the stems then read as
//! a deck reads them, and what's left with the vocals muted measured against the whole.

use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

use pina_live_engine::{audio::Audio, deck::DeckCmd, engine::engine, stems::Stems};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (file, secs, out) = (
        Path::new(&a[0]),
        a[1].parse::<f64>().unwrap(),
        Path::new(&a[2]),
    );
    std::fs::create_dir_all(out).unwrap();
    let (sides, rate) = pina_engine::audio::engine::read_audio(file).unwrap();
    let n = ((secs * rate) as usize).min(sides[0].len());
    let cut = out.join("excerpt.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(&cut, spec).unwrap();
    for i in 0..n {
        w.write_sample(sides[0][i]).unwrap();
        w.write_sample(sides.get(1).unwrap_or(&sides[0])[i])
            .unwrap();
    }
    w.finalize().unwrap();
    let model = pina_engine::separate::find(&pina_engine::separate::folder(), None).unwrap();
    let t = Instant::now();
    let stems_dir = out.join("stems");
    pina_engine::separate::separate(&cut, &model, &stems_dir, |_| {}, &AtomicBool::new(false))
        .unwrap();
    let took = t.elapsed().as_secs_f64();
    println!(
        "{}: {secs} s parted in {took:.1} s ({:.2}x real time)",
        model.about.name,
        took / secs
    );
    let er = 48_000.0;
    let audio = Arc::new(Audio::load(&cut, er).unwrap());
    let stems = Stems::load(&stems_dir, er, audio.len()).unwrap();
    println!(
        "stems found: vocals {} drums {} bass {}",
        stems.has[0], stems.has[1], stems.has[2]
    );
    let (mut e, mut r) = engine(er);
    r.deck(
        0,
        DeckCmd::Load {
            audio: audio.clone(),
            id: 1,
            grid: None,
            cue: 0.0,
            hot: [None; 8],
        },
    );
    r.deck(
        0,
        DeckCmd::Stems {
            id: 1,
            stems: Arc::new(stems),
        },
    );
    r.deck(0, DeckCmd::Play);
    let half = audio.len() / 2;
    let mut whole = vec![0.0; half];
    let mut rest = vec![0.0; half];
    let (mut a1, mut a2, mut a3) = (vec![0.0; half], vec![0.0; half], vec![0.0; half]);
    e.process(&mut whole, &mut a1, &mut a2, &mut a3);
    r.deck(0, DeckCmd::StemGain { stem: 0, gain: 0.0 });
    e.process(&mut rest, &mut a1, &mut a2, &mut a3);
    let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
    println!(
        "level with vocals {:.4}, without {:.4}",
        rms(&whole),
        rms(&rest[4800..])
    );
}
