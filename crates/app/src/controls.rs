//! What's turned, slid and held: each knob and fader's reach and what it sets, dragging them
//! (shift for fine), the platter and waveform under the pointer (scratching), and buttons that
//! act while held (cue, rolls, censor), let go wherever the pointer ends up.

use gpui::{
    Bounds, Context, Div, MouseButton, MouseDownEvent, Pixels, Point, ScrollWheelEvent, canvas,
    div, fill, point, prelude::*, rgb, size,
};
use pina_live_engine::{deck::DeckCmd, mixer::MixCmd};

use crate::{
    live::Live,
    ui::{scale::px, theme::*},
};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Ctl {
    Trim(usize),
    Eq(usize, usize),
    Filter(usize),
    Fader(usize),
    Xf,
    Master,
    CueMix,
    Phones,
    /// The pitch fader, -1 to 1 of its range.
    Tempo(usize),
    FxLevel(usize),
    FxParam(usize),
}

/// Something held down, let go when the button comes up (wherever the pointer is then).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hold {
    Cue,
    HotCue(usize),
    Roll(f64),
    Censor,
    /// The pitch bent (nudged) while it's held: by this share of its speed.
    Bend(f64),
}

pub enum Drag {
    Ctl {
        ctl: Ctl,
        from: (f32, f32),
        value: f32,
    },
    /// Turning the platter by hand: the angle it was last at.
    Platter {
        deck: usize,
        angle: f32,
    },
    /// The waveform pulled like a record: the pointer's last x.
    Wave {
        deck: usize,
        x: f32,
    },
    Hold {
        deck: usize,
        hold: Hold,
    },
    /// An edge between panels: where the pointer was, and the size then.
    Resize {
        edge: crate::layout::Edge,
        from: f32,
        value: f32,
    },
}

impl Ctl {
    /// Its reach, and where a double-click puts it.
    pub fn range(self) -> (f32, f32, f32) {
        match self {
            Ctl::Trim(_) => (-12.0, 12.0, 0.0),
            Ctl::Eq(..) => (0.0, 1.0, 0.5),
            Ctl::Filter(_) | Ctl::Xf | Ctl::Tempo(_) => (-1.0, 1.0, 0.0),
            Ctl::Fader(_) => (0.0, 1.0, 1.0),
            Ctl::Master => (-24.0, 6.0, 0.0),
            Ctl::CueMix => (0.0, 1.0, 0.5),
            Ctl::Phones => (-40.0, 6.0, 0.0),
            Ctl::FxLevel(_) => (0.0, 1.0, 0.5),
            Ctl::FxParam(_) => (0.0, 1.0, 0.5),
        }
    }

    /// Turned from the middle rather than the bottom.
    pub fn bipolar(self) -> bool {
        matches!(
            self,
            Ctl::Trim(_) | Ctl::Filter(_) | Ctl::Xf | Ctl::Tempo(_) | Ctl::Eq(..)
        )
    }

    /// Pixels of drag across its whole reach.
    fn travel(self) -> f32 {
        match self {
            Ctl::Fader(_) => 86.0,
            Ctl::Tempo(_) => 240.0,
            Ctl::Xf => 180.0,
            _ => 180.0,
        }
    }

    pub fn get(self, live: &Live) -> f32 {
        let m = &live.mix;
        match self {
            Ctl::Trim(c) => m.trim[c],
            Ctl::Eq(c, b) => m.eq[c][b],
            Ctl::Filter(c) => m.filter[c],
            Ctl::Fader(c) => m.fader[c],
            Ctl::Xf => m.xf,
            Ctl::Master => m.master,
            Ctl::CueMix => m.cue_mix,
            Ctl::Phones => m.phones,
            Ctl::Tempo(d) => (live.decks[d].fader / live.decks[d].range) as f32,
            Ctl::FxLevel(u) => live.fx[u].settings.level,
            Ctl::FxParam(u) => live.fx[u].settings.param,
        }
    }

    /// Set (clamped to its reach), and sent to the engine.
    pub fn set(self, live: &mut Live, v: f32) {
        let (lo, hi, _) = self.range();
        let v = v.clamp(lo, hi);
        let m = &mut live.mix;
        let cmd = match self {
            Ctl::Trim(ch) => {
                m.trim[ch] = v;
                MixCmd::Trim { ch, db: v }
            }
            Ctl::Eq(ch, band) => {
                m.eq[ch][band] = v;
                MixCmd::Eq { ch, band, knob: v }
            }
            Ctl::Filter(ch) => {
                m.filter[ch] = v;
                MixCmd::Filter { ch, knob: v }
            }
            Ctl::Fader(ch) => {
                m.fader[ch] = v;
                MixCmd::Fader { ch, v }
            }
            Ctl::Xf => {
                m.xf = v;
                MixCmd::Crossfader(v)
            }
            Ctl::Master => {
                m.master = v;
                MixCmd::MasterDb(v)
            }
            Ctl::CueMix => {
                m.cue_mix = v;
                MixCmd::CueMix(v)
            }
            Ctl::Phones => {
                m.phones = v;
                MixCmd::PhonesDb(v)
            }
            Ctl::FxLevel(u) => {
                live.fx[u].settings.level = v;
                live.send_fx(u);
                return;
            }
            Ctl::FxParam(u) => {
                live.fx[u].settings.param = v;
                live.send_fx(u);
                return;
            }
            Ctl::Tempo(d) => {
                let deck = &mut live.decks[d];
                deck.fader = v as f64 * deck.range;
                let t = 1.0 + deck.fader;
                live.deck(d, DeckCmd::Tempo(t));
                return;
            }
        };
        live.mixer(cmd);
    }
}

pub fn pt(p: Point<Pixels>) -> (f32, f32) {
    (f32::from(p.x), f32::from(p.y))
}

impl Live {
    pub fn press_ctl(&mut self, ctl: Ctl, e: &MouseDownEvent, cx: &mut Context<Self>) {
        let target = match ctl {
            Ctl::Tempo(d) => crate::midi::Target::Tempo(d),
            c => crate::midi::Target::Mix(c),
        };
        if self.arm(target, cx) {
            return;
        }
        if e.click_count >= 2 {
            let (_, _, rest) = match ctl {
                Ctl::FxParam(u) => (0.0, 1.0, self.fx[u].settings.kind.param_rest()),
                _ => ctl.range(),
            };
            // A fader's double-click doesn't jump it (a slip of the hand on the floor).
            if !matches!(ctl, Ctl::Fader(_)) {
                ctl.set(self, rest);
            }
        }
        self.drag = Some(Drag::Ctl {
            ctl,
            from: pt(e.position),
            value: ctl.get(self),
        });
        cx.notify();
    }

    pub fn drag_to(&mut self, at: Point<Pixels>, fine: bool, cx: &mut Context<Self>) {
        let (x, y) = pt(at);
        match self.drag {
            Some(Drag::Ctl { ctl, from, value }) => {
                let (lo, hi, _) = ctl.range();
                let moved = match ctl {
                    Ctl::Xf => x - from.0,
                    // The pitch fader: toward you is faster, as on a turntable.
                    Ctl::Tempo(_) => y - from.1,
                    _ => from.1 - y,
                };
                let scale = if fine { 0.1 } else { 1.0 };
                let to = value + moved / ctl.travel() * (hi - lo) * scale;
                // Pulled to its detents as it nears them, unless Shift's held.
                ctl.set(
                    self,
                    if fine {
                        to
                    } else {
                        crate::detail::snap(ctl, to)
                    },
                );
            }
            Some(Drag::Platter { deck, angle }) => {
                let b = self.decks[deck].platter.get();
                let c = b.center();
                let a = (y - f32::from(c.y)).atan2(x - f32::from(c.x));
                let mut d = a - angle;
                if d > std::f32::consts::PI {
                    d -= std::f32::consts::TAU;
                } else if d < -std::f32::consts::PI {
                    d += std::f32::consts::TAU;
                }
                self.deck(deck, DeckCmd::Scratch(d as f64 / std::f64::consts::TAU));
                self.drag = Some(Drag::Platter { deck, angle: a });
            }
            Some(Drag::Wave { deck, x: last }) => {
                let w = f32::from(self.decks[deck].wave_at.get().size.width).max(1.0);
                // The waveform pulled left moves the track on: its seconds across, in turns.
                let seconds = -(x - last) / w * self.zoom * self.status().decks[deck].tempo as f32;
                let turns = seconds as f64 / pina_live_engine::deck::SECONDS_PER_TURN;
                self.deck(deck, DeckCmd::Scratch(turns));
                self.drag = Some(Drag::Wave { deck, x });
            }
            Some(Drag::Resize { edge, from, value }) => {
                let by = match edge {
                    crate::layout::Edge::Side => x - from,
                    _ => y - from,
                };
                self.resize(edge, value, by);
            }
            Some(Drag::Hold { .. }) | None => return,
        }
        cx.notify();
    }

    pub fn drag_end(&mut self, d: Drag, cx: &mut Context<Self>) {
        match d {
            Drag::Platter { deck, .. } | Drag::Wave { deck, .. } => {
                self.deck(deck, DeckCmd::Touch(false))
            }
            Drag::Hold { deck, hold } => self.release(deck, hold),
            Drag::Resize { .. } => self.settings.save(),
            Drag::Ctl { .. } => {}
        }
        cx.notify();
    }

    /// A held button pressed.
    pub fn hold(&mut self, deck: usize, hold: Hold, cx: &mut Context<Self>) {
        use crate::midi::Target;
        let target = match hold {
            Hold::Cue => Some(Target::Cue(deck)),
            Hold::HotCue(k) => Some(Target::HotCue(deck, k)),
            Hold::Roll(b) => Some(Target::Roll(deck, b)),
            Hold::Censor => Some(Target::Censor(deck)),
            Hold::Bend(_) => None,
        };
        if let Some(t) = target
            && self.arm(t, cx)
        {
            return;
        }
        let c = match hold {
            Hold::Cue => DeckCmd::Cue(true),
            Hold::HotCue(slot) => DeckCmd::HotCue { slot, down: true },
            Hold::Roll(beats) => DeckCmd::Roll { beats, on: true },
            Hold::Censor => DeckCmd::Censor(true),
            Hold::Bend(b) => DeckCmd::Bend(b),
        };
        self.deck(deck, c);
        self.drag = Some(Drag::Hold { deck, hold });
        cx.notify();
    }

    fn release(&mut self, deck: usize, hold: Hold) {
        let c = match hold {
            Hold::Cue => DeckCmd::Cue(false),
            Hold::HotCue(slot) => DeckCmd::HotCue { slot, down: false },
            Hold::Roll(beats) => DeckCmd::Roll { beats, on: false },
            Hold::Censor => DeckCmd::Censor(false),
            Hold::Bend(_) => DeckCmd::Bend(0.0),
        };
        self.deck(deck, c);
    }

    pub fn holding(&self, deck: usize, hold: Hold) -> bool {
        matches!(self.drag, Some(Drag::Hold { deck: d, hold: h }) if d == deck && h == hold)
    }

    /// The wheel over a control: a notch a step (a tenth with Shift), the way the control's
    /// drawn moving (up is up; the pitch fader's cap goes up, so it slows).
    pub fn wheel_ctl(&mut self, ctl: Ctl, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let dy = f32::from(e.delta.pixel_delta(px(20.)).y) / 20.0;
        if dy == 0.0 {
            return;
        }
        let sign = if matches!(ctl, Ctl::Tempo(_)) {
            -1.0
        } else {
            1.0
        };
        let fine = if e.modifiers.shift { 0.1 } else { 1.0 };
        ctl.set(
            self,
            ctl.get(self) + sign * dy * crate::detail::step(ctl) * fine,
        );
        cx.stop_propagation();
        cx.notify();
    }

    /// Whether `ctl` is being dragged.
    pub fn turning(&self, ctl: Ctl) -> bool {
        matches!(self.drag, Some(Drag::Ctl { ctl: c, .. }) if c == ctl)
    }

    /// A knob with its caption above (lit while it's turned): dragged up and down, wheeled,
    /// double-clicked home.
    pub fn knob(
        &self,
        ctl: Ctl,
        caption: &'static str,
        color: u32,
        size: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let (lo, hi, _) = ctl.range();
        let v = self.ctl_value(ctl);
        let shown = match ctl.bipolar() {
            true => (v - (lo + hi) / 2.0) / ((hi - lo) / 2.0),
            false => (v - lo) / (hi - lo),
        };
        let on = self.mix_lit(ctl);
        let turning = self.turning(ctl);
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(1.))
            .when(!caption.is_empty(), |d| {
                d.child(
                    div()
                        .h(px(11.))
                        .line_height(px(11.))
                        .text_size(px(9.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .whitespace_nowrap()
                        // Lit while it's turned. What it reads goes on a line of its own where
                        // it's used, sized for it: never in place of its name.
                        .text_color(rgb(if turning { ACCENT_TEXT } else { TEXT_4 }))
                        .child(caption),
                )
            })
            .child(
                div()
                    .cursor(gpui::CursorStyle::ResizeUpDown)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            live.press_ctl(ctl, e, cx)
                        }),
                    )
                    .on_scroll_wheel(cx.listener(move |live, e: &ScrollWheelEvent, _, cx| {
                        live.wheel_ctl(ctl, e, cx)
                    }))
                    .child(crate::detail::knob(
                        shown,
                        size,
                        if on || turning { color } else { TEXT_3 },
                        ctl.bipolar(),
                    )),
            )
    }

    /// Whether a knob's moved from home (lit), or at home (quiet).
    fn mix_lit(&self, ctl: Ctl) -> bool {
        let (_, _, home) = ctl.range();
        (self.ctl_value(ctl) - home).abs() > 0.01
    }

    /// A vertical fader `h` tall and `w` wide: its slot, its scale of ticks (the pitch fader's
    /// numbered, −8 to +8 for ±8, its 0 lit when it's there; a channel fader's either side of
    /// the slot), how far the pitch is pulled from the middle, where sync has really taken the
    /// tempo when that's not where the fader is, and its cap with a grip. Dragged anywhere on
    /// it, snapping at its detents (Shift passes them by), wheeled, double-clicked home.
    pub fn fader(&self, ctl: Ctl, h: f32, w: f32, cx: &mut Context<Self>) -> Div {
        let (lo, hi, _) = ctl.range();
        let raw = self.ctl_value(ctl);
        // Held at its end past its range (sync can take the tempo past the pitch fader's ±).
        let v = ((raw - lo) / (hi - lo)).clamp(0.0, 1.0);
        let deck = match ctl {
            Ctl::Tempo(d) => Some(d),
            _ => None,
        };
        let tempo = deck.is_some();
        // The pitch fader's faster end is toward you (its bottom).
        let at = if tempo { v } else { 1.0 - v };
        let cap = 16.0;
        let travel = h - cap;
        let top = at * travel;
        // Its numbers: the ends, the halves and 0, in percent of its reach.
        let labels: Vec<(f32, String)> = deck.map_or(Vec::new(), |d| {
            let r = self.decks[d].range * 100.0;
            [-1.0, -0.5, 0.0, 0.5, 1.0]
                .into_iter()
                .map(|f: f64| {
                    let n = f * r;
                    let t = match n {
                        n if n.abs() < 1e-9 => "0".to_string(),
                        n if n < 0.0 => format!("−{}", -n),
                        n => format!("+{n}"),
                    };
                    (((f + 1.0) / 2.0) as f32, t)
                })
                .collect()
        });
        let widest = labels
            .iter()
            .map(|l| l.1.chars().count())
            .max()
            .unwrap_or(0) as f32;
        let label_w = if tempo { widest * 4.3 + 1.0 } else { 0.0 };
        let cap_w = if tempo { w - label_w - 7.0 } else { w - 12.0 };
        let slot_x = if tempo {
            label_w + 7.0 + cap_w / 2.0
        } else {
            w / 2.0
        };
        let zero = tempo && raw.abs() < 1e-6;
        // Where sync has really put the tempo, as a share down the fader.
        let ghost = deck.and_then(|d| {
            let s = self.status().decks[d];
            let real = ((s.tempo - 1.0) / self.decks[d].range) as f32;
            (s.loaded && s.sync && (real - raw).abs() > 0.004)
                .then_some(((real + 1.0) / 2.0).clamp(0.0, 1.0))
        });
        let ticks = if tempo { 16 } else { 10 };
        let major = if tempo { 4 } else { 5 };
        let scale = canvas(
            |_, _, _| {},
            move |b, _, window, _| {
                let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
                let q = |window: &mut gpui::Window, x: f32, y: f32, w: f32, h: f32, c: u32| {
                    window.paint_quad(fill(
                        Bounds::new(point(px(x0 + x), px(y0 + y)), size(px(w), px(h))),
                        rgb(c),
                    ))
                };
                for k in 0..=ticks {
                    let y = cap / 2.0 + k as f32 / ticks as f32 * travel - 0.5;
                    let mid = tempo && k == ticks / 2;
                    let big = k % major == 0;
                    let len = if mid {
                        6.0
                    } else if big {
                        5.0
                    } else {
                        3.0
                    };
                    let c = if mid {
                        TEXT_3
                    } else if big {
                        LINE_STRONG
                    } else {
                        LINE
                    };
                    match tempo {
                        true => q(window, label_w + 6.0 - len, y, len, 1.0, c),
                        false => {
                            q(window, 5.0 - len, y, len, 1.0, c);
                            q(window, w - 5.0, y, len, 1.0, c);
                        }
                    }
                }
                // The slot, sunk.
                window.paint_quad(
                    fill(
                        Bounds::new(
                            point(px(x0 + slot_x - 2.5), px(y0 + cap / 2.0 - 2.0)),
                            size(px(5.), px(travel + 4.0)),
                        ),
                        rgb(WELL),
                    )
                    .corner_radii(px(2.5))
                    .border_widths(px(1.))
                    .border_color(rgb(LINE)),
                );
                // How far the pitch is pulled from the middle.
                if tempo && !zero {
                    let (a, z) = (cap / 2.0 + travel / 2.0, top + cap / 2.0);
                    q(window, slot_x - 1.0, a.min(z), 2.0, (a - z).abs(), ACCENT);
                }
                if let Some(g) = ghost {
                    q(
                        window,
                        label_w,
                        cap / 2.0 + g * travel - 1.0,
                        7.0,
                        2.0,
                        ACCENT_TEXT,
                    );
                }
            },
        )
        .absolute()
        .inset_0();
        div()
            .relative()
            .w(px(w))
            .h(px(h))
            .flex_none()
            .cursor(gpui::CursorStyle::ResizeUpDown)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    live.press_ctl(ctl, e, cx)
                }),
            )
            .on_scroll_wheel(
                cx.listener(move |live, e: &ScrollWheelEvent, _, cx| live.wheel_ctl(ctl, e, cx)),
            )
            .child(scale)
            .children(labels.into_iter().map(|(f, t)| {
                let lit = zero && t == "0";
                div()
                    .absolute()
                    .left_0()
                    .w(px(label_w))
                    .top(px(cap / 2.0 + f * travel - 5.0))
                    .h(px(10.))
                    .line_height(px(10.))
                    .flex()
                    .justify_end()
                    .font_family(MONO)
                    .text_size(px(7.))
                    .text_color(rgb(if lit { MINT } else { TEXT_4 }))
                    .child(t)
            }))
            // The cap: lit from above, ridged to grip, its line across where it's set.
            .child(
                div()
                    .absolute()
                    .left(px(slot_x - cap_w / 2.0))
                    .top(px(top))
                    .w(px(cap_w))
                    .h(px(cap))
                    .rounded(px(3.))
                    .bg(gpui::linear_gradient(
                        180.0,
                        gpui::linear_color_stop(rgb(0x41414b), 0.0),
                        gpui::linear_color_stop(rgb(0x26262d), 1.0),
                    ))
                    .border_1()
                    .border_color(rgb(0x52525e))
                    .shadow_md()
                    .children([3.0, 11.0].map(|y| {
                        div()
                            .absolute()
                            .left(px(4.))
                            .right(px(4.))
                            .top(px(y - 1.0))
                            .h(px(1.))
                            .bg(alpha(0xffffff, 0.09))
                    }))
                    .child(
                        div()
                            .absolute()
                            .left(px(2.))
                            .right(px(2.))
                            .top(px(cap / 2.0 - 1.5))
                            .h(px(1.))
                            .bg(rgb(if zero { MINT } else { TEXT })),
                    ),
            )
    }
}

/// A performance pad: lit in `color` when `on`, its label in the middle.
pub fn pad(label: impl Into<gpui::SharedString>, color: Option<u32>, on: bool, h: f32) -> Div {
    pad_with(label, None, color, on, h)
}

/// `c` darkened to `k` of itself.
fn shade(c: u32, k: f32) -> u32 {
    let ch = |shift: u32| ((((c >> shift) & 0xff) as f32 * k).round() as u32).min(255);
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// A performance pad, as a controller's rubber pads are: lit from within. Set (it has a
/// `color`), its edge glows dimly and an LED strip along its foot is lit; playing or held
/// (`on`), it's lit right through with a glow round it. Its label in the middle, a detail (a hot
/// cue's time) in its corner.
pub fn pad_with(
    label: impl Into<gpui::SharedString>,
    detail: Option<String>,
    color: Option<u32>,
    on: bool,
    h: f32,
) -> Div {
    let (top, bottom) = match (color, on) {
        (Some(c), true) => (c, shade(c, 0.72)),
        (None, true) => (0x3c3c46, 0x2c2c34),
        _ => (0x26262c, 0x1a1a1f),
    };
    let border: gpui::Hsla = match (color, on) {
        (Some(c), true) => rgb(c).into(),
        (Some(c), false) => alpha(c, 0.4),
        (None, true) => rgb(0x5a5a66).into(),
        (None, false) => rgb(0x303038).into(),
    };
    let fg = match (color, on) {
        (Some(_), true) => 0x0c0c0f,
        (Some(c), false) => c,
        (None, true) => TEXT,
        (None, false) => TEXT_3,
    };
    div()
        .relative()
        .flex_1()
        .min_w(px(0.))
        .h(px(h))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .border_1()
        .border_color(border)
        .bg(gpui::linear_gradient(
            180.0,
            gpui::linear_color_stop(rgb(top), 0.0),
            gpui::linear_color_stop(rgb(bottom), 1.0),
        ))
        .text_color(rgb(fg))
        .text_size(px(if h >= 40.0 { 13. } else { 11. }))
        .font_family(MONO)
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .cursor_pointer()
        .when_some(color.filter(|_| on), |d, c| {
            d.shadow(vec![gpui::BoxShadow {
                color: alpha(c, 0.45),
                offset: point(px(0.), px(0.)),
                blur_radius: px(14.),
                spread_radius: px(0.),
                inset: false,
            }])
        })
        .when(!on, |d| d.hover(|s| s.border_color(rgb(0x4a4a55))))
        // Set and waiting: the LED strip along its foot.
        .when_some(color.filter(|_| !on && h >= 26.0), |d, c| {
            d.child(
                div()
                    .absolute()
                    .bottom(px(4.))
                    .left(px(8.))
                    .right(px(8.))
                    .h(px(2.))
                    .rounded(px(1.))
                    .bg(rgb(c)),
            )
        })
        .when_some(detail.filter(|_| h >= 34.0), |d, t| {
            d.child(
                div()
                    .absolute()
                    .top(px(4.))
                    .right(px(6.))
                    .text_size(px(8.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(match on {
                        true => alpha(0x0c0c0f, 0.75),
                        false => alpha(fg, 0.75),
                    })
                    .child(t),
            )
        })
        .child(label.into())
}
