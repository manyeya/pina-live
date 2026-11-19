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

use gpui::{Context, Div, FontWeight, MouseButton, div, prelude::*, rgb};
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

/// The model to part tracks with: of those added, the one that gives the most of vocals, drums
/// and bass (then the most stems).
pub fn best_model() -> Option<Model> {
    let (found, _) = separate::models(&separate::folder());
    found.into_iter().max_by_key(|m| {
        let s = &m.about.stems;
        let wanted = ["vocals", "drums", "bass"]
            .iter()
            .filter(|w| s.iter().any(|x| x.eq_ignore_ascii_case(w)))
            .count();
        (wanted, s.len())
    })
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
    pub fn progress(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed))
    }
}

impl Live {
    /// Parting `id` started (if nothing else is being parted).
    pub fn separate(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.separating.is_some() {
            return;
        }
        let Some(track) = self.lib.get(id) else {
            return;
        };
        let Some(model) = best_model() else {
            self.stem_note =
                Some("Add a separation model in Pina Studio: Settings › Stem models".into());
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
                let r = separate::separate(
                    &file,
                    &model,
                    &out,
                    |p| progress.store(p.to_bits(), Ordering::Relaxed),
                    &stop,
                )
                .map(|_| ());
                if r.is_err() {
                    let _ = std::fs::remove_dir_all(&out);
                }
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
            }
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
        if s.stems {
            let g = s.stem_gain;
            let mut top = div().flex().gap(px(5.));
            for k in 0..STEMS {
                let on = g[k] > 0.5;
                top = top.child(
                    pad(
                        if self.narrow {
                            &NAMES[k][..1]
                        } else {
                            NAMES[k]
                        },
                        Some(STEM_COLORS[k]),
                        on,
                        h,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            let now = live.status().decks[i].stem_gain[k];
                            live.deck(
                                i,
                                DeckCmd::StemGain {
                                    stem: k,
                                    gain: if now > 0.5 { 0.0 } else { 1.0 },
                                },
                            );
                            cx.notify();
                        }),
                    ),
                );
            }
            let preset = |label: &'static str, gains: [f32; STEMS], cx: &mut Context<Self>| {
                let lit = g == gains;
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
            let four = self.narrow;
            let bottom = div()
                .flex()
                .gap(px(5.))
                .child(preset(
                    if four { "Vox" } else { "Acapella" },
                    [1.0, 0.0, 0.0, 0.0],
                    cx,
                ))
                .child(preset(
                    if four { "Inst" } else { "Instrumental" },
                    [0.0, 1.0, 1.0, 1.0],
                    cx,
                ))
                .child(preset(
                    if four { "Beat" } else { "Drums only" },
                    [0.0, 1.0, 0.0, 0.0],
                    cx,
                ))
                .child(preset("All", [1.0; STEMS], cx));
            return div().flex().flex_col().gap(px(5.)).child(top).child(bottom);
        }
        let id = self.decks[i].track;
        let mine = self.separating.as_ref().filter(|j| Some(j.id) == id);
        let busy_other = self.separating.is_some() && mine.is_none();
        let box_ = div()
            .h(px(h * 2.0 + 5.0))
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(6.))
            .px(px(10.))
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(CARD));
        if !separate::available() {
            return box_.child(
                div()
                    .text_color(rgb(TEXT_3))
                    .child("Stems need a Mac with Apple silicon, or Windows on x64."),
            );
        }
        if let Some(job) = mine {
            let p = job.progress();
            return box_
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_color(rgb(TEXT_2))
                                .child(format!("Parting it · {}", job.model)),
                        )
                        .child(
                            div()
                                .font_family(MONO)
                                .text_size(px(11.))
                                .text_color(rgb(TEXT_3))
                                .child({
                                    let gone = job.started.elapsed().as_secs_f64();
                                    // Time left at the pace so far, once there's a pace to go by.
                                    let left =
                                        (p > 0.02).then(|| gone * (1.0 - p as f64) / p as f64);
                                    match left {
                                        Some(l) => format!(
                                            "{:.0}%  ·  about {} left",
                                            p * 100.0,
                                            crate::decks::clock(l, false)
                                        ),
                                        None => format!(
                                            "{:.0}%  ·  {}",
                                            p * 100.0,
                                            crate::decks::clock(gone, false)
                                        ),
                                    }
                                }),
                        ),
                )
                .child(
                    div()
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
                );
        }
        let model = best_model();
        box_.child(
            div()
                .text_color(rgb(TEXT_2))
                .font_weight(FontWeight::MEDIUM)
                .child(match &model {
                    Some(_) => "Part this track into vocals, drums, bass and melody",
                    None => "No separation model yet",
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(match (&model, id) {
                (Some(m), Some(id)) => button(
                    if busy_other {
                        "Waiting for another track…".to_string()
                    } else {
                        format!("Separate · {}", m.about.name)
                    },
                    !busy_other,
                )
                .h(px(24.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |live, _, _, cx| live.separate(id, cx)),
                )
                .into_any_element(),
                (None, _) => div()
                    .text_size(px(11.))
                    .text_color(rgb(TEXT_3))
                    .child(
                        "Add one in Pina Studio › Settings › Stem models; Pina Live uses the same.",
                    )
                    .into_any_element(),
                _ => div()
                    .text_color(rgb(TEXT_4))
                    .child("Load a track first")
                    .into_any_element(),
            }),
        )
        .when_some(self.stem_note.clone(), |d, n| {
            d.child(div().text_size(px(11.)).text_color(rgb(RECORD)).child(n))
        })
    }
}
