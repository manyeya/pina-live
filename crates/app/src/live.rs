//! The window: the decks and their waveforms on top, the mixer between them, the library below.
//! It holds the engine's remote (commands out, status in), what each deck has loaded, the
//! mixer's controls as last set, and the library, analysed in the background.

use std::{
    cell::Cell,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    App, Bounds, Context, ExternalPaths, FocusHandle, Focusable, KeyBinding, MouseButton,
    MouseMoveEvent, MouseUpEvent, Pixels, Window, actions, div, prelude::*, rgb,
};
use pina_live_engine::{
    analyze::{Analysis, Wave},
    audio::Audio,
    deck::{DeckCmd, HOT_CUES},
    engine::{Command, DECKS, Engine, Remote, Status, engine},
    library::{self, Library, Track},
    mixer::{MixCmd, Side},
    output::{Output, device_rate},
};

use crate::{
    analysis::Analyzer,
    browser::Browser,
    controls::{Ctl, Drag},
    ui::{scale::px, theme::*},
};

pub const MIN_W: f32 = 1100.0;
pub const MIN_H: f32 = 700.0;
/// The loudness auto gain brings every track to.
pub const TARGET_LUFS: f32 = -10.0;

actions!(
    pina_live,
    [
        Quit,
        AddFolder,
        FocusSearch,
        ToggleFour,
        ZoomIn,
        ZoomOut,
        PlayA,
        PlayB,
        CueA,
        CueB,
        SyncA,
        SyncB,
        LoadA,
        LoadB,
        BrowseUp,
        BrowseDown,
    ]
);

pub fn bind_keys(cx: &mut App) {
    let ctx = Some("Live");
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-o", AddFolder, ctx),
        KeyBinding::new("cmd-f", FocusSearch, ctx),
        KeyBinding::new("cmd-4", ToggleFour, ctx),
        KeyBinding::new("cmd-=", ZoomIn, ctx),
        KeyBinding::new("cmd--", ZoomOut, ctx),
        KeyBinding::new("shift-left", LoadA, ctx),
        KeyBinding::new("shift-right", LoadB, ctx),
        KeyBinding::new("up", BrowseUp, ctx),
        KeyBinding::new("down", BrowseDown, ctx),
    ]);
}

/// Each deck's color, for what belongs to it.
pub const DECK_COLORS: [u32; DECKS] = [ACCENT, CORAL, MINT, VIOLET];
pub const DECK_NAMES: [&str; DECKS] = ["A", "B", "C", "D"];
/// Hot cues' colors, by pad.
pub const CUE_COLORS: [u32; HOT_CUES] = [CORAL, AMBER, MINT, SKY, VIOLET, PINK, TEAL, SLATE];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pads {
    HotCue,
    Loop,
    Roll,
    Jump,
    Stems,
}

/// A deck, as the window keeps it: what's loaded and the controls only the window knows.
pub struct DeckUi {
    pub track: Option<u64>,
    pub title: String,
    pub artist: String,
    pub wave: Option<Arc<Wave>>,
    pub analysis: Option<Analysis>,
    /// Being read: since when.
    pub loading: Option<Instant>,
    pub error: Option<String>,
    pub pads: Pads,
    /// The pitch fader's reach either way, as a fraction (0.08 is ±8%).
    pub range: f64,
    /// The pitch fader, as a fraction off the track's own speed.
    pub fader: f64,
    pub keylock: bool,
    pub slip: bool,
    pub loop_beats: f64,
    pub jump_beats: f64,
    /// Seconds it's played this load, to log it in the history once it's been heard a while.
    pub heard: f64,
    pub logged: bool,
    /// Where its platter and waveform were drawn, for the pointer.
    pub platter: Rc<Cell<Bounds<Pixels>>>,
    pub wave_at: Rc<Cell<Bounds<Pixels>>>,
    pub overview_at: Rc<Cell<Bounds<Pixels>>>,
}

impl DeckUi {
    fn new() -> DeckUi {
        DeckUi {
            track: None,
            title: String::new(),
            artist: String::new(),
            wave: None,
            analysis: None,
            loading: None,
            error: None,
            pads: Pads::HotCue,
            range: 0.08,
            fader: 0.0,
            keylock: true,
            slip: false,
            loop_beats: 4.0,
            jump_beats: 4.0,
            heard: 0.0,
            logged: false,
            platter: Rc::default(),
            wave_at: Rc::default(),
            overview_at: Rc::default(),
        }
    }
}

/// The mixer's controls as last set (the engine has them too; this is what's drawn).
pub struct MixUi {
    pub trim: [f32; DECKS],
    pub eq: [[f32; 3]; DECKS],
    pub filter: [f32; DECKS],
    pub fader: [f32; DECKS],
    pub side: [Side; DECKS],
    pub cue: [bool; DECKS],
    pub auto_gain: bool,
    pub xf: f32,
    pub master: f32,
    pub cue_mix: f32,
    pub phones: f32,
}

impl Default for MixUi {
    fn default() -> MixUi {
        MixUi {
            trim: [0.0; DECKS],
            eq: [[0.5; 3]; DECKS],
            filter: [0.0; DECKS],
            fader: [1.0; DECKS],
            side: [Side::A, Side::B, Side::A, Side::B],
            cue: [false; DECKS],
            auto_gain: true,
            xf: 0.0,
            master: 0.0,
            cue_mix: 0.5,
            phones: 0.0,
        }
    }
}

/// The engine run here, silently, when there's no sound card to run it (and in tests).
struct Offline {
    engine: Engine,
    since: Instant,
    bufs: [Vec<f32>; 4],
}

pub struct Live {
    pub rate: f64,
    pub remote: Remote,
    pub output: Option<Output>,
    offline: Option<Offline>,
    pub audio_note: Option<String>,
    pub decks: [DeckUi; DECKS],
    pub four: bool,
    /// The deck whose beat grid is being set by hand.
    pub grid_edit: Option<usize>,
    /// A right-click menu open, and where; a crate being renamed, and its field.
    pub menu: Option<(crate::crates::Menu, gpui::Point<Pixels>)>,
    pub renaming: Option<(usize, gpui::Entity<crate::ui::text::TextInput>)>,
    /// The decks drawn compact: four of them, or a window too narrow for two in full.
    pub narrow: bool,
    /// Each waveform's height, made to leave the library room in a short window.
    pub wave_h: f32,
    pub mix: MixUi,
    pub fx: [crate::fxbar::FxUi; pina_live_engine::fx::UNITS],
    /// The FX unit whose effect menu is open.
    pub fx_menu: Option<usize>,
    pub quantize: bool,
    /// Seconds across the waveforms.
    pub zoom: f32,
    pub lib: Library,
    lib_dirty: bool,
    saved_at: Instant,
    pub browser: Browser,
    pub analyzer: Analyzer,
    pub drag: Option<Drag>,
    pub settings: crate::settings::Settings,
    pub show_settings: bool,
    /// A track being parted into stems, and the last thing that went wrong doing so.
    pub separating: Option<crate::stems::Job>,
    pub stem_note: Option<String>,
    pub midi: Option<crate::midi::Midi>,
    /// The engine's second way in, for the controllers' thread.
    midi_out: std::sync::Arc<std::sync::Mutex<Option<rtrb::Producer<Command>>>>,
    /// MIDI learn: on, the control picked to map, and what was mapped last.
    pub learning: bool,
    pub armed: Option<crate::midi::Target>,
    pub learned: Option<String>,
    pub recording: Option<pina_live_engine::record::Recording>,
    /// The last recording's file, or why it failed.
    pub rec_note: Option<String>,
    /// Each channel's meter and the master's (last), as drawn: falling back between peaks.
    pub meters: [[f32; 2]; DECKS + 1],
    /// The master limiter's reduction, dB, as drawn.
    pub reduction: f32,
    /// When the status last came in, to move the waveforms on smoothly between.
    status_at: Instant,
    focus: FocusHandle,
}

impl Live {
    /// The app as it opens: the engine on the sound card, the library from last time.
    pub fn start(window: &mut Window, cx: &mut Context<Self>) -> Live {
        let settings = crate::settings::Settings::load().audio;
        let rate = device_rate(&settings).unwrap_or(48_000) as f64;
        let (e, remote) = engine(rate);
        let (output, offline, note) = match Output::start(e, &settings) {
            Ok(o) => (Some(o), None, None),
            Err(err) => {
                // Shown, and the decks still run (silently) so nothing else breaks.
                let (e, _) = engine(rate);
                (None, Some(e), Some(format!("No sound: {err}")))
            }
        };
        let mut live = Live::new(rate, remote, offline, Library::load(), window, cx);
        live.output = output;
        live.audio_note = note;
        let fresh: Vec<u64> = live.lib.scan();
        live.queue_analysis(&fresh);
        live.connect_midi(cx);
        live
    }

    /// Every MIDI input, listened to; what they do picked up here as it comes.
    pub fn connect_midi(&mut self, cx: &mut Context<Self>) {
        use futures::StreamExt;
        let maps = match &self.midi {
            Some(m) => m.mappings(),
            None => crate::midi::load_maps(),
        };
        if let Some(p) = self.remote.take_control() {
            self.midi_out = std::sync::Arc::new(std::sync::Mutex::new(Some(p)));
        }
        // The old connections close as the new ones take over.
        self.midi = None;
        let (midi, mut rx) = crate::midi::Midi::connect(self.midi_out.clone(), maps);
        midi.learning
            .store(self.learning, std::sync::atomic::Ordering::Relaxed);
        self.midi = Some(midi);
        cx.spawn(async move |this, cx| {
            while let Some(ev) = rx.next().await {
                if this.update(cx, |live, cx| live.midi_event(ev, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn set_learning(&mut self, on: bool, cx: &mut Context<Self>) {
        self.learning = on;
        self.armed = None;
        if let Some(m) = &self.midi {
            m.learning.store(on, std::sync::atomic::Ordering::Relaxed);
        }
        cx.notify();
    }

    /// While learning, a control clicked is picked to map (true: the click's used up).
    pub fn arm(&mut self, t: crate::midi::Target, cx: &mut Context<Self>) -> bool {
        if !self.learning {
            return false;
        }
        self.armed = Some(t);
        cx.notify();
        true
    }

    /// A deck command from the window's own controls (learning, it picks the control instead);
    /// whether it was done.
    pub fn act(&mut self, i: usize, c: DeckCmd) -> bool {
        use crate::midi::Target;
        if self.learning {
            let t = match c {
                DeckCmd::Play => Some(Target::Play(i)),
                DeckCmd::Sync(_) => Some(Target::Sync(i)),
                DeckCmd::Keylock(_) => Some(Target::Keylock(i)),
                DeckCmd::Slip(_) => Some(Target::Slip(i)),
                DeckCmd::AutoLoop(b) => Some(Target::AutoLoop(i, b)),
                DeckCmd::LoopScale(f) if f < 1.0 => Some(Target::LoopHalve(i)),
                DeckCmd::LoopScale(_) => Some(Target::LoopDouble(i)),
                DeckCmd::Reloop => Some(Target::Reloop(i)),
                DeckCmd::BeatJump(b) => Some(Target::BeatJump(i, b)),
                DeckCmd::Touch(_) => Some(Target::Jog(i)),
                _ => None,
            };
            if t.is_some() {
                self.armed = t;
            }
            return false;
        }
        self.remote.deck(i, c);
        true
    }

    fn midi_event(&mut self, ev: crate::midi::Event, cx: &mut Context<Self>) {
        use crate::midi::{Event, Input, Target};
        match ev {
            Event::Raw {
                device,
                kind,
                channel,
                number,
                value,
            } => {
                let (Some(t), Some(m)) = (self.armed, &self.midi) else {
                    return;
                };
                // A button's let go, a knob's settling: only what moves it maps it.
                if kind == crate::midi::Kind::Note && value == 0 {
                    return;
                }
                let made = m.learn(&device, kind, channel, number, value, t);
                self.learned = Some(format!(
                    "{} ← {} ({device})",
                    made.target.name(),
                    made.message()
                ));
                // The jog learns its touch and its turning both: it stays picked.
                if !matches!(t, Target::Jog(_)) {
                    self.armed = None;
                }
            }
            Event::Mapped(t, inp) => {
                let press = inp == Input::Button(true);
                let s = self.remote.last.decks;
                match (t, inp) {
                    (Target::Sync(d), _) if press => {
                        self.remote.deck(d, DeckCmd::Sync(!s[d].sync));
                    }
                    (Target::Keylock(d), _) if press => {
                        let on = !self.decks[d].keylock;
                        self.decks[d].keylock = on;
                        self.remote.deck(d, DeckCmd::Keylock(on));
                    }
                    (Target::Slip(d), _) if press => {
                        let on = !self.decks[d].slip;
                        self.decks[d].slip = on;
                        self.remote.deck(d, DeckCmd::Slip(on));
                    }
                    (Target::Tempo(d), Input::Value(v)) => Ctl::Tempo(d).set(self, v * 2.0 - 1.0),
                    (Target::Mix(c), Input::Value(v)) => {
                        let (lo, hi, _) = c.range();
                        let v = if (v - 0.5).abs() < 0.008 { 0.5 } else { v };
                        c.set(self, lo + v * (hi - lo));
                    }
                    (Target::Pfl(ch), _) if press => {
                        let on = !self.mix.cue[ch];
                        self.mix.cue[ch] = on;
                        self.mixer(MixCmd::Cue { ch, on });
                    }
                    (Target::FxOn(u), _) if press => {
                        self.fx[u].settings.on = !self.fx[u].settings.on;
                        self.send_fx(u);
                    }
                    (Target::FxAssign(u, ch), _) if press => {
                        let on = !self.fx[u].on[ch];
                        self.fx[u].on[ch] = on;
                        self.remote
                            .fx(pina_live_engine::fx::FxCmd::Assign { unit: u, ch, on });
                    }
                    (Target::BrowseScroll, Input::Delta(t)) => {
                        self.browse_by(t.signum() as isize, cx)
                    }
                    (Target::BrowseLoad(d), _) if press => self.load_selected(d, cx),
                    _ => {}
                }
            }
        }
        cx.notify();
    }

    /// The window over an engine `remote` drives (run here if it's `offline`).
    pub fn new(
        rate: f64,
        remote: Remote,
        offline: Option<Engine>,
        lib: Library,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Live {
        let browser = Browser::new(window, cx);
        let workers = std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(1, 6));
        let mut live = Live {
            rate,
            remote,
            output: None,
            offline: offline.map(|engine| Offline {
                engine,
                since: Instant::now(),
                bufs: std::array::from_fn(|_| vec![0.0; 4096]),
            }),
            audio_note: None,
            decks: std::array::from_fn(|_| DeckUi::new()),
            four: false,
            grid_edit: None,
            menu: None,
            renaming: None,
            narrow: false,
            wave_h: 88.0,
            mix: MixUi::default(),
            fx: Default::default(),
            fx_menu: None,
            quantize: true,
            zoom: 10.0,
            lib,
            lib_dirty: false,
            saved_at: Instant::now(),
            browser,
            analyzer: Analyzer::new(workers),
            drag: None,
            settings: crate::settings::Settings::load(),
            show_settings: false,
            separating: None,
            stem_note: None,
            midi: None,
            midi_out: Default::default(),
            learning: false,
            armed: None,
            learned: None,
            recording: None,
            rec_note: None,
            meters: [[0.0; 2]; DECKS + 1],
            reduction: 0.0,
            status_at: Instant::now(),
            focus: cx.focus_handle(),
        };
        // The mixer as drawn, sent: decks A and C on the crossfader's left, B and D its right.
        for ch in 0..DECKS {
            live.remote.mix(MixCmd::Side {
                ch,
                side: live.mix.side[ch],
            });
            live.remote
                .deck(ch, DeckCmd::Keylock(live.decks[ch].keylock));
        }
        live.remote.mix(MixCmd::CueMix(live.mix.cue_mix));
        live.remote
            .mix(MixCmd::CrossfaderCurve(live.settings.xf_curve));
        live.mix.auto_gain = live.settings.auto_gain;
        live.browser.refresh(&live.lib);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                if this.update(cx, |live, cx| live.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        window.focus(&live.focus, cx);
        live
    }

    /// Where the sound goes: the device, its rate.
    pub fn output_name(&self) -> String {
        match &self.output {
            Some(o) => format!("{} · {} kHz", o.device, self.rate / 1000.0),
            None => "Not playing out".into(),
        }
    }

    pub fn status(&self) -> &Status {
        &self.remote.last
    }

    /// Where deck `i` is heard now, seconds: the last status, moved on by the time since.
    pub fn deck_seconds(&self, i: usize) -> f64 {
        let s = &self.remote.last.decks[i];
        let since = self.status_at.elapsed().as_secs_f64().min(0.05);
        (s.pos + s.speed * self.rate * since) / self.rate
    }

    pub fn any_playing(&self) -> bool {
        self.remote
            .last
            .decks
            .iter()
            .any(|d| d.playing || d.touched)
    }

    /// The decks shown.
    pub fn shown(&self) -> usize {
        if self.four { 4 } else { 2 }
    }

    /// The status in, analysis picked up, cues kept, the library saved now and then.
    pub fn tick(&mut self, cx: &mut Context<Self>) {
        let mut changed = self.run_offline();
        if self.remote.poll() {
            self.status_at = Instant::now();
            changed = true;
        }
        self.fall_meters();
        if self.separating.is_some() {
            self.poll_separation(cx);
            changed = true;
        }
        for (id, found) in self.analyzer.poll() {
            self.analysed(id, found);
            changed = true;
        }
        // Sync moves the tempo: the pitch fader shown (and taken over) where it's put it.
        for i in 0..DECKS {
            let s = self.remote.last.decks[i];
            if s.sync && s.loaded {
                self.decks[i].fader = s.tempo - 1.0;
            }
        }
        self.keep_cues();
        self.log_plays();
        if self.lib_dirty && self.saved_at.elapsed() > Duration::from_secs(3) {
            self.save_library(cx);
        }
        if changed {
            cx.notify();
        }
    }

    /// The engine run for the time gone by, when it's run here.
    fn run_offline(&mut self) -> bool {
        let Some(o) = &mut self.offline else {
            return false;
        };
        let frames = (o.since.elapsed().as_secs_f64() * self.rate) as usize;
        if frames == 0 {
            return false;
        }
        o.since = Instant::now();
        let mut left = frames.min(self.rate as usize);
        let [ml, mr, pl, pr] = &mut o.bufs;
        while left > 0 {
            let n = left.min(ml.len());
            o.engine
                .process(&mut ml[..n], &mut mr[..n], &mut pl[..n], &mut pr[..n]);
            left -= n;
        }
        true
    }

    pub fn deck(&mut self, i: usize, c: DeckCmd) {
        self.remote.deck(i, c);
    }

    pub fn mixer(&mut self, c: MixCmd) {
        self.remote.mix(c);
    }

    /// A library track onto a deck: read in the background, the deck showing it's loading.
    pub fn load(&mut self, deck: usize, id: u64, cx: &mut Context<Self>) {
        let Some(track) = self.lib.get(id).cloned() else {
            return;
        };
        let s = self.remote.last.decks[deck];
        if self.settings.load_lock && s.playing && s.loaded {
            self.decks[deck].error = Some("Playing: stop it to load another".into());
            cx.notify();
            return;
        }
        let d = &mut self.decks[deck];
        d.track = Some(id);
        d.title = track.tags.title.clone();
        d.artist = track.tags.artist.clone();
        d.analysis = track.analysis.clone();
        d.wave = None;
        d.loading = Some(Instant::now());
        d.error = None;
        d.heard = 0.0;
        d.logged = false;
        if self.settings.keylock && !d.keylock {
            d.keylock = true;
            self.remote.deck(deck, DeckCmd::Keylock(true));
        }
        if track.needs_analysis() {
            self.analyzer.push(id, track.path.clone(), true);
        }
        let rate = self.rate;
        let path = track.path.clone();
        let read = cx.background_spawn(async move {
            let audio = Audio::load(&path, rate);
            (audio, library::load_wave(id))
        });
        cx.spawn(async move |this, cx| {
            let (audio, wave) = read.await;
            this.update(cx, |live, cx| live.loaded(deck, id, audio, wave, cx))
                .ok();
        })
        .detach();
        cx.notify();
    }

    fn loaded(
        &mut self,
        deck: usize,
        id: u64,
        audio: Result<Audio, String>,
        wave: Option<Wave>,
        cx: &mut Context<Self>,
    ) {
        // Another track's been put on it meanwhile.
        if self.decks[deck].track != Some(id) {
            return;
        }
        self.decks[deck].loading = None;
        let audio = match audio {
            Ok(a) => Arc::new(a),
            Err(e) => {
                self.decks[deck].error = Some(e);
                self.decks[deck].track = None;
                cx.notify();
                return;
            }
        };
        if self.decks[deck].wave.is_none() {
            self.decks[deck].wave = wave.map(Arc::new);
        }
        let rate = self.rate;
        let track = self.lib.get(id);
        let cue = track.map_or(0.0, |t| t.cue * rate);
        let hot =
            std::array::from_fn(|k| track.and_then(|t| t.hot[k].as_ref()).map(|h| h.at * rate));
        let grid = self.decks[deck].analysis.as_ref().and_then(|a| a.grid);
        self.deck(
            deck,
            DeckCmd::Load {
                audio,
                id,
                grid,
                cue,
                hot,
            },
        );
        self.send_gain(deck);
        if crate::stems::has_stems(id) {
            self.load_stems(deck, id, cx);
        }
        // The pitch fader as it's set carries on to the new track.
        self.deck(deck, DeckCmd::Tempo(1.0 + self.decks[deck].fader));
        cx.notify();
    }

    pub fn send_gain(&mut self, deck: usize) {
        let db = match (self.mix.auto_gain, &self.decks[deck].analysis) {
            (true, Some(a)) => a.gain_db(TARGET_LUFS),
            _ => 0.0,
        };
        self.mixer(MixCmd::AutoGain { ch: deck, db });
    }

    pub fn queue_analysis(&mut self, ids: &[u64]) {
        for &id in ids {
            if let Some(t) = self.lib.get(id)
                && t.needs_analysis()
            {
                let path = t.path.clone();
                self.analyzer.push(id, path, false);
            }
        }
    }

    fn analysed(
        &mut self,
        id: u64,
        found: Result<(Analysis, pina_live_engine::analyze::Tags, Wave), String>,
    ) {
        let Some(t) = self.lib.get_mut(id) else {
            return;
        };
        match found {
            Ok((a, tags, wave)) => {
                t.take(a, tags);
                let _ = library::save_wave(id, &wave);
                let (a, title, artist) = (
                    t.analysis.clone(),
                    t.tags.title.clone(),
                    t.tags.artist.clone(),
                );
                let wave = Arc::new(wave);
                for deck in 0..DECKS {
                    if self.decks[deck].track == Some(id) {
                        let d = &mut self.decks[deck];
                        (d.title, d.artist) = (title.clone(), artist.clone());
                        d.analysis = a.clone();
                        d.wave = Some(wave.clone());
                        let grid = a.as_ref().and_then(|a| a.grid);
                        self.deck(deck, DeckCmd::Grid(grid));
                        self.send_gain(deck);
                    }
                }
            }
            Err(e) => t.error = Some(e),
        }
        self.lib_dirty = true;
        self.browser.refresh(&self.lib);
    }

    /// Cues set on a deck kept with its track.
    fn keep_cues(&mut self) {
        let rate = self.rate;
        for deck in 0..DECKS {
            let s = self.remote.last.decks[deck];
            let Some(id) = self.decks[deck].track.filter(|&id| s.loaded && s.id == id) else {
                continue;
            };
            let Some(t) = self.lib.get_mut(id) else {
                continue;
            };
            let mut changed = false;
            if (t.cue * rate - s.cue).abs() > 1.0 {
                t.cue = s.cue / rate;
                changed = true;
            }
            for k in 0..HOT_CUES {
                let now = (!s.hot[k].is_nan()).then(|| s.hot[k] / rate);
                let was = t.hot[k].as_ref().map(|h| h.at);
                let same = match (now, was) {
                    (Some(a), Some(b)) => (a - b).abs() * rate < 1.0,
                    (None, None) => true,
                    _ => false,
                };
                if !same {
                    t.hot[k] = now.map(|at| library::HotCue {
                        at,
                        name: String::new(),
                        color: CUE_COLORS[k],
                    });
                    changed = true;
                }
            }
            self.lib_dirty |= changed;
        }
    }

    /// A track heard for half a minute (playing, and up on the mixer) goes in the history.
    fn log_plays(&mut self) {
        for deck in 0..DECKS {
            let s = self.remote.last.decks[deck];
            let up = self.mix.fader[deck] > 0.05;
            let d = &mut self.decks[deck];
            if s.playing && up && !d.logged {
                d.heard += 0.033;
                if d.heard > 30.0
                    && let Some(id) = d.track
                {
                    d.logged = true;
                    self.lib.played(id, deck as u8);
                    self.lib_dirty = true;
                }
            }
        }
    }

    fn save_library(&mut self, cx: &mut Context<Self>) {
        self.lib_dirty = false;
        self.saved_at = Instant::now();
        let lib = self.lib.clone();
        cx.background_spawn(async move {
            if let Err(e) = lib.save() {
                eprintln!("library: {e}");
            }
        })
        .detach();
    }

    pub fn add_folder(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let new = self.lib.add_folder(&dir);
        self.queue_analysis(&new);
        self.lib_dirty = true;
        self.browser.refresh(&self.lib);
        cx.notify();
    }

    pub fn remove_folder(&mut self, k: usize, cx: &mut Context<Self>) {
        if k < self.lib.folders.len() {
            let dir = self.lib.folders.remove(k);
            // Its tracks leave with it, unless they're on a deck.
            let loaded: Vec<u64> = self.decks.iter().filter_map(|d| d.track).collect();
            self.lib
                .tracks
                .retain(|t| !t.path.starts_with(&dir) || loaded.contains(&t.id));
            self.lib_dirty = true;
            self.browser.source = crate::browser::Source::All;
            self.browser.refresh(&self.lib);
        }
        cx.notify();
    }

    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        let new = self.lib.scan();
        self.queue_analysis(&new);
        self.lib_dirty = true;
        self.browser.refresh(&self.lib);
        cx.notify();
    }

    /// Recording the mix, started or stopped (the file finished in the background).
    pub fn toggle_record(&mut self, cx: &mut Context<Self>) {
        use pina_live_engine::record::{Recording, mix_path};
        match self.recording.take() {
            Some(rec) => {
                self.remote.send(Command::Record(None));
                let done = cx.background_spawn(async move { rec.finish() });
                cx.spawn(async move |this, cx| {
                    let r = done.await;
                    this.update(cx, |live, cx| {
                        live.rec_note = Some(match r {
                            Ok(p) => format!(
                                "Saved {}",
                                p.file_name()
                                    .map_or(String::new(), |n| n.to_string_lossy().into_owned())
                            ),
                            Err(e) => format!("Recording failed: {e}"),
                        });
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
            None => match Recording::start(&mix_path(&stamp()), self.rate as u32) {
                Ok((rec, producer)) => {
                    self.remote.send(Command::Record(Some(producer)));
                    self.recording = Some(rec);
                    self.rec_note = None;
                }
                Err(e) => self.rec_note = Some(format!("Can't record: {e}")),
            },
        }
        cx.notify();
    }

    fn pick_folder(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Add".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(dirs))) = picked.await {
                this.update(cx, |live, cx| {
                    for d in dirs {
                        live.add_folder(d, cx);
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    /// Files dropped on a deck (or the library): taken in, the first one loaded.
    pub fn dropped(&mut self, deck: Option<usize>, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let mut first = None;
        for p in paths.paths() {
            if p.is_dir() {
                self.add_folder(p.clone(), cx);
            } else if library::is_audio(p) {
                let id = self.lib.add_file(p);
                self.queue_analysis(&[id]);
                first = first.or(Some(id));
            }
        }
        self.lib_dirty = true;
        self.browser.refresh(&self.lib);
        if let (Some(deck), Some(id)) = (deck, first) {
            self.load(deck, id, cx);
        }
        cx.notify();
    }

    /// A deck's beat grid changed by hand (`f` given it and where the track is, seconds): sent,
    /// kept with the track, and locked from analysis.
    pub fn edit_grid(&mut self, i: usize, f: fn(&mut pina_live_engine::grid::Grid, f64)) {
        let now = self.deck_seconds(i);
        let Some(id) = self.decks[i].track else {
            return;
        };
        let mut g = self.decks[i]
            .analysis
            .as_ref()
            .and_then(|a| a.grid)
            .unwrap_or_else(|| pina_live_engine::grid::Grid::new(120.0, now));
        f(&mut g, now);
        g.bpm = g.bpm.clamp(40.0, 300.0);
        if let Some(a) = &mut self.decks[i].analysis {
            a.grid = Some(g);
        }
        if let Some(t) = self.lib.get_mut(id) {
            if let Some(a) = &mut t.analysis {
                a.grid = Some(g);
            }
            t.grid_locked = true;
        }
        self.remote.deck(i, DeckCmd::Grid(Some(g)));
        self.lib_dirty = true;
        self.browser.refresh(&self.lib);
    }

    /// The library to be saved on the next tick that saves.
    pub fn mark_dirty(&mut self) {
        self.lib_dirty = true;
    }

    pub fn track(&self, id: u64) -> Option<&Track> {
        self.lib.get(id)
    }

    /// The deck sync follows, if any.
    pub fn leader(&self) -> Option<usize> {
        self.remote.last.leader
    }

    pub fn set_quantize(&mut self, on: bool) {
        self.quantize = on;
        self.remote.send(Command::Quantize(on));
    }

    fn on_mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.drag.is_some() {
            self.drag_to(e.position, e.modifiers.shift, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = self.drag.take() {
            self.drag_end(d, cx);
        }
    }

    /// The deck the browser loads into by default: the first stopped one shown.
    pub fn free_deck(&self) -> usize {
        (0..self.shown())
            .find(|&i| !self.remote.last.decks[i].playing)
            .unwrap_or(0)
    }

    pub fn ctl_value(&self, c: Ctl) -> f32 {
        c.get(self)
    }
}

impl Live {
    pub fn focus_handle_ref(&self) -> &FocusHandle {
        &self.focus
    }
}

impl Focusable for Live {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Live {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.remote.poll() {
            self.status_at = Instant::now();
        }
        if self.any_playing()
            || self.drag.is_some()
            || self.recording.is_some()
            || self.decks.iter().any(|d| d.loading.is_some())
        {
            window.request_animation_frame();
        }
        let shown = self.shown();
        let view = window.viewport_size();
        let (w, h) = (f32::from(view.width), f32::from(view.height));
        self.narrow = self.four || w < 1300.0;
        // What's left for the waveforms once the bars, decks and a usable library are in.
        let panel = if self.narrow {
            crate::decks::PANEL_H_FOUR
        } else {
            crate::decks::PANEL_H
        };
        let spare = h
            - 40.0
            - panel
            - crate::fxbar::FX_BAR_H
            - 230.0
            - if self.learning { 34.0 } else { 0.0 };
        self.wave_h = (spare / shown as f32).clamp(40.0, if shown > 2 { 56.0 } else { 88.0 });
        div()
            .id("live")
            .relative()
            .key_context("Live")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .font_family(SANS)
            .text_size(px(12.))
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_action(cx.listener(|live, _: &AddFolder, _, cx| live.pick_folder(cx)))
            .on_action(cx.listener(|live, _: &ToggleFour, _, cx| {
                live.four = !live.four;
                cx.notify();
            }))
            .on_action(cx.listener(|live, _: &ZoomIn, _, cx| {
                live.zoom = (live.zoom / 1.5).max(2.0);
                cx.notify();
            }))
            .on_action(cx.listener(|live, _: &ZoomOut, _, cx| {
                live.zoom = (live.zoom * 1.5).min(60.0);
                cx.notify();
            }))
            .on_action(cx.listener(|live, _: &FocusSearch, window, cx| {
                live.browser.focus_search(window, cx);
            }))
            .on_action(cx.listener(|live, _: &LoadA, _, cx| live.load_selected(0, cx)))
            .on_action(cx.listener(|live, _: &LoadB, _, cx| live.load_selected(1, cx)))
            .on_action(cx.listener(|live, _: &BrowseUp, _, cx| live.browse_by(-1, cx)))
            .on_action(cx.listener(|live, _: &BrowseDown, _, cx| live.browse_by(1, cx)))
            .child(self.top_bar(cx))
            .when(self.learning, |d| d.child(self.learn_bar(cx)))
            .child(self.waveforms(shown, cx))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .when(self.four, |d| d.child(self.deck_panel(2, cx)))
                    .child(self.deck_panel(0, cx))
                    .child(self.mixer_panel(cx))
                    .child(self.deck_panel(1, cx))
                    .when(self.four, |d| d.child(self.deck_panel(3, cx))),
            )
            .child(self.fx_bar(cx))
            .child(self.browser_panel(window, cx))
            .when(self.show_settings, |d| d.child(self.settings_panel(cx)))
            .children(self.menu_overlay(cx))
    }
}

/// "2026-11-19 22.04", local time, for a recording's name.
fn stamp() -> String {
    std::process::Command::new("date")
        .arg("+%Y-%m-%d %H.%M")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| library::now().to_string())
}
