//! What's turned, slid and held: each knob and fader's reach and what it sets, dragging them
//! (shift for fine), the platter and waveform under the pointer (scratching), and buttons that
//! act while held (cue, rolls, censor), let go wherever the pointer ends up.

use gpui::{Context, Div, MouseButton, MouseDownEvent, Pixels, Point, div, prelude::*, rgb};
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
                ctl.set(self, value + moved / ctl.travel() * (hi - lo) * scale);
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

    /// A knob with its caption above: dragged up and down, double-clicked home.
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
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(1.))
            .when(!caption.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(9.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT_4))
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
                    .child(knob(
                        shown,
                        size,
                        if on { color } else { TEXT_3 },
                        ctl.bipolar(),
                    )),
            )
    }

    /// Whether a knob's moved from home (lit), or at home (quiet).
    fn mix_lit(&self, ctl: Ctl) -> bool {
        let (_, _, home) = ctl.range();
        (self.ctl_value(ctl) - home).abs() > 0.01
    }

    /// A vertical fader `h` tall: its cap where it's set, dragged anywhere on it.
    pub fn fader(&self, ctl: Ctl, h: f32, w: f32, cx: &mut Context<Self>) -> Div {
        let (lo, hi, _) = ctl.range();
        let v = (self.ctl_value(ctl) - lo) / (hi - lo);
        let tempo = matches!(ctl, Ctl::Tempo(_));
        // The pitch fader's faster end is toward you (its bottom).
        let at = if tempo { v } else { 1.0 - v };
        let cap = 14.0;
        let top = at * (h - cap);
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
            // Its slot, and (the pitch fader's) the middle marked.
            .child(
                div()
                    .absolute()
                    .left(px(w / 2.0 - 2.0))
                    .top(px(cap / 2.0))
                    .w(px(4.))
                    .h(px(h - cap))
                    .rounded(px(2.))
                    .bg(rgb(WELL))
                    .border_1()
                    .border_color(rgb(LINE)),
            )
            .when(tempo, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(2.))
                        .top(px(h / 2.0 - 0.5))
                        .w(px(w - 4.0))
                        .h(px(1.))
                        .bg(rgb(LINE_STRONG)),
                )
            })
            .child(
                div()
                    .absolute()
                    .left(px(1.))
                    .top(px(top))
                    .w(px(w - 2.0))
                    .h(px(cap))
                    .rounded(px(3.))
                    .bg(rgb(RAISED))
                    .border_1()
                    .border_color(rgb(LINE_STRONG))
                    .child(
                        div()
                            .absolute()
                            .left(px(3.))
                            .top(px(cap / 2.0 - 1.5))
                            .w(px(w - 10.0))
                            .h(px(1.))
                            .bg(rgb(TEXT_2)),
                    ),
            )
    }
}

/// A pad: lit in `color` when `on`, its label in the middle.
pub fn pad(label: impl Into<gpui::SharedString>, color: Option<u32>, on: bool, h: f32) -> Div {
    let (bg, fg, border) = match (color, on) {
        (Some(c), true) => (c, 0x0c0c0f, c),
        (Some(c), false) => (CARD, c, LINE),
        (None, true) => (RAISED, TEXT, LINE_STRONG),
        (None, false) => (CARD, TEXT_3, LINE),
    };
    div()
        .flex_1()
        .min_w(px(0.))
        .h(px(h))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .border_1()
        .border_color(rgb(border))
        .bg(rgb(bg))
        .text_color(rgb(fg))
        .text_size(px(11.))
        .font_family(MONO)
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .cursor_pointer()
        .when(!on, |d| d.hover(|s| s.border_color(rgb(LINE_STRONG))))
        .child(label.into())
}
