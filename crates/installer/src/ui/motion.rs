//! How the installer moves: easing, and the marks it draws as it goes.

use super::*;

/// Eased: quick, then settling.
pub(crate) fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// Eased past where it's going, and back: a landing.
pub(crate) fn ease_back(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let (c1, c3) = (1.70158, 2.70158);
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

/// How far in something's entrance is, `since` seconds on, starting `delay` in, over `dur`.
pub(crate) fn appear(since: f32, delay: f32, dur: f32) -> f32 {
    ease_out((since - delay) / dur)
}

/// Rising in: faded and a little lower until it's there.
pub(crate) fn rise(el: Div, p: f32) -> Div {
    el.opacity(p).relative().top(px((1.0 - p) * 14.0))
}

/// A number from 0 to 1, the same every time for `i`: where a note starts, how fast it goes.
pub(crate) fn hashed(i: usize, k: f32) -> f32 {
    let v = ((i as f32 + 1.0) * 12.9898 + k * 78.233).sin() * 43_758.547;
    v - v.floor()
}

/// What the decks are doing: how far they've faded in, how lively (faster while it works), how
/// calm (settling when it's over), and the seconds since Pina Live was ready (sparks fly then).
#[derive(Clone, Copy)]
pub(crate) struct Motion {
    pub(crate) phase: f32,
    pub(crate) reveal: f32,
    pub(crate) energy: f32,
    pub(crate) calm: f32,
    pub(crate) party: Option<f32>,
}

/// Deck A's colour and deck B's, as Pina Live draws them.
const DECK_A: u32 = ACCENT;
const DECK_B: u32 = 0xff7a59;

/// Under Pina Live's icon: two decks mixing. Each a jog, its sixteen beats lit round it in its
/// deck's colour (the one playing bright, those gone by dim) round a grooved platter and a needle
/// turning with the track; between them the crossfader going across and back. They fade in,
/// turn faster while it works, and when it's ready flash on the beat with sparks flying off.
pub(crate) fn decks(m: Motion) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
            let o = b.origin;
            let at = |x: f32, y: f32| point(o.x + px(x), o.y + px(y));
            let tau = std::f32::consts::TAU;
            // 124 beats a minute while it rests, quicker while it works.
            let beat = m.phase * 2.1;
            let r = (w * 0.2).min(h * 0.34);
            let cy = h * 0.44;
            let flash = m.party.map_or(0.0, |_| (1.0 - beat.fract()).powi(3));
            for (k, (cx, color, offset)) in [(w * 0.27, DECK_A, 0.0f32), (w * 0.73, DECK_B, 0.37)]
                .into_iter()
                .enumerate()
            {
                let c = at(cx, cy);
                let shown = m.reveal * (0.86 + 0.14 * k as f32).min(1.0);
                // The rim and platter, lit from above, grooved.
                for (rr, shade) in [(r, 0x101014), (r * 0.8, 0x17171c), (r * 0.62, 0x1b1b21)] {
                    if let Some(p) = dot(c, rr) {
                        window.paint_path(p, alpha(shade, shown));
                    }
                }
                for g in 0..5 {
                    if let Some(p) = arc(c, r * (0.66 + g as f32 * 0.03), 0.0, 359.0, 1.0) {
                        window.paint_path(p, alpha(0xffffff, 0.025 * shown));
                    }
                }
                // Its beats round it: sixteen, a phrase, the one playing lit.
                let now = (beat + offset * 16.0).rem_euclid(16.0);
                for s in 0..16 {
                    let a0 = s as f32 * 22.5 - 90.0 + 2.5;
                    let lit = s == now.floor() as usize;
                    let gone = (s as f32) < now.floor();
                    let a = match (lit, gone) {
                        (true, _) => 1.0,
                        (_, true) => 0.35 + 0.5 * flash,
                        _ => 0.12 + 0.3 * flash,
                    };
                    if let Some(p) = arc(
                        c,
                        r * 0.9,
                        a0,
                        a0 + 17.5,
                        if s % 4 == 0 { 4.0 } else { 3.0 },
                    ) {
                        window.paint_path(p, alpha(color, a * shown));
                    }
                }
                // The needle, turning with the track (B a little behind A).
                let turn =
                    (m.phase * (0.55 + 0.9 * m.energy) * (1.0 - 0.6 * m.calm) + offset) * tau;
                let mut needle = PathBuilder::stroke(px(2.));
                needle.move_to(at(cx + r * 0.3 * turn.cos(), cy + r * 0.3 * turn.sin()));
                needle.line_to(at(cx + r * 0.7 * turn.cos(), cy + r * 0.7 * turn.sin()));
                if let Ok(p) = needle.build() {
                    window.paint_path(p, alpha(0xffffff, 0.9 * shown));
                }
                // Its middle, a dark screen with the deck's letter's colour round it.
                if let Some(p) = dot(c, r * 0.24) {
                    window.paint_path(p, alpha(0x09090c, shown));
                }
                if let Some(p) = arc(c, r * 0.24, 0.0, 359.0, 1.0) {
                    window.paint_path(p, alpha(color, 0.5 * shown));
                }
            }
            // The crossfader under them, going across and back.
            let (x0, x1, y) = (w * 0.32, w * 0.68, h * 0.86);
            let mut slot = PathBuilder::stroke(px(4.));
            slot.move_to(at(x0, y));
            slot.line_to(at(x1, y));
            if let Ok(p) = slot.build() {
                window.paint_path(p, alpha(0x050507, m.reveal));
            }
            for t in 0..=8 {
                let x = x0 + (x1 - x0) * t as f32 / 8.0;
                let mut tk = PathBuilder::stroke(px(1.));
                tk.move_to(at(x, y - 9.0));
                tk.line_to(at(x, y - if t % 4 == 0 { 15.0 } else { 12.0 }));
                if let Ok(p) = tk.build() {
                    window.paint_path(p, alpha(0xffffff, 0.12 * m.reveal));
                }
            }
            let xf = 0.5 + 0.42 * (m.phase * 0.35).sin() * (1.0 - 0.7 * m.calm);
            let cap_x = x0 + (x1 - x0) * xf;
            let mut cap = PathBuilder::stroke(px(9.));
            cap.move_to(at(cap_x, y - 7.0));
            cap.line_to(at(cap_x, y + 7.0));
            if let Ok(p) = cap.build() {
                window.paint_path(p, alpha(0x3a3a44, m.reveal));
            }
            let mut line = PathBuilder::stroke(px(1.5));
            line.move_to(at(cap_x, y - 5.0));
            line.line_to(at(cap_x, y + 5.0));
            if let Ok(p) = line.build() {
                window.paint_path(p, alpha(0xffffff, m.reveal));
            }
            // Ready: sparks fly off the decks, on the beat.
            if let Some(c) = m.party {
                for i in 0..40 {
                    let age = c - hashed(i, 1.0) * 1.2;
                    let life = 1.8 + hashed(i, 2.0) * 0.9;
                    if !(0.0..life).contains(&age) {
                        continue;
                    }
                    let from = if i % 2 == 0 { w * 0.27 } else { w * 0.73 };
                    let a = hashed(i, 3.0) * tau;
                    let dist = r * 0.9 + age * (40.0 + hashed(i, 4.0) * 70.0);
                    let x = from + a.cos() * dist;
                    let y = cy + a.sin() * dist - age * 18.0;
                    let fade = (1.0 - age / life) * (age * 6.0).min(1.0);
                    let color = [DECK_A, DECK_B, MINT, 0xffffff][i % 4];
                    if let Some(p) = dot(at(x, y), 1.3 + hashed(i, 5.0) * 2.2) {
                        window.paint_path(p, alpha(color, fade));
                    }
                }
            }
        },
    )
    .size_full()
}

/// A filled circle.
pub(crate) fn dot(c: gpui::Point<gpui::Pixels>, r: f32) -> Option<gpui::Path<gpui::Pixels>> {
    let mut p = PathBuilder::fill();
    p.move_to(point(c.x + px(r), c.y));
    p.arc_to(
        point(px(r), px(r)),
        px(0.),
        false,
        false,
        point(c.x - px(r), c.y),
    );
    p.arc_to(
        point(px(r), px(r)),
        px(0.),
        false,
        false,
        point(c.x + px(r), c.y),
    );
    p.close();
    p.build().ok()
}

/// An arc round `c`, from one angle to another (degrees, clockwise from the right).
pub(crate) fn arc(
    c: gpui::Point<gpui::Pixels>,
    r: f32,
    from: f32,
    to: f32,
    width: f32,
) -> Option<gpui::Path<gpui::Pixels>> {
    if (to - from).abs() < 0.5 {
        return None;
    }
    let mut p = PathBuilder::stroke(px(width));
    let steps = (((to - from).abs() / 8.0).ceil() as usize).max(2);
    for k in 0..=steps {
        let a = (from + (to - from) * k as f32 / steps as f32).to_radians();
        let q = point(c.x + px(r * a.cos()), c.y + px(r * a.sin()));
        if k == 0 {
            p.move_to(q);
        } else {
            p.line_to(q);
        }
    }
    p.build().ok()
}

/// A tick, drawn as far as `p` (0 to 1) along its two strokes.
pub(crate) fn tick(
    c: gpui::Point<gpui::Pixels>,
    size: f32,
    width: f32,
    p: f32,
) -> Option<gpui::Path<gpui::Pixels>> {
    if p <= 0.0 {
        return None;
    }
    let pts = [(-0.45, 0.05), (-0.15, 0.35), (0.45, -0.35)].map(|(x, y)| (x * size, y * size));
    let lens = [
        ((pts[1].0 - pts[0].0).powi(2) + (pts[1].1 - pts[0].1).powi(2)).sqrt(),
        ((pts[2].0 - pts[1].0).powi(2) + (pts[2].1 - pts[1].1).powi(2)).sqrt(),
    ];
    let mut left = p.min(1.0) * (lens[0] + lens[1]);
    let at = |q: (f32, f32)| point(c.x + px(q.0), c.y + px(q.1));
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(at(pts[0]));
    for k in 0..2 {
        let f = (left / lens[k]).min(1.0);
        let to = (
            pts[k].0 + (pts[k + 1].0 - pts[k].0) * f,
            pts[k].1 + (pts[k + 1].1 - pts[k].1) * f,
        );
        path.line_to(at(to));
        left -= lens[k];
        if left <= 0.0 {
            break;
        }
    }
    path.build().ok()
}

/// A step's mark: a faint ring waiting, a turning arc, a tick drawing itself (with its ring
/// going round), or a dash.
pub(crate) fn mark(state: &State, t: f32, since: Option<f32>) -> impl IntoElement {
    let state = state.clone();
    canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            let c = b.center();
            let r = 8.0;
            let s = since.unwrap_or(10.0);
            match &state {
                State::Waiting => {
                    if let Some(p) = arc(c, r, 0.0, 360.0, 1.5) {
                        window.paint_path(p, rgb(RAISED));
                    }
                }
                State::Running => {
                    if let Some(p) = arc(c, r, 0.0, 360.0, 2.0) {
                        window.paint_path(p, rgb(RAISED));
                    }
                    let a = t * 400.0;
                    let len = 70.0 + 50.0 * (t * 3.0).sin();
                    if let Some(p) = arc(c, r, a, a + len, 2.0) {
                        window.paint_path(p, rgb(ACCENT));
                    }
                }
                State::Done(_) => {
                    let ring = ease_out(s / 0.35);
                    if let Some(p) = dot(c, r * ease_back(s / 0.4).max(0.0)) {
                        window.paint_path(p, alpha(MINT, 0.18));
                    }
                    if let Some(p) = arc(c, r, -90.0, -90.0 + 360.0 * ring, 2.0) {
                        window.paint_path(p, rgb(MINT));
                    }
                    if let Some(p) = tick(c, 10.0, 2.0, ease_out((s - 0.15) / 0.3)) {
                        window.paint_path(p, rgb(MINT));
                    }
                }
                State::Skipped(_) => {
                    let p = ease_out(s / 0.3);
                    let mut dash = PathBuilder::stroke(px(2.));
                    dash.move_to(point(c.x - px(3.5 * p), c.y));
                    dash.line_to(point(c.x + px(3.5 * p), c.y));
                    if let Some(q) = arc(c, r, 0.0, 360.0, 1.5) {
                        window.paint_path(q, rgb(TEXT_3));
                    }
                    if let Ok(q) = dash.build() {
                        window.paint_path(q, rgb(TEXT_3));
                    }
                }
            }
        },
    )
    .size(px(20.))
    .flex_none()
}

/// The big check when it's done: its ring going round, filling, the tick drawn last, a pop.
pub(crate) fn big_check(since: f32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            let c = b.center();
            let pop =
                1.0 + 0.08 * (std::f32::consts::PI * ((since - 0.55) / 0.35).clamp(0.0, 1.0)).sin();
            let r = 22.0 * pop;
            if let Some(p) = dot(c, r * ease_out(since / 0.5)) {
                window.paint_path(p, alpha(MINT, 0.16));
            }
            if let Some(p) = arc(c, r, -90.0, -90.0 + 360.0 * ease_out(since / 0.5), 2.5) {
                window.paint_path(p, rgb(MINT));
            }
            if let Some(p) = tick(c, 24.0 * pop, 3.0, ease_out((since - 0.3) / 0.35)) {
                window.paint_path(p, rgb(MINT));
            }
        },
    )
    .size(px(56.))
    .flex_none()
}

/// Behind the icon: a soft glow that breathes; when it's done, rings going out from it.
pub(crate) fn halo(t: f32, open: f32, party: Option<f32>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            let c = b.center();
            let breathe = 1.0 + 0.05 * (t * 1.3).sin();
            for k in 0..7 {
                let r = (54.0 + k as f32 * 13.0) * breathe * (0.6 + 0.4 * open);
                if let Some(p) = dot(c, r) {
                    window.paint_path(p, alpha(ACCENT, 0.035 * open));
                }
            }
            if let Some(c0) = party {
                for k in 0..3 {
                    let age = c0 - k as f32 * 0.28;
                    if !(0.0..1.4).contains(&age) {
                        continue;
                    }
                    let r = 72.0 + ease_out(age / 1.4) * 130.0;
                    if let Some(p) = arc(c, r, 0.0, 360.0, 2.0) {
                        window.paint_path(p, alpha(MINT, 0.5 * (1.0 - age / 1.4)));
                    }
                }
            }
        },
    )
    .size_full()
}
