//! The mix recorded: the master as it plays, into a WAV file (24-bit, the engine's rate), by a
//! thread of its own so the audio thread never waits on the disk.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use rtrb::{Consumer, Producer, RingBuffer};

pub struct Recording {
    pub path: PathBuf,
    /// Frames written so far.
    pub frames: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    writer: Option<JoinHandle<Result<(), String>>>,
}

impl Recording {
    /// Starts writing to `path`; the producer goes to the engine (`Command::Record`).
    pub fn start(path: &Path, rate: u32) -> Result<(Recording, Producer<f32>), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: rate,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        };
        let wav = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
        // Four seconds of room: the disk can stall that long without a frame lost.
        let (producer, consumer) = RingBuffer::new(rate as usize * 2 * 4);
        let frames = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let (frames, stop) = (frames.clone(), stop.clone());
            thread::spawn(move || write(wav, consumer, &frames, &stop))
        };
        Ok((
            Recording {
                path: path.to_path_buf(),
                frames,
                stop,
                writer: Some(writer),
            },
            producer,
        ))
    }

    /// Stops (after what's waiting is written) and finishes the file.
    pub fn finish(mut self) -> Result<PathBuf, String> {
        self.stop.store(true, Ordering::Relaxed);
        match self.writer.take().map(|w| w.join()) {
            Some(Ok(r)) => r.map(|_| self.path.clone()),
            _ => Err("the recording's writer stopped".into()),
        }
    }
}

fn write(
    mut wav: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    mut from: Consumer<f32>,
    frames: &AtomicU64,
    stop: &AtomicBool,
) -> Result<(), String> {
    let scale = (1 << 23) as f32 - 1.0;
    loop {
        let done = stop.load(Ordering::Relaxed);
        let n = from.slots() / 2 * 2;
        if n > 0 {
            let chunk = from.read_chunk(n).map_err(|e| e.to_string())?;
            for x in chunk.into_iter() {
                wav.write_sample((x.clamp(-1.0, 1.0) * scale) as i32)
                    .map_err(|e| e.to_string())?;
            }
            frames.fetch_add(n as u64 / 2, Ordering::Relaxed);
        } else if done {
            break;
        } else {
            thread::sleep(Duration::from_millis(20));
        }
    }
    wav.finalize().map_err(|e| e.to_string())
}

/// Where a mix started now is kept: ~/Music/Pina Live/Recordings/Mix 2026-11-19 22.04.wav.
pub fn mix_path(stamp: &str) -> PathBuf {
    let music = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map_or_else(std::env::temp_dir, |h| PathBuf::from(h).join("Music"));
    music
        .join("Pina Live")
        .join("Recordings")
        .join(format!("Mix {stamp}.wav"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Command, engine};

    /// A second of the engine recorded: the file's a second long, at its rate.
    #[test]
    fn records_the_master() {
        let dir = std::env::temp_dir().join(format!("pina-live-rec-{}", std::process::id()));
        let path = dir.join("mix.wav");
        let (mut e, mut r) = engine(48_000.0);
        let (rec, producer) = Recording::start(&path, 48_000).unwrap();
        r.send(Command::Record(Some(producer)));
        let mut b = vec![vec![0.0f32; 480]; 4];
        for _ in 0..100 {
            let [ml, mr, pl, pr] = &mut b[..] else {
                unreachable!()
            };
            e.process(ml, mr, pl, pr);
        }
        r.send(Command::Record(None));
        e.process(&mut [0.0; 1], &mut [0.0; 1], &mut [0.0; 1], &mut [0.0; 1]);
        let out = rec.finish().unwrap();
        let wav = hound::WavReader::open(&out).unwrap();
        assert_eq!(wav.spec().sample_rate, 48_000);
        assert_eq!(wav.duration(), 48_000);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
