//! `cargo run --release --example analyze -- <files>`: what Pina Live's analysis finds in each.

use std::{path::Path, time::Instant};

fn main() {
    for f in std::env::args().skip(1) {
        let t = Instant::now();
        match pina_live_engine::analyze::analyze(Path::new(&f)) {
            Ok((a, tags, wave)) => {
                let g = a.grid.map_or("no grid".into(), |g| {
                    format!("{} BPM, downbeat {:.3}s", g.bpm, g.anchor)
                });
                println!(
                    "{} — {} | {g} (steady {:.2}) | {} {} | {:.1} LUFS | {} phrases: {} | {} wave cols | {:.2}s",
                    tags.artist,
                    tags.title,
                    a.steady,
                    a.key,
                    a.camelot,
                    a.lufs,
                    a.phrases.len(),
                    a.phrases
                        .iter()
                        .map(|p| format!("{}@{:.0}", p.name, p.start))
                        .collect::<Vec<_>>()
                        .join(" "),
                    wave.cols.len(),
                    t.elapsed().as_secs_f32()
                );
            }
            Err(e) => println!("{f}: {e}"),
        }
    }
}
