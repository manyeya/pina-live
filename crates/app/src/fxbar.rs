//! The FX bar: two units side by side, each its effect (a dropdown), its length in beats, its
//! level and second knob, on or off, and which decks it's on.

use gpui::{Context, Div, FontWeight, MouseButton, anchored, deferred, div, prelude::*, rgb};
use pina_live_engine::{
    engine::{BLOCK, DECKS},
    fx::{FxCmd, FxKind, FxSettings, Insert, UNITS},
};

use crate::{
    controls::Ctl,
    decks::{beats_text, caption},
    live::{DECK_COLORS, DECK_NAMES, Live},
    ui::{scale::px, theme::*},
};

pub const FX_BAR_H: f32 = 56.0;
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

    fn fx_unit(&self, u: usize, cx: &mut Context<Self>) -> Div {
        let f = self.fx[u];
        let s = f.settings;
        let open = self.fx_menu == Some(u);
        let compact = self.narrow;
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
        // Its length in beats: a chip each where there's room, else stepped through ◀ ▶.
        let chips: &[f64] = &[0.25, 0.5, 0.75, 1.0, 2.0, 4.0, 8.0];
        let beats = match compact {
            true => {
                let step = |by: isize, label: &'static str, cx: &mut Context<Self>| {
                    seg(label, false, 22.0).px(px(6.)).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            let b = live.fx[u].settings.beats;
                            let k = FX_BEATS
                                .iter()
                                .position(|&x| (x - b).abs() < 1e-9)
                                .unwrap_or(5) as isize;
                            live.fx[u].settings.beats =
                                FX_BEATS[(k + by).clamp(0, FX_BEATS.len() as isize - 1) as usize];
                            live.send_fx(u);
                            cx.notify();
                        }),
                    )
                };
                tray()
                    .child(step(-1, "◀", cx))
                    .child(
                        div()
                            .w(px(34.))
                            .text_center()
                            .font_family(MONO)
                            .text_size(px(11.))
                            .text_color(rgb(TEXT))
                            .child(beats_text_fx(s.beats)),
                    )
                    .child(step(1, "▶", cx))
            }
            false => tray().children(chips.iter().map(|&b| {
                let on = (s.beats - b).abs() < 1e-9;
                div()
                    .min_w(px(27.))
                    .h(px(22.))
                    .px(px(3.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(5.))
                    .cursor_pointer()
                    .font_family(MONO)
                    .text_size(px(11.))
                    .bg(rgb(if on { RAISED } else { BG }))
                    .text_color(rgb(if on { TEXT } else { TEXT_3 }))
                    .when(!on, |d| d.hover(|st| st.text_color(rgb(TEXT_2))))
                    .child(beats_text_fx(b))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            live.fx[u].settings.beats = b;
                            live.send_fx(u);
                            cx.notify();
                        }),
                    )
            })),
        };
        let on_any = f.on.iter().any(|x| *x);
        div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .relative()
            .overflow_hidden()
            .when(u > 0, |d| d.border_l_1().border_color(rgb(LINE)))
            .child(crate::probe::probe(&self.probes, format!("fx{u}")))
            .child(caption(if u == 0 { "FX 1" } else { "FX 2" }))
            .child(
                div()
                    .h(px(30.))
                    .px(px(11.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(s.on, |d| d.bg(rgb(ACCENT)).text_color(rgb(0xffffff)))
                    .when(!s.on, |d| {
                        d.bg(rgb(CARD))
                            .border_1()
                            .border_color(rgb(LINE))
                            .text_color(rgb(TEXT_3))
                            .hover(|st| st.text_color(rgb(TEXT_2)))
                    })
                    .child(icon(Icon::Power, 13.).text_color(rgb(if s.on {
                        0xffffff
                    } else {
                        TEXT_3
                    })))
                    .child(if s.on { "On" } else { "Off" })
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
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        dropdown(s.kind.name(), 30.0)
                            .w(px(if compact { 100. } else { 112. }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |live, _, _, cx| {
                                    live.fx_menu = if live.fx_menu == Some(u) {
                                        None
                                    } else {
                                        Some(u)
                                    };
                                    cx.notify();
                                }),
                            ),
                    )
                    .when(open, |d| {
                        d.child(deferred(anchored().snap_to_window().child(menu)).with_priority(1))
                    }),
            )
            .child(beats)
            .child(self.knob_row(Ctl::FxLevel(u), if compact { "" } else { "LEVEL" }, cx))
            .child(self.knob_row(
                Ctl::FxParam(u),
                if compact { "" } else { s.kind.param_name() },
                cx,
            ))
            .child(div().flex_1())
            .when(s.on && !on_any, |d| {
                d.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(AMBER))
                        .child("On no deck"),
                )
            })
            .child(tray().children((0..self.shown()).map(|d| {
                let deck = self.channel_order()[d];
                let on = f.on[deck];
                seg(DECK_NAMES[deck], on, 24.0)
                    .w(px(26.))
                    .when(on, |el| {
                        el.bg(alpha(DECK_COLORS[deck], 0.22))
                            .text_color(rgb(DECK_COLORS[deck]))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            if live.arm(crate::midi::Target::FxAssign(u, deck), cx) {
                                return;
                            }
                            live.toggle_send(u, deck, cx);
                        }),
                    )
            })))
            .child(crate::probe::mark(&self.probes, format!("fx{u}.end")))
    }

    /// A small knob with its caption beside it, for the FX bar.
    fn knob_row(&self, ctl: Ctl, caption: &'static str, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(self.knob(ctl, "", ACCENT, 26.0, cx))
            .child(
                div()
                    .text_size(px(9.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(TEXT_3))
                    .child(caption),
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
