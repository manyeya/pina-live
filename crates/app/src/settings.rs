//! Settings, kept between runs: where the sound goes (the device, its buffer, the master's pair
//! and the headphones': a pair of their own, or split on one jack), the crossfader's curve,
//! auto gain, keylock as tracks load, the load lock, and the library's folders.

use gpui::{Context, Div, FontWeight, MouseButton, div, prelude::*, rgb};
use pina_live_engine::{
    library::data_dir,
    mixer::MixCmd,
    output::{AudioSettings, Phones, devices},
};
use serde::{Deserialize, Serialize};

use crate::{
    live::Live,
    ui::{scale::px, theme::*},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub audio: AudioSettings,
    /// 0 smooth, 1 a sharp cut.
    pub xf_curve: f32,
    pub auto_gain: bool,
    pub keylock: bool,
    /// A playing deck won't take another track.
    pub load_lock: bool,
    /// A track loaded, its channel starts clean: trim, EQ and filter back home, off the
    /// effects that are on.
    pub reset_on_load: bool,
    /// The panels' sizes and the library's view.
    pub layout: crate::layout::Layout,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            audio: AudioSettings::default(),
            xf_curve: 0.0,
            auto_gain: true,
            keylock: true,
            load_lock: true,
            reset_on_load: true,
            layout: crate::layout::Layout::default(),
        }
    }
}

impl Settings {
    fn file() -> std::path::PathBuf {
        data_dir().join("settings.json")
    }

    pub fn load() -> Settings {
        std::fs::read(Settings::file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let f = Settings::file();
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(f, b);
        }
    }
}

const BUFFERS: [u32; 5] = [128, 256, 512, 1024, 2048];

impl Live {
    /// The sound moved to the settings' output; failing that, why, in the panel.
    fn apply_audio(&mut self) {
        let audio = self.settings.audio.clone();
        self.audio_note = match self.output.as_mut().map(|o| o.change(&audio)) {
            Some(Err(e)) => Some(format!("No sound: {e}")),
            _ => None,
        };
        self.settings.save();
    }

    pub fn settings_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.settings;
        let found = devices();
        let now = self.output.as_ref().map(|o| o.device.clone());
        let channels = self.output.as_ref().map_or(2, |o| o.channels);
        let section = |title: &'static str| {
            div()
                .pt(px(14.))
                .pb(px(6.))
                .text_size(px(10.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT_4))
                .child(title)
        };
        let line = |label: &'static str, control: Div| {
            div()
                .h(px(36.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .border_t_1()
                .border_color(rgb(LINE_SOFT))
                .child(div().text_color(rgb(TEXT_2)).child(label))
                .child(control)
        };
        // Pairs of outputs: "1–2", "3–4"…
        let pairs: Vec<u16> = (0..(channels / 2).max(1)).map(|p| p * 2).collect();
        let pair_name = |p: u16| format!("{}–{}", p + 1, p + 2);

        let mut devs = div().flex().flex_col();
        for (name, ch) in found {
            let on = now.as_deref() == Some(name.as_str());
            let pick = name.clone();
            devs = devs.child(
                div()
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .bg(rgb(if on { RAISED } else { CARD }))
                    .hover(|st| st.bg(rgb(RAISED)))
                    .child(
                        icon(if on { Icon::Check } else { Icon::Speaker }, 14.)
                            .text_color(rgb(if on { ACCENT_TEXT } else { TEXT_3 })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_color(rgb(if on { TEXT } else { TEXT_2 }))
                            .child(name),
                    )
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(10.5))
                            .text_color(rgb(TEXT_4))
                            .child(format!("{ch} out")),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            live.settings.audio.device = Some(pick.clone());
                            live.settings.audio.master = 0;
                            live.settings.audio.phones = Phones::None;
                            live.apply_audio();
                            cx.notify();
                        }),
                    ),
            );
        }

        let buffer = s.audio.buffer;
        let mut buffers = tray();
        for b in BUFFERS {
            buffers = buffers.child(seg(format!("{b}"), buffer == Some(b), 24.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    live.settings.audio.buffer = Some(b);
                    live.apply_audio();
                    cx.notify();
                }),
            ));
        }
        let mut master = tray();
        for &p in &pairs {
            master = master.child(seg(pair_name(p), s.audio.master == p, 24.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    live.settings.audio.master = p;
                    live.apply_audio();
                    cx.notify();
                }),
            ));
        }
        let mut phones = tray().child(
            seg("None", s.audio.phones == Phones::None, 24.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(|live, _, _, cx| {
                    live.settings.audio.phones = Phones::None;
                    live.apply_audio();
                    cx.notify();
                }),
            ),
        );
        phones = phones.child(
            seg("Split", s.audio.phones == Phones::Split, 24.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(|live, _, _, cx| {
                    live.settings.audio.phones = Phones::Split;
                    live.apply_audio();
                    cx.notify();
                }),
            ),
        );
        for &p in pairs.iter().filter(|&&p| p != s.audio.master) {
            phones = phones.child(
                seg(pair_name(p), s.audio.phones == Phones::Pair(p), 24.0).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |live, _, _, cx| {
                        live.settings.audio.phones = Phones::Pair(p);
                        live.apply_audio();
                        cx.notify();
                    }),
                ),
            );
        }
        let curve = s.xf_curve;
        let xf = tray()
            .child(seg("Smooth", curve < 0.5, 24.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(|live, _, _, cx| live.set_xf_curve(0.0, cx)),
            ))
            .child(seg("Scratch", curve >= 0.5, 24.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(|live, _, _, cx| live.set_xf_curve(1.0, cx)),
            ));
        let toggle = |on: bool, f: fn(&mut Settings), cx: &mut Context<Self>| {
            div().cursor_pointer().child(switch(on)).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    f(&mut live.settings);
                    live.settings_changed();
                    cx.notify();
                }),
            )
        };
        let mut folders = div().flex().flex_col();
        for (k, f) in self.lib.folders.iter().enumerate() {
            folders = folders.child(
                div()
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .border_t_1()
                    .border_color(rgb(LINE_SOFT))
                    .child(icon(Icon::Folder, 14.).text_color(rgb(TEXT_3)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_color(rgb(TEXT_2))
                            .child(f.display().to_string()),
                    )
                    .child(button("Remove", false).h(px(22.)).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| live.remove_folder(k, cx)),
                    )),
            );
        }

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(alpha(0x000000, 0.55))
            .on_mouse_down(MouseButton::Left, cx.listener(|live, _, _, cx| {
                live.show_settings = false;
                cx.notify();
            }))
            .child(
                div()
                    .id("settings")
                    .w(px(560.))
                    .max_h(px(720.))
                    .overflow_y_scroll()
                    .p(px(18.))
                    .rounded(px(12.))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(LINE_STRONG))
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Settings"))
                            .child(icon_seg(Icon::Close, false, 26.0).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|live, _, _, cx| {
                                    live.show_settings = false;
                                    cx.notify();
                                }),
                            )),
                    )
                    .when_some(self.audio_note.clone(), |d, note| {
                        d.child(div().pt(px(10.)).text_color(rgb(RECORD)).child(note))
                    })
                    .child(section("OUTPUT"))
                    .child(devs)
                    .child(line("Buffer (frames)", buffers))
                    .child(line("Master on outputs", master))
                    .child(line("Headphones", phones))
                    .child(
                        div()
                            .pb(px(2.))
                            .text_size(px(11.))
                            .text_color(rgb(TEXT_4))
                            .child("Split plays the master in your left ear and what's cued in your right, both in mono, from one jack."),
                    )
                    .child(section("MIXER"))
                    .child(line("Crossfader curve", xf))
                    .child(line("Auto gain (every track to −10 LUFS)", toggle(s.auto_gain, |s| s.auto_gain = !s.auto_gain, cx)))
                    .child(section("DECKS"))
                    .child(line("Keylock on as tracks load", toggle(s.keylock, |s| s.keylock = !s.keylock, cx)))
                    .child(line("Don't load onto a playing deck", toggle(s.load_lock, |s| s.load_lock = !s.load_lock, cx)))
                    .child(line("A new track's channel starts clean (EQ, filter, effects)", toggle(s.reset_on_load, |s| s.reset_on_load = !s.reset_on_load, cx)))
                    .child(section("CONTROLLERS"))
                    .child(self.midi_settings(cx))
                    .child(section("LIBRARY FOLDERS"))
                    .child(folders)
                    .child(
                        div().pt(px(10.)).flex().gap(px(8.))
                            .child(button("Add a folder…", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|_, _, window, cx| window.dispatch_action(Box::new(crate::live::AddFolder), cx)),
                            ))
                            .child(button("Look for new music", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|live, _, _, cx| live.rescan(cx)),
                            )),
                    ),
            )
    }

    /// The controllers found and every mapping (flip it, or let it go).
    fn midi_settings(&self, cx: &mut Context<Self>) -> Div {
        let (devices, maps) = self.midi.as_ref().map_or((Vec::new(), Vec::new()), |m| {
            (m.devices.clone(), m.mappings())
        });
        let mut col = div().flex().flex_col();
        col = col.child(
            div()
                .pb(px(6.))
                .text_color(rgb(TEXT_3))
                .child(match devices.is_empty() {
                    true => "No MIDI controller connected.".to_string(),
                    false => devices.join(", "),
                }),
        );
        for (k, m) in maps.iter().enumerate() {
            col = col.child(
                div()
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .border_t_1()
                    .border_color(rgb(LINE_SOFT))
                    .child(
                        div()
                            .flex_1()
                            .text_color(rgb(TEXT_2))
                            .child(m.target.name()),
                    )
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(10.5))
                            .text_color(rgb(TEXT_4))
                            .child(m.message()),
                    )
                    .child(seg("Flip", m.invert, 22.0).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            if let Some(midi) = &live.midi {
                                midi.flip(k);
                            }
                            cx.notify();
                        }),
                    ))
                    .child(icon_seg(Icon::Close, false, 22.0).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            if let Some(midi) = &live.midi {
                                midi.forget(k);
                            }
                            cx.notify();
                        }),
                    )),
            );
        }
        col.child(
            div()
                .pt(px(8.))
                .flex()
                .gap(px(8.))
                .child(button("Learn…", false).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|live, _, _, cx| {
                        live.show_settings = false;
                        live.set_learning(true, cx);
                    }),
                )),
        )
    }

    fn set_xf_curve(&mut self, c: f32, cx: &mut Context<Self>) {
        self.settings.xf_curve = c;
        self.mixer(MixCmd::CrossfaderCurve(c));
        self.settings.save();
        cx.notify();
    }

    /// The settings that act at once, sent; kept.
    pub fn settings_changed(&mut self) {
        self.mix.auto_gain = self.settings.auto_gain;
        for d in 0..pina_live_engine::engine::DECKS {
            self.send_gain(d);
        }
        self.settings.save();
    }
}
