//! A deck, as a card: what's on it (title, artist, key, tempo, BPM), the whole track small to jump
//! about in, the beat ring (the phrase's sixteen beats going round, and a jog to scratch with),
//! the pads (hot cues, loops, rolls, beat jumps, stems), the loop, the pitch fader, and the
//! transport. A track dragged onto it (from the library or the Finder) says it'll load here.

use gpui::{
    Context, Div, ExternalPaths, FontWeight, MouseButton, MouseDownEvent, PathBuilder, Pixels,
    Point, canvas, div, point, prelude::*, rgb,
};
use pina_live_engine::{
    deck::{DeckCmd, DeckStatus, SECONDS_PER_TURN},
    engine::Command,
};

use pina_live_engine::grid::Grid;

use crate::{
    controls::{Ctl, Drag, Hold, pad, pt},
    live::{CUE_COLORS, DECK_COLORS, DECK_NAMES, Live, Pads},
    probe::{mark, probe},
    ui::{scale::px, theme::*},
};

/// The deck cards' and the mixer's height as they start (the layout's default; dragged to size).
pub const PANEL_H: f32 = 442.0;
/// The space round the cards and between them.
pub const GAP: f32 = 10.0;

/// A card's fixed parts: its header, the overview (or grid tools), the transport; compact and
/// in full. What's left is the body's, shared out by `budget`.
pub const HEADER_H: f32 = 60.0;
pub fn over_h(compact: bool) -> f32 {
    if compact { 56.0 } else { 62.0 }
}
pub fn transport_h(compact: bool) -> f32 {
    if compact { 52.0 } else { 58.0 }
}
const BODY_PT: f32 = 10.0;
pub const BODY_PB: f32 = 8.0;

/// What a card's height and width leave its parts: the overview, the body, a pad's height, the
/// pitch fader's, and the beat ring's size. Everything else has a fixed height, so these are
/// worked out exactly: every column ends at the body's bottom (the loop, keylock and slip, the
/// fader's reach in one line), nothing spills, and nothing's left empty. Pads grow to a
/// comfortable height; what the card has past that goes to the overview, the track drawn
/// bigger. The ring's as big as the card's width allows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    pub over: f32,
    pub body: f32,
    pub pad: f32,
    pub fader: f32,
    pub ring: f32,
}

/// The tallest a pad grows.
const PAD_MAX: f32 = 64.0;
/// The loop's row (and keylock and slip's, in line with it).
pub const ROW_H: f32 = 34.0;

pub fn budget(panel: f32, compact: bool, four: bool, card_w: f32) -> Budget {
    // The pads' column: the captions, the mode's tabs (or, four decks, its dropdown), the loop.
    let column = match (compact, four) {
        (false, _) => 12.0 + 6.0 + 32.0 + 6.0 + 6.0 + 6.0 + 14.0 + 6.0 + ROW_H,
        (true, false) => 32.0 + 6.0 + 6.0 + 6.0 + ROW_H,
        (true, true) => 26.0 + 6.0 + 6.0 + 6.0 + ROW_H,
    };
    let fixed = 2.0 + HEADER_H + transport_h(compact) + BODY_PT + BODY_PB;
    let inner = panel - fixed - over_h(compact);
    let pad = ((inner - column) / 2.0).max(22.0);
    // Pads past their tallest: the rest to the overview.
    let spare = (2.0 * (pad - PAD_MAX)).max(0.0);
    let (pad, inner) = (pad.min(PAD_MAX), inner - spare);
    // The tempo's column: its caption, its readout, its reach, and (compact) the key.
    // The tempo's column: its caption, its readout, its reach, the nudges and (compact) the key.
    let tempo = 12.0 + 14.0 + 20.0 + 18.0 + 4.0 * 5.0 + if compact { 18.0 + 5.0 } else { 0.0 };
    Budget {
        over: over_h(compact) + spare,
        body: inner + BODY_PT + BODY_PB,
        pad,
        fader: (inner - tempo).max(36.0),
        ring: (inner - ROW_H - 10.0)
            .min((card_w * 0.3).clamp(110.0, 230.0))
            .max(60.0),
    }
}

/// A section's caption: "PADS", "LOOP", "TEMPO" (12 high, exactly: the budget counts on it).
pub fn caption(s: &'static str) -> Div {
    div()
        .h(px(12.))
        .flex_none()
        .line_height(px(12.))
        .text_size(px(9.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(TEXT_4))
        .child(s)
}

pub const LOOP_SIZES: [f64; 8] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0];
pub const ROLL_SIZES: [f64; 8] = [1.0 / 32.0, 1.0 / 16.0, 0.125, 0.25, 0.5, 1.0, 2.0, 4.0];
pub const JUMPS: [f64; 8] = [-32.0, -16.0, -4.0, -1.0, 1.0, 4.0, 16.0, 32.0];
pub const RANGES: [f64; 4] = [0.08, 0.16, 0.5, 1.0];

/// "1/4", "2", "32": a size in beats.
pub fn beats_text(b: f64) -> String {
    match b < 1.0 {
        true => format!("1/{}", (1.0 / b).round()),
        false => format!("{}", b.round()),
    }
}

/// "3:07.4" or "-3:07".
pub fn clock(seconds: f64, tenths: bool) -> String {
    let neg = seconds < 0.0;
    let s = seconds.abs();
    let m = (s / 60.0).floor();
    let rest = s - m * 60.0;
    let body = match tenths {
        true => format!("{}:{:04.1}", m, rest),
        false => format!("{}:{:02}", m, rest.floor()),
    };
    if neg { format!("−{body}") } else { body }
}

impl Live {
    pub fn deck_panel(&self, i: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = self.narrow;
        let h = self.panel_h;
        let b = budget(h, compact, self.four, self.card_w);
        let s = self.status().decks[i];
        let dropping = self
            .drop_on
            .as_ref()
            .filter(|(d, _)| *d == i)
            .map(|(_, what)| what.clone());
        let color = DECK_COLORS[i];
        div()
            .id(("deck", i))
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h(px(h))
            .flex()
            .flex_col()
            .bg(rgb(SURFACE))
            .rounded(px(10.))
            .border_1()
            .border_color(rgb(LINE))
            .overflow_hidden()
            // A track dragged over it: it says it'll load here.
            .on_drag_move(cx.listener(
                move |live, e: &gpui::DragMoveEvent<crate::browser::TrackDrag>, _, cx| {
                    let what = e.drag(cx).title.clone();
                    live.drag_over(i, e.bounds.contains(&e.event.position), what, cx);
                },
            ))
            .on_drag_move(cx.listener(
                move |live, e: &gpui::DragMoveEvent<ExternalPaths>, _, cx| {
                    let what = e
                        .drag(cx)
                        .paths()
                        .first()
                        .and_then(|p| p.file_stem())
                        .map_or(String::new(), |n| n.to_string_lossy().into_owned());
                    live.drag_over(i, e.bounds.contains(&e.event.position), what, cx);
                },
            ))
            .on_drop(
                cx.listener(move |live, t: &crate::browser::TrackDrag, _, cx| {
                    live.drop_on = None;
                    live.load(i, t.id, cx);
                }),
            )
            .on_drop(cx.listener(move |live, paths: &ExternalPaths, _, cx| {
                live.drop_on = None;
                live.dropped(Some(i), paths, cx)
            }))
            .child(probe(&self.probes, format!("deck{i}")))
            .child(self.deck_header(i, &s, cx))
            .child(match self.grid_edit == Some(i) {
                true => self.grid_tray(i, b.over, cx),
                false => self.overview_strip(i, &s, b.over, cx),
            })
            .child(
                div()
                    .relative()
                    .h(px(b.body))
                    .flex_none()
                    .overflow_hidden()
                    .flex()
                    .gap(px(if compact { 10. } else { 16. }))
                    .px(px(if compact { 10. } else { 14. }))
                    .pt(px(BODY_PT))
                    .pb(px(BODY_PB))
                    .child(probe(&self.probes, format!("deck{i}.body")))
                    .when(!compact, |d| d.child(self.ring_column(i, &s, b.ring, cx)))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(probe(&self.probes, format!("deck{i}.body.mid")))
                            .when(!compact, |d| d.child(caption("PADS")))
                            .child(self.pad_tabs(i, cx))
                            .child(
                                div()
                                    .relative()
                                    .child(probe(&self.probes, format!("deck{i}.body.pads")))
                                    .child(self.pads(i, &s, b.pad, cx)),
                            )
                            .when(!compact, |d| {
                                d.child(div().pt(px(2.)).child(caption("LOOP")))
                            })
                            .child(
                                div()
                                    .relative()
                                    .child(probe(&self.probes, format!("deck{i}.body.loop")))
                                    .child(self.loop_row(i, &s, cx)),
                            ),
                    )
                    .child(self.tempo_column(i, &s, b.fader, cx)),
            )
            .child(self.transport(i, &s, cx))
            .when_some(dropping, |d, what| {
                d.child(self.drop_overlay(i, &s, what, color))
            })
    }

    /// Over the card while a track's dragged onto it: what'll happen if it's let go.
    fn drop_overlay(&self, i: usize, s: &DeckStatus, what: String, color: u32) -> Div {
        let locked = self.settings.load_lock && s.playing && s.loaded;
        let edge = if locked { RECORD } else { color };
        let off = match (&self.decks[i].track, locked) {
            (_, true) => {
                "It's playing: stop it first, or turn the load lock off in Settings".to_string()
            }
            (Some(_), false) => format!("{} comes off", self.decks[i].title),
            (None, false) => "The deck's empty".to_string(),
        };
        div()
            .absolute()
            .inset_0()
            .rounded(px(10.))
            .border_2()
            .border_color(rgb(edge))
            .bg(alpha(edge, 0.10))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .px(px(18.))
                    .py(px(12.))
                    .rounded(px(9.))
                    .bg(alpha(SURFACE, 0.96))
                    .border_1()
                    .border_color(rgb(LINE_STRONG))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(5.))
                    .child(
                        div()
                            .flex()
                            .gap(px(5.))
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(if locked { "Can't load on" } else { "Load on" })
                            .child(div().text_color(rgb(color)).child(DECK_NAMES[i])),
                    )
                    .child(
                        div()
                            .text_color(rgb(TEXT_2))
                            .max_w(px(280.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(what),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(if locked { RECORD } else { TEXT_3 }))
                            .child(off),
                    ),
            )
    }

    fn deck_header(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let d = &self.decks[i];
        let color = DECK_COLORS[i];
        let loaded = d.track.is_some();
        let compact = self.narrow;
        let title = match (&d.loading, &d.error, loaded) {
            (_, Some(e), _) => e.clone(),
            (_, _, true) => d.title.clone(),
            _ => "Drag a track here".into(),
        };
        let key = d
            .analysis
            .as_ref()
            .map(|a| a.camelot.clone())
            .unwrap_or_default();
        let shifted = s.key.abs() > 0.01;
        let bpm = match s.bpm > 0.0 {
            true => format!("{:.2}", s.bpm * self.bend_of(s)),
            false => "—".into(),
        };
        let analysing = d.track.is_some_and(|id| self.analyzer.pending(id));
        let readout = |label: &'static str, value: String, size: f32, color: u32| {
            div()
                .flex()
                .flex_col()
                .items_end()
                .child(caption(label))
                .child(
                    div()
                        .font_family(MONO)
                        .text_size(px(size))
                        .text_color(rgb(color))
                        .child(value),
                )
        };
        div()
            .relative()
            .h(px(HEADER_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(if compact { 8. } else { 12. }))
            .px(px(if compact { 10. } else { 14. }))
            .border_b_1()
            .border_color(rgb(LINE_SOFT))
            .child(probe(&self.probes, format!("deck{i}.header")))
            .child(match self.layered {
                // Four decks, two a side: which of this side's two is shown.
                true => self.layer_switch(i, cx),
                false => div()
                    .size(px(24.))
                    .flex_none()
                    .rounded(px(5.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(alpha(color, 0.16))
                    .text_color(rgb(color))
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(12.))
                    .child(DECK_NAMES[i]),
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(14.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(if d.error.is_some() {
                                RECORD
                            } else if loaded {
                                TEXT
                            } else {
                                TEXT_4
                            }))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_color(rgb(TEXT_3))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(match &d.loading {
                                Some(at) => {
                                    format!("Reading the file… {:.1}s", at.elapsed().as_secs_f32())
                                }
                                None if analysing => "Finding the beat and key…".into(),
                                None if !loaded => "or double-click one in the library".into(),
                                None => d.artist.clone(),
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .when(!key.is_empty(), |r| {
                        r.child(readout(
                            "KEY",
                            match shifted {
                                true => format!("{key} {:+}", s.key.round()),
                                false => key,
                            },
                            14.0,
                            if shifted { AMBER } else { ACCENT_TEXT },
                        ))
                    })
                    .when(!compact, |r| {
                        r.child(readout(
                            "TEMPO",
                            format!("{:+.2}%", (s.tempo - 1.0) * 100.0),
                            14.0,
                            if s.tempo == 1.0 { TEXT_3 } else { TEXT },
                        ))
                    })
                    // The BPM: clicked, the beat grid's tools.
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_end()
                            // The track's own tempo beside it, when it's playing at another.
                            .child(
                                div()
                                    .h(px(12.))
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .when(s.bpm > 0.0 && (s.tempo - 1.0).abs() > 0.0005, |d| {
                                        d.child(
                                            div()
                                                .font_family(MONO)
                                                .text_size(px(9.))
                                                .text_color(rgb(TEXT_4))
                                                .child(format!("{:.2} →", s.bpm / s.tempo)),
                                        )
                                    })
                                    .child(caption("BPM")),
                            )
                            .child(
                                div()
                                    .px(px(3.))
                                    .rounded(px(4.))
                                    .cursor_pointer()
                                    .hover(|st| st.bg(rgb(RAISED)))
                                    .when(self.grid_edit == Some(i), |d| d.bg(rgb(RAISED)))
                                    .font_family(MONO)
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(if s.sync { ACCENT_TEXT } else { TEXT }))
                                    .child(bpm)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |live, _, _, cx| {
                                            live.grid_edit = match live.grid_edit == Some(i) {
                                                true => None,
                                                false => Some(i),
                                            };
                                            cx.notify();
                                        }),
                                    ),
                            ),
                    ),
            )
            .child(mark(&self.probes, format!("deck{i}.header.end")))
    }

    /// Four decks, two a side: the two this side has (A and C, or B and D), the one shown lit.
    fn layer_switch(&self, i: usize, cx: &mut Context<Self>) -> Div {
        let side = i % 2;
        tray().children([side, side + 2].into_iter().map(|d| {
            let on = d == i;
            div()
                .w(px(22.))
                .h(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .cursor_pointer()
                .font_weight(FontWeight::BOLD)
                .text_size(px(11.))
                .bg(if on {
                    alpha(DECK_COLORS[d], 0.2)
                } else {
                    gpui::Hsla::transparent_black()
                })
                .text_color(rgb(if on { DECK_COLORS[d] } else { TEXT_4 }))
                .hover(|s| s.text_color(rgb(DECK_COLORS[d])))
                .child(DECK_NAMES[d])
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |live, _, _, cx| {
                        live.layer[side] = d;
                        cx.notify();
                    }),
                )
        }))
    }

    /// The beat grid's tools: half or double the tempo, nudge it, move the grid, or put the
    /// downbeat where the track is. Kept with the track (and left alone by analysis after).
    fn grid_tray(&self, i: usize, over: f32, cx: &mut Context<Self>) -> Div {
        let g = self.decks[i].analysis.as_ref().and_then(|a| a.grid);
        let btn = |label: &'static str, f: fn(&mut Grid, f64), cx: &mut Context<Self>| {
            seg(label, false, 24.0).px(px(7.)).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    live.edit_grid(i, f);
                    cx.notify();
                }),
            )
        };
        div()
            .relative()
            .h(px(over))
            .flex_none()
            .px(px(10.))
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(11.))
                    .text_color(rgb(TEXT_3))
                    .child("Beat grid")
                    .child(
                        div()
                            .font_family(MONO)
                            .text_color(rgb(TEXT_2))
                            .child(match g {
                                Some(g) => format!("{:.2} BPM · downbeat {:.3} s", g.bpm, g.anchor),
                                None => "none yet".into(),
                            }),
                    ),
            )
            .child(
                tray()
                    .child(btn("÷2", |g, _| g.bpm /= 2.0, cx))
                    .child(btn("×2", |g, _| g.bpm *= 2.0, cx))
                    .child(btn("−.01", |g, _| g.bpm -= 0.01, cx))
                    .child(btn("+.01", |g, _| g.bpm += 0.01, cx))
                    .child(btn("◀", |g, _| g.anchor -= 0.005, cx))
                    .child(btn("▶", |g, _| g.anchor += 0.005, cx))
                    .child(btn("Downbeat here", |g, now| g.anchor = now, cx).flex_1())
                    .child(seg("Done", true, 24.0).px(px(8.)).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|live, _, _, cx| {
                            live.grid_edit = None;
                            cx.notify();
                        }),
                    )),
            )
    }

    /// The tempo heard, with a bend or sync's hand on it (1 with neither).
    fn bend_of(&self, s: &DeckStatus) -> f64 {
        match s.tempo > 0.0 && s.playing {
            true => (s.speed.abs() / s.tempo).clamp(0.9, 1.1),
            false => 1.0,
        }
    }

    fn overview_strip(&self, i: usize, s: &DeckStatus, over: f32, cx: &mut Context<Self>) -> Div {
        let now = self.deck_seconds(i);
        let len = s.len / self.rate;
        let left = len - now;
        div()
            .relative()
            .h(px(over))
            .flex_none()
            .px(px(14.))
            .pt(px(10.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(probe(&self.probes, format!("deck{i}.over")))
            .child(
                div()
                    .id(("overview", i))
                    .h(px(over - 28.0))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                            live.seek_overview(i, e.position, cx)
                        }),
                    )
                    .child(crate::wave::overview(self, i)),
            )
            .child(
                div()
                    .h(px(14.))
                    .line_height(px(14.))
                    .flex()
                    .justify_between()
                    .font_family(MONO)
                    .text_size(px(11.))
                    .text_color(rgb(TEXT_3))
                    .child(clock(now.max(0.0), true))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .children(self.phase_meter(i, s))
                            .child(self.beat_counter(i, s)),
                    )
                    .child(
                        div()
                            .text_color(rgb(if s.loaded && s.playing && left < 30.0 {
                                RECORD
                            } else {
                                TEXT_3
                            }))
                            .child(clock(-left.max(0.0), true)),
                    ),
            )
    }

    /// How far this deck's beat is from the one it's mixed against (the leader; the leader
    /// against the next deck playing): a bar a beat wide, its middle where they're together, lit
    /// green together, amber close, red off. None with nothing to measure against.
    fn phase_meter(&self, i: usize, s: &DeckStatus) -> Option<Div> {
        let all = self.status().decks;
        let other = match self.leader() {
            Some(l) if l != i => Some(l),
            _ => (0..all.len()).find(|&d| d != i && all[d].playing && !all[d].beat.is_nan()),
        }?;
        let o = all[other];
        if s.beat.is_nan() || o.beat.is_nan() || !o.loaded || !s.loaded {
            return None;
        }
        let off = ((s.beat - o.beat + 0.5).rem_euclid(1.0) - 0.5) as f32;
        let w = 64.0;
        let color = match off.abs() {
            a if a < 0.02 => MINT,
            a if a < 0.08 => AMBER,
            _ => RECORD,
        };
        let (a, z) = (w / 2.0, w / 2.0 + off * w);
        Some(
            div()
                .relative()
                .w(px(w))
                .h(px(7.))
                .rounded(px(2.))
                .bg(rgb(WELL))
                .border_1()
                .border_color(rgb(LINE))
                // Quarter beats marked, the middle brighter.
                .children([0.25f32, 0.5, 0.75].map(|f| {
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(f * w - 0.5))
                        .w(px(1.))
                        .bg(rgb(if f == 0.5 { TEXT_3 } else { LINE_STRONG }))
                }))
                .child(
                    div()
                        .absolute()
                        .top(px(1.))
                        .bottom(px(1.))
                        .left(px(a.min(z)))
                        .w(px((a - z).abs().max(2.0)))
                        .rounded(px(1.))
                        .bg(rgb(color)),
                ),
        )
    }

    /// "33.2": the bar and beat it's at, counted from the grid's first downbeat.
    fn beat_counter(&self, i: usize, s: &DeckStatus) -> Div {
        let text = match (
            self.decks[i].analysis.as_ref().and_then(|a| a.grid),
            s.beat.is_nan(),
        ) {
            (Some(g), false) => {
                let per = g.per_bar.max(1) as f64;
                let b = s.beat.floor();
                format!("{}.{}", (b / per).floor() + 1.0, b.rem_euclid(per) + 1.0)
            }
            _ => String::new(),
        };
        div().text_color(rgb(TEXT_4)).child(text)
    }

    fn seek_overview(&mut self, i: usize, at: Point<Pixels>, cx: &mut Context<Self>) {
        let b = self.decks[i].overview_at.get();
        let w = f32::from(b.size.width).max(1.0);
        let t = ((f32::from(at.x) - f32::from(b.origin.x)) / w).clamp(0.0, 1.0) as f64;
        let len = self.status().decks[i].len;
        self.deck(i, DeckCmd::Seek(t * len));
        cx.notify();
    }

    /// The beat ring: the phrase's sixteen beats round it (the one playing lit, those gone by
    /// dim), the bar and beat in the middle. It's a jog too: grabbed, it holds the track; turned,
    /// it scratches. Keylock and slip under it.
    fn ring_column(&self, i: usize, s: &DeckStatus, size: f32, cx: &mut Context<Self>) -> Div {
        let probe_at = probe(&self.probes, format!("deck{i}.body.ring"));
        let color = DECK_COLORS[i];
        let beat = (!s.beat.is_nan()).then_some(s.beat);
        let per = self.decks[i]
            .analysis
            .as_ref()
            .and_then(|a| a.grid)
            .map_or(4, |g| g.per_bar.max(1)) as f64;
        // With no grid, the ring turns with the track as a record does.
        let turn = (self.deck_seconds(i) / SECONDS_PER_TURN).rem_euclid(1.0) as f32;
        let bounds = self.decks[i].platter.clone();
        let touched = s.touched;
        let loaded = s.loaded;
        let center = match beat {
            Some(b) => format!(
                "{}.{}",
                (b.floor() / per).floor() + 1.0,
                b.floor().rem_euclid(per) + 1.0
            ),
            None if loaded => clock(-(s.len - s.pos).max(0.0) / self.rate, false),
            None => "—".into(),
        };
        let d = &self.decks[i];
        // Under the count: the phrase it's in and the bars left of it (what's coming), else
        // what it's doing (slipping, reversed), else what the count is.
        let now = self.deck_seconds(i);
        let grid = d.analysis.as_ref().and_then(|a| a.grid);
        let phrase = d.analysis.as_ref().and_then(|a| {
            a.phrases
                .iter()
                .find(|p| (p.start as f64) <= now && now < p.end as f64)
        });
        let below: (String, u32) = match (s.slip && !s.shadow.is_nan(), s.reverse, phrase, grid) {
            (true, ..) => ("SLIPPING".into(), AMBER),
            (_, true, ..) => ("REVERSE".into(), RECORD),
            (_, _, Some(p), Some(g)) => {
                let bar = 60.0 / g.bpm * per;
                let left = ((p.end as f64 - now) / bar).ceil().max(1.0);
                let name: String = p.name.to_uppercase().chars().take(9).collect();
                (
                    format!("{name} −{left}"),
                    crate::wave::phrase_color(&p.name),
                )
            }
            _ if beat.is_some() => ("BAR · BEAT".into(), TEXT_4),
            _ if loaded => ("TIME LEFT".into(), TEXT_4),
            _ => ("NO TRACK".into(), TEXT_4),
        };
        let jog = Jog {
            color,
            beat,
            turn,
            touched,
            loaded,
            shadow: (s.slip && !s.shadow.is_nan())
                .then(|| (s.shadow / self.rate / SECONDS_PER_TURN).rem_euclid(1.0) as f32),
        };
        // The ring centred in what's over keylock and slip, which sit at the bottom.
        div()
            .relative()
            .w(px(size.max(150.0)))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .child(probe_at)
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(size))
                            .relative()
                            .cursor(gpui::CursorStyle::OpenHand)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                                    let b = live.decks[i].platter.get();
                                    let (x, y) = pt(e.position);
                                    let c = b.center();
                                    let a = (y - f32::from(c.y)).atan2(x - f32::from(c.x));
                                    if !live.act(i, DeckCmd::Touch(true)) {
                                        return;
                                    }
                                    live.drag = Some(Drag::Platter { deck: i, angle: a });
                                    cx.notify();
                                }),
                            )
                            .child(
                                canvas(
                                    move |b, _, _| bounds.set(b),
                                    move |b, _, window, _| {
                                        paint_jog(window, b, &jog);
                                    },
                                )
                                .size_full(),
                            )
                            // Its screen: where it is in the bar, and in the song.
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .font_family(MONO)
                                            .text_size(px((size * 0.13).clamp(15.0, 26.0)))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(if loaded { TEXT } else { TEXT_4 }))
                                            .child(center),
                                    )
                                    .child(
                                        div()
                                            .max_w(px(size * 0.42))
                                            .whitespace_nowrap()
                                            .overflow_hidden()
                                            .font_family(MONO)
                                            .text_size(px(8.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(below.1))
                                            .child(below.0),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .relative()
                    .h(px(ROW_H))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(probe(&self.probes, format!("deck{i}.body.ringrow")))
                    .child(self.keylock_button(i, d.keylock, cx))
                    .child(self.slip_button(i, d.slip, cx)),
            )
    }

    /// KEYLOCK, a key lit blue while the key holds whatever the tempo.
    fn keylock_button(&self, i: usize, on: bool, cx: &mut Context<Self>) -> Div {
        use crate::hw::{Led, key};
        key(
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .child("KEYLOCK")
                .child(icon(Icon::Lock, 11.).text_color(rgb(if on { TEXT } else { TEXT_3 }))),
            if on { Led::On(ACCENT) } else { Led::Off },
            false,
            32.0,
            9.0,
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |live, _, _, cx| {
                let on = !live.decks[i].keylock;
                live.decks[i].keylock = on;
                live.act(i, DeckCmd::Keylock(on));
                cx.notify();
            }),
        )
    }

    /// SLIP, lit amber while the track carries on underneath what's done to it.
    fn slip_button(&self, i: usize, on: bool, cx: &mut Context<Self>) -> Div {
        use crate::hw::{Led, key};
        key(
            "SLIP",
            if on { Led::On(AMBER) } else { Led::Off },
            false,
            32.0,
            9.0,
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |live, _, _, cx| {
                let on = !live.decks[i].slip;
                live.decks[i].slip = on;
                live.act(i, DeckCmd::Slip(on));
                cx.notify();
            }),
        )
    }

    /// The pads' modes, as a controller's: a key each, lit in its pads' colour when it's the one.
    fn pad_tabs(&self, i: usize, cx: &mut Context<Self>) -> Div {
        use crate::hw::{Led, key};
        let mode = self.decks[i].pads;
        if self.four {
            return self.pad_dropdown(i, mode, cx);
        }
        let short = self.narrow || self.card_w < 540.0;
        let tab = |label: &'static str, m: Pads, color: u32, cx: &mut Context<Self>| {
            key(
                label,
                if mode == m { Led::On(color) } else { Led::Off },
                false,
                32.0,
                4.0,
            )
            .flex_1()
            .min_w(px(0.))
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    live.decks[i].pads = m;
                    cx.notify();
                }),
            )
        };
        div()
            .relative()
            .flex()
            .gap(px(4.))
            .child(tab(
                if short { "CUE" } else { "HOT CUE" },
                Pads::HotCue,
                AMBER,
                cx,
            ))
            .child(tab("LOOP", Pads::Loop, MINT, cx))
            .child(tab("ROLL", Pads::Roll, TEAL, cx))
            .child(tab("JUMP", Pads::Jump, SKY, cx))
            .child(tab(
                if short { "STEM" } else { "STEMS" },
                Pads::Stems,
                PINK,
                cx,
            ))
            .child(mark(&self.probes, format!("deck{i}.body.mid.tabsend")))
    }

    /// Four decks: no room for five tabs, so the pad mode is picked from a dropdown.
    fn pad_dropdown(&self, i: usize, mode: Pads, cx: &mut Context<Self>) -> Div {
        let modes = [
            (Pads::HotCue, "Hot Cue"),
            (Pads::Loop, "Loop"),
            (Pads::Roll, "Roll"),
            (Pads::Jump, "Jump"),
            (Pads::Stems, "Stems"),
        ];
        let name = modes
            .iter()
            .find(|(m, _)| *m == mode)
            .map_or("", |(_, n)| *n);
        let open = self.pad_menu == Some(i);
        let menu = div()
            .w(px(150.))
            .p(px(4.))
            .rounded(px(8.))
            .bg(rgb(CARD))
            .border_1()
            .border_color(rgb(LINE_STRONG))
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|live, _, _, cx| {
                live.pad_menu = None;
                cx.notify();
            }))
            .children(modes.into_iter().map(|(m, n)| {
                div()
                    .h(px(26.))
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .rounded(px(5.))
                    .cursor_pointer()
                    .bg(rgb(if m == mode { RAISED } else { CARD }))
                    .text_color(rgb(if m == mode { TEXT } else { TEXT_2 }))
                    .hover(|st| st.bg(rgb(RAISED)))
                    .child(n)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            cx.stop_propagation();
                            live.decks[i].pads = m;
                            live.pad_menu = None;
                            cx.notify();
                        }),
                    )
            }));
        div()
            .flex()
            .flex_col()
            // The mode as a key, lit in its pads' colour, opening the list.
            .child(
                {
                    let color = match mode {
                        Pads::HotCue => AMBER,
                        Pads::Loop => MINT,
                        Pads::Roll => TEAL,
                        Pads::Jump => SKY,
                        Pads::Stems => PINK,
                    };
                    crate::hw::key(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(name.to_uppercase())
                            .child(icon(Icon::Chevron, 11.).text_color(rgb(TEXT_3))),
                        crate::hw::Led::On(color),
                        false,
                        26.0,
                        9.0,
                    )
                    .w_full()
                }
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |live, _, _, cx| {
                        live.pad_menu = if live.pad_menu == Some(i) {
                            None
                        } else {
                            Some(i)
                        };
                        cx.notify();
                    }),
                ),
            )
            .when(open, |d| {
                d.child(
                    gpui::deferred(gpui::anchored().snap_to_window().child(menu)).with_priority(1),
                )
            })
    }

    fn pads(&self, i: usize, s: &DeckStatus, h: f32, cx: &mut Context<Self>) -> Div {
        let period = self.decks[i]
            .analysis
            .as_ref()
            .and_then(|a| a.grid)
            .map_or(0.5 * self.rate, |g| g.period(self.rate));
        if self.decks[i].pads == Pads::Stems {
            return self.stem_pads(i, s, h, cx);
        }
        let mut rows = div().flex().flex_col().gap(px(6.));
        for row in 0..2 {
            let mut r = div().flex().gap(px(6.));
            for col in 0..4 {
                let k = row * 4 + col;
                let p = match self.decks[i].pads {
                    Pads::HotCue => {
                        let set = !s.hot[k].is_nan();
                        crate::controls::pad_with(
                            format!("{}", k + 1),
                            set.then(|| clock(s.hot[k] / self.rate, false)),
                            set.then_some(CUE_COLORS[k]),
                            self.holding(i, Hold::HotCue(k)),
                            h,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                                // Shift-click clears it.
                                if e.modifiers.shift {
                                    live.act(i, DeckCmd::SetHotCue { slot: k, at: None });
                                } else {
                                    live.hold(i, Hold::HotCue(k), cx);
                                }
                            }),
                        )
                    }
                    Pads::Loop => {
                        let b = LOOP_SIZES[k];
                        let on = s.loop_on
                            && !s.roll
                            && ((s.loop_end - s.loop_start) - b * period).abs() < 2.0;
                        pad(beats_text(b), Some(MINT), on, h).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |live, _, _, cx| {
                                live.decks[i].loop_beats = b;
                                live.act(i, DeckCmd::AutoLoop(b));
                                cx.notify();
                            }),
                        )
                    }
                    Pads::Roll => {
                        let b = ROLL_SIZES[k];
                        pad(beats_text(b), Some(TEAL), self.holding(i, Hold::Roll(b)), h)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |live, _, _, cx| live.hold(i, Hold::Roll(b), cx)),
                            )
                    }
                    Pads::Stems => unreachable!("drawn by stem_pads"),
                    Pads::Jump => {
                        let b = JUMPS[k];
                        let label =
                            format!("{}{}", if b < 0.0 { "−" } else { "+" }, beats_text(b.abs()));
                        pad(label, Some(SKY), false, h).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |live, _, _, cx| {
                                live.act(i, DeckCmd::BeatJump(b));
                                cx.notify();
                            }),
                        )
                    }
                };
                r = r.child(p);
            }
            rows = rows.child(r);
        }
        rows
    }

    /// The loop, as a player's: IN (blinking while it waits for its OUT), OUT (lit while it
    /// loops), halve, a display of its length (clicked, a loop that long; lit while it loops),
    /// double, and RELOOP or EXIT.
    fn loop_row(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        use crate::hw::{Led, display, key};
        let size = self.decks[i].loop_beats;
        let kp = if self.narrow || self.card_w < 520.0 {
            6.0
        } else {
            9.0
        };
        let kh = 32.0;
        let on = s.loop_on && !s.roll;
        let set_in = !s.loop_in.is_nan();
        let blink = self.blink;
        let act = |label: &'static str, led: Led, cmd: fn() -> DeckCmd, cx: &mut Context<Self>| {
            key(label, led, false, kh, kp).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    live.act(i, cmd());
                    cx.notify();
                }),
            )
        };
        let resize = move |by: f64, label: &'static str, cx: &mut Context<Self>| {
            key(label, Led::None, false, kh, kp).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    let b = (live.decks[i].loop_beats * by).clamp(1.0 / 32.0, 64.0);
                    live.decks[i].loop_beats = b;
                    if live.status().decks[i].loop_on {
                        live.act(i, DeckCmd::LoopScale(by));
                    }
                    cx.notify();
                }),
            )
        };
        let four = self.narrow;
        let in_led = match (on, set_in) {
            (true, _) => Led::On(MINT),
            (false, true) => Led::Blink(AMBER, blink),
            _ => Led::Off,
        };
        div()
            .h(px(ROW_H))
            .flex()
            .items_center()
            .gap(px(4.))
            .when(!four, |t| {
                t.child(act("IN", in_led, || DeckCmd::LoopIn, cx))
                    .child(act(
                        "OUT",
                        if on { Led::On(MINT) } else { Led::Off },
                        || DeckCmd::LoopOut,
                        cx,
                    ))
            })
            .child(resize(0.5, "½", cx))
            .child(
                display(kh)
                    .flex_1()
                    .min_w(px(0.))
                    .justify_center()
                    .gap(px(5.))
                    .cursor_pointer()
                    .when(on, |d| d.border_color(alpha(MINT, 0.6)))
                    .child(icon(Icon::Loop, 13.).text_color(rgb(if on { MINT } else { TEXT_3 })))
                    .child(
                        div()
                            .font_family(MONO)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(13.))
                            .text_color(rgb(if on { MINT } else { TEXT }))
                            .child(beats_text(size)),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            let b = live.decks[i].loop_beats;
                            live.act(i, DeckCmd::AutoLoop(b));
                            cx.notify();
                        }),
                    ),
            )
            .child(resize(2.0, "×2", cx))
            .child(act(
                if on { "EXIT" } else { "RELOOP" },
                if on { Led::On(MINT) } else { Led::Off },
                || DeckCmd::Reloop,
                cx,
            ))
            .child(mark(&self.probes, format!("deck{i}.body.mid.loopend")))
    }

    /// The pitch fader: TEMPO over it, its reach under it (clicked round ±8, 16, 50, 100%).
    fn tempo_column(&self, i: usize, s: &DeckStatus, fh: f32, cx: &mut Context<Self>) -> Div {
        let range = self.decks[i].range;
        let compact = self.narrow;
        let w = if compact { 40. } else { 48. };
        // Held, the track's nudged slower or faster (a twentieth), as a jog's edge does.
        let nudge = |by: f64, label: &'static str, cx: &mut Context<Self>| {
            let held = self.holding(i, Hold::Bend(by));
            crate::hw::key(label, crate::hw::Led::None, held, 18.0, 0.0)
                .flex_1()
                .pt(px(0.))
                .font_family(MONO)
                .text_size(px(11.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |live, _, _, cx| {
                        cx.stop_propagation();
                        live.hold(i, Hold::Bend(by), cx)
                    }),
                )
        };
        div()
            .relative()
            .w(px(w))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(5.))
            .child(probe(&self.probes, format!("deck{i}.body.tempo")))
            .child(caption("TEMPO"))
            .child(
                div()
                    .h(px(14.))
                    .line_height(px(14.))
                    .font_family(MONO)
                    .text_size(px(10.5))
                    .text_color(rgb(if s.sync { ACCENT_TEXT } else { TEXT_2 }))
                    .child(match compact {
                        true => format!("{:+.1}%", (s.tempo - 1.0) * 100.0),
                        false => format!("{:+.2}%", (s.tempo - 1.0) * 100.0),
                    }),
            )
            .child(self.fader(Ctl::Tempo(i), fh, w, cx))
            .child(
                div()
                    .w(px(w))
                    .flex()
                    .gap(px(2.))
                    .child(nudge(-0.05, "−", cx))
                    .child(nudge(0.05, "+", cx)),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(5.))
                    .child(probe(&self.probes, format!("deck{i}.body.range")))
                    .child(
                        crate::hw::key(
                            format!("±{}", (range * 100.0).round()),
                            crate::hw::Led::None,
                            false,
                            20.0,
                            6.0,
                        )
                        .pt(px(0.))
                        .font_family(MONO)
                        .text_size(px(10.))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |live, _, _, cx| {
                                let d = &mut live.decks[i];
                                let k = RANGES.iter().position(|&r| r == d.range).unwrap_or(0);
                                let v = d.fader / d.range;
                                d.range = RANGES[(k + 1) % RANGES.len()];
                                // The fader stays where it is; the tempo it means changes.
                                Ctl::Tempo(i).set(live, v as f32);
                                cx.notify();
                            }),
                        ),
                    )
                    // Compact, the key shift lives here (the transport has no room for it).
                    .when(compact, |d| d.child(self.key_steps(i, s, cx))),
            )
    }

    /// The key shift, two small steps and what it's at, stacked for the tempo column.
    fn key_steps(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let key = s.key;
        let step = |by: f64, label: &'static str, cx: &mut Context<Self>| {
            seg(label, false, 18.0).px(px(4.)).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                    let k = live.status().decks[i].key;
                    let next = if e.modifiers.alt {
                        0.0
                    } else {
                        (k + by).clamp(-12.0, 12.0)
                    };
                    live.act(i, DeckCmd::Key(next));
                    cx.notify();
                }),
            )
        };
        div()
            .h(px(18.))
            .flex()
            .items_center()
            .child(step(-1.0, "♭", cx))
            .child(
                div()
                    .font_family(MONO)
                    .text_size(px(9.5))
                    .text_color(rgb(if key.abs() > 0.01 { AMBER } else { TEXT_4 }))
                    .child(if key.abs() < 0.01 {
                        "0".to_string()
                    } else {
                        format!("{:+}", key.round())
                    }),
            )
            .child(step(1.0, "♯", cx))
    }

    /// The deck's hardware along its foot, as a player's: CUE and PLAY round and lit (PLAY
    /// blinking while it's paused, CUE while it's away from the cue), SYNC and LEAD keys, then
    /// the key (a display of the key it's playing in, ♭ and ♯ either side, MATCH lit while it
    /// mixes with the deck it's against) and REV; compact, keylock and slip instead of the key.
    fn transport(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        use crate::hw::{Led, key, round};
        let d = &self.decks[i];
        let leader = self.leader() == Some(i);
        let compact = self.narrow;
        // Sized to the card: four a side's narrowest, compact narrower than full.
        let big = if self.four {
            34.0
        } else if compact {
            38.0
        } else {
            46.0
        };
        let kh = if compact { 30.0 } else { 32.0 };
        let kp = if self.four {
            7.0
        } else if compact {
            8.0
        } else {
            9.0
        };
        let blink = self.blink;
        let held = self.holding(i, Hold::Cue);
        let at_cue = (s.pos - s.cue).abs() < 1.0;
        let cue_led = match (s.loaded, held || s.previewing, s.playing, at_cue) {
            (false, ..) => Led::Off,
            (_, true, ..) => Led::On(AMBER),
            (_, _, true, _) => Led::Off,
            (_, _, false, true) => Led::On(AMBER),
            (_, _, false, false) => Led::Blink(AMBER, blink),
        };
        // Playing, its ring brightens on each beat and fades to the next: the tempo, seen.
        let play_led = match (s.loaded, s.playing && !s.previewing, s.beat.is_nan()) {
            (false, ..) => Led::Off,
            (true, true, true) => Led::On(MINT),
            (true, true, false) => Led::Beat(MINT, (1.0 - s.beat.rem_euclid(1.0) as f32).powi(3)),
            (true, false, _) => Led::Blink(MINT, blink),
        };
        let cue_word = div()
            .text_size(px(if self.four {
                9.
            } else if compact {
                10.
            } else {
                11.
            }))
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(if matches!(cue_led, Led::On(_)) {
                TEXT
            } else {
                TEXT_2
            }))
            .child("CUE");
        let play_icon = icon(
            if s.playing && !s.previewing {
                Icon::Pause
            } else {
                Icon::Play
            },
            if compact { 15. } else { 17. },
        )
        .text_color(rgb(if s.playing { TEXT } else { TEXT_2 }));
        div()
            .relative()
            .h(px(transport_h(compact)))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(if compact { 5. } else { 6. }))
            .px(px(if compact { 10. } else { 14. }))
            .border_t_1()
            .border_color(rgb(LINE_SOFT))
            .bg(alpha(0x000000, 0.12))
            .child(probe(&self.probes, format!("deck{i}.transport")))
            .child(round(big, cue_led, held, cue_word).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| live.hold(i, Hold::Cue, cx)),
            ))
            .child(round(big, play_led, false, play_icon).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    live.act(i, DeckCmd::Play);
                    cx.notify();
                }),
            ))
            .child(div().w(px(if compact { 2. } else { 4. })))
            .child(
                key(
                    "SYNC",
                    if s.sync { Led::On(ACCENT) } else { Led::Off },
                    false,
                    kh,
                    kp,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |live, _, _, cx| {
                        let on = !live.status().decks[i].sync;
                        live.act(i, DeckCmd::Sync(on));
                        cx.notify();
                    }),
                ),
            )
            .child(
                key(
                    "LEAD",
                    if leader { Led::On(AMBER) } else { Led::Off },
                    false,
                    kh,
                    kp,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |live, _, _, cx| {
                        let picked = live.leader() == Some(i);
                        live.remote.send(Command::Leader((!picked).then_some(i)));
                        cx.notify();
                    }),
                ),
            )
            .child(div().flex_1())
            // Compact, keylock and slip come down here from under the ring.
            .when(compact, |t| {
                t.child(
                    key(
                        icon(Icon::Lock, 12.).text_color(rgb(if d.keylock {
                            TEXT
                        } else {
                            TEXT_3
                        })),
                        if d.keylock { Led::On(ACCENT) } else { Led::Off },
                        false,
                        kh,
                        kp,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            let on = !live.decks[i].keylock;
                            live.decks[i].keylock = on;
                            live.act(i, DeckCmd::Keylock(on));
                            cx.notify();
                        }),
                    ),
                )
            })
            .when(compact && !self.four, |t| {
                t.child(
                    key(
                        "SLIP",
                        if d.slip { Led::On(AMBER) } else { Led::Off },
                        false,
                        kh,
                        kp,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            let on = !live.decks[i].slip;
                            live.decks[i].slip = on;
                            live.act(i, DeckCmd::Slip(on));
                            cx.notify();
                        }),
                    ),
                )
            })
            .when(!compact, |t| t.child(self.key_module(i, s, kh, cx)))
            .when(!self.four, |t| {
                let rev = self.holding(i, Hold::Censor);
                t.child(
                    key(
                        "REV",
                        if rev { Led::On(RECORD) } else { Led::Off },
                        rev,
                        kh,
                        kp,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| live.hold(i, Hold::Censor, cx)),
                    ),
                )
            })
            .child(mark(&self.probes, format!("deck{i}.transport.end")))
    }

    /// The deck it's mixed against for its key: the leader, else the next deck playing.
    fn key_partner(&self, i: usize) -> Option<usize> {
        let all = self.status().decks;
        match self.leader() {
            Some(l) if l != i && all[l].loaded => Some(l),
            _ => (0..all.len()).find(|&d| d != i && all[d].playing),
        }
    }

    /// The key deck `d` is heard in: its own, moved by its key shift and (keylock off) by its
    /// tempo, to the nearest semitone.
    fn heard_key(&self, d: usize) -> Option<String> {
        let s = self.status().decks[d];
        let own = self.decks[d].analysis.as_ref().map(|a| a.camelot.clone())?;
        let by = s.key
            + if s.keylock {
                0.0
            } else {
                12.0 * s.tempo.max(1e-6).log2()
            };
        camelot_shift(&own, by.round() as i32)
    }

    /// Deck `i`'s key shifted to the nearest one that mixes with its partner's (the fewest
    /// semitones; none if it mixes already).
    fn match_key(&mut self, i: usize) {
        let Some(p) = self.key_partner(i) else { return };
        let (Some(theirs), Some(own)) = (
            self.heard_key(p),
            self.decks[i].analysis.as_ref().map(|a| a.camelot.clone()),
        ) else {
            return;
        };
        let s = self.status().decks[i];
        let tempo = if s.keylock {
            0.0
        } else {
            12.0 * s.tempo.max(1e-6).log2()
        };
        let best = [0, 1, -1, 2, -2, 3, -3, 4, -4, 5, -5, 6]
            .into_iter()
            .find(|&k| {
                camelot_shift(&own, (k as f64 + tempo).round() as i32)
                    .is_some_and(|k| pina_live_engine::analyze::compatible(&k, &theirs))
            });
        if let Some(k) = best {
            self.act(i, DeckCmd::Key(k as f64));
        }
    }

    /// The key, as a player's: ♭, a display of the key it's heard in (its place on the wheel,
    /// how far it's shifted; clicked, back to its own), ♯, and MATCH, lit while it mixes with
    /// the deck it's against, pressed to shift it to the nearest key that does.
    fn key_module(&self, i: usize, s: &DeckStatus, kh: f32, cx: &mut Context<Self>) -> Div {
        use crate::hw::{Led, display, key};
        let kp = 9.0;
        let shift = s.key.round() as i32;
        let heard = self.heard_key(i);
        let mixes = self
            .key_partner(i)
            .and_then(|p| self.heard_key(p))
            .zip(heard.clone())
            .map(|(t, m)| pina_live_engine::analyze::compatible(&m, &t));
        let step = |by: f64, label: &'static str, cx: &mut Context<Self>| {
            key(label, Led::None, false, kh, 8.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                    let k = live.status().decks[i].key;
                    // Alt-click: back to the track's own key.
                    let next = if e.modifiers.alt {
                        0.0
                    } else {
                        (k + by).clamp(-12.0, 12.0)
                    };
                    live.act(i, DeckCmd::Key(next));
                    cx.notify();
                }),
            )
        };
        div()
            .flex()
            .items_center()
            .gap(px(3.))
            .child(step(-1.0, "♭", cx))
            .child(
                display(kh)
                    .w(px(52.))
                    .px(px(6.))
                    .gap(px(4.))
                    .cursor_pointer()
                    .children(
                        heard
                            .as_deref()
                            .and_then(crate::browser::key_hue)
                            .map(|c| div().size(px(6.)).flex_none().rounded(px(3.)).bg(c)),
                    )
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(if heard.is_some() { TEXT } else { TEXT_4 }))
                            .child(heard.clone().unwrap_or_else(|| "—".into())),
                    )
                    .when(shift != 0, |d| {
                        d.child(
                            div()
                                .font_family(MONO)
                                .text_size(px(8.5))
                                .text_color(rgb(AMBER))
                                .child(format!("{shift:+}")),
                        )
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            live.act(i, DeckCmd::Key(0.0));
                            cx.notify();
                        }),
                    ),
            )
            .child(step(1.0, "♯", cx))
            .child(
                key(
                    "MATCH",
                    match mixes {
                        Some(true) => Led::On(MINT),
                        Some(false) => Led::Off,
                        None => Led::Off,
                    },
                    false,
                    kh,
                    kp,
                )
                .ml(px(4.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |live, _, _, cx| {
                        live.match_key(i);
                        cx.notify();
                    }),
                ),
            )
    }
}

/// What a jog's drawn from: the deck's colour, the beat it's on (none without a grid), how far
/// round the platter's turned, whether it's held, and where slip has the track underneath.
pub struct Jog {
    color: u32,
    beat: Option<f64>,
    turn: f32,
    touched: bool,
    loaded: bool,
    shadow: Option<f32>,
}

/// A jog wheel, as a player's display draws one: a ridged rim; round it the phrase's sixteen
/// beats as LEDs (the one playing lit, those gone by dim); a platter, lit from above and grooved,
/// tinted while it's held; the needle turning with the track (an amber one where slip has it
/// underneath); and a dark screen in the middle for the count.
fn paint_jog(window: &mut gpui::Window, b: gpui::Bounds<Pixels>, j: &Jog) {
    let c = b.center();
    let rr = f32::from(b.size.width.min(b.size.height)) / 2.0;
    let tau = std::f32::consts::TAU;
    let at =
        |a: f32, r: f32| -> Point<Pixels> { point(c.x + px(r * a.cos()), c.y + px(r * a.sin())) };
    let disc = |window: &mut gpui::Window, r: f32, fill: gpui::Hsla| {
        window.paint_quad(
            gpui::fill(
                gpui::Bounds::centered_at(c, gpui::size(px(r * 2.0), px(r * 2.0))),
                fill,
            )
            .corner_radii(px(r)),
        );
    };
    let circle = |window: &mut gpui::Window, r: f32, w: f32, color: gpui::Hsla| {
        let mut p = PathBuilder::stroke(px(w));
        p.move_to(at(0.0, r));
        p.arc_to(
            point(px(r), px(r)),
            px(0.),
            false,
            true,
            at(std::f32::consts::PI, r),
        );
        p.arc_to(point(px(r), px(r)), px(0.), false, true, at(0.0, r));
        if let Ok(p) = p.build() {
            window.paint_path(p, color);
        }
    };
    let line = |window: &mut gpui::Window, a: f32, r0: f32, r1: f32, w: f32, color: gpui::Hsla| {
        let mut p = PathBuilder::stroke(px(w));
        p.move_to(at(a, r0));
        p.line_to(at(a, r1));
        if let Ok(p) = p.build() {
            window.paint_path(p, color);
        }
    };
    // The rim, ridged.
    disc(window, rr - 0.5, rgb(0x0f0f13).into());
    for k in 0..96 {
        line(
            window,
            k as f32 / 96.0 * tau,
            rr - 4.5,
            rr - 1.0,
            1.0,
            alpha(0xffffff, 0.06),
        );
    }
    // The beats round it.
    let r_led = rr - 10.0;
    let now16 = j.beat.map(|b| b.rem_euclid(16.0) as f32);
    for k in 0..16 {
        let a0 = k as f32 / 16.0 * tau - tau / 4.0 + 0.04;
        let a1 = (k + 1) as f32 / 16.0 * tau - tau / 4.0 - 0.04;
        let w = if k % 4 == 0 { 6.0 } else { 4.5 };
        let fill: gpui::Hsla = match now16 {
            Some(n) if n.floor() as usize == k => rgb(j.color).into(),
            Some(n) if (k as f32) < n.floor() => alpha(j.color, 0.32),
            _ => rgb(0x24242a).into(),
        };
        let mut p = PathBuilder::stroke(px(w));
        p.move_to(at(a0, r_led));
        p.arc_to(
            point(px(r_led), px(r_led)),
            px(0.),
            false,
            true,
            at(a1, r_led),
        );
        if let Ok(p) = p.build() {
            window.paint_path(p, fill);
        }
    }
    // The platter: lit from above (rings lighter toward the middle), grooved.
    let rp = rr - 16.0;
    disc(window, rp, rgb(0x16161b).into());
    disc(window, rp * 0.88, rgb(0x19191f).into());
    disc(window, rp * 0.74, rgb(0x1c1c22).into());
    for k in 0..7 {
        circle(
            window,
            rp * (0.64 + k as f32 * 0.05),
            1.0,
            alpha(0xffffff, 0.022),
        );
    }
    circle(window, rp, 1.0, alpha(0xffffff, 0.08));
    if j.touched {
        disc(window, rp, alpha(j.color, 0.12));
    }
    if j.loaded {
        if let Some(t) = j.shadow {
            line(
                window,
                t * tau - tau / 4.0,
                rp * 0.62,
                rp - 2.0,
                2.0,
                alpha(AMBER, 0.85),
            );
        }
        let a = j.turn * tau - tau / 4.0;
        line(window, a, rp * 0.62, rp - 2.0, 2.5, rgb(TEXT).into());
        let tip = at(a, rp - 5.0);
        window.paint_quad(
            gpui::fill(
                gpui::Bounds::centered_at(tip, gpui::size(px(5.), px(5.))),
                rgb(TEXT),
            )
            .corner_radii(px(2.5)),
        );
    }
    // The screen in the middle, sunk.
    let rc = rp * 0.58;
    window.paint_quad(
        gpui::fill(
            gpui::Bounds::centered_at(c, gpui::size(px(rc * 2.0), px(rc * 2.0))),
            rgb(0x09090c),
        )
        .corner_radii(px(rc))
        .border_widths(px(1.))
        .border_color(rgb(0x26262d)),
    );
}

/// A Camelot key moved `semis` semitones: seven places round the wheel a semitone.
pub fn camelot_shift(k: &str, semis: i32) -> Option<String> {
    let (n, l) = k.split_at(k.len().checked_sub(1)?);
    let n: i32 = n.parse().ok().filter(|n| (1..=12).contains(n))?;
    Some(format!("{}{l}", (n - 1 + 7 * semis).rem_euclid(12) + 1))
}

/// For the browser: the ids of decks with `id` on them.
pub fn decks_with(live: &Live, id: u64) -> Vec<usize> {
    (0..live.shown())
        .filter(|&d| live.decks[d].track == Some(id))
        .collect()
}
