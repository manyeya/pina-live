//! Probes: where the window's panels and the ends of their rows were laid out, by name, so the
//! integration tests can check nothing spills out of what holds it. A probe named "deck0.tempo"
//! must sit inside "deck0"; "deck0.transport.end" (the end of a row's content) inside
//! "deck0.transport". They cost a canvas each and nothing more.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gpui::{Bounds, IntoElement, Pixels, Styled, canvas};

pub type Probes = Rc<RefCell<HashMap<String, Bounds<Pixels>>>>;

/// Over the whole of what it's put in (absolutely, so it moves nothing): its bounds.
pub fn probe(p: &Probes, name: impl Into<String>) -> impl IntoElement {
    let (p, name) = (p.clone(), name.into());
    canvas(
        move |b, _, _| {
            p.borrow_mut().insert(name, b);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A point at the end of a row's or column's content: where it got to.
pub fn mark(p: &Probes, name: impl Into<String>) -> impl IntoElement {
    let (p, name) = (p.clone(), name.into());
    canvas(
        move |b, _, _| {
            p.borrow_mut().insert(name, b);
        },
        |_, _, _, _| {},
    )
    .flex_none()
    .w(gpui::px(0.))
    .h(gpui::px(0.))
}

#[cfg(all(test, target_os = "macos"))]
/// Every probe that's outside the one holding it (by name: "a.b.c" in "a.b", else "a"), by more
/// than half a pixel: what spills over, said.
pub fn spills(p: &HashMap<String, Bounds<Pixels>>) -> Vec<String> {
    let mut out = Vec::new();
    for (name, b) in p {
        let mut parent = name.as_str();
        let holder = loop {
            match parent.rsplit_once('.') {
                Some((up, _)) => {
                    if let Some(h) = p.get(up) {
                        break Some((up, h));
                    }
                    parent = up;
                }
                None => break None,
            }
        };
        let Some((up, h)) = holder else { continue };
        let f = |x: Pixels| f32::from(x);
        let (l, t, r, bt) = (
            f(b.origin.x),
            f(b.origin.y),
            f(b.origin.x + b.size.width),
            f(b.origin.y + b.size.height),
        );
        let (hl, ht, hr, hb) = (
            f(h.origin.x),
            f(h.origin.y),
            f(h.origin.x + h.size.width),
            f(h.origin.y + h.size.height),
        );
        let over = [hl - l, ht - t, r - hr, bt - hb]
            .into_iter()
            .fold(0f32, f32::max);
        if over > 0.5 {
            out.push(format!("{name} spills {over:.1}px out of {up}"));
        }
    }
    out.sort();
    out
}
