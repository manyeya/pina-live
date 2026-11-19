//! Stems for the decks: a track parted by a separation model you've added to Pina Studio (the
//! same Models folder: nothing's bundled), one at a time in the background, kept so it's done
//! once; then on the deck, each part muted or alone from the Stems pads.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{Receiver, channel},
    },
    time::Instant,
};

use gpui::{Context, Div, MouseButton, div, prelude::*, rgb};
use pina_engine::separate::{self, Model};
use pina_live_engine::{
    deck::{DeckCmd, DeckStatus},
    library::data_dir,
    stems::{NAMES, STEMS, Stems},
};

use crate::{
    controls::pad,
    live::Live,
    ui::{scale::px, theme::*},
};

pub const STEM_COLORS: [u32; STEMS] = [PINK, AMBER, SKY, MINT];

/// What to part a track into. Asking for less saves time only by the model: one pass of a model
/// gives all its stems, so a part or two goes to the quickest model that gives them, and all
/// four to the finest. What wasn't asked for isn't kept: it plays as the rest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Parts {
    Vocals,
    Beat,
    VocalsBeat,
    #[default]
    All,
}

impl Parts {
    pub const ALL: [Parts; 4] = [Parts::Vocals, Parts::Beat, Parts::VocalsBeat, Parts::All];

    pub fn name(self) -> &'static str {
        match self {
            Parts::Vocals => "Vocals only",
            Parts::Beat => "Beat only",
            Parts::VocalsBeat => "Vocals and beat",
            Parts::All => "All four",
        }
    }

    /// The stems a model has to give, by the models' names for them.
    pub fn wants(self) -> &'static [&'static str] {
        match self {
            Parts::Vocals => &["vocals"],
            Parts::Beat => &["drums"],
            Parts::VocalsBeat => &["vocals", "drums"],
            Parts::All => &["vocals", "drums", "bass"],
        }
    }

    /// The model for it, of those added that give what's wanted: the quickest for a part or two,
    /// the finest for all four.
    // shortcut: a model's file size stands in for its speed (HTDemucs 165 MB parts at 0.45x real
    // time, BS-RoFormer 353 MB at ~3x); time a short excerpt per model if that stops holding.
    pub fn model(self) -> Option<Model> {
        let (found, _) = separate::models(&separate::folder());
        let size = |m: &Model| std::fs::metadata(m.file()).map_or(0, |f| f.len());
        let fits = found.into_iter().filter(|m| {
            self.wants()
                .iter()
                .all(|w| m.about.stems.iter().any(|x| x.eq_ignore_ascii_case(w)))
        });
        // Seconds a second of sound takes it: as Pina measured, for the models it knows; else
        // guessed from its file (a quarter-gigabyte model takes about a second).
        let pace = |m: &Model| {
            separate::catalog::Known::named(&m.about.name)
                .map_or(size(m) as f32 / 250e6, |k| k.speed)
        };
        match self {
            Parts::All => fits.max_by_key(size),
            _ => fits.min_by(|a, b| pace(a).total_cmp(&pace(b))),
        }
    }
}

/// Where a track's stems are kept.
pub fn stems_dir(id: u64) -> PathBuf {
    data_dir().join("stems").join(format!("{id:016x}"))
}

/// Whether a track's been parted already.
pub fn has_stems(id: u64) -> bool {
    let d = stems_dir(id);
    ["vocals", "drums", "bass"]
        .iter()
        .any(|s| d.join(format!("{s}.wav")).is_file())
}

/// A track being parted.
pub struct Job {
    pub id: u64,
    pub model: String,
    pub started: Instant,
    progress: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    done: Receiver<Result<(), String>>,
}

impl Job {
    /// One that's a part of the way through and stays there (tests).
    #[cfg(test)]
    pub fn stuck(id: u64, at: f32) -> Job {
        Job {
            id,
            model: "BS-RoFormer SW".into(),
            started: Instant::now(),
            progress: Arc::new(AtomicU32::new(at.to_bits())),
            stop: Arc::default(),
            done: channel().1,
        }
    }

    pub fn progress(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed))
    }
}

impl Live {
    /// Parting `id` started (if nothing else is being parted).
    pub fn separate(&mut self, id: u64, parts: Parts, cx: &mut Context<Self>) {
        if self.separating.is_some() {
            return;
        }
        let Some(track) = self.lib.get(id) else {
            return;
        };
        let Some(model) = parts.model() else {
            self.stem_note = Some(format!(
                "No model gives {}: add one in Pina Studio's Settings › Stem models",
                parts.name().to_lowercase()
            ));
            cx.notify();
            return;
        };
        let (file, out) = (track.path.clone(), stems_dir(id));
        let progress = Arc::new(AtomicU32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let name = model.about.name.clone();
        {
            let (progress, stop) = (progress.clone(), stop.clone());
            std::thread::spawn(move || {
                // In pieces at once, on as many cores as there are; only what was asked for kept
                // (the rest plays as the remainder). A failed one leaves what was there.
                let r = pina_live_engine::part::part(
                    &file,
                    &model,
                    &out,
                    parts.wants(),
                    pina_live_engine::part::pieces(),
                    |p| progress.store(p.to_bits(), Ordering::Relaxed),
                    &stop,
                );
                let _ = tx.send(r);
            });
        }
        self.separating = Some(Job {
            id,
            model: name,
            started: Instant::now(),
            progress,
            stop,
            done: rx,
        });
        self.stem_note = None;
        cx.notify();
    }

    pub fn stop_separating(&mut self) {
        if let Some(j) = &self.separating {
            j.stop.store(true, Ordering::Relaxed);
        }
    }

    /// A finished parting picked up: its stems onto any deck with its track.
    pub fn poll_separation(&mut self, cx: &mut Context<Self>) {
        let Some(job) = &self.separating else { return };
        let Ok(r) = job.done.try_recv() else { return };
        let id = job.id;
        self.separating = None;
        match r {
            Ok(()) => {
                for d in 0..pina_live_engine::engine::DECKS {
                    if self.decks[d].track == Some(id) {
                        self.load_stems(d, id, cx);
                    }
                }
                // One at a time: parting as tracks load, the next deck's track that has none.
                if self.settings.part_on_load {
                    let next = (0..pina_live_engine::engine::DECKS).find_map(|d| {
                        let t = self.decks[d].track.filter(|&t| t != id && !has_stems(t))?;
                        let parts = self.decks[d].parts;
                        parts.model().map(|_| (t, parts))
                    });
                    if let Some((t, parts)) = next {
                        self.separate(t, parts, cx);
                    }
                }
            }
            // Stopped or failed, it's left: not started again on its own.
            Err(e) if e == "Stopped" => {}
            Err(e) => self.stem_note = Some(e),
        }
        cx.notify();
    }

    /// A track's kept stems read (in the background) onto deck `d`.
    pub fn load_stems(&mut self, d: usize, id: u64, cx: &mut Context<Self>) {
        let (rate, len) = (self.rate, self.status().decks[d].len as usize);
        let read = cx.background_spawn(async move { Stems::load(&stems_dir(id), rate, len) });
        cx.spawn(async move |this, cx| {
            let stems = read.await;
            this.update(cx, |live, cx| {
                match stems {
                    Ok(s) if live.decks[d].track == Some(id) => {
                        live.decks[d].stem_has = s.has;
                        live.decks[d].more_parts = false;
                        live.deck(
                            d,
                            DeckCmd::Stems {
                                id,
                                stems: Arc::new(s),
                            },
                        );
                    }
                    Ok(_) => {}
                    Err(e) => live.stem_note = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The Stems pads: with stems, each part on or off, and the usual few at a touch; without,
    /// parting the track (and how far it's got).
    pub fn stem_pads(&self, i: usize, s: &DeckStatus, h: f32, cx: &mut Context<Self>) -> Div {
        if s.stems && !self.decks[i].more_parts {
            return self.part_pads(i, s, h, cx);
        }
        let id = self.decks[i].track;
        let mine = self.separating.as_ref().filter(|j| Some(j.id) == id);
        let busy_other = self.separating.is_some() && mine.is_none();
        // The box is exactly the pads' two rows; each line in it is one line (cut short with an
        // ellipsis, never wrapped), and in a short box only the button's left.
        let bh = h * 2.0 + 6.0;
        let roomy = bh >= 62.0;
        let line = |text: String, color: u32| {
            div()
                .h(px(16.))
                .line_height(px(16.))
                .w_full()
                .min_w(px(0.))
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .text_color(rgb(color))
                .child(text)
        };
        // The box, and what's in it (measured on its own, so the tests see it if it's too much
        // for the box: the box clips it, so it'd otherwise just vanish).
        let outer = div()
            .relative()
            .h(px(bh))
            .flex_none()
            .overflow_hidden()
            .flex()
            .flex_col()
            .justify_center()
            .px(px(10.))
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(CARD))
            .child(crate::probe::probe(
                &self.probes,
                format!("deck{i}.body.mid.stems"),
            ));
        let box_ = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(crate::probe::probe(
                &self.probes,
                format!("deck{i}.body.mid.stems.content"),
            ));
        let end = div();
        if !separate::available() {
            return outer.child(
                box_.child(line(
                    "Stems need Apple silicon, or Windows on x64".into(),
                    TEXT_3,
                ))
                .child(end),
            );
        }
        if let Some(job) = mine {
            let p = job.progress();
            let gone = job.started.elapsed().as_secs_f64();
            // Time left at the pace so far, once there's a pace to go by.
            let left = (p > 0.02).then(|| gone * (1.0 - p as f64) / p as f64);
            let said = match left {
                Some(l) => format!(
                    "{} · {:.0}% · about {} left",
                    job.model,
                    p * 100.0,
                    crate::decks::clock(l, false)
                ),
                None => format!(
                    "{} · {:.0}% · {}",
                    job.model,
                    p * 100.0,
                    crate::decks::clock(gone, false)
                ),
            };
            return outer.child(
                box_.when(roomy, |d| d.child(line(said, TEXT_2)))
                    .child(
                        div()
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(4.))
                                    .rounded(px(2.))
                                    .bg(rgb(RAISED))
                                    .child(
                                        div()
                                            .h_full()
                                            .w(gpui::relative(p.clamp(0.0, 1.0)))
                                            .rounded(px(2.))
                                            .bg(rgb(PINK)),
                                    ),
                            )
                            .child(button("Stop", false).h(px(22.)).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|live, _, _, cx| {
                                    live.stop_separating();
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(end),
            );
        }
        let parts = self.decks[i].parts;
        let model = parts.model();
        let say = match (&self.stem_note, &model) {
            (Some(n), _) => (n.clone(), RECORD),
            (None, Some(m)) => (format!("{} · {}", parts.name(), m.about.name), TEXT_2),
            (None, None) => ("No model gives that yet".to_string(), TEXT_2),
        };
        let back = s.stems;
        // A narrow card's picker over its button, when the box is tall enough: side by side
        // it'd be cut to "All f…".
        let stack = self.narrow && bh >= 16.0 + 6.0 + 54.0 + 12.0;
        outer.child(
            box_.when(roomy, |d| d.child(line(say.0, say.1)))
                .child(
                    div()
                        .h(px(if stack { 54. } else { 24. }))
                        .flex()
                        .when(stack, |d| d.flex_col())
                        .when(!stack, |d| d.items_center())
                        .gap(px(6.))
                        .child(
                            self.parts_dropdown(i, parts, cx)
                                .when(!stack, |d| d.flex_1()),
                        )
                        .child(match id {
                            Some(id) if model.is_some() => button(
                                if busy_other { "Waiting…" } else { "Separate" },
                                !busy_other,
                            )
                            .h(px(24.))
                            .flex_none()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |live, _, _, cx| live.separate(id, parts, cx)),
                            )
                            .into_any_element(),
                            None => div()
                                .flex_none()
                                .text_color(rgb(TEXT_4))
                                .child("Load a track")
                                .into_any_element(),
                            _ => button("Get models…", true)
                                .h(px(24.))
                                .flex_none()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|live, _, _, cx| {
                                        live.show_models = true;
                                        cx.notify();
                                    }),
                                )
                                .into_any_element(),
                        })
                        // Asked for more parts of a parted track: back to the parts it has.
                        .when(back, |d| {
                            d.child(button("Back", false).h(px(24.)).flex_none().on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |live, _, _, cx| {
                                    live.decks[i].more_parts = false;
                                    cx.notify();
                                }),
                            ))
                        }),
                )
                .child(end),
        )
    }

    /// A pick of what to part the track into, each with the model that'll do it.
    fn parts_dropdown(&self, i: usize, parts: Parts, cx: &mut Context<Self>) -> Div {
        let open = self.parts_menu == Some(i);
        let menu = div()
            .w(px(250.))
            .p(px(4.))
            .rounded(px(8.))
            .bg(rgb(CARD))
            .border_1()
            .border_color(rgb(LINE_STRONG))
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|live, _, _, cx| {
                live.parts_menu = None;
                cx.notify();
            }))
            .children(Parts::ALL.into_iter().map(|p| {
                let model = p.model().map(|m| m.about.name);
                div()
                    .h(px(28.))
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .bg(rgb(if p == parts { RAISED } else { CARD }))
                    .text_color(rgb(if model.is_some() { TEXT } else { TEXT_4 }))
                    .hover(|st| st.bg(rgb(RAISED)))
                    .child(p.name())
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(TEXT_4))
                            .child(model.unwrap_or_else(|| "no model".into())),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            cx.stop_propagation();
                            live.decks[i].parts = p;
                            live.parts_menu = None;
                            live.stem_note = None;
                            cx.notify();
                        }),
                    )
            }));
        div()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .child(dropdown(parts.name(), 24.0).min_w(px(0.)).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    live.parts_menu = if live.parts_menu == Some(i) {
                        None
                    } else {
                        Some(i)
                    };
                    cx.notify();
                }),
            ))
            .when(open, |d| {
                d.child(
                    gpui::deferred(gpui::anchored().snap_to_window().child(menu)).with_priority(1),
                )
            })
    }

    /// The parts the track has (as it was parted), each on or off, and the usual few at a touch:
    /// a part alone, the track without it, all of it; and, parted only partly, parting more.
    fn part_pads(&self, i: usize, s: &DeckStatus, h: f32, cx: &mut Context<Self>) -> Div {
        let has = self.decks[i].stem_has;
        let g = s.stem_gain;
        let narrow = self.narrow;
        // What's apart, and the rest (named by what it is).
        let rest = match has {
            [true, true, true] => NAMES[3],
            // (Not "Instrumental": that's the preset beside it, vocals off.)
            [true, false, false] => "Music",
            _ => "Rest",
        };
        let mut shown: Vec<(usize, &'static str)> = (0..3)
            .filter(|&k| has[k])
            .map(|k| (k, if k == 1 && !has[2] { "Beat" } else { NAMES[k] }))
            .collect();
        shown.push((3, rest));
        let mut top = div().flex().gap(px(6.));
        for (k, label) in shown {
            let on = g[k] > 0.5;
            let label = if narrow && label.len() > 6 {
                &label[..1]
            } else {
                label
            };
            top = top.child(
                pad(label, Some(STEM_COLORS[k]), on, h)
                    .font_family(SANS)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            let now = live.status().decks[i].stem_gain[k];
                            let gain = if now > 0.5 { 0.0 } else { 1.0 };
                            live.deck(i, DeckCmd::StemGain { stem: k, gain });
                            cx.notify();
                        }),
                    ),
            );
        }
        let preset = |label: &'static str, gains: [f32; STEMS], cx: &mut Context<Self>| {
            // Lit when what's apart is as it says (what isn't apart can't differ).
            let lit = (0..STEMS).all(|k| (k < 3 && !has[k]) || g[k] == gains[k]);
            pad(label, None, lit, h).font_family(SANS).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    for (stem, gain) in gains.into_iter().enumerate() {
                        live.deck(i, DeckCmd::StemGain { stem, gain });
                    }
                    cx.notify();
                }),
            )
        };
        let mut bottom = div().flex().gap(px(6.));
        if has[0] {
            bottom = bottom
                .child(preset(
                    if narrow { "Vox" } else { "Acapella" },
                    [1.0, 0.0, 0.0, 0.0],
                    cx,
                ))
                .child(preset(
                    if narrow { "Inst" } else { "Instrumental" },
                    [0.0, 1.0, 1.0, 1.0],
                    cx,
                ));
        }
        if has[1] {
            bottom = bottom.child(preset(
                if narrow { "Beat" } else { "Beat only" },
                [0.0, 1.0, 0.0, 0.0],
                cx,
            ));
            if !has[0] {
                bottom = bottom.child(preset("No beat", [1.0, 0.0, 1.0, 1.0], cx));
            }
        }
        bottom = bottom.child(preset("All", [1.0; STEMS], cx));
        if has != [true; 3] {
            bottom = bottom.child(
                pad(if narrow { "More" } else { "More parts" }, None, false, h)
                    .font_family(SANS)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            live.decks[i].more_parts = true;
                            live.decks[i].parts = Parts::All;
                            cx.notify();
                        }),
                    ),
            );
        }
        div().flex().flex_col().gap(px(6.)).child(top).child(bottom)
    }
}
