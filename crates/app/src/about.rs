//! About Pina Live: who made it, and the open-source software it's built on, credited. Pina Live
//! is designed and written by Manyeya Khutso Siema, who made Pina Studio too. Every crate in it
//! is listed (`credits.rs`, made from Cargo.lock by scripts/credits.py), and the projects it
//! leans on most are named first, with what each one does here.

use std::sync::Arc;

use gpui::{Context, Div, FontWeight, Image, ImageFormat, MouseButton, div, img, prelude::*, rgb};

use crate::{
    credits::CRATES,
    live::Live,
    ui::{scale::px, theme::*},
};

/// Pina Live's author.
pub const AUTHOR: &str = "Manyeya Khutso Siema";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COPYRIGHT: &str = "© 2026 Manyeya Khutso Siema";
const AUTHOR_URL: &str = "https://github.com/manyeya";
/// The author's own site.
const HOME: &str = "https://www.manyeya.dev";
const SITE: &str = "https://manyeya.github.io/pina-studio/live/";
const SOURCE: &str = "https://github.com/manyeya/pina-live";
const LICENSE: &str = "https://github.com/manyeya/pina-live/blob/main/LICENSE";
const NOTICES: &str = "https://github.com/manyeya/pina-live/blob/main/THIRD-PARTY-NOTICES.md";

/// What Pina Live is built on, most of all: its name, what it does here, its licence, its home.
pub const BUILT_ON: &[(&str, &str, &str, &str)] = &[
    (
        "Pina Studio's engine",
        "Decoding, the song study (beat, bar, key, phrases), the effects, the limiter and stem separation. Also by Manyeya Khutso Siema.",
        "MIT",
        "https://github.com/manyeya/pina-studio",
    ),
    (
        "GPUI",
        "The whole interface, drawn on the GPU. By Zed Industries.",
        "Apache-2.0",
        "https://github.com/zed-industries/zed",
    ),
    (
        "Signalsmith Stretch",
        "Keylock: the tempo moved, the key kept. By Geraint Luff, Signalsmith Audio.",
        "MIT",
        "https://signalsmith-audio.co.uk/code/stretch/",
    ),
    (
        "Symphonia",
        "Reading MP3, AAC, FLAC, WAV, AIFF and Ogg. By Philip Deljanov and contributors.",
        "MPL-2.0",
        "https://github.com/pdeljanov/Symphonia",
    ),
    (
        "ONNX Runtime and ort",
        "Running the stem separation models. By Microsoft, and pyke.",
        "MIT",
        "https://onnxruntime.ai",
    ),
    (
        "cpal",
        "Sound out, through Core Audio, WASAPI and Web Audio. By RustAudio.",
        "Apache-2.0",
        "https://github.com/RustAudio/cpal",
    ),
    (
        "midir",
        "MIDI controllers. By Patrick Reisert and contributors.",
        "MIT",
        "https://github.com/Boddlnagg/midir",
    ),
    (
        "rtrb",
        "The lock-free rings between the audio thread and everything else. By Matthias Geier.",
        "MIT OR Apache-2.0",
        "https://github.com/mgeier/rtrb",
    ),
    (
        "wgpu and wasm-bindgen",
        "Pina Live in a browser: WebGPU, and Rust talking to the page.",
        "MIT OR Apache-2.0",
        "https://wgpu.rs",
    ),
    (
        "Geist and Geist Mono",
        "The type. By Vercel.",
        "OFL-1.1",
        "https://vercel.com/font",
    ),
    ("Lucide", "The icons' designs.", "ISC", "https://lucide.dev"),
];

impl Live {
    pub fn about_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let caption = |t: &'static str| {
            div()
                .pt(px(18.))
                .pb(px(8.))
                .text_size(px(10.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT_4))
                .child(t)
        };
        let link = |label: &'static str, url: &'static str, cx: &mut Context<Self>| {
            button(label, false).h(px(26.)).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _, _, cx| cx.open_url(url)),
            )
        };
        // A row that opens its project's home.
        let row = |url: &'static str, cx: &mut Context<Self>| {
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(8.))
                .border_t_1()
                .border_color(rgb(LINE_SOFT))
                .cursor_pointer()
                .hover(|d| d.bg(rgb(CARD)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, _, _, cx| cx.open_url(url)),
                )
        };

        let built_on: Vec<Div> = BUILT_ON
            .iter()
            .map(|&(name, what, license, url)| {
                row(url, cx)
                    .py(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().font_weight(FontWeight::MEDIUM).child(name))
                            .child(div().text_size(px(11.)).text_color(rgb(TEXT_3)).child(what)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font_family(MONO)
                            .text_size(px(10.5))
                            .text_color(rgb(TEXT_3))
                            .child(license),
                    )
            })
            .collect();

        // Every crate: how many under each licence, then (asked for) each one.
        let mut tally: Vec<(&str, usize)> = Vec::new();
        for c in CRATES {
            match tally.iter_mut().find(|(l, _)| *l == c.license) {
                Some((_, n)) => *n += 1,
                None => tally.push((c.license, 1)),
            }
        }
        tally.sort_by(|a, b| b.1.cmp(&a.1));
        let tally = tally
            .iter()
            .map(|(l, n)| format!("{l} {n}"))
            .collect::<Vec<_>>()
            .join("  ·  ");
        let every: Vec<Div> = match self.about_all {
            false => Vec::new(),
            true => CRATES
                .iter()
                .map(|c| {
                    row(c.url, cx)
                        .h(px(26.))
                        .text_size(px(11.))
                        .child(
                            div()
                                .w(px(190.))
                                .flex_none()
                                .text_color(rgb(TEXT_2))
                                .truncate()
                                .child(c.name),
                        )
                        .child(
                            div()
                                .w(px(110.))
                                .flex_none()
                                .font_family(MONO)
                                .text_size(px(10.5))
                                .text_color(rgb(TEXT_4))
                                .truncate()
                                .child(c.version),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_color(rgb(TEXT_3))
                                .truncate()
                                .child(c.by),
                        )
                        .child(
                            div()
                                .flex_none()
                                .font_family(MONO)
                                .text_size(px(10.5))
                                .text_color(rgb(TEXT_3))
                                .child(c.license),
                        )
                })
                .collect(),
        };

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(alpha(0x000000, 0.55))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|live, _, _, cx| {
                    live.show_about = false;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("about")
                    .w(px(640.))
                    .max_w(gpui::relative(0.94))
                    .max_h(gpui::relative(0.9))
                    .overflow_y_scroll()
                    .p(px(22.))
                    .rounded(px(12.))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(LINE_STRONG))
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    // Pina Live, its version, its maker.
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .child(
                                img(Arc::new(Image::from_bytes(
                                    ImageFormat::Svg,
                                    include_bytes!("../icon/live.svg").to_vec(),
                                )))
                                .size(px(72.))
                                .flex_none(),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child("Pina Live"))
                                    .child(
                                        div()
                                            .text_color(rgb(TEXT_3))
                                            .child(format!("Version {VERSION}  ·  Pina Studio's sister app")),
                                    ),
                            )
                            .child(icon_seg(Icon::Close, false, 26.0).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|live, _, _, cx| {
                                    live.show_about = false;
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(caption("MADE BY"))
                    .child(
                        div()
                            .p(px(14.))
                            .rounded(px(10.))
                            .bg(rgb(CARD))
                            .border_1()
                            .border_color(rgb(LINE))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child(AUTHOR))
                            .child(div().text_color(rgb(TEXT_2)).child(
                                "Pina Live is designed and written by Manyeya Khutso Siema: the decks, the mixer, \
                                 the effects, the library, the browser build and the installer. Manyeya Khutso Siema \
                                 made Pina Studio too, and Pina Live plays through its engine.",
                            ))
                            .child(
                                div()
                                    .pt(px(6.))
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    .child(link("manyeya.dev", HOME, cx))
                                    .child(link("github.com/manyeya", AUTHOR_URL, cx))
                                    .child(link("Pina Live's page", SITE, cx))
                                    .child(link("The source", SOURCE, cx)),
                            )
                            .child(div().pt(px(4.)).text_size(px(11.)).text_color(rgb(TEXT_3)).child(COPYRIGHT)),
                    )
                    .child(caption("LICENCE"))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(div().text_color(rgb(TEXT_2)).child(
                                "Pina Live is free software under the MIT License: use it, change it and share it, \
                                 keeping its copyright notice and licence with it. Every open-source component in it \
                                 is listed below and in its third-party notices, each with its licence.",
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    .child(link("The MIT License", LICENSE, cx))
                                    .child(link("Third-party notices", NOTICES, cx)),
                            ),
                    )
                    .child(caption("BUILT ON"))
                    .child(div().flex().flex_col().children(built_on))
                    .child(caption("EVERY OPEN-SOURCE COMPONENT"))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(div().text_color(rgb(TEXT_2)).child(format!(
                                "Pina Live is built from {} open-source crates. Thank you to everyone who wrote them.",
                                CRATES.len()
                            )))
                            .child(
                                div()
                                    .font_family(MONO)
                                    .text_size(px(10.5))
                                    .text_color(rgb(TEXT_3))
                                    .child(tally),
                            )
                            .child(
                                div().pt(px(4.)).flex().child(
                                    button(
                                        match self.about_all {
                                            true => "Hide the list".to_string(),
                                            false => format!("Show all {}", CRATES.len()),
                                        },
                                        self.about_all,
                                    )
                                    .h(px(26.))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|live, _, _, cx| {
                                            live.about_all = !live.about_all;
                                            cx.notify();
                                        }),
                                    ),
                                ),
                            ),
                    )
                    .child(div().pt(px(8.)).flex().flex_col().children(every)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The credits are Cargo.lock's: every crate listed is there at that version (run
    /// scripts/credits.py again after a dependency changes).
    #[test]
    fn credits_match_cargo_lock() {
        let lock = include_str!("../../../Cargo.lock");
        for c in CRATES {
            let entry = format!("name = \"{}\"\nversion = \"{}\"", c.name, c.version);
            assert!(
                lock.contains(&entry),
                "{} {} isn't in Cargo.lock",
                c.name,
                c.version
            );
        }
        for name in [
            "gpui-pre",
            "symphonia",
            "signalsmith-stretch",
            "cpal",
            "ort",
        ] {
            assert!(
                CRATES.iter().any(|c| c.name == name),
                "{name} isn't credited"
            );
        }
    }
}
