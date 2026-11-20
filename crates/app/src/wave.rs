//! The waveforms. Each deck's scrolls past its playhead in three bands (lows blue, mids amber,
//! highs white), drawn at the speed it plays so two decks at one tempo line up beat for beat,
//! over its beat grid (bars full height, beats as ticks), with its cue, hot cues, loop and, while
//! slipping, where the track really is. Under each deck, the whole track small, to jump about in,
//! its phrases along the bottom.

use std::sync::Arc;

use gpui::{
    App, Bounds, Context, Div, Hsla, MouseButton, MouseDownEvent, Pixels, ScrollWheelEvent,
    SharedString, TextRun, Window, canvas, div, fill, font, point, prelude::*, rgb, size,
};
use pina_live_engine::{
    analyze::{Phrase, WAVE_PER_SECOND, Wave},
    deck::{DeckCmd, HOT_CUES},
    grid::Grid,
};

use crate::{
    controls::{Drag, pt},
    live::{CUE_COLORS, DECK_COLORS, DECK_NAMES, Live},
    ui::{scale::px, theme::*},
};

/// How much each band counts as drawn (lows, mids, highs): the lows the body, the mids and highs
/// within it, as a three-band DJ waveform reads.
pub const WEIGHTS: [f32; 3] = [2.4, 1.6, 1.9];

pub const LOW: u32 = 0x3070f0;
pub const MID: u32 = 0xf0a23c;
pub const HIGH: u32 = 0xeef0f4;

/// What a deck's waveforms need, taken for the paint.
#[derive(Clone)]
struct View {
    wave: Option<Arc<Wave>>,
    /// What brings this track up to fill its lane.
    fill: f32,
    /// Seconds.
    now: f64,
    len: f64,
    tempo: f64,
    grid: Option<Grid>,
    cue: f64,
    hot: [f64; HOT_CUES],
    /// Start, end, on.
    lp: Option<(f64, f64, bool)>,
    shadow: f64,
    color: u32,
    phrases: Vec<Phrase>,
    loaded: bool,
    zoom: f64,
}

impl Live {
    fn view(&self, i: usize) -> View {
        let s = self.status().decks[i];
        let r = self.rate;
        let d = &self.decks[i];
        View {
            fill: d.wave.as_ref().map_or(1.0, |w| w.fill(WEIGHTS)),
            wave: d.wave.clone(),
            now: self.deck_seconds(i),
            len: s.len / r,
            tempo: s.tempo,
            grid: d.analysis.as_ref().and_then(|a| a.grid),
            cue: s.cue / r,
            hot: s.hot.map(|h| h / r),
            lp: (!s.loop_start.is_nan()).then(|| (s.loop_start / r, s.loop_end / r, s.loop_on)),
            shadow: s.shadow / r,
            color: DECK_COLORS[i],
            phrases: d
                .analysis
                .as_ref()
                .map_or(Vec::new(), |a| a.phrases.clone()),
            loaded: s.loaded,
            zoom: self.zoom as f64,
        }
    }

    /// Every deck shown's waveform, one over another.
    pub fn waveforms(&self, shown: usize, cx: &mut Context<Self>) -> Div {
        let h = self.wave_h;
        let order: Vec<usize> = (0..shown).collect();
        let mut col = div().flex().flex_col().flex_none().bg(rgb(LANE));
        for (k, i) in order.into_iter().enumerate() {
            col = col.child(self.wave_row(i, h, k > 0, cx));
        }
        col
    }

    fn wave_row(&self, i: usize, h: f32, line: bool, cx: &mut Context<Self>) -> Div {
        let s = self.status().decks[i];
        let color = DECK_COLORS[i];
        let grid = self.decks[i].analysis.as_ref().and_then(|a| a.grid);
        let beat_in_bar = match (grid, s.beat.is_nan()) {
            (Some(g), false) => Some((s.beat.floor().rem_euclid(g.per_bar.max(1) as f64)) as usize),
            _ => None,
        };
        let view = self.view(i);
        let bounds = self.decks[i].wave_at.clone();
        div()
            .h(px(h))
            .flex()
            .when(line, |d| d.border_t_1().border_color(rgb(LINE)))
            .child(
                div()
                    .w(px(64.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(5.))
                    .px(px(10.))
                    .bg(rgb(HEADERS))
                    .border_r_1()
                    .border_color(rgb(LINE))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_size(px(13.))
                                    .text_color(rgb(color))
                                    .child(DECK_NAMES[i]),
                            )
                            .when(self.leader() == Some(i), |d| {
                                d.child(
                                    div()
                                        .text_size(px(8.5))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(ACCENT_TEXT))
                                        .child("LEAD"),
                                )
                            }),
                    )
                    // The beat it's on in the bar, as a CDJ's countdown dots.
                    .child(div().flex().gap(px(3.)).children((0..4).map(|b| {
                        let on = beat_in_bar == Some(b) && s.loaded;
                        div()
                            .flex_1()
                            .h(px(5.))
                            .rounded(px(1.5))
                            .bg(rgb(match (on, b == 0) {
                                (true, true) => color,
                                (true, false) => TEXT_2,
                                _ => RAISED,
                            }))
                    }))),
            )
            .child(
                div()
                    .id(("wave", i))
                    .relative()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .cursor(gpui::CursorStyle::ResizeLeftRight)
                    // A track dropped on the lane loads on its deck, as on the deck's card.
                    .drag_over::<crate::browser::TrackDrag>(move |st, _, _, _| {
                        st.bg(alpha(color, 0.12))
                            .border_1()
                            .border_color(rgb(color))
                    })
                    .drag_over::<gpui::ExternalPaths>(move |st, _, _, _| {
                        st.bg(alpha(color, 0.12))
                            .border_1()
                            .border_color(rgb(color))
                    })
                    .on_drop(
                        cx.listener(move |live, t: &crate::browser::TrackDrag, _, cx| {
                            live.drop_on = None;
                            live.load(i, t.id, cx);
                        }),
                    )
                    .on_drop(
                        cx.listener(move |live, paths: &gpui::ExternalPaths, _, cx| {
                            live.dropped(Some(i), paths, cx)
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                            if !live.status().decks[i].loaded {
                                return;
                            }
                            live.deck(i, DeckCmd::Touch(true));
                            live.drag = Some(Drag::Wave {
                                deck: i,
                                x: pt(e.position).0,
                            });
                            cx.notify();
                        }),
                    )
                    .on_scroll_wheel(cx.listener(|live, e: &ScrollWheelEvent, _, cx| {
                        let dy = f32::from(e.delta.pixel_delta(gpui::px(16.)).y);
                        live.zoom = (live.zoom * (1.0 - dy * 0.004)).clamp(2.0, 60.0);
                        cx.notify();
                    }))
                    .child(
                        canvas(
                            move |b, _, _| bounds.set(b),
                            move |b, _, window, cx| paint_scrolling(&view, b, window, cx),
                        )
                        .size_full(),
                    )
                    .when(!s.loaded, |d| {
                        d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(11.5))
                                .text_color(rgb(TEXT_4))
                                .child(match self.decks[i].loading {
                                    Some(_) => format!("Reading the track onto {}…", DECK_NAMES[i]),
                                    None => format!(
                                        "Deck {} is empty: drag a track here",
                                        DECK_NAMES[i]
                                    ),
                                }),
                        )
                    }),
            )
    }
}

/// The whole track small, for the deck's panel.
pub fn overview(live: &Live, i: usize) -> impl IntoElement {
    let view = live.view(i);
    let bounds = live.decks[i].overview_at.clone();
    canvas(
        move |b, _, _| bounds.set(b),
        move |b, _, window, cx| paint_overview(&view, b, window, cx),
    )
    .size_full()
}

fn quad(window: &mut Window, x: f32, y: f32, w: f32, h: f32, c: impl Into<Hsla>) {
    window.paint_quad(fill(
        Bounds::new(
            point(gpui::px(x), gpui::px(y)),
            size(gpui::px(w), gpui::px(h)),
        ),
        c.into(),
    ));
}

/// Text painted straight onto a picture.
fn text(window: &mut Window, cx: &mut App, s: &str, x: f32, y: f32, size_px: f32, color: u32) {
    let s: SharedString = s.to_string().into();
    let run = TextRun {
        len: s.len(),
        font: gpui::Font {
            weight: gpui::FontWeight::BOLD,
            ..font(MONO)
        },
        color: rgb(color).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(s, gpui::px(size_px), &[run], None);
    let _ = line.paint(
        point(gpui::px(x), gpui::px(y)),
        gpui::px(size_px + 2.0),
        gpui::TextAlign::Left,
        None,
        window,
        cx,
    );
}

/// The loudest of each band over columns `a..b`.
fn columns(cols: &[[u8; 4]], a: isize, b: isize) -> Option<[u8; 4]> {
    let n = cols.len() as isize;
    let (a, b) = (a.max(0), b.min(n));
    if a >= b {
        return None;
    }
    let mut m = [0u8; 4];
    for c in &cols[a as usize..b as usize] {
        for k in 0..4 {
            m[k] = m[k].max(c[k]);
        }
    }
    Some(m)
}

/// Columns of waveform, each band one shape: every column's bar two triangles of it. (Three
/// quads a pixel column were tens of thousands a frame, each filed away by GPUI: most of what
/// drawing the decks cost, and the pause coming back to them from the library.)
#[derive(Default)]
struct Bands([Option<gpui::Path<Pixels>>; 3]);

impl Bands {
    /// A column at `x`, `w` wide, `half` tall either side of `mid`: each band at its weight,
    /// brought up by `fill`, and eased (a power under one) so quiet detail still shows.
    fn column(&mut self, x: f32, w: f32, mid: f32, half: f32, c: [u8; 4], fill: f32) {
        let hgt = |v: u8, b: usize| (Wave::level(v) * WEIGHTS[b] * fill).min(1.0).powf(0.8) * half;
        let heights = [hgt(c[1], 0), hgt(c[2], 1) * 0.8, hgt(c[3], 2) * 0.6];
        let p = |x: f32, y: f32| point(gpui::px(x), gpui::px(y));
        // Solid: no curve to shade.
        let st = (point(0., 1.), point(0., 1.), point(0., 1.));
        for (band, h) in self.0.iter_mut().zip(heights) {
            if h < 0.05 {
                continue;
            }
            let (a, b, c, d) = (
                p(x, mid - h),
                p(x + w, mid - h),
                p(x + w, mid + h),
                p(x, mid + h),
            );
            let path = band.get_or_insert_with(|| gpui::Path::new(a));
            path.push_triangle((a, b, c), st);
            path.push_triangle((a, c, d), st);
        }
    }

    fn paint(self, window: &mut Window, a: f32) {
        for (path, color) in self.0.into_iter().zip([LOW, MID, HIGH]) {
            if let Some(path) = path {
                window.paint_path(path, alpha(color, a));
            }
        }
    }
}

fn paint_scrolling(v: &View, b: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    let mid = y0 + h / 2.0;
    let half = h / 2.0 - 3.0;
    let center = x0 + (w / 2.0).round();
    if !v.loaded {
        return;
    }
    let speed = v.tempo.abs().max(0.05);
    // Seconds of the track a pixel covers.
    let per_px = v.zoom / w as f64 * speed;
    let x_of = |t: f64| center + ((t - v.now) / per_px) as f32;
    // Loop under everything.
    if let Some((a, z, on)) = v.lp {
        let (xa, xz) = (x_of(a).max(x0), x_of(z).min(x0 + w));
        if xz > xa {
            quad(
                window,
                xa,
                y0,
                xz - xa,
                h,
                alpha(if on { MINT } else { TEXT }, if on { 0.14 } else { 0.05 }),
            );
        }
        for x in [x_of(a), x_of(z)] {
            if (x0..x0 + w).contains(&x) {
                quad(
                    window,
                    x,
                    y0,
                    1.0,
                    h,
                    alpha(MINT, if on { 0.8 } else { 0.3 }),
                );
            }
        }
    }
    if let Some(wave) = &v.wave {
        let cols = &wave.cols;
        let mut bands = Bands::default();
        for k in 0..w as usize {
            let t0 = v.now + (k as f64 - w as f64 / 2.0) * per_px;
            let c0 = (t0 * WAVE_PER_SECOND).floor() as isize;
            let c1 = (((t0 + per_px) * WAVE_PER_SECOND).floor() as isize).max(c0 + 1);
            if let Some(c) = columns(cols, c0, c1) {
                bands.column(x0 + k as f32, 1.0, mid, half, c, v.fill);
            }
        }
        bands.paint(window, 1.0);
    }
    // The grid: bars full height, beats as ticks top and bottom.
    if let Some(g) = v.grid {
        let p = 60.0 / g.bpm;
        let left = v.now - per_px * w as f64 / 2.0;
        let right = v.now + per_px * w as f64 / 2.0;
        let (b0, b1) = (
            ((left - g.anchor) / p).floor() as i64,
            ((right - g.anchor) / p).ceil() as i64,
        );
        // Beats closer than 4 px apart aren't drawn, only bars.
        let beats = p / per_px > 4.0;
        for k in b0..=b1 {
            let x = x_of(g.anchor + k as f64 * p);
            if !(x0..x0 + w).contains(&x) {
                continue;
            }
            let per = g.per_bar.max(1) as i64;
            let bar = k.rem_euclid(per) == 0;
            if bar {
                let phrase = k.rem_euclid(per * 16) == 0;
                quad(
                    window,
                    x,
                    y0,
                    1.0,
                    h,
                    alpha(TEXT, if phrase { 0.5 } else { 0.24 }),
                );
                // Its number at the top, every bar when they're far enough apart (else each
                // fourth), as the counter under the deck counts them.
                let bar_px = p * per as f64 / per_px;
                let every = if bar_px > 36.0 { 1 } else { 4 };
                let n = k.div_euclid(per) + 1;
                if bar_px * every as f64 > 36.0 && (n - 1).rem_euclid(every) == 0 && n > 0 {
                    text(
                        window,
                        cx,
                        &n.to_string(),
                        x + 3.0,
                        y0 + 1.0,
                        8.5,
                        if phrase { TEXT_2 } else { TEXT_4 },
                    );
                }
            } else if beats {
                quad(window, x, y0, 1.0, 5.0, alpha(TEXT, 0.4));
                quad(window, x, y0 + h - 5.0, 1.0, 5.0, alpha(TEXT, 0.4));
            }
        }
    }
    // The cue, a hot cue each.
    let cue_x = x_of(v.cue);
    if (x0..x0 + w).contains(&cue_x) {
        quad(window, cue_x, y0, 1.0, h, alpha(AMBER, 0.7));
        quad(window, cue_x - 4.0, y0, 9.0, 5.0, rgb(AMBER));
    }
    for (k, at) in v.hot.iter().enumerate() {
        if at.is_nan() {
            continue;
        }
        let x = x_of(*at);
        if (x0 - 14.0..x0 + w).contains(&x) {
            quad(window, x, y0, 1.0, h, alpha(CUE_COLORS[k], 0.85));
            quad(window, x, y0, 13.0, 13.0, rgb(CUE_COLORS[k]));
            text(
                window,
                cx,
                &format!("{}", k + 1),
                x + 3.0,
                y0,
                10.0,
                0x0c0c0f,
            );
        }
    }
    // Slipping: where the track really is, faint.
    if !v.shadow.is_nan() {
        let x = x_of(v.shadow);
        if (x0..x0 + w).contains(&x) {
            let mut y = y0;
            while y < y0 + h {
                quad(window, x, y, 2.0, 4.0, alpha(v.color, 0.9));
                y += 7.0;
            }
        }
    }
    // The playhead.
    quad(window, center - 1.0, y0, 2.0, h, rgb(TEXT));
    quad(window, center - 4.0, y0, 8.0, 3.0, rgb(v.color));
    quad(window, center - 4.0, y0 + h - 3.0, 8.0, 3.0, rgb(v.color));
}

/// A phrase's color, by what it's called: quiet for the ends, warm for what's loud.
pub fn phrase_color(name: &str) -> u32 {
    let n = name.to_lowercase();
    if n.starts_with("intro") || n.starts_with("outro") {
        SLATE
    } else if n.starts_with("drop") || n.starts_with("chorus") || n.starts_with("hook") {
        CORAL
    } else if n.starts_with("build") || n.starts_with("pre") {
        AMBER
    } else if n.starts_with("break") || n.starts_with("bridge") || n.starts_with("interlude") {
        VIOLET
    } else {
        SKY
    }
}

fn paint_overview(v: &View, b: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    quad(window, x0, y0, w, h, rgb(WELL));
    if !v.loaded || v.len <= 0.0 {
        return;
    }
    let strip = if v.phrases.is_empty() { 0.0 } else { 4.0 };
    let mid = y0 + (h - strip) / 2.0;
    let half = (h - strip) / 2.0 - 2.0;
    let x_of = |t: f64| x0 + (t / v.len * w as f64) as f32;
    let play_x = x_of(v.now);
    if let Some((a, z, on)) = v.lp {
        quad(
            window,
            x_of(a),
            y0,
            (x_of(z) - x_of(a)).max(1.0),
            h - strip,
            alpha(MINT, if on { 0.3 } else { 0.12 }),
        );
    }
    if let Some(wave) = &v.wave {
        let n = wave.cols.len() as f64;
        let (mut played, mut ahead) = (Bands::default(), Bands::default());
        for k in 0..w as usize {
            let c0 = (k as f64 / w as f64 * n) as isize;
            let c1 = (((k + 1) as f64 / w as f64 * n) as isize).max(c0 + 1);
            if let Some(c) = columns(&wave.cols, c0, c1) {
                let x = x0 + k as f32;
                let bands = if x < play_x { &mut played } else { &mut ahead };
                bands.column(x, 1.0, mid, half, c, v.fill);
            }
        }
        played.paint(window, 0.4);
        ahead.paint(window, 0.95);
    }
    // Phrases along the bottom, a gap between each.
    for p in &v.phrases {
        let (a, z) = (x_of(p.start as f64), x_of(p.end as f64));
        quad(
            window,
            a,
            y0 + h - 3.0,
            (z - a - 1.0).max(1.0),
            3.0,
            rgb(phrase_color(&p.name)),
        );
    }
    // A tick each half-minute along the top (a minute's longer), and the last thirty seconds
    // warmed red: where to have the next one ready.
    let mut t = 30.0;
    while t < v.len {
        let minute = (t / 60.0).fract() < 1e-6;
        quad(
            window,
            x_of(t),
            y0,
            1.0,
            if minute { 5.0 } else { 3.0 },
            rgb(if minute { TEXT_3 } else { LINE_STRONG }),
        );
        t += 30.0;
    }
    if v.len > 60.0 {
        let a = x_of(v.len - 30.0);
        quad(window, a, y0, x0 + w - a, h - strip, alpha(RECORD, 0.08));
    }
    quad(window, x_of(v.cue), y0, 1.0, h - strip, rgb(AMBER));
    for (k, at) in v.hot.iter().enumerate() {
        if !at.is_nan() {
            let x = x_of(*at);
            quad(window, x, y0, 1.0, h - strip, rgb(CUE_COLORS[k]));
            quad(window, x, y0, 10.0, 10.0, rgb(CUE_COLORS[k]));
            text(
                window,
                cx,
                &format!("{}", k + 1),
                x + 2.0,
                y0 - 1.0,
                8.5,
                0x0c0c0f,
            );
        }
    }
    quad(window, play_x - 1.0, y0, 2.0, h, rgb(v.color));
}
