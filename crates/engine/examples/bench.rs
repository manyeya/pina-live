//! `cargo run --release --example bench`: the engine's worst ordinary load (four decks playing
//! with keylock and stems, both FX units on every channel), timed against real time.

use std::{f64::consts::PI, sync::Arc, time::Instant};

use pina_live_engine::{
    audio::Audio,
    deck::DeckCmd,
    engine::{BLOCK, Command, DECKS, engine},
    fx::{FxCmd, FxKind, FxSettings, Insert},
    grid::Grid,
    stems::Stems,
};

fn main() {
    let rate = 48_000.0;
    let n = (60.0 * rate) as usize;
    let tone = |f: f64| -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * f * i as f64 / rate).sin() as f32 * 0.1)
            .collect()
    };
    let mix = tone(220.0);
    let (mut e, mut r) = engine(rate);
    for d in 0..DECKS {
        r.deck(
            d,
            DeckCmd::Load {
                audio: Arc::new(Audio::new(mix.clone(), mix.clone())),
                id: d as u64 + 1,
                grid: Some(Grid::new(124.0, 0.0)),
                cue: 0.0,
                hot: [None; 8],
            },
        );
        let p = [tone(440.0), tone(440.0)];
        r.deck(
            d,
            DeckCmd::Stems {
                id: d as u64 + 1,
                stems: Arc::new(Stems::from_parts([p.clone(), p.clone(), p])),
            },
        );
        r.deck(d, DeckCmd::StemGain { stem: 0, gain: 0.0 });
        r.deck(d, DeckCmd::Keylock(true));
        r.deck(d, DeckCmd::Tempo(1.0 + 0.02 * d as f64));
        r.deck(d, DeckCmd::Sync(d > 0));
        r.deck(d, DeckCmd::Play);
    }
    for (u, kind) in [FxKind::Reverb, FxKind::Echo].into_iter().enumerate() {
        r.send(Command::Fx(FxCmd::Kind {
            unit: u,
            inserts: (0..DECKS)
                .map(|_| Box::new(Insert::new(kind, rate, BLOCK)))
                .collect(),
        }));
        r.send(Command::Fx(FxCmd::Settings {
            unit: u,
            settings: FxSettings {
                kind,
                on: true,
                beats: 1.0,
                level: 0.5,
                param: 0.5,
            },
        }));
        for ch in 0..DECKS {
            r.send(Command::Fx(FxCmd::Assign {
                unit: u,
                ch,
                on: true,
            }));
        }
    }
    let secs = 10.0;
    let mut b = vec![vec![0.0f32; 256]; 4];
    let t = Instant::now();
    let mut worst = 0.0f64;
    for _ in 0..(secs * rate / 256.0) as usize {
        let bt = Instant::now();
        let [ml, mr, pl, pr] = &mut b[..] else {
            unreachable!()
        };
        e.process(ml, mr, pl, pr);
        worst = worst.max(bt.elapsed().as_secs_f64());
        r.poll();
    }
    let took = t.elapsed().as_secs_f64();
    println!(
        "{secs} s of four keylocked decks with stems and two FX each: {took:.2} s ({:.0}% of one core); worst 256-frame buffer {:.2} ms of its {:.2} ms",
        took / secs * 100.0,
        worst * 1000.0,
        256.0 / rate * 1000.0
    );
}
