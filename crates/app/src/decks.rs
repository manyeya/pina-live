//! A deck's panel: what's on it (title, artist, tempo, key, time), the whole track's waveform to
//! jump about in, the platter, the pads (hot cues, loops, rolls, beat jumps), the loop controls,
//! the transport, and the pitch fader.

use gpui::{
    Context, Div, ExternalPaths, FontWeight, MouseButton, MouseDownEvent, PathBuilder, Pixels,
    Point, canvas, div, point, prelude::*, rgb,
};
use pina_live_engine::{
    deck::{DeckCmd, DeckStatus, SECONDS_PER_TURN},
    engine::Command,
};

use crate::{
    controls::{Ctl, Drag, Hold, pad, pt},
    live::{CUE_COLORS, DECK_COLORS, DECK_NAMES, Live, Pads},
    ui::{scale::px, theme::*},
};

/// The deck panels' and the mixer's height, with two decks shown and four.
pub const PANEL_H: f32 = 336.0;
pub const PANEL_H_FOUR: f32 = 304.0;

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
        let four = self.four;
        let h = if four { PANEL_H_FOUR } else { PANEL_H };
        let s = self.status().decks[i];
        div()
            .id(("deck", i))
            .flex_1()
            .min_w(px(0.))
            .h(px(h))
            .flex()
            .flex_col()
            .bg(rgb(SURFACE))
            .border_r_1()
            .border_color(rgb(LINE))
            .on_drop(cx.listener(move |live, paths: &ExternalPaths, _, cx| {
                live.dropped(Some(i), paths, cx)
            }))
            .child(self.deck_header(i, &s))
            .child(self.overview_strip(i, &s, cx))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .gap(px(10.))
                    .px(px(10.))
                    .pt(px(8.))
                    .when(!four, |d| d.child(self.platter(i, &s, cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(self.pad_tabs(i, cx))
                            .child(self.pads(i, &s, cx))
                            .child(self.loop_row(i, &s, cx)),
                    )
                    .child(self.tempo_column(i, &s, cx)),
            )
            .child(self.transport(i, &s, cx))
    }

    fn deck_header(&self, i: usize, s: &DeckStatus) -> Div {
        let d = &self.decks[i];
        let color = DECK_COLORS[i];
        let loaded = d.track.is_some();
        let title = match (&d.loading, &d.error, loaded) {
            (_, Some(e), _) => e.clone(),
            (_, _, true) => d.title.clone(),
            _ => "Drop a track here, or load one from the library".into(),
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
        div()
            .h(px(50.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .border_b_1()
            .border_color(rgb(LINE_SOFT))
            .child(
                div()
                    .size(px(26.))
                    .flex_none()
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(alpha(color, 0.16))
                    .text_color(rgb(color))
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(13.))
                    .child(DECK_NAMES[i]),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        div()
                            .text_size(px(13.5))
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
                            .text_size(px(11.5))
                            .text_color(rgb(TEXT_3))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(match &d.loading {
                                Some(at) => {
                                    format!("Reading the file… {:.1}s", at.elapsed().as_secs_f32())
                                }
                                None if analysing => "Finding the beat and key…".into(),
                                None => d.artist.clone(),
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(1.))
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(if s.sync { ACCENT_TEXT } else { TEXT }))
                            .child(bpm),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .font_family(MONO)
                            .text_size(px(11.))
                            .child(
                                div()
                                    .text_color(rgb(if s.tempo == 1.0 { TEXT_4 } else { TEXT_2 }))
                                    .child(format!("{:+.2}%", (s.tempo - 1.0) * 100.0)),
                            )
                            .when(!key.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_color(rgb(if shifted { AMBER } else { TEXT_2 }))
                                        .child(match shifted {
                                            true => format!("{key} {:+}", s.key.round()),
                                            false => key,
                                        }),
                                )
                            }),
                    ),
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
            .px(px(10.))
            .pt(px(7.))
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(
                div()
                    .id(("overview", i))
                    .h(px(if self.four { 30. } else { 38. }))
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

    /// The platter: it turns with the track (a mark on it, as on a record) and a ring round it
    /// fills as the track plays; grabbed, it holds the record; turned, it scratches.
    fn platter(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let size = 168.0;
        let color = DECK_COLORS[i];
        let now = self.deck_seconds(i);
        let angle = (now / SECONDS_PER_TURN).fract() as f32 * std::f32::consts::TAU;
        let done = if s.len > 0.0 {
            (s.pos / s.len) as f32
        } else {
            0.0
        };
        let touched = s.touched;
        let bounds = self.decks[i].platter.clone();
        let loaded = s.loaded;
        div()
            .size(px(size))
            .flex_none()
            .relative()
            .cursor(gpui::CursorStyle::OpenHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                    let b = live.decks[i].platter.get();
                    let (x, y) = pt(e.position);
                    let c = b.center();
                    let a = (y - f32::from(c.y)).atan2(x - f32::from(c.x));
                    live.deck(i, DeckCmd::Touch(true));
                    live.drag = Some(Drag::Platter { deck: i, angle: a });
                    cx.notify();
                }),
            )
            .child(
                canvas(
                    move |b, _, _| bounds.set(b),
                    move |b, _, window, _| {
                        let c = b.center();
                        let r = f32::from(b.size.width) / 2.0 - 3.0;
                        let at = |deg: f32, rr: f32| -> Point<Pixels> {
                            point(c.x + px(rr * deg.cos()), c.y + px(rr * deg.sin()))
                        };
                        // The record.
                        window.paint_quad(
                            gpui::fill(
                                gpui::Bounds::centered_at(c, gpui::size(px(r * 2.0), px(r * 2.0))),
                                rgb(if touched { CARD } else { WELL }),
                            )
                            .corner_radii(px(r))
                            .border_widths(px(1.))
                            .border_color(rgb(LINE)),
                        );
                        // Its grooves.
                        for k in 1..4 {
                            let rr = r * (0.5 + k as f32 * 0.12);
                            let mut p = PathBuilder::stroke(px(1.));
                            p.move_to(at(0.0, rr));
                            p.arc_to(point(px(rr), px(rr)), px(0.), false, true, at(3.14159, rr));
                            p.arc_to(point(px(rr), px(rr)), px(0.), false, true, at(6.28, rr));
                            if let Ok(p) = p.build() {
                                window.paint_path(p, alpha(LINE_SOFT, 0.9));
                            }
                        }
                        // How far through the track: an arc from the top.
                        if loaded && done > 0.001 {
                            let start = -std::f32::consts::FRAC_PI_2;
                            let end = start + done.min(0.9999) * std::f32::consts::TAU;
                            let mut p = PathBuilder::stroke(px(3.));
                            p.move_to(at(start, r - 2.0));
                            let mid = (start + end) / 2.0;
                            p.arc_to(
                                point(px(r - 2.0), px(r - 2.0)),
                                px(0.),
                                false,
                                true,
                                at(mid, r - 2.0),
                            );
                            p.arc_to(
                                point(px(r - 2.0), px(r - 2.0)),
                                px(0.),
                                false,
                                true,
                                at(end, r - 2.0),
                            );
                            if let Ok(p) = p.build() {
                                window.paint_path(p, rgb(color));
                            }
                        }
                        // The mark that turns.
                        let a = angle - std::f32::consts::FRAC_PI_2;
                        let mut p = PathBuilder::stroke(px(3.));
                        p.move_to(at(a, r * 0.42));
                        p.line_to(at(a, r - 8.0));
                        if let Ok(p) = p.build() {
                            window.paint_path(p, rgb(if loaded { TEXT } else { TEXT_4 }));
                        }
                        // The label in the middle.
                        let lr = r * 0.34;
                        window.paint_quad(
                            gpui::fill(
                                gpui::Bounds::centered_at(
                                    c,
                                    gpui::size(px(lr * 2.0), px(lr * 2.0)),
                                ),
                                alpha(color, if loaded { 0.22 } else { 0.08 }),
                            )
                            .corner_radii(px(lr)),
                        );
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
                    .font_family(MONO)
                    .text_size(px(10.5))
                    .text_color(rgb(TEXT_2))
                    .when(s.slip && !s.shadow.is_nan(), |d| {
                        d.child(div().text_color(rgb(AMBER)).child("SLIP"))
                    })
                    .when(s.reverse, |d| {
                        d.child(div().text_color(rgb(AMBER)).child("REV"))
                    }),
            )
    }

    fn pad_tabs(&self, i: usize, cx: &mut Context<Self>) -> Div {
        let mode = self.decks[i].pads;
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
            .child(tab(if self.four { "Cue" } else { "Hot Cue" }, Pads::HotCue, cx))
            .child(tab("Loop", Pads::Loop, cx))
            .child(tab("Roll", Pads::Roll, cx))
            .child(tab("Jump", Pads::Jump, cx))
    }

    fn pads(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let h = if self.four { 27.0 } else { 34.0 };
        let four = self.four;
        let period = self.decks[i]
            .analysis
            .as_ref()
            .and_then(|a| a.grid)
            .map_or(0.5 * self.rate, |g| g.period(self.rate));
        let mut rows = div().flex().flex_col().gap(px(5.));
        for row in 0..2 {
            let mut r = div().flex().gap(px(5.));
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
                                    live.deck(i, DeckCmd::SetHotCue { slot: k, at: None });
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
                                live.deck(i, DeckCmd::AutoLoop(b));
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
                    Pads::Jump => {
                        let b = JUMPS[k];
                        let label =
                            format!("{}{}", if b < 0.0 { "−" } else { "+" }, beats_text(b.abs()));
                        pad(label, None, false, h).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |live, _, _, cx| {
                                live.deck(i, DeckCmd::BeatJump(b));
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
            seg(label, lit, 24.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    live.deck(i, cmd());
                    cx.notify();
                }),
            )
        };
        let resize = move |by: f64, label: &'static str, cx: &mut Context<Self>| {
            seg(label, false, 24.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    let b = (live.decks[i].loop_beats * by).clamp(1.0 / 32.0, 64.0);
                    live.decks[i].loop_beats = b;
                    if live.status().decks[i].loop_on {
                        live.deck(i, DeckCmd::LoopScale(by));
                    }
                    cx.notify();
                }),
            )
        };
        let four = self.four;
        tray()
            .when(!four, |t| {
                t.child(btn("In", set_in, || DeckCmd::LoopIn, cx))
                    .child(btn("Out", false, || DeckCmd::LoopOut, cx))
            })
            .child(resize(0.5, "½", cx))
            .child(
                div()
                    .flex_1()
                    .h(px(24.))
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
                            live.deck(i, DeckCmd::AutoLoop(b));
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

    /// The pitch fader, its reach, and keylock and the key beside it.
    fn tempo_column(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let d = &self.decks[i];
        let range = d.range;
        let fh = if self.four { 112.0 } else { 150.0 };
        div()
            .w(px(if self.four { 42. } else { 56. }))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(5.))
            .child(
                div()
                    .font_family(MONO)
                    .text_size(px(10.5))
                    .text_color(rgb(if s.sync { ACCENT_TEXT } else { TEXT_2 }))
                    .child(format!("{:+.1}%", (s.tempo - 1.0) * 100.0)),
            )
            .child(self.fader(Ctl::Tempo(i), fh, 26.0, cx))
            .child(
                seg(format!("±{}", (range * 100.0).round()), false, 20.0)
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
    }

    fn transport(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let d = &self.decks[i];
        let leader = self.leader() == Some(i);
        let four = self.four;
        let size = if four { 34.0 } else { 40.0 };
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
        div()
            .h(px(if four { 48. } else { 54. }))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(if four { 6. } else { 8. }))
            .px(px(10.))
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
                        live.deck(i, DeckCmd::Play);
                        cx.notify();
                    }),
                ),
            )
            .child(
                seg("Sync", s.sync, 28.0)
                    .when(s.sync, |d| d.bg(rgb(ACCENT)).text_color(rgb(0xffffff)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            let on = !live.status().decks[i].sync;
                            live.deck(i, DeckCmd::Sync(on));
                            cx.notify();
                        }),
                    ),
            )
            .child(
                seg("Lead", leader, 28.0)
                    .when(leader, |d| d.text_color(rgb(ACCENT_TEXT)))
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
            .child(icon_seg(Icon::Lock, d.keylock, 28.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    let on = !live.decks[i].keylock;
                    live.decks[i].keylock = on;
                    live.deck(i, DeckCmd::Keylock(on));
                    cx.notify();
                }),
            ))
            .when(!four, |t| t.child(self.key_buttons(i, s, cx)))
            .when(!four, |t| {
                t.child(
                    seg("Slip", d.slip, 28.0)
                        .when(d.slip, |d| d.text_color(rgb(AMBER)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |live, _, _, cx| {
                                let on = !live.decks[i].slip;
                                live.decks[i].slip = on;
                                live.deck(i, DeckCmd::Slip(on));
                                cx.notify();
                            }),
                        ),
                )
            })
            .when(!four, |t| {
                t.child(
                    seg("Rev", self.holding(i, Hold::Censor), 28.0).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| live.hold(i, Hold::Censor, cx)),
                    ),
                )
            })
    }

    fn key_buttons(&self, i: usize, s: &DeckStatus, cx: &mut Context<Self>) -> Div {
        let key = s.key;
        let step = |by: f64, label: &'static str, cx: &mut Context<Self>| {
            seg(label, false, 28.0).px(px(7.)).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                    let k = live.status().decks[i].key;
                    // Alt-click: back to the track's own key.
                    let next = if e.modifiers.alt {
                        0.0
                    } else {
                        (k + by).clamp(-12.0, 12.0)
                    };
                    live.deck(i, DeckCmd::Key(next));
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
