//! Stem models: those Pina Studio has been run with, downloaded from where their makers host
//! them when asked (Pina Live ships none), others added from a file, and removing them. The
//! folder is Pina Studio's own, so a model got in either app is in both.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, channel},
    },
};

use gpui::{Context, Div, FontWeight, MouseButton, div, prelude::*, rgb};
use pina_engine::separate::{
    self,
    catalog::{self, KNOWN, Known},
};

use crate::{
    live::Live,
    ui::{scale::px, theme::*},
};

/// A model coming down.
pub struct Download {
    pub id: &'static str,
    pub got: Arc<AtomicU64>,
    pub stop: Arc<AtomicBool>,
    pub done: Receiver<Result<(), String>>,
}

/// How much quicker a model parts a track here than on one run: the pieces a track's cut into
/// run at once (crate::stems, part::pieces).
// shortcut: measured on an M4 with HTDemucs (crates/engine/examples/part.rs); other machines
// and models scale differently, so this only shapes the estimate shown.
const SPEEDUP: f32 = 2.0;

/// Roughly how long a three-minute song takes `k` here.
fn song_time(k: &Known) -> String {
    let s = k.speed * 180.0 / SPEEDUP;
    match s < 90.0 {
        true => format!("about {:.0} s a song", (s / 10.0).round().max(1.0) * 10.0),
        false => format!("about {:.0} min a song", (s / 60.0).round()),
    }
}

/// What a known model gives and how well, as Pina Studio measured it (its time left out: that
/// was on one run).
fn heard(k: &Known) -> String {
    k.heard
        .split(" · ")
        .filter(|p| !p.starts_with("about "))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn mb(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1e6)
}

impl Live {
    pub fn download_model(&mut self, k: &'static Known, cx: &mut Context<Self>) {
        if self.download.is_some() {
            return;
        }
        let got = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        {
            let (got, stop) = (got.clone(), stop.clone());
            std::thread::spawn(move || {
                let r = catalog::download(
                    k,
                    &separate::folder(),
                    |b| got.store(b, Ordering::Relaxed),
                    &stop,
                )
                .map(|_| ());
                let _ = tx.send(r);
            });
        }
        self.download = Some(Download {
            id: k.id,
            got,
            stop,
            done: rx,
        });
        self.model_note = None;
        cx.notify();
    }

    /// A finished download picked up.
    pub fn poll_download(&mut self) {
        let Some(d) = &self.download else { return };
        let Ok(r) = d.done.try_recv() else { return };
        self.download = None;
        self.model_note = match r {
            Ok(()) => None,
            Err(e) if e == "Stopped" => None,
            Err(e) => Some(e),
        };
    }

    /// A model's folder taken out of Models (asked twice: the first click asks).
    fn remove_model(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        if self.remove_armed.as_ref() != Some(&dir) {
            self.remove_armed = Some(dir);
        } else {
            self.remove_armed = None;
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                self.model_note = Some(format!("Couldn't remove it: {e}"));
            }
        }
        cx.notify();
    }

    /// An ONNX file the person has, copied in (a known one known by its size).
    fn add_model(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Add".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(files))) = picked.await else {
                return;
            };
            let Some(file) = files.into_iter().next() else {
                return;
            };
            this.update(cx, |live, cx| {
                live.model_note = Some("Adding it…".into());
                cx.notify();
            })
            .ok();
            let added = cx
                .background_spawn(async move { catalog::add(&file, &separate::folder()) })
                .await;
            this.update(cx, |live, cx| {
                live.model_note = match added {
                    Ok((_, true)) => None,
                    Ok((dir, false)) => Some(format!(
                        "Added, but it isn't one Pina knows: name its stems in {}",
                        dir.join("model.json").display()
                    )),
                    Err(e) => Some(e),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The models panel: each known model to download (or there already), any others added,
    /// adding a file, and the folder.
    pub fn models_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (have, bad) = separate::models(&separate::folder());
        let mut rows = div().flex().flex_col();
        for k in KNOWN {
            let about = k.about();
            let installed = have.iter().find(|m| m.about.name == about.name);
            let coming = self.download.as_ref().filter(|d| d.id == k.id);
            let action: Div = match (installed, coming) {
                (Some(m), _) => {
                    let armed = self.remove_armed.as_ref() == Some(&m.dir);
                    let dir = m.dir.clone();
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().text_color(rgb(MINT)).child("Installed"))
                        .child(
                            button(if armed { "Remove it?" } else { "Remove" }, false)
                                .h(px(24.))
                                .when(armed, |b| b.text_color(rgb(RECORD)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |live, _, _, cx| {
                                        live.remove_model(dir.clone(), cx)
                                    }),
                                ),
                        )
                }
                (None, Some(d)) => {
                    let got = d.got.load(Ordering::Relaxed);
                    let p = (got as f64 / k.bytes as f64).clamp(0.0, 1.0) as f32;
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .w(px(90.))
                                .h(px(4.))
                                .rounded(px(2.))
                                .bg(rgb(RAISED))
                                .child(
                                    div()
                                        .h_full()
                                        .w(gpui::relative(p))
                                        .rounded(px(2.))
                                        .bg(rgb(ACCENT)),
                                ),
                        )
                        .child(
                            div()
                                .w(px(96.))
                                .font_family(MONO)
                                .text_size(px(10.5))
                                .text_color(rgb(TEXT_3))
                                .child(format!("{} / {}", mb(got), mb(k.bytes))),
                        )
                        .child(button("Stop", false).h(px(24.)).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|live, _, _, cx| {
                                if let Some(d) = &live.download {
                                    d.stop.store(true, Ordering::Relaxed);
                                }
                                cx.notify();
                            }),
                        ))
                }
                (None, None) => {
                    let busy = self.download.is_some();
                    div().child(
                        button(format!("Download · {}", mb(k.bytes)), !busy)
                            .h(px(24.))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |live, _, _, cx| live.download_model(k, cx)),
                            ),
                    )
                }
            };
            rows = rows.child(
                div()
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .border_t_1()
                    .border_color(rgb(LINE_SOFT))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(TEXT))
                                            .child(about.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(rgb(TEXT_3))
                                            .child(song_time(k)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(TEXT_2))
                                    .child(heard(k)),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(if k.open { MINT } else { TEXT_4 }))
                                    .child(format!("Licence: {}", k.licence)),
                            ),
                    )
                    .child(action),
            );
        }
        // Models added that Pina doesn't know.
        let others: Vec<_> = have
            .iter()
            .filter(|m| !KNOWN.iter().any(|k| k.about().name == m.about.name))
            .collect();
        for m in others {
            let armed = self.remove_armed.as_ref() == Some(&m.dir);
            let dir = m.dir.clone();
            rows = rows.child(
                div()
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .border_t_1()
                    .border_color(rgb(LINE_SOFT))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(m.about.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(TEXT_2))
                                    .child(format!("Added by you · {}", m.about.stems.join(", "))),
                            ),
                    )
                    .child(
                        button(if armed { "Remove it?" } else { "Remove" }, false)
                            .h(px(24.))
                            .when(armed, |b| b.text_color(rgb(RECORD)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |live, _, _, cx| live.remove_model(dir.clone(), cx)),
                            ),
                    ),
            );
        }
        let note = self.model_note.clone();
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(alpha(0x000000, 0.55))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|live, _, _, cx| {
                    live.show_models = false;
                    live.remove_armed = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("models")
                    .w(px(640.))
                    .max_h(px(720.))
                    .overflow_y_scroll()
                    .p(px(18.))
                    .rounded(px(12.))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(LINE_STRONG))
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Stem models"),
                            )
                            .child(icon_seg(Icon::Close, false, 26.0).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|live, _, _, cx| {
                                    live.show_models = false;
                                    live.remove_armed = None;
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        div()
                            .pt(px(8.))
                            .pb(px(12.))
                            .text_size(px(11.5))
                            .text_color(rgb(TEXT_3))
                            .child(
                                "Pina Live ships no models: these come from where their makers host them, \
                                 each under its own licence. They're shared with Pina Studio. A part or \
                                 two (vocals, the beat) goes to the quickest one you have; all four to the finest.",
                            ),
                    )
                    .when_some(note, |d, n| {
                        d.child(div().pb(px(10.)).text_color(rgb(RECORD)).child(n))
                    })
                    .children(bad.into_iter().map(|e| {
                        div()
                            .pb(px(6.))
                            .text_size(px(11.))
                            .text_color(rgb(AMBER))
                            .child(e)
                    }))
                    .child(rows)
                    .child(
                        div()
                            .pt(px(12.))
                            .flex()
                            .gap(px(8.))
                            .child(button("Add a model file…", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|live, _, _, cx| live.add_model(cx)),
                            ))
                            .child(button("Show the folder", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|_, _, _, cx| {
                                    let dir = separate::folder();
                                    let _ = std::fs::create_dir_all(&dir);
                                    cx.open_with_system(&dir);
                                }),
                            )),
                    ),
            )
    }
}
