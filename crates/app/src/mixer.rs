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
const XF_H: f32 = 42.0;

/// The master's knob and the headphones' knobs, sized to its strip `w` wide.
fn master_knobs(w: f32, compact: bool) -> (f32, f32) {
    match compact {
        true => ((w * 0.46).clamp(28.0, 38.0), (w * 0.36).clamp(22.0, 28.0)),
        false => ((w * 0.5).clamp(34.0, 46.0), (w * 0.38).clamp(26.0, 34.0)),
    }
}

/// A channel's knobs' sizes: trim, each EQ band, the filter.
#[derive(Clone, Copy, Debug)]
pub struct Knobs {
    pub trim: f32,
    pub eq: f32,
    pub filter: f32,
}

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

    /// A channel's knob sizes, and what the decks' height leaves its fader (its other parts all
    /// fixed) and the master's meter. The knobs are as big as the room allows while the fader
    /// keeps a usable throw: trim smallest, the EQ bigger, the filter biggest, as on a mixer.
    fn mixer_budget(&self) -> (f32, f32, Knobs, bool) {
        let compact = self.narrow;
        // The strips' row, less each strip's border and a hair for type's rounding.
        let room = self.panel_h - XF_H - 8.0 - 2.0 - 4.0;
        // Everything but the knobs: padding, the deck's chip, three dividers, the EQ's panel
        // (padding, gaps and border), the sends and cue, the fader's top, its dB, and the gaps between.
        let fixed = match compact {
            true => 16.0 + 20.0 + 3.0 + 12.0 + 20.0 + 2.0 + 14.0 + 9.0 * 3.0,
            false => 16.0 + 20.0 + 3.0 + 12.0 + 22.0 + 2.0 + 14.0 + 9.0 * 5.0,
        };
        let (sw, mw) = crate::layout::mixer_dims(self.view.0, self.four);
        let tiers: &[(f32, f32, f32)] = match (compact, self.four) {
            (false, _) => &[
                (40., 60., 66.),
                (38., 56., 62.),
                (36., 52., 58.),
                (34., 49., 54.),
                (32., 46., 50.),
                (30., 42., 46.),
                (28., 38., 42.),
                (26., 34., 38.),
                (24., 30., 34.),
            ],
            (true, false) => &[
                (32., 46., 50.),
                (30., 42., 46.),
                (28., 38., 42.),
                (26., 34., 38.),
                (24., 30., 34.),
                (22., 26., 30.),
                (20., 24., 26.),
            ],
            (true, true) => &[
                (32., 44., 48.),
                (30., 40., 44.),
                (28., 36., 40.),
                (26., 32., 34.),
                (24., 30., 32.),
                (22., 26., 28.),
                (20., 24., 26.),
            ],
        };
        let least = if compact { 64.0 } else { 80.0 };
        // As big as the strip's height leaves its fader its throw, and its width the knobs room
        // beside their names (and, full size, their scales' ends).
        let (eq_cap, filter_cap) = match compact {
            true => (sw - 50.0, sw - 48.0),
            false => (sw - 70.0, sw - 66.0),
        };
        let fits = |t: &(f32, f32, f32)| {
            room - fixed - (t.0 + 3.0 * t.1 + t.2) >= least && t.1 <= eq_cap && t.2 <= filter_cap
        };
        let &(trim, eq, filter) = tiers
            .iter()
            .find(|t| fits(t))
            .unwrap_or(tiers.last().unwrap());
        // The master's strip but its meter: padding, its name, knob and level, two rules, its
        // peak and limiter lines, the headphones' panel (padding, border, its name, two knobs,
        // the mix's ends, the level, its gaps), and the gaps between.
        let (mk, pk) = master_knobs(mw, compact);
        let phones = |k: f32| 8.0 + 2.0 + 12.0 + 2.0 * k + 9.0 + 12.0 + 4.0 * 3.0;
        let full = match compact {
            // (Plus a hair each for type's rounding, measured, so it ends level with the
            // channels.)
            true => 20.0 + 12.0 + mk + 12.0 + 2.0 + 24.0 + phones(pk) + 8.0 * 4.0 + 2.0,
            false => 20.0 + 12.0 + mk + 12.0 + 2.0 + 24.0 + phones(pk) + 8.0 * 6.0 + 1.0,
        };
        // Short of room, the master's peak line goes (its meter still holds the peak).
        let tight = room - full < 56.0;
        let master = if tight {
            full - 12.0 - if compact { 4.0 } else { 6.0 }
        } else {
            full
        };
        (
            (room - fixed - (trim + 3.0 * eq + filter)).max(36.0),
            // The master's meter fills its card: no gap under it.
            (room - master).max(40.0),
            Knobs { trim, eq, filter },
            tight,
        )
    }

    /// A channel: its deck and crossfader side, trim and the three bands and the filter, its FX
    /// sends, cue, and its fader by its meter on a dB scale.
    fn strip(&self, ch: usize, cx: &mut Context<Self>) -> Div {
        let compact = self.narrow;
        let four = self.four;
        let color = DECK_COLORS[ch];
        let w = crate::layout::mixer_dims(self.view.0, four).0;
        let (fh, _, k, _) = self.mixer_budget();
        // A knob's row: its name and what it reads (compact, what it reads while it's turned).
        // `rw` its width: the strip's less a margin each side, or less the EQ panel's too.
        let row = |label: &'static str,
                   ctl: Ctl,
                   lit: u32,
                   ks: f32,
                   ends: (&'static str, &'static str),
                   rw: f32,
                   cx: &mut Context<Self>| {
            let turning = self.turning(ctl);
            let v = self.ctl_value(ctl);
            // In dB unless it says otherwise: the unit's left off beside the knob.
            let said = crate::detail::value_text(self, ctl, v)
                .trim_end_matches(" dB")
                .to_string();
            let (_, _, home) = ctl.range();
            let moved = (v - home).abs() > 0.01;
            let reads = div()
                .relative()
                .h(px(10.))
                .line_height(px(10.))
                .child(probe(
                    &self.probes,
                    format!("mixer.strips.strip{ch}.{label}"),
                ))
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
            // The knob with its scale's ends printed at the ends of its sweep (not compact).
            let side = if compact { 0.0 } else { 9.0 };
            let end = |t: &'static str, left: bool| {
                div()
                    .absolute()
                    .top(px(ks * 0.66))
                    .w(px(side + 2.0))
                    .when(left, |d| d.left_0())
                    .when(!left, |d| d.right_0().flex().justify_end())
                    .font_family(MONO)
                    .text_size(px(7.))
                    .text_color(rgb(TEXT_4))
                    .child(t)
            };
            let knob = div()
                .relative()
                .w(px(ks + 2.0 * side))
                .h(px(ks))
                .flex()
                .justify_center()
                .flex_none()
                .child(self.knob(ctl, "", lit, ks, cx))
                .when(!compact, |d| {
                    d.child(end(ends.0, true)).child(end(ends.1, false))
                });
            div()
                .w(px(rw))
                .h(px(ks))
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
                .child(knob)
        };
        // A printed line between sections, as on a mixer's faceplate.
        let rule = || div().w(px(w - 16.0)).h(px(1.)).flex_none().bg(rgb(LINE));
        let eq = |b: usize| {
            if self.mix.eq[ch][b] < 0.01 {
                RECORD
            } else {
                TEXT_2
            }
        };
        let side = self.mix.side[ch];
        let cue = self.mix.cue[ch];
        let gain = pina_live_engine::mixer::fader_gain(self.mix.fader[ch], 0.6);
        let db = 20.0 * gain.max(1e-6).log10();
        let send = |u: usize, cx: &mut Context<Self>| {
            let on = self.fx[u].on[ch];
            let ringing = on && self.fx[u].settings.on;
            div()
                .h(px(if compact { 20. } else { 22. }))
                .px(px(if compact { 3. } else { 6. }))
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
            .child(row(
                "TRIM",
                Ctl::Trim(ch),
                TEXT_2,
                k.trim,
                ("−", "+"),
                w - 16.0,
                cx,
            ))
            .child(rule())
            // The EQ, sunk in a panel of its own.
            .child(
                div()
                    .w(px(w - 10.0))
                    .py(px(3.))
                    .px(px(3.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(2.))
                    .rounded(px(6.))
                    .bg(alpha(0x000000, 0.22))
                    .border_1()
                    .border_color(rgb(LINE_SOFT))
                    .child(row(
                        "HIGH",
                        Ctl::Eq(ch, 2),
                        eq(2),
                        k.eq,
                        ("−∞", "+6"),
                        w - 26.0,
                        cx,
                    ))
                    .child(row(
                        "MID",
                        Ctl::Eq(ch, 1),
                        eq(1),
                        k.eq,
                        ("−∞", "+6"),
                        w - 26.0,
                        cx,
                    ))
                    .child(row(
                        "LOW",
                        Ctl::Eq(ch, 0),
                        eq(0),
                        k.eq,
                        ("−∞", "+6"),
                        w - 26.0,
                        cx,
                    )),
            )
            .child(rule())
            .child(row(
                "FILTER",
                Ctl::Filter(ch),
                ACCENT,
                k.filter,
                ("LP", "HP"),
                w - 16.0,
                cx,
            ))
            .child(rule())
            // Its FX sends and cue together, over its fader.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(if compact { 3. } else { 4. }))
                    .child(send(0, cx))
                    .child(send(1, cx))
                    .child(
                        div()
                            .h(px(if compact { 20. } else { 22. }))
                            .w(px(if compact { 22. } else { 28. }))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(5.))
                            .cursor_pointer()
                            .when(cue, |d| d.bg(rgb(AMBER)))
                            .when(!cue, |d| {
                                d.border_1()
                                    .border_color(rgb(LINE))
                                    .hover(|s| s.border_color(rgb(LINE_STRONG)))
                            })
                            .child(icon(Icon::Headphones, 13.).text_color(rgb(if cue {
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
        let (_, mh, _, tight) = self.mixer_budget();
        let w = crate::layout::mixer_dims(self.view.0, self.four).1;
        let (mk, pk) = master_knobs(w, compact);
        let segs = if compact { 16 } else { 24 };
        let rule = || div().w(px(w - 16.0)).h(px(1.)).flex_none().bg(rgb(LINE));
        // A readout on a line of its own, as wide as what it says (measured by the tests at
        // its longest, so it never runs out of the strip).
        let line = |name: &'static str, text: String, color: u32| {
            div()
                .relative()
                .h(px(12.))
                .line_height(px(12.))
                .whitespace_nowrap()
                .font_family(MONO)
                .text_size(px(8.5))
                .text_color(rgb(color))
                .child(probe(&self.probes, format!("mixer.strips.master.{name}")))
                .child(text)
        };
        let reads = |ctl: Ctl| {
            let (_, _, home) = ctl.range();
            let v = self.ctl_value(ctl);
            let color = match (self.turning(ctl), (v - home).abs() > 0.05) {
                (true, _) => ACCENT_TEXT,
                (_, true) => TEXT_2,
                _ => TEXT_4,
            };
            (crate::detail::value_text(self, ctl, v), color)
        };
        let (level, level_c) = reads(Ctl::Master);
        let (phones, phones_c) = reads(Ctl::Phones);
        // The meter's held peak, or CLIP while it's clipped.
        let held = self.holds[DECKS][0].0.max(self.holds[DECKS][1].0);
        let (peak, peak_c) = match (self.clips[DECKS] > 0, held > 1e-4) {
            (true, _) => ("CLIP".to_string(), RECORD),
            (_, true) => (db_text(20.0 * held.log10()), TEXT_3),
            _ => ("−∞".to_string(), TEXT_4),
        };
        let limiting = self.reduction > 0.5;

        div()
            .relative()
            .w(px(w))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(if compact { 4. } else { 6. }))
            .py(px(10.))
            .rounded(px(8.))
            .bg(rgb(STRIP))
            .border_1()
            .border_color(rgb(LINE))
            .child(probe(&self.probes, "mixer.strips.master"))
            .child(caption("MASTER"))
            .child(self.knob(Ctl::Master, "", TEXT, mk, cx))
            .child(line("level", level, level_c))
            .child(rule())
            // What goes out: the limiter's reduction down from the top, the two sides, the scale.
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(3.))
                    .child(gr_meter(self.reduction, mh, segs))
                    .child(meter_n(
                        self.meters[DECKS],
                        self.holds[DECKS].map(|h| h.0),
                        self.clips[DECKS] > 0,
                        mh,
                        segs,
                        4.0,
                    ))
                    .child(
                        div()
                            .h(px(mh))
                            .flex()
                            .flex_col()
                            .justify_between()
                            .font_family(MONO)
                            .text_size(px(7.5))
                            .text_color(rgb(TEXT_4))
                            .child("+3")
                            .child("0")
                            .child("−12")
                            .child("−24")
                            .child("−48"),
                    ),
            )
            .when(!tight, |d| d.child(line("peak", peak, peak_c)))
            .child(line(
                "limit",
                match limiting {
                    true => format!("LIM {}", db_text(-self.reduction)),
                    false => "LIM".into(),
                },
                if limiting { AMBER } else { TEXT_4 },
            ))
            .child(rule())
            // The headphones, sunk in a panel as the channels' EQ is: what's heard in them (the
            // cue against the master, its ends printed under it), and how loud.
            .child(
                div()
                    .w(px(w - 8.0))
                    .py(px(4.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(3.))
                    .rounded(px(6.))
                    .bg(alpha(0x000000, 0.22))
                    .border_1()
                    .border_color(rgb(LINE_SOFT))
                    .child(
                        div()
                            .h(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(3.))
                            .child(icon(Icon::Headphones, 11.).text_color(rgb(TEXT_3)))
                            .child(
                                div()
                                    .text_size(px(8.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT_4))
                                    .child("PHONES"),
                            ),
                    )
                    .child(self.knob(Ctl::CueMix, "", AMBER, pk, cx))
                    .child(
                        div()
                            .relative()
                            .w(px(w - 16.0))
                            .h(px(9.))
                            .flex()
                            .justify_between()
                            .font_family(MONO)
                            .text_size(px(7.))
                            .text_color(rgb(TEXT_4))
                            .child(probe(&self.probes, "mixer.strips.master.mix"))
                            .child("CUE")
                            .child("MST"),
                    )
                    .child(self.knob(Ctl::Phones, "", AMBER, pk, cx))
                    .child(line("phones", phones, phones_c)),
            )
            .child(mark(&self.probes, "mixer.strips.master.end"))
    }

    /// The crossfader, as a mixer's: the decks on each end lit in their colours over how much
    /// of that side is heard (as the curve has it), the fader in a sunk slot with its ticks and
    /// its middle marked, a long cap ridged to grip (its line green in the middle), and the
    /// curve beside it, drawn, clicked round smooth, blend and scratch.
    fn crossfader(&self, cx: &mut Context<Self>) -> Div {
        use pina_live_engine::mixer::side_gain;
        let x = self.mix.xf;
        let curve = self.settings.xf_curve;
        let v = (x + 1.0) / 2.0;
        let (w, cap) = (if self.four { 268.0 } else { 188.0 }, 22.0);
        let centred = x.abs() < 1e-4;
        let ends = |side: Side| {
            let on: Vec<usize> = self
                .channel_order()
                .into_iter()
                .filter(|&ch| self.mix.side[ch] == side)
                .collect();
            let heard = side_gain(side, x, curve);
            div()
                .w(px(30.))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(4.))
                .when(side == Side::A, |d| d.items_end())
                .child(
                    div()
                        .h(px(13.))
                        .flex()
                        .gap(px(4.))
                        .when(on.is_empty(), |d| {
                            d.child(div().text_color(rgb(TEXT_4)).child("—"))
                        })
                        .children(on.iter().map(|&ch| {
                            div()
                                .text_size(px(11.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(DECK_COLORS[ch]))
                                .child(DECK_NAMES[ch])
                        })),
                )
                // How much of this side's heard: its level through the crossfader.
                .child(
                    div()
                        .w(px(26.))
                        .h(px(3.))
                        .rounded(px(1.5))
                        .bg(rgb(WELL))
                        .flex()
                        .when(side == Side::A, |d| d.justify_end())
                        .child(
                            div()
                                .w(px(26.0 * heard))
                                .h_full()
                                .rounded(px(1.5))
                                .bg(rgb(on.first().map_or(TEXT_3, |&c| DECK_COLORS[c]))),
                        ),
                )
        };
        let ticks = 16;
        let scale = canvas(
            |_, _, _| {},
            move |b, _, window, _| {
                let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
                let span = w - cap;
                let q =
                    |window: &mut gpui::Window, x: f32, y: f32, w: f32, h: f32, c: gpui::Hsla| {
                        window.paint_quad(gpui::fill(
                            gpui::Bounds::new(
                                gpui::point(px(x0 + x), px(y0 + y)),
                                gpui::size(px(w), px(h)),
                            ),
                            c,
                        ))
                    };
                for k in 0..=ticks {
                    let tx = cap / 2.0 + k as f32 / ticks as f32 * span - 0.5;
                    let mid = k == ticks / 2;
                    let big = k % 4 == 0;
                    let h = if mid {
                        7.0
                    } else if big {
                        5.0
                    } else {
                        3.0
                    };
                    let c = if mid {
                        TEXT_3
                    } else if big {
                        LINE_STRONG
                    } else {
                        LINE
                    };
                    q(window, tx, 0.0, 1.0, h, rgb(c).into());
                    q(window, tx, 30.0 - h, 1.0, h, rgb(c).into());
                }
                // The slot, sunk.
                window.paint_quad(
                    gpui::fill(
                        gpui::Bounds::new(
                            gpui::point(px(x0 + cap / 2.0 - 3.0), px(y0 + 12.0)),
                            gpui::size(px(span + 6.0), px(6.)),
                        ),
                        rgb(0x08080b),
                    )
                    .corner_radii(px(3.))
                    .border_widths(px(1.))
                    .border_color(rgb(LINE)),
                );
            },
        )
        .absolute()
        .inset_0();
        div()
            .relative()
            .h(px(XF_H))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .px(px(12.))
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
                    .h(px(30.))
                    .flex_none()
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
                    .child(scale)
                    .child(
                        div()
                            .absolute()
                            .left(px(v * (w - cap)))
                            .top_0()
                            .w(px(cap))
                            .h(px(30.))
                            .rounded(px(4.))
                            .bg(gpui::linear_gradient(
                                180.0,
                                gpui::linear_color_stop(rgb(0x44444e), 0.0),
                                gpui::linear_color_stop(rgb(0x24242b), 1.0),
                            ))
                            .border_1()
                            .border_color(rgb(0x55555f))
                            .shadow_md()
                            .children([4.0, 7.0, 14.0, 17.0].map(|x| {
                                div()
                                    .absolute()
                                    .top(px(5.))
                                    .bottom(px(5.))
                                    .left(px(x - 1.0))
                                    .w(px(1.))
                                    .bg(alpha(0xffffff, 0.08))
                            }))
                            .child(
                                div()
                                    .absolute()
                                    .top(px(3.))
                                    .bottom(px(3.))
                                    .left(px(cap / 2.0 - 1.5))
                                    .w(px(1.))
                                    .bg(rgb(if centred { MINT } else { TEXT })),
                            ),
                    ),
            )
            .child(ends(Side::B))
            .child(self.curve_button(curve, cx))
            .child(mark(&self.probes, "mixer.xf.end"))
    }

    /// The crossfader's curve, drawn (each side's level across the fader), clicked round
    /// smooth, blend and scratch.
    fn curve_button(&self, curve: f32, cx: &mut Context<Self>) -> Div {
        use pina_live_engine::mixer::side_gain;
        let glyph = canvas(
            |_, _, _| {},
            move |b, _, window, _| {
                let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
                let (gw, gh) = (f32::from(b.size.width), f32::from(b.size.height));
                for (side, c) in [(Side::A, TEXT_2), (Side::B, TEXT_3)] {
                    let mut p = gpui::PathBuilder::stroke(px(1.25));
                    for k in 0..=24 {
                        let t = k as f32 / 24.0;
                        let g = side_gain(side, t * 2.0 - 1.0, curve);
                        let at =
                            gpui::point(px(x0 + t * gw), px(y0 + 1.0 + (1.0 - g) * (gh - 2.0)));
                        match k {
                            0 => p.move_to(at),
                            _ => p.line_to(at),
                        }
                    }
                    if let Ok(p) = p.build() {
                        window.paint_path(p, rgb(c));
                    }
                }
            },
        )
        .w(px(20.))
        .h(px(11.));
        crate::hw::display(34.0)
            .w(px(32.))
            .flex_col()
            .justify_center()
            .gap(px(3.))
            .cursor_pointer()
            .hover(|s| s.border_color(rgb(LINE_STRONG)))
            .child(glyph)
            .child(
                div()
                    .text_size(px(6.5))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(TEXT_4))
                    .child(match curve {
                        c if c < 0.25 => "SMOOTH",
                        c if c < 0.75 => "BLEND",
                        _ => "CUT",
                    }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    let next = match live.settings.xf_curve {
                        c if c < 0.25 => 0.5,
                        c if c < 0.75 => 1.0,
                        _ => 0.0,
                    };
                    live.set_xf_curve(next, cx);
                }),
            )
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
