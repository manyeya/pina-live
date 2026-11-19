//! The window's layout: how tall the waveforms are, how tall the decks, how wide the library's
//! sidebar, and whether the library's open, shut, or the whole window. Each edge between them is
//! dragged to size it (double-clicked, back as it was), and the sizes are kept between runs.
//! What's asked is fitted to the window every frame: the library keeps room to be useful, the
//! decks never shrink past what their controls need.

use gpui::{Context, Div, MouseButton, MouseDownEvent, div, prelude::*, rgb};
use serde::{Deserialize, Serialize};

use crate::{
    controls::{Drag, pt},
    decks::GAP,
    fxbar::FX_BAR_H,
    live::Live,
    ui::{scale::px, theme::*},
};

pub const TOP_H: f32 = 44.0;
pub const LEARN_H: f32 = 34.0;
/// The least the library's given while it's open.
pub const LIB_MIN: f32 = 170.0;
/// Four decks two at a time a side below this width; a deck's card compact below this width
/// (in full it needs its beat ring, its pads' five tabs and its pitch fader side by side).
pub const LAYERED_BELOW: f32 = 1400.0;
pub const FULL_CARD: f32 = 480.0;
/// A channel strip's and the master's widths in a window `w` wide: the mixer grows with the
/// window as the decks do (its knobs with it), from what it needs to a half again.
pub fn mixer_dims(w: f32, four: bool) -> (f32, f32) {
    match four {
        false => (
            (w * 0.085).clamp(116.0, 172.0),
            (w * 0.05).clamp(68.0, 96.0),
        ),
        true => ((w * 0.056).clamp(82.0, 124.0), (w * 0.04).clamp(60.0, 84.0)),
    }
}

/// The mixer's width in a window `w` wide: its strips and the master, the gaps between.
pub fn mixer_w(w: f32, four: bool) -> f32 {
    let (strip, master) = mixer_dims(w, four);
    let n = if four { 4.0 } else { 2.0 };
    n * strip + master + n * 8.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibView {
    #[default]
    Open,
    Closed,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// A waveform's height.
    pub waves: f32,
    /// The decks' (and mixer's) height.
    pub panel: f32,
    /// The library's sidebar's width.
    pub side: f32,
    pub library: LibView,
}

impl Default for Layout {
    fn default() -> Layout {
        Layout {
            waves: 76.0,
            panel: 442.0,
            side: 200.0,
            library: LibView::Open,
        }
    }
}

/// An edge dragged to size what's either side of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Waves,
    Panel,
    Side,
}

/// What's drawn this frame: the layout as asked, fitted to the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
    pub wave: f32,
    pub panel: f32,
    pub compact: bool,
    pub layered: bool,
    /// A deck card's width.
    pub card: f32,
}

/// How short and tall the decks can be: never shorter than their controls need, never taller
/// than they've use for (crate::decks::panel_max).
pub fn panel_range(compact: bool, four: bool) -> (f32, f32) {
    let least = if compact { 330.0 } else { 412.0 };
    (least, crate::decks::panel_max(compact, four).max(least))
}

pub fn wave_range(rows: usize) -> (f32, f32) {
    (if rows > 2 { 30.0 } else { 36.0 }, 260.0)
}

pub fn side_range() -> (f32, f32) {
    (150.0, 380.0)
}

/// The layout fitted to a window `w` by `h` with `rows` waveforms (and the learn bar's room).
pub fn fit(l: &Layout, w: f32, h: f32, rows: usize, four: bool, learning: bool) -> Sizes {
    let layered = four && w < LAYERED_BELOW;
    let cards = if four && !layered { 4.0 } else { 2.0 };
    let mixer = mixer_w(w, four);
    let card = (w - mixer - (cards + 2.0) * GAP) / cards;
    let compact = four || card < FULL_CARD;
    let n = rows.max(1) as f32;
    let (pmin, pmax) = panel_range(compact, four);
    let (wmin, wmax) = wave_range(rows);
    let mut panel = l.panel.clamp(pmin, pmax);
    let mut wave = l.waves.clamp(wmin, wmax);
    let fixed = TOP_H + FX_BAR_H + 2.0 * GAP + if learning { LEARN_H } else { 0.0 } + 2.0;
    match l.library {
        LibView::Open => {
            // The library keeps its room: the waveforms give way first, then the decks.
            let room = h - fixed - LIB_MIN;
            if wave * n + panel > room {
                wave = ((room - panel) / n).max(wmin);
            }
            if wave * n + panel > room {
                panel = (room - wave * n).max(pmin);
            }
        }
        // Shut: the waveforms as they're set (as long as the decks keep their least), and the
        // decks fill the rest to the bottom: nothing left empty.
        // At their tallest, the decks leave the rest to the waveforms (and past theirs, take
        // it back: nothing's left empty).
        LibView::Closed => {
            wave = wave.min(((h - fixed - pmin) / n).max(wmin));
            panel = (h - fixed - wave * n).clamp(pmin, pmax);
            wave = ((h - fixed - panel) / n).max(wave);
            if wave > wmax {
                wave = wmax;
                panel = (h - fixed - wave * n).max(pmin);
            }
        }
        LibView::Full => {}
    }
    Sizes {
        wave,
        panel,
        compact,
        layered,
        card,
    }
}

impl Live {
    /// An edge's grip: a strip a few pixels across over the line between two panels, the line
    /// lit while it's held. Dragged, it sizes them; double-clicked, it puts them back.
    pub fn grip(&self, edge: Edge, cx: &mut Context<Self>) -> Div {
        let held = matches!(self.drag, Some(Drag::Resize { edge: e, .. }) if e == edge);
        let across = edge == Edge::Side;
        let line = div()
            .absolute()
            .bg(rgb(if held { ACCENT } else { LINE }))
            .when(across, |d| {
                d.top_0()
                    .bottom_0()
                    .left(px(3.))
                    .w(px(if held { 2. } else { 1. }))
            })
            .when(!across, |d| {
                d.left_0()
                    .right_0()
                    .top(px(3.))
                    .h(px(if held { 2. } else { 1. }))
            });
        let hit = div()
            .absolute()
            .when(across, |d| {
                d.top_0()
                    .bottom_0()
                    .left(px(-3.))
                    .w(px(7.))
                    .cursor(gpui::CursorStyle::ResizeLeftRight)
            })
            .when(!across, |d| {
                d.left_0()
                    .right_0()
                    .top(px(-3.))
                    .h(px(7.))
                    .cursor(gpui::CursorStyle::ResizeUpDown)
            })
            .hover(|s| s.bg(alpha(ACCENT, 0.25)))
            .when(held, |d| d.bg(alpha(ACCENT, 0.25)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    let (x, y) = pt(e.position);
                    if e.click_count >= 2 {
                        let d = Layout::default();
                        match edge {
                            Edge::Waves => live.layout_mut().waves = d.waves,
                            Edge::Panel => live.layout_mut().panel = d.panel,
                            Edge::Side => live.layout_mut().side = d.side,
                        }
                        live.settings.save();
                        cx.notify();
                        return;
                    }
                    let value = match edge {
                        Edge::Waves => live.wave_h,
                        Edge::Panel => live.panel_h,
                        Edge::Side => live.settings.layout.side,
                    };
                    live.drag = Some(Drag::Resize {
                        edge,
                        from: if across { x } else { y },
                        value,
                    });
                    cx.notify();
                }),
            );
        div()
            .relative()
            .flex_none()
            .when(across, |d| d.w(px(0.)).h_full())
            .when(!across, |d| d.h(px(0.)).w_full())
            .child(line)
            .child(hit.child(crate::probe::probe(&self.probes, format!("grip {edge:?}"))))
    }

    pub fn layout_mut(&mut self) -> &mut Layout {
        &mut self.settings.layout
    }

    /// An edge dragged `by` pixels from where it was (`value` then). What's kept is what's
    /// drawn: the sizes are fitted to the window straight away and stored as fitted, so an edge
    /// dragged past where it can go doesn't run on unseen and jump later.
    pub fn resize(&mut self, edge: Edge, value: f32, by: f32) {
        let shown = self.shown();
        let rows = shown.max(1) as f32;
        let narrow = self.narrow;
        let l = &mut self.settings.layout;
        match edge {
            Edge::Waves => {
                let (lo, hi) = wave_range(shown);
                l.waves = (value + by / rows).clamp(lo, hi);
            }
            Edge::Panel => {
                let (lo, hi) = panel_range(narrow, self.four);
                l.panel = (value + by).clamp(lo, hi);
            }
            Edge::Side => {
                let (lo, hi) = side_range();
                l.side = (value + by).clamp(lo, hi);
            }
        }
        let (w, h) = self.view;
        let s = fit(&self.settings.layout, w, h, shown, self.four, self.learning);
        let l = &mut self.settings.layout;
        match (edge, l.library) {
            // Shut, the decks only follow the waveforms: their height isn't kept.
            (_, LibView::Closed) => l.waves = s.wave,
            (Edge::Side, _) => {}
            _ => (l.waves, l.panel) = (s.wave, s.panel),
        }
    }

    pub fn set_library(&mut self, v: LibView, cx: &mut Context<Self>) {
        self.settings.layout.library = v;
        self.settings.save();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_keeps_the_library_and_the_decks_usable() {
        let l = Layout::default();
        // Room for all of it as asked.
        let s = fit(&l, 1440.0, 900.0, 2, false, false);
        assert_eq!((s.wave, s.panel), (76.0, 442.0));
        // Short: the waveforms give way, then the decks, never past their least.
        let s = fit(&l, 1100.0, 700.0, 2, false, false);
        assert!(s.compact && !s.layered);
        let (pmin, _) = panel_range(true, false);
        assert!(s.panel >= pmin && s.wave >= 36.0);
        // Library shut: the decks at their tallest, the waveforms the rest, nothing empty.
        let shut = Layout {
            library: LibView::Closed,
            ..l
        };
        let s = fit(&shut, 1440.0, 900.0, 2, false, false);
        let fixed = TOP_H + FX_BAR_H + 2.0 * GAP + 2.0;
        assert_eq!(s.panel, crate::decks::panel_max(false, false));
        assert!(s.wave > 76.0, "{}", s.wave);
        assert!(
            (s.wave * 2.0 + s.panel - (900.0 - fixed)).abs() < 0.5,
            "{}",
            s.panel
        );
        // Waveforms dragged too tall: the decks keep their least.
        let tall = Layout {
            waves: 260.0,
            ..shut
        };
        let s = fit(&tall, 1440.0, 700.0, 2, false, false);
        assert!(
            s.panel >= panel_range(false, false).0 && s.wave < 260.0,
            "{s:?}"
        );
        // Four decks in a narrow window go two a side.
        assert!(fit(&l, 1300.0, 900.0, 4, true, false).layered);
        assert!(!fit(&l, 1600.0, 900.0, 4, true, false).layered);
    }
}
