//! The mixer between the decks, laid out as a club mixer: a row a control (trim, the three
//! bands, the filter) across the channels with its name in the middle, then cue, the channel
//! faders with their meters, which side of the crossfader each is on, and the crossfader; the
//! master, the cue mix and the headphones' level on the right with the master's meter.

use gpui::{Context, Div, FontWeight, MouseButton, MouseDownEvent, canvas, div, prelude::*, rgb};
use pina_live_engine::{
    engine::DECKS,
    mixer::{MixCmd, Side},
};

use crate::{
    controls::Ctl,
    decks::{PANEL_H, PANEL_H_FOUR},
    live::{DECK_COLORS, DECK_NAMES, Live},
    ui::{scale::px, theme::*},
};

/// Level meters fall back this much each frame (of 30 a second).
const FALL: f32 = 0.82;

impl Live {
    /// The channels, left to right, as the decks sit.
    pub fn channel_order(&self) -> Vec<usize> {
        match self.four {
            true => vec![2, 0, 1, 3],
            false => vec![0, 1],
        }
    }

    /// The meters moved on: up to what's come in, else falling back.
    pub fn fall_meters(&mut self) {
        let p = self.remote.take_peaks();
        for ch in 0..DECKS {
            for s in 0..2 {
                self.meters[ch][s] = p.levels[ch][s].max(self.meters[ch][s] * FALL);
            }
        }
        for s in 0..2 {
            self.meters[DECKS][s] = p.master[s].max(self.meters[DECKS][s] * FALL);
        }
        self.reduction = p.reduction.max(self.reduction * FALL);
    }

    pub fn mixer_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let order = self.channel_order();
        let h = if self.narrow { PANEL_H_FOUR } else { PANEL_H };
        let half = order.len() / 2;
        let knob_size = if self.narrow { 24.0 } else { 28.0 };
        let row = |label: &'static str,
                   ctl: &dyn Fn(usize) -> Ctl,
                   color: u32,
                   cx: &mut Context<Self>| {
            let mut r = div().flex().items_center().justify_between();
            for (k, &ch) in order.iter().enumerate() {
                if k == half {
                    r = r.child(
                        div()
                            .w(px(40.))
                            .text_center()
                            .text_size(px(9.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT_4))
                            .child(label),
                    );
                }
                r = r.child(
                    div()
                        .w(px(knob_size + 12.0))
                        .flex()
                        .justify_center()
                        .child(self.knob(ctl(ch), "", color, knob_size, cx)),
                );
            }
            r
        };
        div()
            .w(px(if self.four { 380. } else { 300. }))
            .h(px(h))
            .flex_none()
            .flex()
            .bg(rgb(STRIP))
            .border_r_1()
            .border_color(rgb(LINE))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .px(px(8.))
                    .pt(px(6.))
                    .gap(px(2.))
                    .child(self.channel_heads(&order, half, cx))
                    .child(row("TRIM", &Ctl::Trim, TEXT_2, cx))
                    .child(row("HI", &|c| Ctl::Eq(c, 2), TEXT_2, cx))
                    .child(row("MID", &|c| Ctl::Eq(c, 1), TEXT_2, cx))
                    .child(row("LOW", &|c| Ctl::Eq(c, 0), TEXT_2, cx))
                    .child(row("FILTER", &Ctl::Filter, ACCENT, cx))
                    .child(self.cue_row(&order, half, cx))
                    .child(self.fader_row(&order, half, cx))
                    .child(self.crossfader(cx)),
            )
            .child(self.master_column(cx))
    }

    fn channel_heads(&self, order: &[usize], half: usize, cx: &mut Context<Self>) -> Div {
        let mut r = div().flex().items_center().justify_between().h(px(20.));
        for (k, &ch) in order.iter().enumerate() {
            if k == half {
                r = r.child(div().w(px(40.)));
            }
            let side = self.mix.side[ch];
            r = r.child(
                div().w(px(40.)).flex().justify_center().child(
                    div()
                        .px(px(6.))
                        .h(px(18.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(RAISED)))
                        .text_size(px(10.5))
                        .font_weight(FontWeight::BOLD)
                        .child(div().text_color(rgb(DECK_COLORS[ch])).child(DECK_NAMES[ch]))
                        // Which side of the crossfader: clicked round A, thru, B.
                        .child(
                            div()
                                .font_family(MONO)
                                .text_size(px(9.))
                                .text_color(rgb(TEXT_3))
                                .child(match side {
                                    Side::A => "◀",
                                    Side::Thru => "·",
                                    Side::B => "▶",
                                }),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |live, _, _, cx| {
                                let next = match live.mix.side[ch] {
                                    Side::A => Side::Thru,
                                    Side::Thru => Side::B,
                                    Side::B => Side::A,
                                };
                                live.mix.side[ch] = next;
                                live.mixer(MixCmd::Side { ch, side: next });
                                cx.notify();
                            }),
                        ),
                ),
            );
        }
        r
    }

    fn cue_row(&self, order: &[usize], half: usize, cx: &mut Context<Self>) -> Div {
        let mut r = div().flex().items_center().justify_between().h(px(28.));
        for (k, &ch) in order.iter().enumerate() {
            if k == half {
                r = r.child(
                    div()
                        .w(px(40.))
                        .text_center()
                        .text_size(px(9.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT_4))
                        .child("CUE"),
                );
            }
            let on = self.mix.cue[ch];
            r = r.child(
                div().w(px(40.)).flex().justify_center().child(
                    div()
                        .w(px(34.))
                        .h(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .cursor_pointer()
                        .border_1()
                        .border_color(rgb(if on { AMBER } else { LINE }))
                        .bg(rgb(if on { AMBER } else { CARD }))
                        .child(icon(Icon::Headphones, 13.).text_color(rgb(if on {
                            0x0c0c0f
                        } else {
                            TEXT_3
                        })))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |live, _, _, cx| {
                                if live.arm(crate::midi::Target::Pfl(ch), cx) {
                                    return;
                                }
                                let on = !live.mix.cue[ch];
                                live.mix.cue[ch] = on;
                                live.mixer(MixCmd::Cue { ch, on });
                                cx.notify();
                            }),
                        ),
                ),
            );
        }
        r
    }

    fn fader_row(&self, order: &[usize], half: usize, cx: &mut Context<Self>) -> Div {
        let fh = if self.narrow { 64.0 } else { 82.0 };
        let mut r = div().flex().items_end().justify_between().pt(px(4.));
        for (k, &ch) in order.iter().enumerate() {
            if k == half {
                r = r.child(div().w(px(40.)));
            }
            r = r.child(
                div()
                    .w(px(40.))
                    .flex()
                    .justify_center()
                    .gap(px(3.))
                    .child(meter(self.meters[ch], fh))
                    .child(self.fader(Ctl::Fader(ch), fh, 22.0, cx)),
            );
        }
        r
    }

    fn crossfader(&self, cx: &mut Context<Self>) -> Div {
        let v = (self.mix.xf + 1.0) / 2.0;
        let (w, cap) = (150.0, 18.0);
        div()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .pt(px(8.))
            .child(
                div()
                    .text_size(px(10.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(TEXT_3))
                    .child("A"),
            )
            .child(
                div()
                    .relative()
                    .w(px(w))
                    .h(px(22.))
                    .cursor(gpui::CursorStyle::ResizeLeftRight)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|live, e: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            live.press_ctl(Ctl::Xf, e, cx)
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(cap / 2.0))
                            .top(px(9.))
                            .w(px(w - cap))
                            .h(px(4.))
                            .rounded(px(2.))
                            .bg(rgb(WELL))
                            .border_1()
                            .border_color(rgb(LINE)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(w / 2.0))
                            .top(px(5.))
                            .w(px(1.))
                            .h(px(12.))
                            .bg(rgb(LINE_STRONG)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(v * (w - cap)))
                            .top(px(2.))
                            .w(px(cap))
                            .h(px(18.))
                            .rounded(px(3.))
                            .bg(rgb(RAISED))
                            .border_1()
                            .border_color(rgb(LINE_STRONG))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(cap / 2.0 - 0.5))
                                    .top(px(3.))
                                    .w(px(1.))
                                    .h(px(10.))
                                    .bg(rgb(TEXT_2)),
                            ),
                    ),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(TEXT_3))
                    .child("B"),
            )
    }

    fn master_column(&self, cx: &mut Context<Self>) -> Div {
        let limiting = self.reduction > 0.5;
        div()
            .w(px(78.))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .pt(px(8.))
            .border_l_1()
            .border_color(rgb(LINE_SOFT))
            .child(self.knob(Ctl::Master, "MASTER", TEXT, 34.0, cx))
            .child(self.knob(Ctl::CueMix, "CUE MIX", AMBER, 28.0, cx))
            .child(self.knob(Ctl::Phones, "PHONES", AMBER, 28.0, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(3.))
                    .child(meter(self.meters[DECKS], 70.0))
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(9.))
                            .text_color(rgb(if limiting { AMBER } else { TEXT_4 }))
                            .child(match limiting {
                                true => format!("−{:.1} dB", self.reduction),
                                false => "LIMIT".into(),
                            }),
                    ),
            )
    }
}

/// A stereo meter `h` tall: -48 to +3 dB, green to amber at -12, red from -3.
pub fn meter(level: [f32; 2], h: f32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
            let segs = 20;
            let gap = 1.0;
            let sh = (h - gap * (segs - 1) as f32) / segs as f32;
            for (side, &v) in level.iter().enumerate() {
                let db = 20.0 * v.max(1e-6).log10();
                let lit = ((db + 48.0) / 51.0 * segs as f32).ceil() as i32;
                for k in 0..segs {
                    let seg_db = -48.0 + (k as f32 + 1.0) * 51.0 / segs as f32;
                    let color = if seg_db > -3.0 {
                        METER_HIGH
                    } else if seg_db > -12.0 {
                        METER_MID
                    } else {
                        METER_LOW
                    };
                    let y = y0 + h - (k + 1) as f32 * (sh + gap) + gap;
                    let on = (k as i32) < lit;
                    window.paint_quad(gpui::fill(
                        gpui::Bounds::new(
                            gpui::point(gpui::px(x0 + side as f32 * 4.0), gpui::px(y)),
                            gpui::size(gpui::px(3.), gpui::px(sh)),
                        ),
                        if on {
                            gpui::Hsla::from(rgb(color))
                        } else {
                            alpha(RAISED, 0.8)
                        },
                    ));
                }
            }
        },
    )
    .w(px(7.))
    .h(px(h))
    .flex_none()
}
