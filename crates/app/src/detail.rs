//! The fine print on every control: what a control reads while it's turned (as a mixer prints
//! it), where it snaps as it's dragged near (a centre detent, the pitch fader's numbered ticks;
//! Shift passes them by), a wheel notch's step, and a knob drawn with its scale of ticks.

use gpui::{Bounds, Pixels, Point, canvas, point, prelude::*, rgb};

use crate::{
    controls::Ctl,
    live::Live,
    ui::{scale::px, theme::*},
};

/// The channel fader's curve, as the strips read it.
pub const FADER_CURVE: f32 = 0.6;

/// What `ctl` reads at `v`.
pub fn value_text(live: &Live, ctl: Ctl, v: f32) -> String {
    let gain = |g: f32| match g <= 1e-5 {
        true => "−∞".to_string(),
        false => db_text(20.0 * g.log10()),
    };
    match ctl {
        Ctl::Trim(_) | Ctl::Master | Ctl::Phones => format!("{} dB", db_text(v)),
        Ctl::Eq(..) if v < 0.005 => "KILL".into(),
        Ctl::Eq(..) => format!("{} dB", gain(pina_live_engine::dsp::eq_gain(v))),
        Ctl::Filter(_) if v.abs() < 0.005 => "OFF".into(),
        Ctl::Filter(_) => format!(
            "{} {:.0}",
            if v < 0.0 { "LP" } else { "HP" },
            v.abs() * 100.0
        ),
        Ctl::Fader(_) => format!(
            "{} dB",
            gain(pina_live_engine::mixer::fader_gain(v, FADER_CURVE))
        ),
        Ctl::Xf if v.abs() < 0.005 => "Centre".into(),
        Ctl::Xf => format!("{} {:.0}", if v < 0.0 { "A" } else { "B" }, v.abs() * 100.0),
        Ctl::CueMix => format!("{:.0}% mix", v * 100.0),
        Ctl::Tempo(d) => format!("{:+.2}%", v as f64 * live.decks[d].range * 100.0),
        Ctl::FxLevel(_) | Ctl::FxParam(_) => format!("{:.0}%", v * 100.0),
    }
}

/// Where `ctl` is pulled to when it's dragged to `v`: its home if it's turned from the middle
/// (a mixer's centre detent), and the pitch fader's and crossfader's numbered ticks.
pub fn snap(ctl: Ctl, v: f32) -> f32 {
    let (lo, hi, home) = ctl.range();
    let near = |at: f32, zone: f32| (v - at).abs() < zone * (hi - lo);
    if ctl.bipolar() && near(home, 0.02) {
        return home;
    }
    if matches!(ctl, Ctl::Tempo(_) | Ctl::Xf) {
        for at in [-0.5, 0.5] {
            if near(at, 0.01) {
                return at;
            }
        }
    }
    v
}

/// What a wheel notch turns `ctl` by (a tenth of it with Shift).
pub fn step(ctl: Ctl) -> f32 {
    let (lo, hi, _) = ctl.range();
    (hi - lo)
        * match ctl {
            // ±8: 0.04% a notch.
            Ctl::Tempo(_) => 1.0 / 400.0,
            _ => 1.0 / 50.0,
        }
}

/// A knob `size` across at `value` (0 to 1, or -1 to 1 turned from the middle) lit in `color`:
/// its scale of eleven ticks round the outside (the ends and the middle longer, the middle
/// brighter for one turned from it), its track and how far it's turned, and its cap with a
/// pointer.
pub fn knob(value: f32, size: f32, color: u32, bipolar: bool) -> impl IntoElement {
    let thick = if size >= 30.0 { 2.5 } else { 2.0 };
    canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            let c = b.center();
            let outer = f32::from(b.size.width.min(b.size.height)) / 2.0;
            let at = |deg: f32, rr: f32| -> Point<Pixels> {
                let a = deg.to_radians();
                point(c.x + px(rr * a.cos()), c.y + px(rr * a.sin()))
            };
            for k in 0..=10 {
                let deg = 135.0 + k as f32 * 27.0;
                let major = k == 0 || k == 5 || k == 10;
                let mid = bipolar && k == 5;
                let mut t = gpui::PathBuilder::stroke(px(if mid { 1.25 } else { 1.0 }));
                t.move_to(at(deg, outer - 0.5));
                t.line_to(at(deg, outer - if major { 3.0 } else { 2.0 }));
                if let Ok(p) = t.build() {
                    window.paint_path(p, rgb(if mid { TEXT_3 } else { LINE_STRONG }));
                }
            }
            let r = outer - 3.5 - thick / 2.0;
            let arc = |from: f32, to: f32| {
                let mut p = gpui::PathBuilder::stroke(px(thick));
                p.move_to(at(from, r));
                let mid = (from + to) / 2.0;
                p.arc_to(point(px(r), px(r)), px(0.), false, to > from, at(mid, r));
                p.arc_to(point(px(r), px(r)), px(0.), false, to > from, at(to, r));
                p.build().ok()
            };
            if let Some(track) = arc(135.0, 405.0) {
                window.paint_path(track, rgb(WELL));
            }
            let (from, to) = match bipolar {
                true => (270.0, 270.0 + value.clamp(-1.0, 1.0) * 135.0),
                false => (135.0, 135.0 + value.clamp(0.0, 1.0) * 270.0),
            };
            if (to - from).abs() > 0.5
                && let Some(lit) = arc(from.min(to), from.max(to))
            {
                window.paint_path(lit, rgb(color));
            }
            // The cap: its shadow under it, lit from above, a hairline rim, and (big enough to
            // see) ridged round its edge to grip, as a mixer's rubber caps are.
            let body = r - thick / 2.0 - 1.5;
            window.paint_quad(
                gpui::fill(
                    Bounds::centered_at(
                        point(c.x, c.y + px(1.5)),
                        gpui::size(px(body * 2.0 + 1.0), px(body * 2.0 + 1.0)),
                    ),
                    gpui::hsla(0.0, 0.0, 0.0, 0.45),
                )
                .corner_radii(px(body + 0.5)),
            );
            window.paint_quad(
                gpui::fill(
                    Bounds::centered_at(c, gpui::size(px(body * 2.0), px(body * 2.0))),
                    gpui::linear_gradient(
                        180.0,
                        gpui::linear_color_stop(rgb(0x3a3a43), 0.0),
                        gpui::linear_color_stop(rgb(0x24242b), 1.0),
                    ),
                )
                .corner_radii(px(body))
                .border_widths(px(1.))
                .border_color(rgb(0x4a4a55)),
            );
            if size >= 30.0 {
                for k in 0..36 {
                    let deg = k as f32 * 10.0;
                    let mut g = gpui::PathBuilder::stroke(px(1.));
                    g.move_to(at(deg, body - 2.5));
                    g.line_to(at(deg, body - 0.75));
                    if let Ok(p) = g.build() {
                        window.paint_path(p, gpui::hsla(0.0, 0.0, 1.0, 0.07));
                    }
                }
            }
            let mut pointer = gpui::PathBuilder::stroke(px(if size >= 30.0 { 2.0 } else { 1.75 }));
            pointer.move_to(at(to, body * 0.25));
            pointer.line_to(at(to, body * 0.92));
            if let Ok(p) = pointer.build() {
                window.paint_path(p, rgb(TEXT));
            }
        },
    )
    .size(px(size))
    .flex_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_snap_home_and_to_their_ticks_only_when_near() {
        assert_eq!(snap(Ctl::Tempo(0), 0.03), 0.0);
        assert_eq!(snap(Ctl::Tempo(0), 0.2), 0.2);
        assert_eq!(snap(Ctl::Tempo(0), 0.51), 0.5);
        assert_eq!(snap(Ctl::Eq(0, 1), 0.51), 0.5);
        assert_eq!(snap(Ctl::Eq(0, 1), 0.6), 0.6);
        assert_eq!(snap(Ctl::Trim(0), 0.3), 0.0);
        // A channel fader has no detent.
        assert_eq!(snap(Ctl::Fader(0), 0.99), 0.99);
    }
}
