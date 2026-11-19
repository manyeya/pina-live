//! The engine: four decks into the mixer, run block by block on the audio thread. The UI talks to
//! it through a `Remote`: commands in, a status snapshot out every block, and tracks a deck let
//! go of handed back to be freed off the audio thread. Nothing here waits or allocates.
//!
//! Sync: a leader (picked, or the deck that's been playing longest) sets the tempo; each deck
//! with sync on takes it (at half or double time if that's nearer its own) and keeps its beats on
//! the leader's, lined up with a jump when it starts and held there with a hand on its speed
//! too small to hear.

use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use pina_engine::dsp::Clock;

use crate::{
    audio::Audio,
    deck::{Deck, DeckCmd, DeckStatus},
    fx::{FxCmd, FxUnit, Insert, UNITS},
    grid::wrap_half,
    mixer::{MixCmd, Mixer},
};

/// What the audio thread lets go of, to be freed on the UI's.
#[derive(Debug)]
pub enum Trash {
    Audio(Arc<Audio>),
    Stems(Arc<crate::stems::Stems>),
    Fx(Box<Insert>),
}

pub const DECKS: usize = 4;
/// The most frames the engine makes at a time; a longer buffer is made in pieces.
pub const BLOCK: usize = 512;

#[derive(Debug)]
pub enum Command {
    Deck(usize, DeckCmd),
    Mix(MixCmd),
    Fx(FxCmd),
    Quantize(bool),
    /// The deck sync follows; None to pick it by itself.
    Leader(Option<usize>),
    /// The master, frame by frame (left, right), into this as it's made; None stops.
    Record(Option<Producer<f32>>),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Status {
    pub decks: [DeckStatus; DECKS],
    /// Each channel's loudest sample since the last status (before its fader), each side.
    pub levels: [[f32; 2]; DECKS],
    pub master: [f32; 2],
    pub phones: [f32; 2],
    /// The most the master's limiter turned it down since the last status, dB.
    pub reduction: f32,
    pub leader: Option<usize>,
    pub quantize: bool,
    /// Frames made since the engine started.
    pub frames: u64,
    /// Frames a recording lost (its writer fell behind): should stay 0.
    pub dropped: u64,
}

pub struct Engine {
    pub rate: f64,
    decks: Vec<Deck>,
    mixer: Mixer,
    fx: Vec<FxUnit>,
    leader: Option<usize>,
    auto_leader: Option<usize>,
    quantize: bool,
    commands: Consumer<Command>,
    /// A second way in, for a MIDI controller's thread: its jog and pads don't wait on the UI.
    control: Consumer<Command>,
    status: Producer<Status>,
    trash: Producer<Trash>,
    bufs: Vec<[Vec<f32>; 2]>,
    frames: u64,
    recorder: Option<Producer<f32>>,
    /// Frames the recording couldn't take (its writer fell behind).
    dropped: u64,
}

/// The UI's end.
pub struct Remote {
    commands: Producer<Command>,
    control: Option<Producer<Command>>,
    status: Consumer<Status>,
    trash: Consumer<Trash>,
    /// The latest status heard.
    pub last: Status,
    /// The loudest of every status since `take_peaks`: meters miss nothing between frames.
    peaks: Peaks,
}

/// Levels, each the loudest since last taken.
#[derive(Clone, Copy, Debug, Default)]
pub struct Peaks {
    pub levels: [[f32; 2]; DECKS],
    pub master: [f32; 2],
    pub phones: [f32; 2],
    pub reduction: f32,
}

impl Remote {
    /// Sends a command; false if the engine's that far behind (it's dropped).
    pub fn send(&mut self, c: Command) -> bool {
        self.commands.push(c).is_ok()
    }

    /// The second way in (once): for a controller's own thread.
    pub fn take_control(&mut self) -> Option<Producer<Command>> {
        self.control.take()
    }

    pub fn take_peaks(&mut self) -> Peaks {
        std::mem::take(&mut self.peaks)
    }

    pub fn deck(&mut self, deck: usize, c: DeckCmd) -> bool {
        self.send(Command::Deck(deck, c))
    }

    pub fn mix(&mut self, c: MixCmd) -> bool {
        self.send(Command::Mix(c))
    }

    pub fn fx(&mut self, c: FxCmd) -> bool {
        self.send(Command::Fx(c))
    }

    /// The newest status (and the tracks let go of, freed here). True if one came in.
    pub fn poll(&mut self) -> bool {
        let mut fresh = false;
        while let Ok(s) = self.status.pop() {
            let p = &mut self.peaks;
            for (a, b) in p.levels.iter_mut().zip(s.levels) {
                *a = [a[0].max(b[0]), a[1].max(b[1])];
            }
            p.master = [p.master[0].max(s.master[0]), p.master[1].max(s.master[1])];
            p.phones = [p.phones[0].max(s.phones[0]), p.phones[1].max(s.phones[1])];
            p.reduction = p.reduction.max(s.reduction);
            self.last = s;
            fresh = true;
        }
        while self.trash.pop().is_ok() {}
        fresh
    }
}

/// An engine at `rate` and the remote that drives it.
pub fn engine(rate: f64) -> (Engine, Remote) {
    let (cp, cc) = RingBuffer::new(1024);
    let (sp, sc) = RingBuffer::new(64);
    let (tp, tc) = RingBuffer::new(64);
    let (mp, mc) = RingBuffer::new(1024);
    let engine = Engine {
        rate,
        decks: (0..DECKS).map(|_| Deck::new(rate)).collect(),
        mixer: Mixer::new(rate, DECKS, BLOCK),
        fx: (0..UNITS)
            .map(|_| FxUnit::new(DECKS, rate, BLOCK))
            .collect(),
        leader: None,
        auto_leader: None,
        quantize: true,
        commands: cc,
        control: mc,
        status: sp,
        trash: tp,
        bufs: (0..DECKS)
            .map(|_| [vec![0.0; BLOCK], vec![0.0; BLOCK]])
            .collect(),
        frames: 0,
        recorder: None,
        dropped: 0,
    };
    let remote = Remote {
        commands: cp,
        control: Some(mp),
        status: sc,
        trash: tc,
        last: Status::default(),
        peaks: Peaks::default(),
    };
    (engine, remote)
}

impl Engine {
    /// Makes `ml.len()` frames (any number; made `BLOCK` at a time): the master and the
    /// headphones.
    pub fn process(&mut self, ml: &mut [f32], mr: &mut [f32], pl: &mut [f32], pr: &mut [f32]) {
        let n = ml.len();
        let mut i = 0;
        while i < n {
            let m = BLOCK.min(n - i);
            self.block(
                &mut ml[i..i + m],
                &mut mr[i..i + m],
                &mut pl[i..i + m],
                &mut pr[i..i + m],
            );
            i += m;
        }
    }

    fn block(&mut self, ml: &mut [f32], mr: &mut [f32], pl: &mut [f32], pr: &mut [f32]) {
        let n = ml.len();
        self.take_commands();
        self.sync(n);
        for (d, [l, r]) in self.decks.iter_mut().zip(self.bufs.iter_mut()) {
            d.quantize = self.quantize;
            d.render(&mut l[..n], &mut r[..n]);
        }
        let clock = self.clock();
        self.mixer
            .process(&mut self.bufs, &mut self.fx, clock, ml, mr, pl, pr);
        if let Some(rec) = &mut self.recorder {
            match rec.write_chunk_uninit(2 * n) {
                Ok(chunk) => {
                    chunk.fill_from_iter(ml.iter().zip(mr.iter()).flat_map(|(a, b)| [*a, *b]));
                }
                Err(_) => self.dropped += n as u64,
            }
        }
        self.frames += n as u64;
        self.publish();
    }

    fn take_commands(&mut self) {
        // A bounded number a block, so a flood can't hold up the sound.
        for _ in 0..256 {
            let Ok(c) = self.control.pop().or_else(|_| self.commands.pop()) else {
                break;
            };
            match c {
                Command::Deck(i, c) if i < DECKS => {
                    for old in self.decks[i].command(c) {
                        // Freed by the UI; if it can't take it, freed here (rare, and safe).
                        let _ = self.trash.push(old);
                    }
                }
                Command::Deck(..) => {}
                Command::Mix(c) => self.mixer.command(c),
                Command::Fx(c) => self.fx_command(c),
                Command::Quantize(q) => self.quantize = q,
                Command::Leader(l) => self.leader = l.filter(|&l| l < DECKS),
                Command::Record(r) => {
                    self.recorder = r;
                    self.dropped = 0;
                }
            }
        }
    }

    /// The deck sync follows: the one picked, else the one that's been leading while it plays
    /// with a grid, else the first that does.
    fn leader(&mut self) -> Option<usize> {
        let leads = |d: &Deck| d.playing() && d.grid().is_some();
        if let Some(l) = self.leader {
            return Some(l);
        }
        if let Some(l) = self.auto_leader
            && leads(&self.decks[l])
        {
            return Some(l);
        }
        let pick = (0..DECKS)
            .find(|&i| leads(&self.decks[i]) && !self.decks[i].sync)
            .or_else(|| (0..DECKS).find(|&i| leads(&self.decks[i])));
        if pick.is_some() {
            self.auto_leader = pick;
        }
        self.auto_leader
    }

    fn sync(&mut self, n: usize) {
        let Some(li) = self.leader() else {
            self.decks.iter_mut().for_each(|d| d.sync_bend = 1.0);
            return;
        };
        let (lead_bpm, lead_beat, lead_playing) = {
            let l = &self.decks[li];
            match (l.grid(), l.beat()) {
                (Some(g), Some(b)) => (g.bpm * l.tempo(), b, l.playing()),
                _ => return,
            }
        };
        let rate = self.rate;
        for (i, d) in self.decks.iter_mut().enumerate() {
            if i == li || !d.sync {
                d.sync_bend = 1.0;
                continue;
            }
            let (Some(g), Some(beat)) = (d.grid().copied(), d.beat()) else {
                continue;
            };
            // Half or double time when that's nearer the deck's own tempo.
            let (m, tempo) = [0.5, 1.0, 2.0]
                .map(|m| (m, lead_bpm * m / g.bpm))
                .into_iter()
                .min_by(|a, b| a.1.ln().abs().total_cmp(&b.1.ln().abs()))
                .unwrap();
            d.set_tempo(tempo);
            if !(d.playing() && lead_playing) {
                d.sync_bend = 1.0;
                continue;
            }
            let period = g.period(rate);
            let err = wrap_half(lead_beat * m - beat);
            if d.align {
                d.align = false;
                d.sync_bend = 1.0;
                // Lined up on the bar too (the same beat of it as the leader), at one tempo;
                // at half or double time, on the beat.
                let per = g.per_bar.max(1) as f64;
                let shift = match m == 1.0 {
                    true => wrap_half((lead_beat - beat) / per) * per,
                    false => err,
                };
                if shift.abs() > 0.002 {
                    d.shift(shift * period);
                }
            } else {
                // The error made up over half a second, never more than half a percent.
                let per_frame = err * period / (0.5 * rate);
                d.sync_bend = 1.0 + (per_frame / tempo.max(0.01)).clamp(-0.005, 0.005);
            }
        }
        let _ = n;
    }

    fn fx_command(&mut self, c: FxCmd) {
        match c {
            FxCmd::Settings { unit, settings } if unit < UNITS => self.fx[unit].settings = settings,
            FxCmd::Kind { unit, inserts } if unit < UNITS && inserts.len() == DECKS => {
                for old in std::mem::replace(&mut self.fx[unit].inserts, inserts) {
                    let _ = self.trash.push(Trash::Fx(old));
                }
            }
            FxCmd::Assign { unit, ch, on } if unit < UNITS && ch < DECKS => {
                self.fx[unit].on[ch] = on
            }
            _ => {}
        }
    }

    /// The FX's time: the leading deck's tempo and beat; with none, 120 BPM from the start.
    fn clock(&self) -> Clock {
        let lead = self.leader.or(self.auto_leader).map(|l| &self.decks[l]);
        match lead.and_then(|d| Some((d.grid()?, d.beat()?, d.tempo()))) {
            Some((g, beat, tempo)) => Clock {
                bpm: g.bpm * tempo,
                beats: beat,
            },
            None => Clock {
                bpm: 120.0,
                beats: self.frames as f64 / self.rate * 2.0,
            },
        }
    }

    fn publish(&mut self) {
        if self.status.slots() == 0 {
            return;
        }
        let mut s = Status {
            leader: self.leader.or(self.auto_leader),
            quantize: self.quantize,
            frames: self.frames,
            master: self.mixer.master_peak,
            phones: self.mixer.phones_peak,
            reduction: self.mixer.reduction,
            dropped: self.dropped,
            ..Status::default()
        };
        for (i, d) in self.decks.iter().enumerate() {
            s.decks[i] = d.status();
            s.levels[i] = self.mixer.chans[i].peak;
        }
        if self.status.push(s).is_ok() {
            self.mixer.chans.iter_mut().for_each(|c| c.peak = [0.0; 2]);
            self.mixer.master_peak = [0.0; 2];
            self.mixer.phones_peak = [0.0; 2];
            self.mixer.reduction = 0.0;
        }
    }
}
