//! The bar along the top: two decks or four, quantize, the tempo sync's following, and the
//! sound (where it's going, or why there's none) with the master's level.

use gpui::{Context, Div, FontWeight, MouseButton, div, prelude::*, rgb};

use crate::{
    live::{DECK_COLORS, DECK_NAMES, Live},
    ui::{scale::px, theme::*},
};

impl Live {
    pub fn top_bar(&self, cx: &mut Context<Self>) -> Div {
        let lead = self.leader().filter(|&l| self.status().decks[l].bpm > 0.0);
        let quantize = self.quantize;
        div()
            .h(px(40.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .bg(rgb(SURFACE))
            .border_b_1()
            .border_color(rgb(LINE))
            .child(
                div()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(TEXT))
                    .child("Pina Live"),
            )
            .child(
                tray()
                    .child(seg("2 decks", !self.four, 26.0).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|live, _, _, cx| {
                            live.four = false;
                            cx.notify();
                        }),
                    ))
                    .child(seg("4 decks", self.four, 26.0).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|live, _, _, cx| {
                            live.four = true;
                            cx.notify();
                        }),
                    )),
            )
            .child(
                tray().child(
                    seg("Quantize", quantize, 26.0)
                        .child(icon(Icon::Magnet, 13.).text_color(rgb(if quantize {
                            ACCENT_TEXT
                        } else {
                            TEXT_3
                        })))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |live, _, _, cx| {
                                live.set_quantize(!quantize);
                                cx.notify();
                            }),
                        ),
                ),
            )
            .child(match lead {
                Some(l) => div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.5))
                    .text_color(rgb(TEXT_3))
                    .child("Sync follows")
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(DECK_COLORS[l]))
                            .child(DECK_NAMES[l]),
                    )
                    .child(
                        div()
                            .font_family(MONO)
                            .text_color(rgb(TEXT))
                            .child(format!("{:.2} BPM", self.status().decks[l].bpm)),
                    ),
                None => div()
                    .text_size(px(11.5))
                    .text_color(rgb(TEXT_4))
                    .child("Nothing playing to sync to"),
            })
            .child(div().flex_1())
            .when_some(self.rec_note.clone(), |d, note| {
                d.child(div().text_size(px(11.)).text_color(rgb(TEXT_3)).child(note))
            })
            .child(self.record_button(cx))
            .child({
                let n = self.midi.as_ref().map_or(0, |m| m.devices.len());
                seg(
                    if n > 0 {
                        format!("MIDI · {n}")
                    } else {
                        "MIDI".into()
                    },
                    self.learning,
                    28.0,
                )
                .when(self.learning, |d| d.text_color(rgb(AMBER)))
                .child(icon(Icon::MidiIn, 14.).text_color(rgb(if self.learning {
                    AMBER
                } else {
                    TEXT_3
                })))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|live, _, _, cx| {
                        let on = !live.learning;
                        live.set_learning(on, cx);
                    }),
                )
            })
            .child(match &self.audio_note {
                Some(note) => div()
                    .text_size(px(11.5))
                    .text_color(rgb(RECORD))
                    .child(note.clone()),
                None => div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.5))
                    .text_color(rgb(TEXT_3))
                    .child(icon(Icon::Speaker, 14.).text_color(rgb(TEXT_3)))
                    .child(self.output_name()),
            })
            .child(
                icon_seg(Icon::Settings, self.show_settings, 28.0).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|live, _, _, cx| {
                        live.show_settings = !live.show_settings;
                        cx.notify();
                    }),
                ),
            )
    }

    /// MIDI learn's bar: what to do, what's picked, what was mapped last.
    pub fn learn_bar(&self, cx: &mut Context<Self>) -> Div {
        let devices = self.midi.as_ref().map_or(0, |m| m.devices.len());
        div()
            .h(px(34.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .bg(rgb(0x221c10))
            .border_b_1()
            .border_color(rgb(LINE))
            .text_size(px(12.))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(AMBER))
                    .child("MIDI learn"),
            )
            .child(
                div()
                    .text_color(rgb(TEXT_2))
                    .child(match (devices, self.armed) {
                        (0, _) => "No controller found. Plug one in, then Look again.".to_string(),
                        (_, None) => "Click a control on screen, then move it on your controller."
                            .to_string(),
                        (_, Some(t)) => match t {
                            crate::midi::Target::Jog(_) => {
                                format!("{}: touch the jog, then turn it.", t.name())
                            }
                            _ => format!("{}: now move it on your controller.", t.name()),
                        },
                    }),
            )
            .when_some(self.learned.clone(), |d, l| {
                d.child(
                    div()
                        .font_family(MONO)
                        .text_size(px(11.))
                        .text_color(rgb(TEXT_3))
                        .child(l),
                )
            })
            .child(div().flex_1())
            .child(button("Look again", false).h(px(24.)).on_mouse_down(
                MouseButton::Left,
                cx.listener(|live, _, _, cx| {
                    live.connect_midi(cx);
                    cx.notify();
                }),
            ))
            .child(button("Done", true).h(px(24.)).on_mouse_down(
                MouseButton::Left,
                cx.listener(|live, _, _, cx| live.set_learning(false, cx)),
            ))
    }

    /// Record: red while it runs, with how long and how big.
    fn record_button(&self, cx: &mut Context<Self>) -> Div {
        let on = self.recording.as_ref();
        let label = match on {
            Some(r) => {
                let frames = r.frames.load(std::sync::atomic::Ordering::Relaxed);
                let secs = frames as f64 / self.rate;
                let mb = frames as f64 * 6.0 / 1e6;
                format!("{}  ·  {:.0} MB", crate::decks::clock(secs, false), mb)
            }
            None => "Record".into(),
        };
        let lost = self.status().dropped > 0;
        div()
            .h(px(28.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(7.))
            .rounded(px(7.))
            .cursor_pointer()
            .border_1()
            .border_color(rgb(if on.is_some() { RECORD } else { LINE }))
            .bg(rgb(if on.is_some() { 0x2a1416 } else { CARD }))
            .hover(|s| s.bg(rgb(RAISED)))
            .text_size(px(11.5))
            .font_family(if on.is_some() { MONO } else { SANS })
            .text_color(rgb(if on.is_some() { TEXT } else { TEXT_2 }))
            .child(icon(Icon::Record, 14.).text_color(rgb(RECORD)))
            .child(label)
            .when(lost, |d| {
                d.child(div().text_color(rgb(AMBER)).child("frames lost"))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|live, _, _, cx| live.toggle_record(cx)),
            )
    }
}
