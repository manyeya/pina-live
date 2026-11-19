//! Files held in memory: what a browser gives (dropped onto the page or picked), since it has no
//! folders to read them from. A track's path names its bytes here; reading the track reads them.
//! Native builds put nothing here and read their files as ever.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

fn files() -> &'static Mutex<HashMap<PathBuf, Arc<Vec<u8>>>> {
    static FILES: OnceLock<Mutex<HashMap<PathBuf, Arc<Vec<u8>>>>> = OnceLock::new();
    FILES.get_or_init(Mutex::default)
}

/// `bytes` kept as the file at `path`.
pub fn put(path: &Path, bytes: Vec<u8>) {
    if let Ok(mut f) = files().lock() {
        f.insert(path.to_path_buf(), Arc::new(bytes));
    }
}

/// The file at `path` let go.
pub fn forget(path: &Path) {
    if let Ok(mut f) = files().lock() {
        f.remove(path);
    }
}

/// The file at `path`, if it's held here.
pub fn get(path: &Path) -> Option<Arc<Vec<u8>>> {
    files().lock().ok()?.get(path).cloned()
}

/// A held file decoded by Symphonia, as Pina Studio decodes one on disk: its first audio track's
/// first two channels, and its rate.
pub fn decode(path: &Path, bytes: &[u8]) -> Result<(Vec<Vec<f32>>, f64), String> {
    use symphonia::core::{
        codecs::audio::AudioDecoderOptions,
        errors::Error,
        formats::{FormatOptions, TrackType, probe::Hint},
        io::MediaSourceStream,
        meta::MetadataOptions,
    };
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let source = std::io::Cursor::new(bytes.to_vec());
    let stream = MediaSourceStream::new(Box::new(source), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| err(&e))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| err(&"no audio in it"))?;
    let id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| err(&"no audio in it"))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| err(&e))?;
    let (mut sides, mut block, mut rate) = (Vec::<Vec<f32>>::new(), Vec::new(), 0);
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(Error::IoError(_)) if !sides.is_empty() => break,
            Err(e) => return Err(err(&e)),
        };
        if packet.track_id != id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio) => {
                rate = audio.spec().rate();
                audio.copy_to_vecs_planar::<f32>(&mut block);
                if sides.is_empty() {
                    sides = vec![Vec::new(); block.len().min(2)];
                }
                for (side, part) in sides.iter_mut().zip(&block) {
                    side.extend_from_slice(part);
                }
            }
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(err(&e)),
        }
    }
    if sides.is_empty() || rate == 0 {
        return Err(err(&"no audio in it"));
    }
    Ok((sides, rate as f64))
}

/// The track at `path` read: from memory if it's held here, else from its file.
pub fn read(path: &Path) -> Result<(Vec<Vec<f32>>, f64), String> {
    match get(path) {
        Some(bytes) => decode(path, &bytes),
        None => pina_engine::audio::engine::read_audio(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAV held in memory reads as the same file on disk would.
    #[test]
    fn a_held_file_reads_as_its_file_would() {
        let mut bytes = Vec::new();
        {
            let spec = hound::WavSpec {
                channels: 2,
                sample_rate: 44_100,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut w = hound::WavWriter::new(std::io::Cursor::new(&mut bytes), spec).unwrap();
            for i in 0..4410 {
                let v = ((i as f32 * 0.05).sin() * 16000.0) as i16;
                w.write_sample(v).unwrap();
                w.write_sample(-v).unwrap();
            }
            w.finalize().unwrap();
        }
        let path = Path::new("/web/tone.wav");
        put(path, bytes);
        let (sides, rate) = read(path).unwrap();
        assert_eq!((sides.len(), rate), (2, 44_100.0));
        assert_eq!(sides[0].len(), 4410);
        assert!((sides[0][100] + sides[1][100]).abs() < 1e-3);
    }
}
