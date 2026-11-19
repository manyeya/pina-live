//! The FX bar: two units side by side, each its effect (a dropdown), its length in beats, its
//! level and second knob, on or off, and which decks it's on.

use gpui::{
    Context, Div, FontWeight, MouseButton, anchored, canvas, deferred, div, prelude::*, rgb,
};
use pina_live_engine::{
    engine::{BLOCK, DECKS},
    fx::{FxCmd, FxKind, FxSettings, Insert, UNITS},
};

use crate::{
    controls::Ctl,
    decks::beats_text,
    live::{DECK_COLORS, DECK_NAMES, Live},
    ui::{scale::px, theme::*},
};

pub const FX_BAR_H: f32 = 70.0;
pub const FX_BEATS: [f64; 11] = [
    1.0 / 16.0,
    0.125,
    0.25,
    0.5,
    0.75,
    1.0,
    2.0,
    4.0,
    8.0,
    16.0,
    32.0,
];

/// A unit as the window keeps it.
#[derive(Clone, Copy, Default)]
pub struct FxUi {
    pub settings: FxSettings,
    pub on: [bool; DECKS],
}

impl Live {
    pub fn send_fx(&mut self, unit: usize) {
        let settings = self.fx[unit].settings;
        self.remote.fx(FxCmd::Settings { unit, settings });
    }

    /// A unit on or off; switched on while it's on no deck, it goes on the leading one (else A),
    /// so On always does something.
    pub fn toggle_fx(&mut self, u: usize, cx: &mut Context<Self>) {
        let on = !self.fx[u].settings.on;
        self.fx[u].settings.on = on;
        if on && !self.fx[u].on.iter().any(|x| *x) {
            let deck = self.leader().unwrap_or(0);
            self.fx[u].on[deck] = true;
            self.remote.fx(FxCmd::Assign {
                unit: u,
                ch: deck,
                on: true,
            });
        }
        self.send_fx(u);
        cx.notify();
    }

    /// A unit's send on a channel, on or off (from the FX bar or the channel's strip).
    pub fn toggle_send(&mut self, u: usize, ch: usize, cx: &mut Context<Self>) {
        let on = !self.fx[u].on[ch];
        self.fx[u].on[ch] = on;
        self.remote.fx(FxCmd::Assign { unit: u, ch, on });
        cx.notify();
    }

    /// Another effect for a unit: its memory made here, its time and knob where it likes them.
    pub fn set_fx_kind(&mut self, unit: usize, kind: FxKind) {
        let inserts = (0..DECKS)
            .map(|_| Box::new(Insert::new(kind, self.rate, BLOCK)))
            .collect();
        self.remote.fx(FxCmd::Kind { unit, inserts });
        let s = &mut self.fx[unit].settings;
        s.kind = kind;
        s.beats = kind.default_beats();
        s.param = kind.param_rest();
        self.send_fx(unit);
    }

    pub fn fx_bar(&self, cx: &mut Context<Self>) -> Div {
        div()
            .h(px(FX_BAR_H))
            .flex_none()
            .flex()
            .items_center()
            .border_t_1()
            .border_b_1()
            .border_color(rgb(LINE))
            .bg(rgb(SURFACE))
            .children((0..UNITS).map(|u| self.fx_unit(u, cx)))
    }

    /// A unit, as a mixer's beat FX section: its ON key (named for the unit, its light
    /// pulsing on the beat while it's on); the effect in a display with ◀ ▶ (clicked, the list);
    /// its length in beats in a display with ◀ ▶ and what that is in milliseconds now; its level
    /// and its own knob, each named over it and read under it; and a key for each deck it can go
    /// on, lit in the deck's colour.
    fn fx_unit(&self, u: usize, cx: &mut Context<Self>) -> Div {
        use crate::hw::{Led, display, key};
        let f = self.fx[u];
        let s = f.settings;
        let open = self.fx_menu == Some(u);
        let compact = self.narrow;
        let all = self.status().decks;
        // The beat it keeps: the leader's, else the next deck playing.
        let clock = self
            .leader()
            .map(|l| all[l])
            .filter(|d| d.loaded && d.bpm > 0.0)
            .or_else(|| all.iter().copied().find(|d| d.playing && d.bpm > 0.0));
        let pulse = clock
            .filter(|d| d.playing && !d.beat.is_nan())
            .map(|d| (1.0 - d.beat.rem_euclid(1.0) as f32).powi(3));
        let on_led = match (s.on, pulse) {
            (true, Some(p)) => Led::Beat(ACCENT, 0.25 + 0.75 * p),
            (true, None) => Led::On(ACCENT),
            _ => Led::Off,
        };
        let ms = clock.map(|d| s.beats * 60_000.0 / d.bpm);
        // Where the beat is in its bar of four, while it plays.
        let phase = clock
            .filter(|d| d.playing && !d.beat.is_nan())
            .map(|d| (d.beat.rem_euclid(4.0) / 4.0) as f32);
        let menu = div()
            .w(px(200.))
            .p(px(4.))
            .rounded(px(8.))
            .bg(rgb(CARD))
            .border_1()
            .border_color(rgb(LINE_STRONG))
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|live, _, _, cx| {
                live.fx_menu = None;
                cx.notify();
            }))
            .children(FxKind::ALL.into_iter().map(|kind| {
                let picked = kind == s.kind;
                div()
                    .h(px(26.))
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(5.))
                    .cursor_pointer()
                    .bg(rgb(if picked { RAISED } else { CARD }))
                    .text_color(rgb(if picked { TEXT } else { TEXT_2 }))
                    .hover(|st| st.bg(rgb(RAISED)))
                    .child(kind.name())
                    .when(kind.trails(), |d| {
                        d.child(
                            div()
                                .text_size(px(9.5))
                                .text_color(rgb(TEXT_4))
                                .child("rings out"),
                        )
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            cx.stop_propagation();
                            live.fx_menu = None;
                            live.set_fx_kind(u, kind);
                            cx.notify();
                        }),
                    )
            }));
        let dh = 40.0;
        let arrow = |label: &'static str, cx: &mut Context<Self>, f: Box<dyn Fn(&mut Live)>| {
            key(label, Led::None, false, dh, if compact { 5.0 } else { 7.0 })
                .text_size(px(9.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |live, _, _, cx| {
                        f(live);
                        cx.notify();
                    }),
                )
        };
        let step_kind = move |by: isize| -> Box<dyn Fn(&mut Live)> {
            Box::new(move |live: &mut Live| {
                let all = FxKind::ALL;
                let k = all
                    .iter()
                    .position(|&k| k == live.fx[u].settings.kind)
                    .unwrap_or(0);
                let next = all[(k as isize + by).rem_euclid(all.len() as isize) as usize];
                live.set_fx_kind(u, next);
            })
        };
        let step_beats = move |by: isize| -> Box<dyn Fn(&mut Live)> {
            Box::new(move |live: &mut Live| {
                let b = live.fx[u].settings.beats;
                let k = FX_BEATS
                    .iter()
                    .position(|&x| (x - b).abs() < 1e-9)
                    .unwrap_or(5) as isize;
                live.fx[u].settings.beats =
                    FX_BEATS[(k + by).clamp(0, FX_BEATS.len() as isize - 1) as usize];
                live.send_fx(u);
            })
        };
        // A knob named over it and read under it.
        let knob = |ctl: Ctl, name: &'static str, cx: &mut Context<Self>| {
            let turning = self.turning(ctl);
            div()
                .min_w(px(if compact { 40. } else { 52. }))
                .flex_none()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(1.))
                .child(
                    div()
                        .h(px(10.))
                        .line_height(px(10.))
                        .whitespace_nowrap()
                        .text_size(px(if compact { 7.5 } else { 8. }))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(if turning { ACCENT_TEXT } else { TEXT_4 }))
                        .child(name.to_uppercase()),
                )
                .child(self.knob(ctl, "", ACCENT, if compact { 26.0 } else { 30.0 }, cx))
                .child(
                    div()
                        .h(px(10.))
                        .line_height(px(10.))
                        .font_family(MONO)
                        .text_size(px(8.5))
                        .text_color(rgb(if turning { ACCENT_TEXT } else { TEXT_3 }))
                        .child(crate::detail::value_text(self, ctl, self.ctl_value(ctl))),
                )
        };
        let on_any = f.on.iter().any(|x| *x);
        div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .p(px(6.))
            .relative()
            .child(crate::probe::probe(&self.probes, format!("fx{u}")))
            .child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .gap(px(if compact { 6. } else { 8. }))
                    .px(px(if compact { 8. } else { 10. }))
                    .rounded(px(8.))
                    .bg(rgb(STRIP))
                    .border_1()
                    .border_color(rgb(LINE))
                    .overflow_hidden()
                    // On, named for the unit.
                    .child(
                        key(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(1.))
                                .child(if u == 0 { "FX 1" } else { "FX 2" })
                                .child(
                                    div()
                                        .text_size(px(8.))
                                        .text_color(rgb(if s.on { ACCENT_TEXT } else { TEXT_4 }))
                                        .child(if s.on { "ON" } else { "OFF" }),
                                ),
                            on_led,
                            false,
                            44.0,
                            if compact { 9.0 } else { 12.0 },
                        )
                        .text_size(px(11.))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |live, _, _, cx| {
                                if live.arm(crate::midi::Target::FxOn(u), cx) {
                                    return;
                                }
                                live.toggle_fx(u, cx);
                            }),
                        ),
                    )
                    // The effect.
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(arrow("◀", cx, step_kind(-1)))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        display(dh)
                                            .w(px(if compact { 96. } else { 128. }))
                                            .px(px(8.))
                                            .flex_col()
                                            .items_start()
                                            .justify_center()
                                            .cursor_pointer()
                                            .hover(|d| d.border_color(rgb(LINE_STRONG)))
                                            .child(
                                                div()
                                                    .max_w_full()
                                                    .overflow_hidden()
                                                    .whitespace_nowrap()
                                                    .text_ellipsis()
                                                    .text_size(px(if compact { 10.5 } else { 12. }))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(rgb(if s.on {
                                                        TEXT
                                                    } else {
                                                        TEXT_2
                                                    }))
                                                    .child(s.kind.name().to_uppercase()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(7.5))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(rgb(TEXT_4))
                                                    .child(match s.kind.trails() {
                                                        true => "RINGS OUT",
                                                        false => "CUT WITH IT",
                                                    }),
                                            )
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |live, _, _, cx| {
                                                    live.fx_menu = match live.fx_menu == Some(u) {
                                                        true => None,
                                                        false => Some(u),
                                                    };
                                                    cx.notify();
                                                }),
                                            ),
                                    )
                                    .when(open, |d| {
                                        d.child(
                                            deferred(anchored().snap_to_window().child(menu))
                                                .with_priority(1),
                                        )
                                    }),
                            )
                            .child(arrow("▶", cx, step_kind(1))),
                    )
                    // Its length.
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(arrow("◀", cx, step_beats(-1)))
                            .child(
                                display(dh)
                                    .w(px(if compact { 56. } else { 72. }))
                                    .flex_col()
                                    .justify_center()
                                    .child(
                                        div()
                                            .font_family(MONO)
                                            .text_size(px(14.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(TEXT))
                                            .child(beats_text_fx(s.beats)),
                                    )
                                    .child(
                                        div()
                                            .font_family(MONO)
                                            .text_size(px(8.))
                                            .text_color(rgb(TEXT_4))
                                            .child(match ms {
                                                Some(m) if m >= 1000.0 => {
                                                    format!("{:.2} s", m / 1000.0)
                                                }
                                                Some(m) => format!("{m:.0} ms"),
                                                None => "BEATS".into(),
                                            }),
                                    ),
                            )
                            .child(arrow("▶", cx, step_beats(1))),
                    )
                    .child(knob(Ctl::FxLevel(u), "LEVEL", cx))
                    .child(knob(Ctl::FxParam(u), s.kind.param_name(), cx))
                    // The bar it's timed against: its length marked as often as it repeats in
                    // a bar of four, the beat moving across it.
                    .child(
                        display(dh).flex_1().min_w(px(0.)).max_w(px(260.)).child(
                            canvas(
                                |_, _, _| {},
                                move |b, _, window, _| {
                                    paint_fx_bar(window, b, s.beats, s.on, phase)
                                },
                            )
                            .size_full(),
                        ),
                    )
                    .when(s.on && !on_any, |d| {
                        d.child(
                            div()
                                .text_size(px(10.))
                                .text_color(rgb(AMBER))
                                .child("On no deck"),
                        )
                    })
                    // The decks it's on.
                    .child(
                        div()
                            .flex()
                            .gap(px(3.))
                            .children((0..self.shown()).map(|d| {
                                let deck = self.channel_order()[d];
                                let on = f.on[deck];
                                key(
                                    div()
                                        .text_color(rgb(if on {
                                            DECK_COLORS[deck]
                                        } else {
                                            TEXT_3
                                        }))
                                        .child(DECK_NAMES[deck]),
                                    if on {
                                        Led::On(DECK_COLORS[deck])
                                    } else {
                                        Led::Off
                                    },
                                    false,
                                    32.0,
                                    if compact { 7.0 } else { 9.0 },
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |live, _, _, cx| {
                                        if live.arm(crate::midi::Target::FxAssign(u, deck), cx) {
                                            return;
                                        }
                                        live.toggle_send(u, deck, cx);
                                    }),
                                )
                            })),
                    )
                    .child(crate::probe::mark(&self.probes, format!("fx{u}.end"))),
            )
    }
}

/// A unit's time: "3/4" for a dotted eighth's worth, as well as "1/4", "2".
fn beats_text_fx(b: f64) -> String {
    if (b - 0.75).abs() < 1e-9 {
        "3/4".into()
    } else {
        beats_text(b)
    }
}

/// The bar an effect's timed against, in its display: four beats (each with its quarters), the
/// effect's length marked each time it comes round in the bar (by turns lighter and darker, lit
/// while it's on), and the beat moving across. Too narrow to read, nothing.
fn paint_fx_bar(
    window: &mut gpui::Window,
    b: gpui::Bounds<gpui::Pixels>,
    beats: f64,
    on: bool,
    phase: Option<f32>,
) {
    let (x0, y0) = (f32::from(b.origin.x) + 8.0, f32::from(b.origin.y));
    let (w, h) = (f32::from(b.size.width) - 16.0, f32::from(b.size.height));
    if w < 48.0 {
        return;
    }
    let q = |window: &mut gpui::Window, x: f32, y: f32, ww: f32, hh: f32, c: gpui::Hsla| {
        window.paint_quad(gpui::fill(
            gpui::Bounds::new(gpui::point(px(x), px(y)), gpui::size(px(ww), px(hh))),
            c,
        ))
    };
    let (top, band) = (y0 + h * 0.3, h * 0.4);
    let lit = if on { ACCENT } else { TEXT_4 };
    // Its length, as often as it comes round in the bar (a long one fills the bar).
    let len = (beats as f32 / 4.0).min(1.0);
    let mut at = 0.0f32;
    let mut k = 0;
    while at < 1.0 - 1e-4 && k < 64 {
        let end = (at + len).min(1.0);
        q(
            window,
            x0 + at * w,
            top,
            ((end - at) * w - 1.5).max(1.0),
            band,
            alpha(lit, if k % 2 == 0 { 0.55 } else { 0.32 }),
        );
        at = end;
        k += 1;
    }
    // The beats and their quarters.
    for t in 0..=16 {
        let beat = t % 4 == 0;
        let x = x0 + t as f32 / 16.0 * w;
        let hh = if beat { band + 8.0 } else { 4.0 };
        q(
            window,
            x - 0.5,
            top + band / 2.0 - hh / 2.0,
            1.0,
            hh,
            alpha(TEXT, if beat { 0.35 } else { 0.15 }),
        );
    }
    if let Some(p) = phase {
        q(
            window,
            x0 + p * w - 1.0,
            y0 + 5.0,
            2.0,
            h - 10.0,
            rgb(TEXT).into(),
        );
    }
}
