//! The FX bar: two units side by side, each its effect (a dropdown), its length in beats, its
//! level and second knob, on or off, and which decks it's on.

use gpui::{Context, Div, FontWeight, MouseButton, anchored, deferred, div, prelude::*, rgb};
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

pub const FX_BAR_H: f32 = 46.0;
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
            .border_color(rgb(LINE))
            .bg(rgb(SURFACE))
            .children((0..UNITS).map(|u| self.fx_unit(u, cx)))
    }

    fn fx_unit(&self, u: usize, cx: &mut Context<Self>) -> Div {
        let f = self.fx[u];
        let s = f.settings;
        let open = self.fx_menu == Some(u);
        let step = |by: isize, label: &'static str, cx: &mut Context<Self>| {
            seg(label, false, 24.0).px(px(6.)).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    let b = live.fx[u].settings.beats;
                    let k = FX_BEATS
                        .iter()
                        .position(|&x| (x - b).abs() < 1e-9)
                        .unwrap_or(5) as isize;
                    let next = (k + by).clamp(0, FX_BEATS.len() as isize - 1) as usize;
                    live.fx[u].settings.beats = FX_BEATS[next];
                    live.send_fx(u);
                    cx.notify();
                }),
            )
        };
        let menu = div()
            .w(px(180.))
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
                    .h(px(24.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(5.))
                    .cursor_pointer()
                    .text_color(rgb(if picked { TEXT } else { TEXT_2 }))
                    .hover(|st| st.bg(rgb(RAISED)))
                    .child(kind.name())
                    .when(kind.trails(), |d| {
                        d.child(
                            div()
                                .text_size(px(9.5))
                                .text_color(rgb(TEXT_4))
                                .child("trails"),
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
        div()
            .flex_1()
            .h_full()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
            .when(u > 0, |d| d.border_l_1().border_color(rgb(LINE)))
            .child(
                div()
                    .text_size(px(10.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(TEXT_4))
                    .child(format!("FX {}", u + 1)),
            )
            .child(
                seg("On", s.on, 26.0)
                    .when(s.on, |d| d.bg(rgb(ACCENT)).text_color(rgb(0xffffff)))
                    .child(icon(Icon::Power, 13.).text_color(rgb(if s.on {
                        0xffffff
                    } else {
                        TEXT_3
                    })))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            if live.arm(crate::midi::Target::FxOn(u), cx) {
                                return;
                            }
                            live.fx[u].settings.on = !live.fx[u].settings.on;
                            live.send_fx(u);
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        dropdown(s.kind.name(), 26.0)
                            .w(px(if self.narrow { 104. } else { 124. }))
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
            .child(
                tray()
                    .child(step(-1, "◀", cx))
                    .child(
                        div()
                            .w(px(40.))
                            .text_center()
                            .font_family(MONO)
                            .text_size(px(11.5))
                            .text_color(rgb(TEXT))
                            .child(beats_text_fx(s.beats)),
                    )
                    .child(step(1, "▶", cx)),
            )
            .child(self.knob_row(Ctl::FxLevel(u), if self.narrow { "" } else { "LEVEL" }, cx))
            .child(self.knob_row(
                Ctl::FxParam(u),
                if self.narrow { "" } else { s.kind.param_name() },
                cx,
            ))
            .child(div().flex_1())
            .child(tray().children((0..self.shown()).map(|d| {
                let deck = self.channel_order()[d];
                let on = f.on[deck];
                seg(DECK_NAMES[deck], on, 24.0)
                    .w(px(26.))
                    .when(on, |el| {
                        el.bg(alpha(DECK_COLORS[deck], 0.25))
                            .text_color(rgb(DECK_COLORS[deck]))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            if live.arm(crate::midi::Target::FxAssign(u, deck), cx) {
                                return;
                            }
                            let on = !live.fx[u].on[deck];
                            live.fx[u].on[deck] = on;
                            live.remote.fx(FxCmd::Assign {
                                unit: u,
                                ch: deck,
                                on,
                            });
                            cx.notify();
                        }),
                    )
            })))
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
