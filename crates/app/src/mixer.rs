//! The mixer between the decks: a strip a channel (its deck and crossfader side, trim, the three
//! bands with their kills, the filter, its FX sends, cue, and its fader by its meter on a dB
//! scale), the master's card between them (level, meter, the limiter at work, the cue mix and the
//! headphones' level), and the crossfader under them.

use gpui::{Context, Div, FontWeight, MouseButton, MouseDownEvent, canvas, div, prelude::*, rgb};
use pina_live_engine::{
    engine::DECKS,
    mixer::{MixCmd, Side},
};

use crate::{
    controls::Ctl,
    decks::caption,
    live::{DECK_COLORS, DECK_NAMES, Live},
    probe::{mark, probe},
    ui::{scale::px, theme::*},
};

/// The crossfader card's height.
const XF_H: f32 = 40.0;

/// Level meters fall back this much each frame (of 30 a second).
const FALL: f32 = 0.82;

impl Live {
    /// The channels, left to right, as the decks sit.
    pub fn channel_order(&self) -> Vec<usize> {
        match self.four {
            true => vec![0, 2, 1, 3],
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
        // Peaks held a second and a half, then falling; a clip lit for three seconds.
        for m in 0..=DECKS {
            let new = if m < DECKS { p.levels[m] } else { p.master };
            for s in 0..2 {
                let h = &mut self.holds[m][s];
                if new[s] >= h.0 {
                    *h = (new[s], 45);
                } else if h.1 > 0 {
                    h.1 -= 1;
                } else {
                    h.0 *= FALL;
                }
            }
            self.clips[m] = match new[0].max(new[1]) >= 0.999 {
                true => 90,
                false => self.clips[m].saturating_sub(1),
            };
        }
    }

    pub fn mixer_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let order = self.channel_order();
        let h = self.panel_h;
        let half = order.len() / 2;
        let mut strips = div()
            .relative()
            .h(px(h - XF_H - 8.0))
            .flex_none()
            .flex()
            .items_start()
            .justify_center()
            .gap(px(8.))
            .child(probe(&self.probes, "mixer.strips"));
        for (k, &ch) in order.iter().enumerate() {
            if k == half {
                strips = strips.child(self.master_card(cx));
            }
            strips = strips.child(self.strip(ch, cx));
        }
        div()
            .relative()
            .h(px(h))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(probe(&self.probes, "mixer"))
            .child(strips)
            .child(self.crossfader(cx))
    }

    /// What the decks' height leaves a channel's fader (its other parts all fixed), and the
    /// master's meter.
    fn mixer_budget(&self) -> (f32, f32) {
        let compact = self.narrow;
        // The strips' row, less each strip's border and a hair for type's rounding.
        let room = self.panel_h - XF_H - 8.0 - 2.0 - 4.0;
        let strip = match compact {
            true => 16.0 + 20.0 + 5.0 * 24.0 + 20.0 + 22.0 + 2.0 + 14.0 + 9.0 * 3.0,
            false => 16.0 + 20.0 + 5.0 * 30.0 + 20.0 + 24.0 + 2.0 + 14.0 + 9.0 * 5.0,
        };
        let master = match compact {
            // Its name, knob, label and the cue mix and phones knobs, as drawn (measured).
            true => 20.0 + 12.0 + 32.0 + 12.0 + 40.0 + 4.0 * 6.0 + 6.0,
            false => 20.0 + 12.0 + 40.0 + 12.0 + 40.0 + 4.0 * 10.0 + 10.0,
        };
        (
            (room - strip).max(36.0),
            // The master's meter fills its card: no gap under it.
            (room - master).max(48.0),
        )
    }

    /// A channel: its deck and crossfader side, trim and the three bands and the filter, its FX
    /// sends, cue, and its fader by its meter on a dB scale.
    fn strip(&self, ch: usize, cx: &mut Context<Self>) -> Div {
        let compact = self.narrow;
        let four = self.four;
        let color = DECK_COLORS[ch];
        let w = if four { 82.0 } else { 104.0 };
        let ks = if compact { 22.0 } else { 28.0 };
        // A knob's row: its name and what it reads (compact, what it reads while it's turned).
        let row = |label: &'static str, ctl: Ctl, lit: u32, cx: &mut Context<Self>| {
            let turning = self.turning(ctl);
            let v = self.ctl_value(ctl);
            let mut said = crate::detail::value_text(self, ctl, v);
            if compact {
                said = said.trim_end_matches(" dB").to_string();
            }
            let (_, _, home) = ctl.range();
            let moved = (v - home).abs() > 0.01;
            let reads = div()
                .h(px(10.))
                .line_height(px(10.))
                .font_family(MONO)
                .text_size(px(8.5))
                .whitespace_nowrap()
                .text_color(rgb(match (said.as_str(), turning, moved) {
                    ("KILL", ..) => RECORD,
                    (_, true, _) => ACCENT_TEXT,
                    (_, _, true) => TEXT_2,
                    _ => TEXT_4,
                }))
                .child(said);
            div()
                .w(px(w - 20.0))
                .h(px(if compact { 24. } else { 30. }))
                .flex()
                .items_center()
                .justify_between()
                .child(match (compact, turning) {
                    (true, true) => reads,
                    (true, false) => div()
                        .text_size(px(9.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT_4))
                        .child(label),
                    (false, _) => div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .h(px(11.))
                                .line_height(px(11.))
                                .text_size(px(9.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT_4))
                                .child(label),
                        )
                        .child(reads),
                })
                .child(self.knob(ctl, "", lit, ks, cx))
        };
        let eq = |b: usize| {
            if self.mix.eq[ch][b] < 0.01 {
                RECORD
            } else {
                TEXT_2
            }
        };
        let side = self.mix.side[ch];
        let cue = self.mix.cue[ch];
        let fh = self.mixer_budget().0;
        let gain = pina_live_engine::mixer::fader_gain(self.mix.fader[ch], 0.6);
        let db = 20.0 * gain.max(1e-6).log10();
        let send = |u: usize, cx: &mut Context<Self>| {
            let on = self.fx[u].on[ch];
            let ringing = on && self.fx[u].settings.on;
            div()
                .h(px(20.))
                .px(px(5.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .when(ringing, |d| d.bg(rgb(ACCENT)).text_color(rgb(0xffffff)))
                .when(on && !ringing, |d| {
                    d.border_1()
                        .border_color(rgb(ACCENT))
                        .text_color(rgb(ACCENT_TEXT))
                })
                .when(!on, |d| {
                    d.border_1()
                        .border_color(rgb(LINE))
                        .text_color(rgb(TEXT_4))
                        .hover(|s| s.text_color(rgb(TEXT_2)))
                })
                .child(format!("FX{}", u + 1))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |live, _, _, cx| {
                        cx.stop_propagation();
                        live.toggle_send(u, ch, cx);
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
            .gap(px(if compact { 3. } else { 5. }))
            .py(px(8.))
            .child(probe(&self.probes, format!("mixer.strips.strip{ch}")))
            .rounded(px(8.))
            .bg(rgb(STRIP))
            .border_1()
            .border_color(rgb(LINE))
            // The deck, and which side of the crossfader it's on (clicked round).
            .child(
                div()
                    .h(px(20.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .rounded(px(4.))
                    .bg(alpha(color, 0.16))
                    .cursor_pointer()
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(color))
                            .child(DECK_NAMES[ch]),
                    )
                    // Its side of the crossfader: ◀ left, ▶ right, or thru (not on it).
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(9.))
                            .text_color(rgb(TEXT_3))
                            .child(match side {
                                Side::A => "◀",
                                Side::Thru => "thru",
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
            )
            .child(row("TRIM", Ctl::Trim(ch), TEXT_2, cx))
            .child(row("HIGH", Ctl::Eq(ch, 2), eq(2), cx))
            .child(row("MID", Ctl::Eq(ch, 1), eq(1), cx))
            .child(row("LOW", Ctl::Eq(ch, 0), eq(0), cx))
            .child(row("FILTER", Ctl::Filter(ch), ACCENT, cx))
            .child(
                div()
                    .flex()
                    .gap(px(4.))
                    .child(send(0, cx))
                    .child(send(1, cx)),
            )
            .child(
                div()
                    .h(px(if compact { 22. } else { 24. }))
                    .w(px(w - 30.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(5.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(cue, |d| d.bg(rgb(AMBER)).text_color(rgb(0x0c0c0f)))
                    .when(!cue, |d| {
                        d.border_1()
                            .border_color(rgb(LINE))
                            .text_color(rgb(TEXT_3))
                            .hover(|s| s.text_color(rgb(TEXT_2)))
                    })
                    .child(icon(Icon::Headphones, 12.).text_color(rgb(if cue {
                        0x0c0c0f
                    } else {
                        TEXT_3
                    })))
                    .child("Cue")
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
            )
            .child(
                div()
                    .pt(px(2.))
                    .flex()
                    .items_end()
                    .gap(px(5.))
                    .child(self.fader(Ctl::Fader(ch), fh, 30.0, cx))
                    .child(meter_n(
                        self.meters[ch],
                        self.holds[ch].map(|h| h.0),
                        self.clips[ch] > 0,
                        fh - 2.0,
                        if compact { 12 } else { 16 },
                        3.0,
                    ))
                    .when(!four, |d| {
                        d.child(
                            div()
                                .h(px(fh - 2.0))
                                .flex()
                                .flex_col()
                                .justify_between()
                                .font_family(MONO)
                                .text_size(px(8.))
                                .text_color(rgb(TEXT_4))
                                .child("0")
                                .child("−12")
                                .child("−24")
                                .child("−∞"),
                        )
                    }),
            )
            .child(
                div()
                    .h(px(14.))
                    .line_height(px(14.))
                    .font_family(MONO)
                    .text_size(px(10.))
                    .text_color(rgb(TEXT_3))
                    .child(match db < -60.0 {
                        true => "−∞ dB".to_string(),
                        false => format!("{} dB", db_text(db)),
                    }),
            )
            .child(mark(&self.probes, format!("mixer.strips.strip{ch}.end")))
    }

    /// The master: its level and meter, the limiter at work, the headphones' cue mix and level.
    fn master_card(&self, cx: &mut Context<Self>) -> Div {
        let compact = self.narrow;
        let limiting = self.reduction > 0.5;
        let mh = self.mixer_budget().1;
        div()
            .relative()
            .w(px(if self.four { 96. } else { 112. }))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(if compact { 6. } else { 10. }))
            .py(px(10.))
            .rounded(px(8.))
            .bg(rgb(STRIP))
            .border_1()
            .border_color(rgb(LINE))
            .child(probe(&self.probes, "mixer.strips.master"))
            .child(caption("MASTER"))
            .child(self.knob(Ctl::Master, "", TEXT, if compact { 32. } else { 40. }, cx))
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(5.))
                    // What the limiter's taking off, down from the top: 0 to 12 dB.
                    .child(gr_meter(self.reduction, mh, if compact { 16 } else { 24 }))
                    .child(meter_n(
                        self.meters[DECKS],
                        self.holds[DECKS].map(|h| h.0),
                        self.clips[DECKS] > 0,
                        mh,
                        if compact { 16 } else { 24 },
                        5.0,
                    ))
                    .child(
                        div()
                            .h(px(mh))
                            .flex()
                            .flex_col()
                            .justify_between()
                            .font_family(MONO)
                            .text_size(px(8.))
                            .text_color(rgb(TEXT_4))
                            .child("+3")
                            .child("0")
                            .child("−12")
                            .child("−24")
                            .child("−48"),
                    ),
            )
            .child(
                div()
                    .font_family(MONO)
                    .text_size(px(9.5))
                    .text_color(rgb(if limiting { AMBER } else { TEXT_4 }))
                    .child(match limiting {
                        true => format!("Limiting −{:.1}", self.reduction),
                        false => "Limiter".into(),
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(self.knob(Ctl::CueMix, "CUE MIX", AMBER, 26.0, cx))
                    .child(self.knob(Ctl::Phones, "PHONES", AMBER, 26.0, cx)),
            )
            .child(mark(&self.probes, "mixer.strips.master.end"))
    }

    fn crossfader(&self, cx: &mut Context<Self>) -> Div {
        let v = (self.mix.xf + 1.0) / 2.0;
        let (w, cap) = (if self.four { 220.0 } else { 240.0 }, 20.0);
        // Each end says which decks are on it (as their channels' strips set them).
        let ends = |side: Side| {
            let on: Vec<usize> = self
                .channel_order()
                .into_iter()
                .filter(|&ch| self.mix.side[ch] == side)
                .collect();
            div()
                .min_w(px(28.))
                .flex()
                .gap(px(4.))
                .when(side == Side::A, |d| d.justify_end())
                .when(on.is_empty(), |d| {
                    d.child(div().text_color(rgb(TEXT_4)).child("—"))
                })
                .children(on.into_iter().map(|ch| {
                    div()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(DECK_COLORS[ch]))
                        .child(DECK_NAMES[ch])
                }))
        };
        div()
            .relative()
            .h(px(XF_H))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .rounded(px(8.))
            .bg(rgb(STRIP))
            .border_1()
            .border_color(rgb(LINE))
            .child(probe(&self.probes, "mixer.xf"))
            .child(ends(Side::A))
            .child(
                div()
                    .relative()
                    .w(px(w))
                    .h(px(28.))
                    .cursor(gpui::CursorStyle::ResizeLeftRight)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|live, e: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            live.press_ctl(Ctl::Xf, e, cx)
                        }),
                    )
                    .on_scroll_wheel(cx.listener(|live, e: &gpui::ScrollWheelEvent, _, cx| {
                        live.wheel_ctl(Ctl::Xf, e, cx)
                    }))
                    .child(
                        div()
                            .absolute()
                            .left(px(cap / 2.0))
                            .top(px(12.))
                            .w(px(w - cap))
                            .h(px(4.))
                            .rounded(px(2.))
                            .bg(rgb(WELL))
                            .border_1()
                            .border_color(rgb(LINE)),
                    )
                    .children((0..=8).map(|k| {
                        div()
                            .absolute()
                            .top(px(2.))
                            .left(px(cap / 2.0 + k as f32 * (w - cap) / 8.0))
                            .w(px(1.))
                            .h(px(if k % 4 == 0 { 7. } else { 4. }))
                            .bg(rgb(LINE_STRONG))
                    }))
                    .child(
                        div()
                            .absolute()
                            .left(px(v * (w - cap)))
                            .top(px(3.))
                            .w(px(cap))
                            .h(px(22.))
                            .rounded(px(3.))
                            .bg(gpui::linear_gradient(
                                90.0,
                                gpui::linear_color_stop(rgb(0x41414b), 0.0),
                                gpui::linear_color_stop(rgb(0x26262d), 1.0),
                            ))
                            .border_1()
                            .border_color(rgb(0x52525e))
                            .shadow_md()
                            .children([4.0, 15.0].map(|x| {
                                div()
                                    .absolute()
                                    .top(px(4.))
                                    .bottom(px(4.))
                                    .left(px(x))
                                    .w(px(1.))
                                    .bg(alpha(0xffffff, 0.09))
                            }))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(cap / 2.0 - 0.5))
                                    .top(px(4.))
                                    .w(px(1.))
                                    .h(px(12.))
                                    .bg(rgb(TEXT)),
                            ),
                    ),
            )
            .child(ends(Side::B))
            .child(mark(&self.probes, "mixer.xf.end"))
    }
}

/// The limiter's gain reduction, `h` tall in `segs` segments, lit amber down from the top: a
/// segment for each half-decibel to 12 dB.
pub fn gr_meter(db: f32, h: f32, segs: usize) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
            let gap = 1.0;
            let sh = (h - gap * (segs - 1) as f32) / segs as f32;
            let lit = (db / 12.0 * segs as f32).ceil() as usize;
            for k in 0..segs {
                window.paint_quad(gpui::fill(
                    gpui::Bounds::new(
                        gpui::point(gpui::px(x0), gpui::px(y0 + k as f32 * (sh + gap))),
                        gpui::size(gpui::px(3.), gpui::px(sh)),
                    ),
                    match k < lit {
                        true => gpui::Hsla::from(rgb(AMBER)),
                        false => alpha(RAISED, 0.5),
                    },
                ));
            }
        },
    )
    .w(px(3.))
    .h(px(h))
    .flex_none()
}

/// A stereo meter `h` tall with `segs` segments, each side `w` wide: lit to its level, its held
/// peak a lit segment over it, its top segment red while it's clipped.
pub fn meter_n(
    level: [f32; 2],
    hold: [f32; 2],
    clip: bool,
    h: f32,
    segs: usize,
    w: f32,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
            let gap = 1.0;
            let sh = (h - gap * (segs - 1) as f32) / segs as f32;
            let seg_of =
                |v: f32| ((20.0 * v.max(1e-6).log10() + 48.0) / 51.0 * segs as f32).ceil() as i32;
            for (side, &v) in level.iter().enumerate() {
                let lit = seg_of(v);
                let held = seg_of(hold[side]) - 1;
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
                    window.paint_quad(gpui::fill(
                        gpui::Bounds::new(
                            gpui::point(gpui::px(x0 + side as f32 * (w + 1.0)), gpui::px(y)),
                            gpui::size(gpui::px(w), gpui::px(sh)),
                        ),
                        if clip && k + 1 == segs {
                            gpui::Hsla::from(rgb(RECORD))
                        } else if (k as i32) < lit || (k as i32 == held && held > 0) {
                            gpui::Hsla::from(rgb(color))
                        } else {
                            alpha(RAISED, 0.8)
                        },
                    ));
                }
            }
        },
    )
    .w(px(w * 2.0 + 1.0))
    .h(px(h))
    .flex_none()
}
