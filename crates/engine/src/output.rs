//! The engine played out through a sound card: the master on one pair of its outputs, the
//! headphones on another (a DJ controller's or interface's), or both on one pair in split mode
//! (the master in the left ear in mono, the cue in the right) for a laptop's single jack.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};

use crate::engine::{BLOCK, Engine};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phones {
    /// No headphones out: cueing needs a second pair.
    #[default]
    None,
    /// The pair from this channel (0-based, 2 is outputs 3 and 4).
    Pair(u16),
    /// One pair shared: the master in mono on the left, the cue in mono on the right.
    Split,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    /// None: the system's output.
    pub device: Option<String>,
    pub buffer: Option<u32>,
    /// The master's first channel (0-based).
    pub master: u16,
    pub phones: Phones,
}

/// The output devices there are: each one's name and most channels (Pina Studio's list).
pub fn devices() -> Vec<(String, u16)> {
    pina_engine::audio::engine::output_info()
        .into_iter()
        .map(|d| (d.name, d.channels))
        .collect()
}

fn device_for(name: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    let named = name.and_then(|n| {
        let is_it = |d: &cpal::Device| d.description().is_ok_and(|d| d.name() == n);
        host.output_devices().ok()?.find(is_it)
    });
    named.or_else(|| host.default_output_device())
}

/// The rate the device runs at on its own: the engine's, so nothing's converted on the way out.
pub fn device_rate(settings: &AudioSettings) -> Result<u32, String> {
    let d = device_for(settings.device.as_deref()).ok_or("no audio output")?;
    Ok(d.default_output_config()
        .map_err(|e| e.to_string())?
        .sample_rate())
}

/// The engine, playing. It outlives the stream, so the output can change without the decks
/// stopping.
pub struct Output {
    engine: Arc<Mutex<Engine>>,
    stream: Option<cpal::Stream>,
    pub device: String,
    pub channels: u16,
}

impl Output {
    pub fn start(engine: Engine, settings: &AudioSettings) -> Result<Output, String> {
        Output::try_start(engine, settings).map_err(|(_, e)| e)
    }

    /// As `start`, but failing, the engine's given back to run without sound.
    pub fn try_start(engine: Engine, settings: &AudioSettings) -> Result<Output, (Engine, String)> {
        let engine = Arc::new(Mutex::new(engine));
        match open(&engine, settings) {
            Ok((stream, device, channels)) => Ok(Output {
                engine,
                stream: Some(stream),
                device,
                channels,
            }),
            Err(e) => match Arc::try_unwrap(engine)
                .ok()
                .and_then(|m| m.into_inner().ok())
            {
                Some(engine) => Err((engine, e)),
                None => panic!("the engine's still shared after its stream failed: {e}"),
            },
        }
    }

    /// Moves to `settings`; failing that, says why (and plays nothing).
    pub fn change(&mut self, settings: &AudioSettings) -> Result<(), String> {
        self.stream = None;
        let (stream, device, channels) = open(&self.engine, settings)?;
        (self.stream, self.device, self.channels) = (Some(stream), device, channels);
        Ok(())
    }
}

fn open(
    engine: &Arc<Mutex<Engine>>,
    settings: &AudioSettings,
) -> Result<(cpal::Stream, String, u16), String> {
    let device = device_for(settings.device.as_deref()).ok_or("no audio output device")?;
    let name = device
        .description()
        .map_or_else(|_| String::new(), |d| d.name().to_string());
    let rate = engine.lock().map_err(|e| e.to_string())?.rate as u32;
    // Every channel it has, so the master and the headphones can each have their pair.
    let range = device
        .supported_output_configs()
        .map_err(|e| e.to_string())?
        .filter(|c| {
            c.sample_format() == cpal::SampleFormat::F32
                && (c.min_sample_rate()..=c.max_sample_rate()).contains(&rate)
        })
        .max_by_key(|c| c.channels())
        .ok_or_else(|| format!("{name} can't run at {rate} Hz"))?;
    let mut config = range.with_sample_rate(rate).config();
    // A browser offers more channels than it'll play (it has a pair): stereo there.
    if cfg!(target_family = "wasm") {
        config.channels = config.channels.min(2);
    }
    if let Some(b) = settings.buffer {
        config.buffer_size = cpal::BufferSize::Fixed(match *range.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => b.clamp(min, max),
            cpal::SupportedBufferSize::Unknown => b,
        });
    }
    let ch = config.channels as usize;
    let fits = move |first: usize| first + 1 < ch;
    let master = if fits(settings.master as usize) {
        settings.master as usize
    } else {
        0
    };
    let phones = settings.phones;
    let engine = engine.clone();
    let mut bufs = [[0f32; BLOCK]; 4];
    let stream = device
        .build_output_stream(
            config,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // Only a change of output holds the lock, and then this stream's gone.
                let Ok(mut engine) = engine.try_lock() else {
                    out.fill(0.0);
                    return;
                };
                for chunk in out.chunks_mut(BLOCK * ch) {
                    let n = chunk.len() / ch;
                    let [ml, mr, pl, pr] = &mut bufs;
                    engine.process(&mut ml[..n], &mut mr[..n], &mut pl[..n], &mut pr[..n]);
                    for (f, frame) in chunk.chunks_mut(ch).enumerate() {
                        frame.fill(0.0);
                        match (ch, phones) {
                            (1, _) => frame[0] = (ml[f] + mr[f]) * 0.5,
                            (_, Phones::Split) => {
                                frame[0] = (ml[f] + mr[f]) * 0.5;
                                frame[1] = (pl[f] + pr[f]) * 0.5;
                            }
                            (_, Phones::Pair(p)) if fits(p as usize) && p as usize != master => {
                                (frame[master], frame[master + 1]) = (ml[f], mr[f]);
                                (frame[p as usize], frame[p as usize + 1]) = (pl[f], pr[f]);
                            }
                            _ => (frame[master], frame[master + 1]) = (ml[f], mr[f]),
                        }
                    }
                }
            },
            |e| eprintln!("audio: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, name, ch as u16))
}
