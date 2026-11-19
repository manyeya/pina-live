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
    ui::{scale::px, theme::*},
};

/// The deck cards' and the mixer's height: in full, and compact (four decks, or a narrow window).
pub const PANEL_H: f32 = 442.0;
pub const PANEL_H_FOUR: f32 = 360.0;
/// The space round the cards and between them.
pub const GAP: f32 = 10.0;

/// A section's caption: "PADS", "LOOP", "TEMPO".
pub fn caption(s: &'static str) -> Div {
    div()
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
        let h = if compact { PANEL_H_FOUR } else { PANEL_H };
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
            .child(self.deck_header(i, &s, cx))
            .child(match self.grid_edit == Some(i) {
                true => self.grid_tray(i, cx),
                false => self.overview_strip(i, &s, cx),
            })
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .gap(px(if compact { 10. } else { 16. }))
                    .px(px(if compact { 10. } else { 14. }))
                    .pt(px(10.))
                    .when(!compact, |d| d.child(self.ring_column(i, &s, cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .when(!compact, |d| d.child(caption("PADS")))
                            .child(self.pad_tabs(i, cx))
                            .child(self.pads(i, &s, cx))
                            .when(!compact, |d| {
                                d.child(div().pt(px(2.)).child(caption("LOOP")))
                            })
                            .child(self.loop_row(i, &s, cx)),
                    )
                    .child(self.tempo_column(i, &s, cx)),
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
            .h(px(60.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .border_b_1()
            .border_color(rgb(LINE_SOFT))
            .child(
                div()
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
            )
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
                            .child(caption("BPM"))
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
    }

    /// The beat grid's tools: half or double the tempo, nudge it, move the grid, or put the
    /// downbeat where the track is. Kept with the track (and left alone by analysis after).
    fn grid_tray(&self, i: usize, cx: &mut Context<Self>) -> Div {
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
            .h(px(if self.narrow { 54. } else { 62. }))
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

    fn overview_strip(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let now = self.deck_seconds(i);
        let len = s.len / self.rate;
        let left = len - now;
        div()
            .flex_none()
            .px(px(14.))
            .pt(px(10.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .id(("overview", i))
                    .h(px(if self.narrow { 28. } else { 34. }))
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
                    .flex()
                    .justify_between()
                    .font_family(MONO)
                    .text_size(px(11.))
                    .text_color(rgb(TEXT_3))
                    .child(clock(now.max(0.0), true))
                    .child(self.beat_counter(i, s))
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
    fn ring_column(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let size = 150.0;
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
        div()
            .w(px(size))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
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
                                let c = b.center();
                                let r = f32::from(b.size.width) / 2.0 - 6.0;
                                let at = |a: f32, rr: f32| -> Point<Pixels> {
                                    point(c.x + px(rr * a.cos()), c.y + px(rr * a.sin()))
                                };
                                if touched {
                                    window.paint_quad(
                                        gpui::fill(
                                            gpui::Bounds::centered_at(
                                                c,
                                                gpui::size(px(r * 2.0), px(r * 2.0)),
                                            ),
                                            alpha(color, 0.08),
                                        )
                                        .corner_radii(px(r)),
                                    );
                                }
                                let tau = std::f32::consts::TAU;
                                let now16 = beat.map(|b| b.rem_euclid(16.0) as f32);
                                for k in 0..16 {
                                    let a0 = k as f32 / 16.0 * tau - tau / 4.0 + 0.035;
                                    let a1 = (k + 1) as f32 / 16.0 * tau - tau / 4.0 - 0.035;
                                    let w = if k % 4 == 0 { 7.0 } else { 5.0 };
                                    let fill = match now16 {
                                        Some(n) if n.floor() as usize == k => rgb(color).into(),
                                        Some(n) if (k as f32) < n.floor() => alpha(color, 0.35),
                                        _ => gpui::Hsla::from(rgb(RAISED)),
                                    };
                                    let mut p = PathBuilder::stroke(px(w));
                                    p.move_to(at(a0, r));
                                    p.arc_to(point(px(r), px(r)), px(0.), false, true, at(a1, r));
                                    if let Ok(p) = p.build() {
                                        window.paint_path(p, fill);
                                    }
                                }
                                // No grid: a mark turning with the track instead.
                                if now16.is_none() && loaded {
                                    let a = turn * tau - tau / 4.0;
                                    let mut p = PathBuilder::stroke(px(3.));
                                    p.move_to(at(a, r * 0.55));
                                    p.line_to(at(a, r - 10.0));
                                    if let Ok(p) = p.build() {
                                        window.paint_path(p, rgb(TEXT));
                                    }
                                }
                            },
                        )
                        .size_full(),
                    )
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
                                    .text_size(px(24.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(if loaded { TEXT } else { TEXT_4 }))
                                    .child(center),
                            )
                            .child(
                                div().text_size(px(10.)).text_color(rgb(TEXT_3)).child(
                                    match (s.slip && !s.shadow.is_nan(), s.reverse) {
                                        (true, _) => "slipping",
                                        (_, true) => "reverse",
                                        _ if beat.is_some() => "bar . beat",
                                        _ => "time left",
                                    },
                                ),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(self.keylock_button(i, d.keylock, true, cx))
                    .child(self.slip_button(i, d.slip, cx)),
            )
    }

    fn keylock_button(&self, i: usize, on: bool, words: bool, cx: &mut Context<Self>) -> Div {
        let b = match words {
            true => seg("Keylock", on, 26.0).child(icon(Icon::Lock, 13.).text_color(rgb(if on {
                ACCENT_TEXT
            } else {
                TEXT_3
            }))),
            false => icon_seg(Icon::Lock, on, 28.0),
        };
        b.when(on, |d| d.text_color(rgb(ACCENT_TEXT)))
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

    fn slip_button(&self, i: usize, on: bool, cx: &mut Context<Self>) -> Div {
        seg("Slip", on, 26.0)
            .border_1()
            .border_color(rgb(if on { AMBER } else { LINE }))
            .when(on, |d| d.text_color(rgb(AMBER)))
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

    fn pad_tabs(&self, i: usize, cx: &mut Context<Self>) -> Div {
        let mode = self.decks[i].pads;
        if self.four {
            return self.pad_dropdown(i, mode, cx);
        }
        let tab = |label: &'static str, m: Pads, cx: &mut Context<Self>| {
            seg(label, mode == m, 24.0).flex_1().on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    live.decks[i].pads = m;
                    cx.notify();
                }),
            )
        };
        tray()
            .child(tab(
                if self.narrow { "Cue" } else { "Hot Cue" },
                Pads::HotCue,
                cx,
            ))
            .child(tab("Loop", Pads::Loop, cx))
            .child(tab("Roll", Pads::Roll, cx))
            .child(tab(
                if self.narrow { "Jmp" } else { "Jump" },
                Pads::Jump,
                cx,
            ))
            .child(tab(
                if self.narrow { "Stem" } else { "Stems" },
                Pads::Stems,
                cx,
            ))
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
            .child(dropdown(name, 26.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    live.pad_menu = if live.pad_menu == Some(i) {
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

    fn pads(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let h = if self.narrow { 30.0 } else { 40.0 };
        let four = self.narrow;
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
                        let label = match set && !four {
                            true => format!("{}  {}", k + 1, clock(s.hot[k] / self.rate, false)),
                            false => format!("{}", k + 1),
                        };
                        pad(
                            label,
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
                        pad(label, None, false, h).on_mouse_down(
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

    fn loop_row(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let size = self.decks[i].loop_beats;
        let on = s.loop_on && !s.roll;
        let set_in = !s.loop_in.is_nan();
        let btn = |label: &'static str, lit: bool, cmd: fn() -> DeckCmd, cx: &mut Context<Self>| {
            seg(label, lit, 26.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    live.act(i, cmd());
                    cx.notify();
                }),
            )
        };
        let resize = move |by: f64, label: &'static str, cx: &mut Context<Self>| {
            seg(label, false, 26.0).on_mouse_down(
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
        tray()
            .when(!four, |t| {
                t.child(btn("In", set_in, || DeckCmd::LoopIn, cx))
                    .child(btn("Out", false, || DeckCmd::LoopOut, cx))
            })
            .child(resize(0.5, "½", cx))
            .child(
                div()
                    .flex_1()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(5.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .bg(rgb(if on { MINT } else { CARD }))
                    .text_color(rgb(if on { 0x0c0c0f } else { TEXT_2 }))
                    .font_family(MONO)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(11.5))
                    .child(icon(Icon::Loop, 13.).text_color(rgb(if on {
                        0x0c0c0f
                    } else {
                        TEXT_3
                    })))
                    .child(beats_text(size))
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
            .child(btn(
                if on { "Exit" } else { "Reloop" },
                false,
                || DeckCmd::Reloop,
                cx,
            ))
    }

    /// The pitch fader: TEMPO over it, its reach under it (clicked round ±8, 16, 50, 100%).
    fn tempo_column(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let range = self.decks[i].range;
        let compact = self.narrow;
        let fh = if compact { 96.0 } else { 190.0 };
        div()
            .w(px(if compact { 40. } else { 48. }))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(5.))
            .child(caption("TEMPO"))
            .child(
                div()
                    .font_family(MONO)
                    .text_size(px(10.5))
                    .text_color(rgb(if s.sync { ACCENT_TEXT } else { TEXT_2 }))
                    .child(format!("{:+.1}%", (s.tempo - 1.0) * 100.0)),
            )
            .child(self.fader(Ctl::Tempo(i), fh, 30.0, cx))
            .child(
                seg(format!("±{}", (range * 100.0).round()), false, 20.0)
                    .text_size(px(10.))
                    .px(px(6.))
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
            .when(compact, |d| d.child(self.key_steps(i, s, cx)))
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

    fn transport(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let d = &self.decks[i];
        let leader = self.leader() == Some(i);
        let compact = self.narrow;
        let size = if compact { 36.0 } else { 42.0 };
        let round = move |label: Icon, on: bool, color: u32, held: bool| {
            div()
                .size(px(size))
                .flex_none()
                .rounded(px(size / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .border_2()
                .border_color(rgb(if on { color } else { LINE_STRONG }))
                .bg(rgb(if held { alpha_mix(color) } else { CARD }))
                .cursor_pointer()
                .hover(|st| st.bg(rgb(RAISED)))
                .child(icon(label, 17.).text_color(rgb(if on { color } else { TEXT_2 })))
        };
        let cue_lit = s.previewing || (!s.playing && s.loaded && (s.pos - s.cue).abs() < 1.0);
        let pad_x = if compact { 7.0 } else { 10.0 };
        let outline = move |d: Div, on: bool| {
            d.border_1()
                .border_color(rgb(if on { ACCENT } else { LINE }))
                .px(px(pad_x))
        };
        div()
            .h(px(if compact { 52. } else { 58. }))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(if compact { 6. } else { 10. }))
            .px(px(if compact { 10. } else { 14. }))
            .border_t_1()
            .border_color(rgb(LINE_SOFT))
            .child(
                round(Icon::ToStart, cue_lit, AMBER, self.holding(i, Hold::Cue)).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |live, _, _, cx| live.hold(i, Hold::Cue, cx)),
                ),
            )
            .child(
                round(
                    if s.playing && !s.previewing {
                        Icon::Pause
                    } else {
                        Icon::Play
                    },
                    s.playing,
                    MINT,
                    false,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |live, _, _, cx| {
                        live.act(i, DeckCmd::Play);
                        cx.notify();
                    }),
                ),
            )
            .child(
                outline(seg("Sync", s.sync, 30.0), s.sync)
                    .when(s.sync, |d| d.bg(rgb(ACCENT)).text_color(rgb(0xffffff)))
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
                outline(seg("Lead", leader, 30.0), false)
                    .when(leader, |d| {
                        d.text_color(rgb(ACCENT_TEXT)).bg(alpha(ACCENT, 0.14))
                    })
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
                t.child(self.keylock_button(i, d.keylock, false, cx))
            })
            .when(compact && !self.four, |t| {
                t.child(self.slip_button(i, d.slip, cx).px(px(pad_x)))
            })
            .when(!compact, |t| t.child(self.key_buttons(i, s, cx)))
            .when(!self.four, |t| {
                t.child(
                    outline(seg("Rev", self.holding(i, Hold::Censor), 30.0), false).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| live.hold(i, Hold::Censor, cx)),
                    ),
                )
            })
    }

    fn key_buttons(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let key = s.key;
        let step = |by: f64, label: &'static str, cx: &mut Context<Self>| {
            seg(label, false, 26.0).px(px(7.)).on_mouse_down(
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
        tray()
            .child(step(-1.0, "♭", cx))
            .child(
                div()
                    .w(px(30.))
                    .text_center()
                    .font_family(MONO)
                    .text_size(px(11.))
                    .text_color(rgb(if key.abs() > 0.01 { AMBER } else { TEXT_3 }))
                    .child(if key.abs() < 0.01 {
                        "Key".to_string()
                    } else {
                        format!("{:+}", key.round())
                    }),
            )
            .child(step(1.0, "♯", cx))
    }
}

/// A held round button's fill: its color, dimmed well down.
fn alpha_mix(c: u32) -> u32 {
    let ch = |shift: u32| (((c >> shift) & 0xff) as f32 * 0.25) as u32;
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// For the browser: the ids of decks with `id` on them.
pub fn decks_with(live: &Live, id: u64) -> Vec<usize> {
    (0..live.shown())
        .filter(|&d| live.decks[d].track == Some(id))
        .collect()
}
