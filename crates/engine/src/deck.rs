//! A deck: one track, played as a DJ plays it. Its tempo (the pitch fader), keylock (the tempo
//! moved, the key kept, by a time-stretch that follows the fader every block), a key shift, cues
//! and hot cues as a CDJ keeps them, loops (auto, manual, rolls), beat jumps, slip (the track
//! playing on underneath, silently, to come back to), reverse and censor, a brake and a spinback,
//! and the platter: touched, it holds the record; turned, it scratches.
//!
//! The deck plays in steps of `STEP` frames. A jump (a cue, a loop's end, a beat jump) lands at
//! the start of one and crossfades over it, so nothing clicks; keylock reads ahead of what's
//! heard by the stretch's latency, so its two stretchers take turns across a jump, the one going
//! out playing on while the other comes in.

use std::sync::Arc;

use signalsmith_stretch::Stretch;

use crate::{
    audio::Audio,
    engine::Trash,
    grid::{Grid, wrap_half},
    stems::{STEMS, Stems},
};

pub const HOT_CUES: usize = 8;
/// Frames a deck plays at a time: where a jump lands, and how long its crossfade lasts.
pub const STEP: usize = 64;
/// A record at 33⅓ turns once in 1.8 seconds: how far one turn of the platter moves the track.
pub const SECONDS_PER_TURN: f64 = 1.8;
/// Outside these speeds keylock lets go: the deck plays as a record does.
const STRETCH_MIN: f64 = 0.1;
const STRETCH_MAX: f64 = 4.0;
/// A loop's seam in vinyl mode: the frames the end fades out over as the start comes in.
const SEAM: usize = 32;
/// With no grid, a beat is this long (120 BPM).
const NO_GRID_BEAT: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loop {
    pub start: f64,
    pub end: f64,
    pub on: bool,
}

impl Loop {
    pub fn len(&self) -> f64 {
        self.end - self.start
    }
}

/// What the deck is told to do. Positions are frames at the engine's rate.
#[derive(Debug)]
pub enum DeckCmd {
    Load {
        audio: Arc<Audio>,
        /// Ours, to tell tracks apart in the status.
        id: u64,
        grid: Option<Grid>,
        cue: f64,
        hot: [Option<f64>; HOT_CUES],
    },
    Unload,
    Grid(Option<Grid>),
    /// Play or pause, as a CDJ's button.
    Play,
    Playing(bool),
    /// The cue button, pressed or let go.
    Cue(bool),
    HotCue {
        slot: usize,
        down: bool,
    },
    /// A hot cue put somewhere (or cleared) by hand.
    SetHotCue {
        slot: usize,
        at: Option<f64>,
    },
    /// Clicked on the waveform: there, in time if quantize is on and it's playing.
    Seek(f64),
    /// The pitch fader: speed as a ratio (1.06 is +6%).
    Tempo(f64),
    /// Pitch bend held (the +/- buttons): a speed offset, 0 when let go.
    Bend(f64),
    /// The jog's edge turned: the track pushed ahead or held back, by turns.
    Nudge(f64),
    /// The jog's top touched or let go.
    Touch(bool),
    /// The jog turned while touched: scratching, by turns.
    Scratch(f64),
    Keylock(bool),
    /// Semitones up or down.
    Key(f64),
    Reverse(bool),
    /// Reverse while held, the track carrying on underneath.
    Censor(bool),
    Slip(bool),
    /// Slows to a stop over this many seconds, as a turntable switched off.
    Brake(f64),
    /// Thrown back, slowing to a stop over this many seconds.
    Spinback(f64),
    BeatJump(f64),
    /// A loop this many beats long from here (or off, if it's the one on).
    AutoLoop(f64),
    LoopIn,
    LoopOut,
    /// The loop back on (jumping into it), or off.
    Reloop,
    LoopExit,
    /// Twice as long (2) or half (0.5).
    LoopScale(f64),
    /// The loop moved by beats, the track with it.
    LoopMove(f64),
    /// A loop while held, the track carrying on underneath.
    Roll {
        beats: f64,
        on: bool,
    },
    SetLoop(Option<Loop>),
    Sync(bool),
    /// The loaded track's stems (for track `id`, in case another's been loaded since).
    Stems {
        id: u64,
        stems: Arc<Stems>,
    },
    /// A stem's level: vocals, drums, bass, melody (0 to 1).
    StemGain {
        stem: usize,
        gain: f32,
    },
}

/// A deck as the UI sees it, a snapshot every block.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DeckStatus {
    pub id: u64,
    pub loaded: bool,
    /// Frames.
    pub len: f64,
    /// What's heard, frames in.
    pub pos: f64,
    pub playing: bool,
    pub previewing: bool,
    /// Frames of track per frame played: 1 is as recorded, negative is backwards.
    pub speed: f64,
    pub tempo: f64,
    /// The tempo heard (the grid's, at the deck's speed), 0 with no grid.
    pub bpm: f64,
    /// Beats from the grid's anchor, NaN with no grid.
    pub beat: f64,
    pub keylock: bool,
    pub key: f64,
    pub stretching: bool,
    pub sync: bool,
    pub slip: bool,
    /// Where the track would be, while slipping; NaN when not.
    pub shadow: f64,
    pub reverse: bool,
    pub touched: bool,
    pub cue: f64,
    /// NaN where none is set.
    pub hot: [f64; HOT_CUES],
    pub loop_start: f64,
    pub loop_end: f64,
    pub loop_on: bool,
    pub roll: bool,
    /// Where a loop being set by hand starts; NaN when none.
    pub loop_in: f64,
    pub stems: bool,
    /// Each stem's level, as asked.
    pub stem_gain: [f32; STEMS],
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Motion {
    Free,
    Brake { per_frame: f64 },
    Spin { vel: f64, decay: f64 },
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Vinyl,
    Stretch(usize),
}

/// A stretcher and where it reads: ahead of what's heard by its latency, in the way it's going.
struct Head {
    st: Stretch,
    read: f64,
    acc: f64,
    lat_in: f64,
    lat_out: f64,
}

impl Head {
    /// How far ahead of what's heard it reads, at `speed`.
    fn lag(&self, speed: f64) -> f64 {
        self.lat_in + self.lat_out * speed
    }
}

pub struct Deck {
    rate: f64,
    audio: Option<Arc<Audio>>,
    stems: Option<Arc<Stems>>,
    /// Each stem's level as heard, gliding to what's asked.
    stem_gain: [f32; STEMS],
    stem_want: [f32; STEMS],
    id: u64,
    grid: Option<Grid>,
    playing: bool,
    /// Held while stopped (the cue, or a hot cue: which), playing till it's let go, then back.
    preview: Option<(f64, Option<usize>)>,
    pos: f64,
    tempo: f64,
    bend: f64,
    /// Frames still to push the track by (the jog's edge).
    nudge: f64,
    /// Sync's hand on the tempo, keeping the beats together.
    pub(crate) sync_bend: f64,
    keylock: bool,
    key: f64,
    reverse: bool,
    censor: bool,
    motion: Motion,
    speed: f64,
    touched: bool,
    /// Scratching: frames still to move, and how fast it's moving.
    pending: f64,
    scratch_vel: f64,
    cue: f64,
    hot: [Option<f64>; HOT_CUES],
    held_hot: Option<usize>,
    lp: Option<Loop>,
    loop_in: Option<f64>,
    roll: bool,
    slip: bool,
    shadow: Option<f64>,
    pub(crate) sync: bool,
    /// Sync is to line the beats up on the next block (just turned on, or just started).
    pub(crate) align: bool,
    pub(crate) quantize: bool,
    vel: f64,
    /// The last block glided (a hand, a brake): the next glides from it too.
    smooth: bool,
    gate: f32,
    mode: Mode,
    jump: Option<f64>,
    heads: [Head; 2],
    /// A loop's seam in vinyl mode: where the end carries on, and frames left of its fade.
    seam: Option<(f64, usize)>,
    pre: usize,
    inbuf: Vec<f32>,
    outbuf: Vec<f32>,
    al: Vec<f32>,
    ar: Vec<f32>,
}

impl Deck {
    pub fn new(rate: f64) -> Deck {
        let head = || {
            let st = Stretch::preset_default(2, rate as u32);
            Head {
                lat_in: st.input_latency() as f64,
                lat_out: st.output_latency() as f64,
                st,
                read: 0.0,
                acc: 0.0,
            }
        };
        let heads = [head(), head()];
        // What a stretcher hears before it's sought to a point: twice its window's lead.
        let pre = heads[0].lat_in as usize * 2 + STEP;
        let most_in = (STEP as f64 * STRETCH_MAX).ceil() as usize + 2;
        Deck {
            rate,
            audio: None,
            stems: None,
            stem_gain: [1.0; STEMS],
            stem_want: [1.0; STEMS],
            id: 0,
            grid: None,
            playing: false,
            preview: None,
            pos: 0.0,
            tempo: 1.0,
            bend: 1.0,
            nudge: 0.0,
            sync_bend: 1.0,
            keylock: false,
            key: 0.0,
            reverse: false,
            censor: false,
            motion: Motion::Free,
            speed: 1.0,
            touched: false,
            pending: 0.0,
            scratch_vel: 0.0,
            cue: 0.0,
            hot: [None; HOT_CUES],
            held_hot: None,
            lp: None,
            loop_in: None,
            roll: false,
            slip: false,
            shadow: None,
            sync: false,
            align: false,
            quantize: true,
            vel: 0.0,
            smooth: false,
            gate: 0.0,
            mode: Mode::Vinyl,
            jump: None,
            heads,
            seam: None,
            pre,
            inbuf: vec![0.0; 2 * pre.max(most_in)],
            outbuf: vec![0.0; 2 * STEP],
            al: vec![0.0; STEP],
            ar: vec![0.0; STEP],
        }
    }

    pub fn grid(&self) -> Option<&Grid> {
        self.grid.as_ref()
    }

    pub fn playing(&self) -> bool {
        self.playing || self.preview.is_some()
    }

    pub fn tempo(&self) -> f64 {
        self.tempo
    }

    pub(crate) fn set_tempo(&mut self, t: f64) {
        self.tempo = t;
    }

    /// The tempo heard: the grid's at the deck's speed (pitch bends and sync's hand aside).
    pub fn bpm(&self) -> f64 {
        self.grid.map_or(0.0, |g| g.bpm * self.tempo)
    }

    /// Beats from the grid's anchor to what's heard.
    pub fn beat(&self) -> Option<f64> {
        // A jump asked for counts as made: sync lines up from where it's going.
        self.grid
            .map(|g| g.beat_at(self.jump.unwrap_or(self.pos), self.rate))
    }

    pub fn period(&self) -> f64 {
        self.grid
            .map_or(NO_GRID_BEAT * self.rate, |g| g.period(self.rate))
    }

    pub fn pos(&self) -> f64 {
        self.pos
    }

    /// Takes in a command; what it replaces (a track, its stems) is handed back, to be let go off
    /// this thread.
    pub fn command(&mut self, cmd: DeckCmd) -> Vec<Trash> {
        let mut out = Vec::new();
        self.take(cmd, &mut out);
        out
    }

    fn take(&mut self, cmd: DeckCmd, out: &mut Vec<Trash>) {
        match cmd {
            DeckCmd::Load {
                audio,
                id,
                grid,
                cue,
                hot,
            } => {
                out.extend(self.audio.replace(audio).map(Trash::Audio));
                out.extend(self.stems.take().map(Trash::Stems));
                self.id = id;
                self.grid = grid;
                self.cue = cue;
                self.hot = hot;
                self.reset_play(cue);
            }
            DeckCmd::Unload => {
                self.reset_play(0.0);
                self.id = 0;
                self.grid = None;
                out.extend(self.audio.take().map(Trash::Audio));
                out.extend(self.stems.take().map(Trash::Stems));
            }
            DeckCmd::Stems { id, stems } => match id == self.id && self.audio.is_some() {
                true => out.extend(self.stems.replace(stems).map(Trash::Stems)),
                false => out.push(Trash::Stems(stems)),
            },
            DeckCmd::StemGain { stem, gain } if stem < STEMS => {
                self.stem_want[stem] = gain.clamp(0.0, 1.0)
            }
            DeckCmd::StemGain { .. } => {}
            DeckCmd::Grid(g) => self.grid = g,
            DeckCmd::Play => {
                if self.preview.take().is_some() {
                    // Play pressed while a cue's held: it plays on once that's let go.
                    self.held_hot = None;
                } else if self.playing {
                    self.playing = false;
                    self.motion = Motion::Free;
                } else if self.audio.is_some() {
                    self.playing = true;
                    self.align = true;
                }
            }
            DeckCmd::Playing(on) => {
                if on != self.playing() {
                    self.take(DeckCmd::Play, out);
                }
            }
            DeckCmd::Cue(down) => self.cue_button(down),
            DeckCmd::HotCue { slot, down } if slot < HOT_CUES => self.hot_cue(slot, down),
            DeckCmd::HotCue { .. } => {}
            DeckCmd::SetHotCue { slot, at } => {
                if let Some(h) = self.hot.get_mut(slot) {
                    *h = at;
                }
            }
            DeckCmd::Seek(at) => {
                let at = self.in_time(at);
                self.go(at);
            }
            DeckCmd::Tempo(t) => self.tempo = t.clamp(0.0, 4.0),
            DeckCmd::Bend(b) => self.bend = 1.0 + b,
            DeckCmd::Nudge(turns) => self.nudge += turns * SECONDS_PER_TURN * self.rate * 0.1,
            DeckCmd::Touch(on) => {
                if on && !self.touched {
                    self.touched = true;
                    self.pending = 0.0;
                    self.scratch_vel = self.vel;
                    self.slip_start(false);
                } else if !on && self.touched {
                    self.touched = false;
                    self.slip_end();
                }
            }
            DeckCmd::Scratch(turns) => match self.touched {
                true => self.pending += turns * SECONDS_PER_TURN * self.rate,
                false => self.nudge += turns * SECONDS_PER_TURN * self.rate * 0.1,
            },
            DeckCmd::Keylock(on) => self.keylock = on,
            DeckCmd::Key(k) => self.key = k.clamp(-12.0, 12.0),
            DeckCmd::Reverse(on) => {
                self.reverse = on;
                match on {
                    true => self.slip_start(false),
                    false => self.slip_end(),
                }
            }
            DeckCmd::Censor(on) => {
                self.censor = on;
                match on {
                    true => self.slip_start(true),
                    false => self.slip_end(),
                }
            }
            DeckCmd::Slip(on) => {
                self.slip = on;
                if !on {
                    self.shadow = None;
                }
            }
            DeckCmd::Brake(seconds) if self.playing => {
                self.slip_start(false);
                self.motion = Motion::Brake {
                    per_frame: 1.0 / (seconds.max(0.01) * self.rate),
                };
            }
            DeckCmd::Spinback(seconds) if self.playing => {
                self.slip_start(false);
                let v = -3.0 * self.tempo.max(0.5);
                self.motion = Motion::Spin {
                    vel: v,
                    decay: (0.02f64 / v.abs()).powf(1.0 / (seconds.max(0.05) * self.rate)),
                };
            }
            DeckCmd::Brake(_) | DeckCmd::Spinback(_) => {}
            DeckCmd::BeatJump(beats) => match self.lp {
                Some(lp) if lp.on => self.take(DeckCmd::LoopMove(beats), out),
                _ => self.go(self.pos + beats * self.period()),
            },
            DeckCmd::AutoLoop(beats) => {
                let len = beats * self.period();
                match self.lp {
                    Some(lp) if lp.on && (lp.len() - len).abs() < 1.0 && !self.roll => {
                        self.exit_loop()
                    }
                    _ => {
                        let start = self.loop_start_here(beats);
                        self.slip_start(false);
                        self.roll = false;
                        self.set_loop(start, start + len);
                    }
                }
            }
            DeckCmd::LoopIn => self.loop_in = Some(self.snapped(self.pos)),
            DeckCmd::LoopOut => match self.loop_in {
                Some(a) => {
                    let b = self.snapped(self.pos);
                    if b > a + STEP as f64 {
                        self.slip_start(false);
                        self.set_loop(a, b);
                        self.loop_in = None;
                    }
                }
                None => self.take(DeckCmd::Reloop, out),
            },
            DeckCmd::Reloop => match self.lp {
                Some(lp) if lp.on => self.exit_loop(),
                Some(lp) => {
                    self.lp = Some(Loop { on: true, ..lp });
                    if self.pos < lp.start || self.pos >= lp.end {
                        self.go(lp.start);
                    }
                }
                None => {}
            },
            DeckCmd::LoopExit => self.exit_loop(),
            DeckCmd::LoopScale(f) => {
                if let Some(lp) = self.lp {
                    let len = (lp.len() * f).max(self.period() / 32.0);
                    self.set_loop(lp.start, lp.start + len);
                    if !lp.on {
                        self.lp = Some(Loop {
                            on: false,
                            ..self.lp.unwrap()
                        });
                    }
                }
            }
            DeckCmd::LoopMove(beats) => {
                if let Some(lp) = self.lp {
                    let d = beats * self.period();
                    self.lp = Some(Loop {
                        start: lp.start + d,
                        end: lp.end + d,
                        on: lp.on,
                    });
                    self.go(self.pos + d);
                }
            }
            DeckCmd::Roll { beats, on } => {
                if on {
                    let start = self.loop_start_here(beats);
                    self.slip_start(true);
                    self.roll = true;
                    self.set_loop(start, start + beats * self.period());
                } else if self.roll {
                    self.roll = false;
                    self.lp = None;
                    self.slip_end();
                }
            }
            DeckCmd::SetLoop(lp) => {
                self.lp = lp;
                self.roll = false;
            }
            DeckCmd::Sync(on) => {
                self.sync = on;
                self.align = on;
                if !on {
                    self.sync_bend = 1.0;
                }
            }
        }
    }

    fn reset_play(&mut self, at: f64) {
        self.playing = false;
        self.preview = None;
        self.held_hot = None;
        self.pos = at;
        self.jump = None;
        self.mode = Mode::Vinyl;
        self.vel = 0.0;
        self.gate = 0.0;
        self.lp = None;
        self.loop_in = None;
        self.roll = false;
        self.shadow = None;
        self.motion = Motion::Free;
        self.speed = 1.0;
        self.reverse = false;
        self.censor = false;
        self.touched = false;
        self.pending = 0.0;
        self.nudge = 0.0;
        self.sync_bend = 1.0;
        self.key = 0.0;
        self.seam = None;
        self.stem_want = [1.0; STEMS];
        self.stem_gain = [1.0; STEMS];
    }

    /// The CDJ's cue button. Playing: back to the cue, stopped. Stopped: the cue set here (if it
    /// moved), and the track heard from it while the button's held.
    fn cue_button(&mut self, down: bool) {
        if self.audio.is_none() {
            return;
        }
        match down {
            true if self.playing && self.preview.is_none() => {
                self.playing = false;
                self.shadow = None;
                self.motion = Motion::Free;
                self.go(self.cue);
            }
            true if !self.playing => {
                if (self.pos - self.cue).abs() > 1.0 {
                    self.cue = self.snapped(self.pos);
                }
                self.go(self.cue);
                self.preview = Some((self.cue, None));
                self.playing = true;
            }
            false => {
                if let Some((back, None)) = self.preview {
                    self.preview = None;
                    self.playing = false;
                    self.go(back);
                }
            }
            _ => {}
        }
    }

    /// A hot cue's pad. Empty, it's set here. Playing, the track jumps there (in time, with
    /// quantize). Stopped, it plays from there while held, then goes back.
    fn hot_cue(&mut self, slot: usize, down: bool) {
        if self.audio.is_none() {
            return;
        }
        if !down {
            if let Some((back, Some(s))) = self.preview
                && s == slot
            {
                self.preview = None;
                self.playing = false;
                self.go(back);
            }
            if self.held_hot == Some(slot) {
                self.held_hot = None;
                self.slip_end();
            }
            return;
        }
        match self.hot[slot] {
            None => self.hot[slot] = Some(self.snapped(self.pos)),
            Some(at) if self.playing && self.preview.is_none() => {
                self.slip_start(false);
                self.held_hot = Some(slot);
                let at = self.in_time(at);
                self.go(at);
            }
            Some(at) => {
                self.go(at);
                self.preview = Some((at, Some(slot)));
                self.playing = true;
            }
        }
    }

    /// `at`, on the beat grid if quantize is on.
    fn snapped(&self, at: f64) -> f64 {
        match (self.quantize, self.grid) {
            (true, Some(g)) => g.snap(at, self.rate, 1.0),
            _ => at,
        }
    }

    /// Where to jump to land on `at` in time: playing with quantize on, as far through its beat
    /// as the track is through this one, so the beat carries on unbroken.
    fn in_time(&self, at: f64) -> f64 {
        match (self.quantize && self.playing, self.grid) {
            (true, Some(g)) => {
                let shift = wrap_half(g.phase(self.pos, self.rate) - g.phase(at, self.rate));
                at + shift * g.period(self.rate)
            }
            _ => at,
        }
    }

    /// Where a loop of `beats` starts from here: the beat it's in (or its part of one, for a
    /// loop under a beat), with quantize; else right here.
    fn loop_start_here(&self, beats: f64) -> f64 {
        match (self.quantize, self.grid) {
            (true, Some(g)) => g.floor(self.pos, self.rate, beats.min(1.0)),
            _ => self.pos,
        }
    }

    fn set_loop(&mut self, start: f64, end: f64) {
        self.lp = Some(Loop {
            start,
            end,
            on: true,
        });
        // Already past its end (a quantized end behind the playhead): round into it.
        if self.pos >= end {
            self.go(start + (self.pos - start).rem_euclid(end - start));
        }
    }

    fn exit_loop(&mut self) {
        if let Some(lp) = &mut self.lp {
            lp.on = false;
        }
        if self.roll {
            self.roll = false;
            self.lp = None;
        }
        self.slip_end();
    }

    fn go(&mut self, at: f64) {
        self.jump = Some(at);
    }

    /// Slip: from here the track carries on underneath, to come back to (`always` for rolls and
    /// censor, which slip whether or not slip is on).
    fn slip_start(&mut self, always: bool) {
        if (self.slip || always) && self.shadow.is_none() && self.playing {
            self.shadow = Some(self.jump.unwrap_or(self.pos));
        }
    }

    fn slip_end(&mut self) {
        if let Some(s) = self.shadow.take() {
            self.go(s);
        }
    }

    /// How fast the track moves this block, frames per frame.
    fn velocity(&mut self, n: usize) -> f64 {
        if self.touched {
            // The platter under a hand: as fast as it's been turned lately, smoothed.
            let want = self.pending / n as f64;
            self.scratch_vel += (want - self.scratch_vel) * 0.5;
            self.pending -= self.scratch_vel * n as f64;
            return self.scratch_vel;
        }
        match self.motion {
            Motion::Spin { vel, decay } => {
                let v = vel * decay.powi(n as i32);
                if v.abs() > 0.02 {
                    self.motion = Motion::Spin { vel: v, decay };
                    return v;
                }
                self.stop_motion();
                return 0.0;
            }
            Motion::Brake { per_frame } => {
                self.speed -= per_frame * n as f64;
                if self.speed <= 0.0 {
                    self.stop_motion();
                    return 0.0;
                }
            }
            Motion::Free => {}
        }
        if !self.playing() {
            return 0.0;
        }
        let push = self.nudge * 0.2;
        self.nudge -= push;
        let dir = if self.reverse != self.censor {
            -1.0
        } else {
            1.0
        };
        dir * self.tempo * self.bend * self.sync_bend * self.speed + push / n as f64
    }

    /// A brake or spinback's end: slipping, back to where the track got to; else stopped.
    fn stop_motion(&mut self) {
        self.motion = Motion::Free;
        self.speed = 1.0;
        match self.shadow.take() {
            Some(s) => self.go(s),
            None => self.playing = false,
        }
    }

    fn wants_stretch(&self, v: f64) -> bool {
        (self.keylock || self.key != 0.0)
            && !self.touched
            && self.motion == Motion::Free
            && (STRETCH_MIN..=STRETCH_MAX).contains(&v.abs())
    }

    /// Plays a block into `l` and `r`.
    pub fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        let Some(audio) = self.audio.clone() else {
            l.fill(0.0);
            r.fill(0.0);
            return;
        };
        // A hand on the platter, a brake or a spinback glide from one block's speed to the next;
        // the rest (play, pause, the pitch fader) take effect at once.
        let smooth = self.touched || self.motion != Motion::Free || self.smooth;
        let v1 = self.velocity(n);
        self.smooth = self.touched || self.motion != Motion::Free;
        let v0 = if smooth { self.vel } else { v1 };
        if v1 == 0.0 && !smooth {
            self.stopped(&audio, l, r);
            self.after(&audio, 0.0, n);
            return;
        }
        let mut i = 0;
        while i < n {
            let m = STEP.min(n - i);
            let a = v0 + (v1 - v0) * (i as f64 / n as f64);
            let b = v0 + (v1 - v0) * ((i + m) as f64 / n as f64);
            self.step(&audio, &mut l[i..i + m], &mut r[i..i + m], a, b);
            i += m;
        }
        self.vel = v1;
        // Moving, heard; still under a hand, quiet (and never a click between).
        let want = if v1.abs() > 1e-4 { 1.0 } else { 0.0 };
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            self.gate += (want - self.gate) * 0.02;
            *a *= self.gate;
            *b *= self.gate;
        }
        self.after(&audio, v1, n);
    }

    /// Stopped: if it was moving, its last moment faded out (where it was, which is where it
    /// stays), then quiet. A jump just moves it; keylock starts afresh when it plays again.
    fn stopped(&mut self, audio: &Audio, l: &mut [f32], r: &mut [f32]) {
        l.fill(0.0);
        r.fill(0.0);
        if self.vel != 0.0 && self.gate > 1e-4 {
            let (from, v) = (self.pos, self.vel);
            let m = STEP.min(l.len());
            self.play(audio, &mut l[..m], &mut r[..m], v, v);
            for k in 0..m {
                let g = self.gate * (1.0 - (k + 1) as f32 / m as f32);
                l[k] *= g;
                r[k] *= g;
            }
            self.pos = from;
        }
        if let Some(j) = self.jump.take() {
            self.pos = j;
        }
        self.mode = Mode::Vinyl;
        self.seam = None;
        self.vel = 0.0;
        self.gate = 0.0;
    }

    fn after(&mut self, audio: &Audio, v: f64, n: usize) {
        if let Some(s) = &mut self.shadow
            && self.playing
        {
            *s += self.tempo * self.sync_bend * n as f64;
        }
        let len = audio.len() as f64;
        let looping = self.lp.is_some_and(|lp| lp.on);
        if (v > 0.0 && self.pos >= len) || (v < 0.0 && self.pos <= 0.0 && !looping) {
            self.pos = self.pos.clamp(0.0, len);
            self.playing = false;
            self.preview = None;
            self.motion = Motion::Free;
            self.speed = 1.0;
            self.shadow = None;
        }
    }

    /// A frame as heard: through the stems at their levels, when there are stems and any is
    /// turned down.
    #[inline]
    fn at(&self, audio: &Audio, i: isize) -> (f32, f32) {
        match &self.stems {
            Some(st) if self.stem_gain != [1.0; STEMS] => st.frame(audio, i, &self.stem_gain),
            _ => audio.at(i),
        }
    }

    /// Between frames (a 4-point Hermite curve), through the stems.
    #[inline]
    fn read(&self, audio: &Audio, pos: f64) -> (f32, f32) {
        if self.stems.is_none() || self.stem_gain == [1.0; STEMS] {
            return audio.read(pos);
        }
        let i = pos.floor();
        let t = (pos - i) as f32;
        let i = i as isize;
        let (a, b, c, d) = (
            self.at(audio, i - 1),
            self.at(audio, i),
            self.at(audio, i + 1),
            self.at(audio, i + 2),
        );
        (
            crate::audio::hermite(a.0, b.0, c.0, d.0, t),
            crate::audio::hermite(a.1, b.1, c.1, d.1, t),
        )
    }

    /// A step: on as it was, or (a jump, or keylock coming or going) the old way out and the
    /// new way in, crossfaded.
    fn step(&mut self, audio: &Audio, l: &mut [f32], r: &mut [f32], v0: f64, v1: f64) {
        // Stems glide to their levels a step at a time: a mute's a quick fade, not a click.
        for k in 0..STEMS {
            let d = self.stem_want[k] - self.stem_gain[k];
            self.stem_gain[k] += d.clamp(-0.04, 0.04);
        }
        let want = self.wants_stretch(v1);
        let jump = self.jump.take();
        let stretching = matches!(self.mode, Mode::Stretch(_));
        if jump.is_none() && want == stretching {
            self.play(audio, l, r, v0, v1);
            return;
        }
        let m = l.len();
        let from = self.pos;
        let (mut al, mut ar) = (std::mem::take(&mut self.al), std::mem::take(&mut self.ar));
        self.play(audio, &mut al[..m], &mut ar[..m], v0, v1);
        self.seam = None;
        self.pos = jump.unwrap_or(from);
        self.mode = match want {
            true => {
                let h = match self.mode {
                    Mode::Stretch(h) => 1 - h,
                    Mode::Vinyl => 0,
                };
                self.seek(audio, h, v1);
                Mode::Stretch(h)
            }
            false => Mode::Vinyl,
        };
        self.play(audio, l, r, v0, v1);
        for k in 0..m {
            let t = (k as f32 + 0.5) / m as f32;
            l[k] = al[k] * (1.0 - t) + l[k] * t;
            r[k] = ar[k] * (1.0 - t) + r[k] * t;
        }
        (self.al, self.ar) = (al, ar);
    }

    fn play(&mut self, audio: &Audio, l: &mut [f32], r: &mut [f32], v0: f64, v1: f64) {
        match self.mode {
            Mode::Vinyl => self.play_vinyl(audio, l, r, v0, v1),
            Mode::Stretch(h) => self.play_stretch(audio, h, l, r, v1),
        }
    }

    /// As a record: read at the speed it's going, pitch and all; a loop's end wraps, by the frame.
    fn play_vinyl(&mut self, audio: &Audio, l: &mut [f32], r: &mut [f32], v0: f64, v1: f64) {
        let m = l.len();
        let lp = self.lp.filter(|lp| lp.on);
        for k in 0..m {
            let v = v0 + (v1 - v0) * (k as f64 / m as f64);
            let (mut a, mut b) = self.read(audio, self.pos);
            if let Some((at, left)) = self.seam {
                let (ta, tb) = self.read(audio, at);
                let g = left as f32 / SEAM as f32;
                a = a * (1.0 - g) + ta * g;
                b = b * (1.0 - g) + tb * g;
                self.seam = (left > 1).then_some((at + v, left - 1));
            }
            l[k] = a;
            r[k] = b;
            let prev = self.pos;
            self.pos += v;
            if let Some(lp) = lp {
                let len = lp.len();
                if v > 0.0 && prev >= lp.start && prev < lp.end && self.pos >= lp.end {
                    self.seam = Some((self.pos, SEAM));
                    self.pos -= len;
                } else if v < 0.0 && prev >= lp.start && prev < lp.end && self.pos < lp.start {
                    self.seam = Some((self.pos, SEAM));
                    self.pos += len;
                }
            }
        }
    }

    /// Keylock: the stretcher fed the track at the speed it's going, giving it back at the key
    /// it was (moved by the key shift). A loop's end comes back round as a jump on the next step.
    fn play_stretch(&mut self, audio: &Audio, h: usize, l: &mut [f32], r: &mut [f32], v: f64) {
        let m = l.len();
        let s = v.abs().clamp(STRETCH_MIN, STRETCH_MAX);
        let dir = if v < 0.0 { -1.0 } else { 1.0 };
        let semis = self.key + if self.keylock { 0.0 } else { 12.0 * s.log2() };
        let acc = self.heads[h].acc + s * m as f64;
        let k = acc.floor() as usize;
        self.heads[h].acc = acc - k as f64;
        let mut read = self.heads[h].read;
        for i in 0..k {
            let (a, b) = self.at(audio, read as isize);
            self.inbuf[2 * i] = a;
            self.inbuf[2 * i + 1] = b;
            read += dir;
        }
        let head = &mut self.heads[h];
        head.read = read;
        head.st.set_transpose_factor_semitones(semis as f32, None);
        head.st
            .process(&self.inbuf[..2 * k], &mut self.outbuf[..2 * m]);
        for i in 0..m {
            l[i] = self.outbuf[2 * i];
            r[i] = self.outbuf[2 * i + 1];
        }
        let prev = self.pos;
        self.pos = head.read - dir * head.lag(s);
        if let Some(lp) = self.lp.filter(|lp| lp.on)
            && prev >= lp.start
            && prev < lp.end
        {
            if dir > 0.0 && self.pos >= lp.end {
                self.jump = Some(self.pos - lp.len());
            } else if dir < 0.0 && self.pos < lp.start {
                self.jump = Some(self.pos + lp.len());
            }
        }
    }

    /// Stretcher `h` made ready to be heard from `self.pos` at speed `v`: reading ahead by its
    /// latency, primed with what comes before that.
    fn seek(&mut self, audio: &Audio, h: usize, v: f64) {
        let s = v.abs().clamp(STRETCH_MIN, STRETCH_MAX);
        let dir = if v < 0.0 { -1.0 } else { 1.0 };
        let semis = self.key + if self.keylock { 0.0 } else { 12.0 * s.log2() };
        let pre = self.pre;
        let read = (self.pos + dir * self.heads[h].lag(s)).round();
        for i in 0..pre {
            let at = read - dir * (pre - i) as f64;
            let (a, b) = self.at(audio, at as isize);
            self.inbuf[2 * i] = a;
            self.inbuf[2 * i + 1] = b;
        }
        let head = &mut self.heads[h];
        head.read = read;
        head.st.set_transpose_factor_semitones(semis as f32, None);
        head.st.seek(&self.inbuf[..2 * pre], s);
        head.acc = 0.0;
    }

    pub fn status(&self) -> DeckStatus {
        let nan = f64::NAN;
        let lp = self.lp;
        DeckStatus {
            id: self.id,
            loaded: self.audio.is_some(),
            len: self.audio.as_ref().map_or(0.0, |a| a.len() as f64),
            pos: self.jump.unwrap_or(self.pos),
            playing: self.playing(),
            previewing: self.preview.is_some(),
            speed: self.vel,
            tempo: self.tempo,
            bpm: self.bpm(),
            beat: self.beat().unwrap_or(nan),
            keylock: self.keylock,
            key: self.key,
            stretching: matches!(self.mode, Mode::Stretch(_)),
            sync: self.sync,
            slip: self.slip,
            shadow: self.shadow.unwrap_or(nan),
            reverse: self.reverse || self.censor,
            touched: self.touched,
            cue: self.cue,
            hot: self.hot.map(|h| h.unwrap_or(nan)),
            loop_start: lp.map_or(nan, |l| l.start),
            loop_end: lp.map_or(nan, |l| l.end),
            loop_on: lp.is_some_and(|l| l.on),
            roll: self.roll,
            loop_in: self.loop_in.unwrap_or(nan),
            stems: self.stems.is_some(),
            stem_gain: self.stem_want,
        }
    }

    /// Moves what's heard by `frames` (sync lining the beats up).
    pub(crate) fn shift(&mut self, frames: f64) {
        self.go(self.jump.unwrap_or(self.pos) + frames);
    }
}
