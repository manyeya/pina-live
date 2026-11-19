//! MIDI controllers. Every input is listened to; what it sends is looked up in the mappings (made
//! by MIDI learn: a control clicked, then moved on the controller). Jogs, pads, transport and the
//! mixer go straight to the engine from the MIDI thread, so scratching never waits on the screen;
//! everything also comes to the window, to keep what it draws in step and to do what needs it
//! (sync, keylock, the pitch fader, the FX, the library).

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use midir::{MidiInput, MidiInputConnection};
use pina_live_engine::{deck::DeckCmd, engine::Command, library::data_dir, mixer::MixCmd};
use rtrb::Producer;
use serde::{Deserialize, Serialize};

use crate::controls::Ctl;

/// What a control on a controller is mapped to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Target {
    Play(usize),
    Cue(usize),
    Sync(usize),
    Keylock(usize),
    Slip(usize),
    HotCue(usize, usize),
    Roll(usize, f64),
    Censor(usize),
    AutoLoop(usize, f64),
    LoopHalve(usize),
    LoopDouble(usize),
    Reloop(usize),
    BeatJump(usize, f64),
    /// The jog: touched (a note) and turned (a relative CC), learned together.
    Jog(usize),
    JogTouch(usize),
    Tempo(usize),
    Mix(Ctl),
    Pfl(usize),
    FxOn(usize),
    FxAssign(usize, usize),
    BrowseScroll,
    BrowseLoad(usize),
}

impl Target {
    pub fn name(&self) -> String {
        let deck = |d: usize| crate::live::DECK_NAMES[d];
        match *self {
            Target::Play(d) => format!("{} Play", deck(d)),
            Target::Cue(d) => format!("{} Cue", deck(d)),
            Target::Sync(d) => format!("{} Sync", deck(d)),
            Target::Keylock(d) => format!("{} Keylock", deck(d)),
            Target::Slip(d) => format!("{} Slip", deck(d)),
            Target::HotCue(d, k) => format!("{} Hot cue {}", deck(d), k + 1),
            Target::Roll(d, b) => format!("{} Roll {}", deck(d), crate::decks::beats_text(b)),
            Target::Censor(d) => format!("{} Reverse (held)", deck(d)),
            Target::AutoLoop(d, b) => format!("{} Loop {}", deck(d), crate::decks::beats_text(b)),
            Target::LoopHalve(d) => format!("{} Loop ½", deck(d)),
            Target::LoopDouble(d) => format!("{} Loop ×2", deck(d)),
            Target::Reloop(d) => format!("{} Reloop", deck(d)),
            Target::BeatJump(d, b) => format!("{} Jump {b:+}", deck(d)),
            Target::Jog(d) => format!("{} Jog turn", deck(d)),
            Target::JogTouch(d) => format!("{} Jog touch", deck(d)),
            Target::Tempo(d) => format!("{} Pitch fader", deck(d)),
            Target::Mix(c) => match c {
                Ctl::Trim(ch) => format!("{} Trim", deck(ch)),
                Ctl::Eq(ch, b) => format!("{} {}", deck(ch), ["Low", "Mid", "High"][b]),
                Ctl::Filter(ch) => format!("{} Filter", deck(ch)),
                Ctl::Fader(ch) => format!("{} Fader", deck(ch)),
                Ctl::Xf => "Crossfader".into(),
                Ctl::Master => "Master".into(),
                Ctl::CueMix => "Cue mix".into(),
                Ctl::Phones => "Headphones".into(),
                Ctl::Tempo(d) => format!("{} Pitch fader", deck(d)),
                Ctl::FxLevel(u) => format!("FX {} Level", u + 1),
                Ctl::FxParam(u) => format!("FX {} Knob", u + 1),
            },
            Target::Pfl(ch) => format!("{} Cue (headphones)", deck(ch)),
            Target::FxOn(u) => format!("FX {} On", u + 1),
            Target::FxAssign(u, ch) => format!("FX {} on {}", u + 1, deck(ch)),
            Target::BrowseScroll => "Library scroll".into(),
            Target::BrowseLoad(d) => format!("Load to {}", deck(d)),
        }
    }

    /// Turned by a relative encoder rather than set.
    fn relative(&self) -> bool {
        matches!(self, Target::Jog(_) | Target::BrowseScroll)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Note,
    Cc,
    /// 14 bits: many controllers' pitch faders.
    PitchBend,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mapping {
    pub device: String,
    pub kind: Kind,
    pub channel: u8,
    pub number: u8,
    pub target: Target,
    /// A relative encoder sending 64 for still (else two's complement round 0).
    #[serde(default)]
    pub centered: bool,
    /// Turned upside down (a pitch fader whose top sends 127).
    #[serde(default)]
    pub invert: bool,
}

impl Mapping {
    pub fn message(&self) -> String {
        let k = match self.kind {
            Kind::Note => "Note",
            Kind::Cc => "CC",
            Kind::PitchBend => "Pitch bend",
        };
        format!("{k} {} · ch {}", self.number, self.channel + 1)
    }
}

/// A message, read: its kind, channel, number and value (0 to 127, or 0 to 16383 for pitch bend).
pub fn parse(b: &[u8]) -> Option<(Kind, u8, u8, u16)> {
    let (status, ch) = (b.first()? & 0xf0, b.first()? & 0x0f);
    match status {
        0x90 => Some((Kind::Note, ch, *b.get(1)?, *b.get(2)? as u16)),
        0x80 => Some((Kind::Note, ch, *b.get(1)?, 0)),
        0xb0 => Some((Kind::Cc, ch, *b.get(1)?, *b.get(2)? as u16)),
        0xe0 => Some((
            Kind::PitchBend,
            ch,
            0,
            *b.get(1)? as u16 | (*b.get(2)? as u16) << 7,
        )),
        _ => None,
    }
}

/// What a mapped control did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    Button(bool),
    /// 0 to 1.
    Value(f32),
    /// Encoder ticks, signed.
    Delta(f32),
}

fn input(m: &Mapping, value: u16) -> Input {
    if m.target.relative() && m.kind == Kind::Cc {
        let v = value as f32;
        return Input::Delta(match m.centered {
            true => v - 64.0,
            false if v < 64.0 => v,
            false => v - 128.0,
        });
    }
    match (
        m.kind,
        matches!(m.target, Target::Tempo(_) | Target::Mix(_)),
    ) {
        (Kind::PitchBend, _) => {
            let v = value as f32 / 16383.0;
            Input::Value(if m.invert { 1.0 - v } else { v })
        }
        (_, true) => {
            let v = value as f32 / 127.0;
            Input::Value(if m.invert { 1.0 - v } else { v })
        }
        _ => Input::Button(value > 0),
    }
}

/// What the MIDI thread tells the window.
#[derive(Clone, Debug)]
pub enum Event {
    /// A message from a device, as it came (when learning).
    Raw {
        device: String,
        kind: Kind,
        channel: u8,
        number: u8,
        value: u16,
    },
    Mapped(Target, Input),
}

/// How far one tick of a jog turns it: controllers send hundreds a revolution.
pub const JOG_TICKS_PER_TURN: f32 = 720.0;

/// The controller's commands for the engine: those that can't wait (jogs, pads, transport, the
/// mixer). The rest the window does.
fn direct(target: Target, inp: Input) -> Option<Command> {
    let deck = |d, c| Some(Command::Deck(d, c));
    match (target, inp) {
        (Target::Play(d), Input::Button(true)) => deck(d, DeckCmd::Play),
        (Target::Cue(d), Input::Button(b)) => deck(d, DeckCmd::Cue(b)),
        (Target::HotCue(d, slot), Input::Button(down)) => deck(d, DeckCmd::HotCue { slot, down }),
        (Target::Roll(d, beats), Input::Button(on)) => deck(d, DeckCmd::Roll { beats, on }),
        (Target::Censor(d), Input::Button(b)) => deck(d, DeckCmd::Censor(b)),
        (Target::AutoLoop(d, b), Input::Button(true)) => deck(d, DeckCmd::AutoLoop(b)),
        (Target::LoopHalve(d), Input::Button(true)) => deck(d, DeckCmd::LoopScale(0.5)),
        (Target::LoopDouble(d), Input::Button(true)) => deck(d, DeckCmd::LoopScale(2.0)),
        (Target::Reloop(d), Input::Button(true)) => deck(d, DeckCmd::Reloop),
        (Target::BeatJump(d, b), Input::Button(true)) => deck(d, DeckCmd::BeatJump(b)),
        (Target::JogTouch(d), Input::Button(b)) => deck(d, DeckCmd::Touch(b)),
        (Target::Jog(d), Input::Delta(t)) => {
            deck(d, DeckCmd::Scratch((t / JOG_TICKS_PER_TURN) as f64))
        }
        (Target::Mix(c), Input::Value(v)) => {
            let (lo, hi, _) = c.range();
            // A knob's middle detent sends 63 or 64: either is the middle.
            let v = if (v - 0.5).abs() < 0.008 { 0.5 } else { v };
            let x = lo + v * (hi - lo);
            let m = match c {
                Ctl::Trim(ch) => MixCmd::Trim { ch, db: x },
                Ctl::Eq(ch, band) => MixCmd::Eq { ch, band, knob: x },
                Ctl::Filter(ch) => MixCmd::Filter { ch, knob: x },
                Ctl::Fader(ch) => MixCmd::Fader { ch, v: x },
                Ctl::Xf => MixCmd::Crossfader(x),
                Ctl::Master => MixCmd::MasterDb(x),
                Ctl::CueMix => MixCmd::CueMix(x),
                Ctl::Phones => MixCmd::PhonesDb(x),
                _ => return None,
            };
            Some(Command::Mix(m))
        }
        _ => None,
    }
}

/// The controllers, listened to.
pub struct Midi {
    _conns: Vec<MidiInputConnection<()>>,
    pub devices: Vec<String>,
    pub maps: Arc<Mutex<Vec<Mapping>>>,
    pub learning: Arc<AtomicBool>,
}

fn maps_file() -> std::path::PathBuf {
    data_dir().join("midi.json")
}

pub fn load_maps() -> Vec<Mapping> {
    std::fs::read(maps_file())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_maps(maps: &[Mapping]) {
    let f = maps_file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(b) = serde_json::to_vec_pretty(maps) {
        let _ = std::fs::write(f, b);
    }
}

impl Midi {
    /// Every input there is, connected; what they send goes to the engine (`engine`, the
    /// remote's second way in, kept across reconnections) and the window (the receiver).
    pub fn connect(
        engine: Arc<Mutex<Option<Producer<Command>>>>,
        maps: Vec<Mapping>,
    ) -> (Midi, UnboundedReceiver<Event>) {
        let (tx, rx) = unbounded();
        let maps = Arc::new(Mutex::new(maps));
        let learning = Arc::new(AtomicBool::new(false));
        let mut conns = Vec::new();
        let mut devices = Vec::new();
        let ports = MidiInput::new("Pina Live")
            .map(|m| m.ports())
            .unwrap_or_default();
        for port in ports {
            let Ok(midi_in) = MidiInput::new("Pina Live") else {
                continue;
            };
            let Ok(name) = midi_in.port_name(&port) else {
                continue;
            };
            let (tx, maps, learning, engine, device): (UnboundedSender<Event>, _, _, _, _) = (
                tx.clone(),
                maps.clone(),
                learning.clone(),
                engine.clone(),
                name.clone(),
            );
            let conn = midi_in.connect(
                &port,
                "pina-live",
                move |_, bytes, _| {
                    let Some((kind, channel, number, value)) = parse(bytes) else {
                        return;
                    };
                    if learning.load(Ordering::Relaxed) {
                        let _ = tx.unbounded_send(Event::Raw {
                            device: device.clone(),
                            kind,
                            channel,
                            number,
                            value,
                        });
                        return;
                    }
                    let hits: Vec<Mapping> = maps.lock().map_or(Vec::new(), |m| {
                        m.iter()
                            .filter(|m| {
                                m.kind == kind
                                    && m.channel == channel
                                    && m.number == number
                                    && m.device == device
                            })
                            .cloned()
                            .collect()
                    });
                    for m in hits {
                        let inp = input(&m, value);
                        if let Some(c) = direct(m.target, inp)
                            && let Ok(mut e) = engine.lock()
                            && let Some(p) = e.as_mut()
                        {
                            let _ = p.push(c);
                        }
                        let _ = tx.unbounded_send(Event::Mapped(m.target, inp));
                    }
                },
                (),
            );
            if let Ok(c) = conn {
                devices.push(name);
                conns.push(c);
            }
        }
        (
            Midi {
                _conns: conns,
                devices,
                maps,
                learning,
            },
            rx,
        )
    }

    /// A message learned for `target`: any mapping of that message before it goes.
    pub fn learn(
        &self,
        device: &str,
        kind: Kind,
        channel: u8,
        number: u8,
        value: u16,
        target: Target,
    ) -> Mapping {
        let target = match (target, kind) {
            // The jog's touch comes as a note, its turning as a CC.
            (Target::Jog(d), Kind::Note) => Target::JogTouch(d),
            (t, _) => t,
        };
        let m = Mapping {
            device: device.to_string(),
            kind,
            channel,
            number,
            target,
            centered: (56..=72).contains(&value),
            invert: false,
        };
        if let Ok(mut maps) = self.maps.lock() {
            maps.retain(|x| {
                !(x.device == m.device
                    && x.kind == m.kind
                    && x.channel == m.channel
                    && x.number == m.number)
            });
            maps.push(m.clone());
            save_maps(&maps);
        }
        m
    }

    pub fn mappings(&self) -> Vec<Mapping> {
        self.maps.lock().map_or(Vec::new(), |m| m.clone())
    }

    pub fn forget(&self, k: usize) {
        if let Ok(mut maps) = self.maps.lock()
            && k < maps.len()
        {
            maps.remove(k);
            save_maps(&maps);
        }
    }

    pub fn flip(&self, k: usize) {
        if let Ok(mut maps) = self.maps.lock()
            && let Some(m) = maps.get_mut(k)
        {
            m.invert = !m.invert;
            save_maps(&maps);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(kind: Kind, target: Target, centered: bool) -> Mapping {
        Mapping {
            device: "X".into(),
            kind,
            channel: 0,
            number: 1,
            target,
            centered,
            invert: false,
        }
    }

    #[test]
    fn messages_read() {
        assert_eq!(parse(&[0x91, 36, 100]), Some((Kind::Note, 1, 36, 100)));
        assert_eq!(parse(&[0x80, 36, 64]), Some((Kind::Note, 0, 36, 0)));
        assert_eq!(parse(&[0xb0, 7, 127]), Some((Kind::Cc, 0, 7, 127)));
        assert_eq!(
            parse(&[0xe2, 0x7f, 0x7f]),
            Some((Kind::PitchBend, 2, 0, 16383))
        );
        assert_eq!(parse(&[0xf8]), None);
    }

    #[test]
    fn inputs_by_what_they_drive() {
        // A jog, two ways of counting.
        assert_eq!(
            input(&map(Kind::Cc, Target::Jog(0), false), 3),
            Input::Delta(3.0)
        );
        assert_eq!(
            input(&map(Kind::Cc, Target::Jog(0), false), 125),
            Input::Delta(-3.0)
        );
        assert_eq!(
            input(&map(Kind::Cc, Target::Jog(0), true), 61),
            Input::Delta(-3.0)
        );
        // A fader, a pitch fader in 14 bits upside down, a pad.
        assert_eq!(
            input(&map(Kind::Cc, Target::Mix(Ctl::Xf), false), 127),
            Input::Value(1.0)
        );
        let mut pb = map(Kind::PitchBend, Target::Tempo(1), false);
        pb.invert = true;
        assert_eq!(input(&pb, 16383), Input::Value(0.0));
        assert_eq!(
            input(&map(Kind::Note, Target::Play(0), false), 0),
            Input::Button(false)
        );
        // The mixer goes straight to the engine; an EQ's detent is its middle.
        match direct(Target::Mix(Ctl::Eq(1, 0)), Input::Value(63.0 / 127.0)) {
            Some(Command::Mix(MixCmd::Eq {
                ch: 1,
                band: 0,
                knob,
            })) => assert_eq!(knob, 0.5),
            other => panic!("{other:?}"),
        }
        assert!(direct(Target::Sync(0), Input::Button(true)).is_none());
    }
}
