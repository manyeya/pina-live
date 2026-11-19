//! A deck's hardware, drawn as a player's is and lit from within. CUE and PLAY are round: a
//! bezel, an LED ring round a cap lit from above, a glow round it when it's lit, the ring
//! blinking when it's asking to be pressed (as a CDJ's do: PLAY while paused, CUE away from the
//! cue). SYNC, LEAD and the rest are keys: a cap with an LED bar along its top. A display is a
//! dark window sunk in the panel.

use gpui::{BoxShadow, Div, FontWeight, Hsla, div, point, prelude::*, rgb};

use crate::ui::{scale::px, theme::*};

/// What an LED's doing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Led {
    /// No LED at all: a plain key.
    None,
    Off,
    On(u32),
    /// On and off by turns (`lit` says which this frame).
    Blink(u32, bool),
    /// On, as bright as the second says (1 on the beat, falling to 0 before the next).
    Beat(u32, f32),
}

impl Led {
    /// Its colour this frame, and whether it's lit.
    fn now(self) -> Option<u32> {
        match self {
            Led::On(c) | Led::Blink(c, true) | Led::Beat(c, _) => Some(c),
            _ => None,
        }
    }

    fn color(self) -> Option<u32> {
        match self {
            Led::On(c) | Led::Blink(c, _) | Led::Beat(c, _) => Some(c),
            Led::Off | Led::None => None,
        }
    }

    /// How brightly it's lit: full, or with the beat.
    fn level(self) -> f32 {
        match self {
            Led::Beat(_, l) => l.clamp(0.0, 1.0),
            _ => 1.0,
        }
    }
}

fn hsla(c: u32, a: f32) -> Hsla {
    let mut h = Hsla::from(rgb(c));
    h.a = a;
    h
}

/// A soft glow round something lit in `c`, `a` strong.
fn glow(c: u32, r: f32, a: f32) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: hsla(c, a),
        offset: point(px(0.), px(0.)),
        blur_radius: px(r),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// A cap's fill: lit from above, darker pressed.
fn cap_fill(pressed: bool) -> gpui::Background {
    let (top, bottom) = match pressed {
        true => (0x1b1b20, 0x24242a),
        false => (0x383841, 0x1f1f25),
    };
    gpui::linear_gradient(
        180.0,
        gpui::linear_color_stop(rgb(top), 0.0),
        gpui::linear_color_stop(rgb(bottom), 1.0),
    )
}

/// A round button `size` across: its bezel, its LED ring, its cap with `content` on it.
pub fn round(size: f32, led: Led, pressed: bool, content: impl IntoElement) -> Div {
    let lit = led.now();
    // Unlit, a ring that can light still shows its colour faintly, as a dark LED does.
    let level = led.level();
    let ring = match (lit, led.color()) {
        (Some(c), _) => hsla(c, 0.5 + 0.5 * level),
        (None, Some(c)) => hsla(c, 0.22),
        (None, None) => Hsla::from(rgb(0x2a2a31)),
    };
    let ring_size = size - 6.0;
    let cap = size - 14.0;
    div()
        .size(px(size))
        .flex_none()
        .rounded(px(size / 2.0))
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(0x0a0a0d))
        .border_1()
        .border_color(rgb(0x2c2c34))
        .cursor_pointer()
        .when_some(lit, |d, c| d.shadow(glow(c, 16.0, 0.18 + 0.32 * level)))
        .child(
            div()
                .size(px(ring_size))
                .rounded(px(ring_size / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .border_2()
                .border_color(ring)
                .child(
                    div()
                        .size(px(cap))
                        .rounded(px(cap / 2.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(cap_fill(pressed))
                        .border_1()
                        .border_color(rgb(if pressed { 0x2c2c34 } else { 0x4a4a55 }))
                        .child(content),
                ),
        )
}

/// A key `h` tall, `pad` either side of its label: its cap with `label` on it, an LED bar along
/// its top lit by `led`.
pub fn key(label: impl IntoElement, led: Led, pressed: bool, h: f32, pad: f32) -> Div {
    let lit = led.now();
    let bar = match (lit, led.color()) {
        (Some(c), _) => Hsla::from(rgb(c)),
        (None, Some(c)) => hsla(c, 0.18),
        (None, None) => Hsla::from(rgb(0x2a2a31)),
    };
    div()
        .relative()
        .h(px(h))
        .flex_none()
        .px(px(pad))
        .pt(px(3.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(5.))
        .rounded(px(6.))
        .bg(cap_fill(pressed))
        .border_1()
        .border_color(rgb(if pressed { 0x2c2c34 } else { 0x41414b }))
        .cursor_pointer()
        .text_size(px(10.))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(match lit {
            Some(_) => TEXT,
            None => TEXT_3,
        }))
        .hover(|s| s.border_color(rgb(0x5a5a66)))
        .child(
            div()
                .absolute()
                .top(px(3.))
                .left(px(pad - 2.0))
                .right(px(pad - 2.0))
                .h(px(2.))
                .rounded(px(1.))
                .bg(bar)
                .when(led == Led::None, |d| d.invisible())
                .when_some(lit, |d, c| d.shadow(glow(c, 6.0, 0.42))),
        )
        .child(label)
}

/// A dark window sunk in the panel, for a readout.
pub fn display(h: f32) -> Div {
    div()
        .h(px(h))
        .flex_none()
        .flex()
        .items_center()
        .rounded(px(5.))
        .bg(rgb(0x09090c))
        .border_1()
        .border_color(rgb(0x26262d))
        .shadow(vec![BoxShadow {
            color: hsla(0x000000, 0.6),
            offset: point(px(0.), px(1.)),
            blur_radius: px(3.),
            spread_radius: px(0.),
            inset: true,
        }])
}
