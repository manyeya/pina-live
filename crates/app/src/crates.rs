//! Crates: a track's right-click menu puts it in one (or starts one with it, or takes it out of
//! the one shown); a crate's renames it in place or deletes it.

use gpui::{
    Context, Div, Entity, Focusable, FontWeight, MouseButton, Pixels, Point, Window, anchored,
    deferred, div, prelude::*, rgb,
};
use pina_live_engine::library::Crate;

use crate::{
    browser::Source,
    live::Live,
    ui::{
        scale::px,
        text::{TextEvent, TextInput},
        theme::*,
    },
};

/// An open right-click menu: on a track, or on a crate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Menu {
    Track(u64),
    Crate(usize),
}

impl Live {
    pub fn open_menu(&mut self, m: Menu, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.menu = Some((m, at));
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
    }

    fn crates_changed(&mut self) {
        self.mark_dirty();
        self.browser.refresh(&self.lib);
    }

    pub fn start_rename(&mut self, k: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = self.lib.crates.get(k) else {
            return;
        };
        let input = cx.new(|cx| TextInput::field(&c.name, cx));
        cx.subscribe(&input, move |live, _, ev: &TextEvent, cx| {
            if let TextEvent::Confirm(name) = ev
                && let Some(c) = live.lib.crates.get_mut(k)
                && !name.trim().is_empty()
            {
                c.name = name.trim().to_string();
                live.crates_changed();
            }
            live.renaming = None;
            cx.notify();
        })
        .detach();
        let h = input.read(cx).focus_handle(cx);
        window.focus(&h, cx);
        self.renaming = Some((k, input));
        cx.notify();
    }

    pub fn renaming_field(&self, k: usize) -> Option<Entity<TextInput>> {
        self.renaming
            .as_ref()
            .filter(|(r, _)| *r == k)
            .map(|(_, e)| e.clone())
    }

    /// The open menu, drawn where it was opened.
    pub fn menu_overlay(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (m, at) = self.menu?;
        let item = |label: String,
                    cx: &mut Context<Self>,
                    f: Box<dyn Fn(&mut Live, &mut Window, &mut Context<Live>)>| {
            div()
                .h(px(26.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .cursor_pointer()
                .text_color(rgb(TEXT_2))
                .hover(|s| s.bg(rgb(RAISED)).text_color(rgb(TEXT)))
                .child(label)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |live, _, window, cx| {
                        cx.stop_propagation();
                        f(live, window, cx);
                        live.close_menu(cx);
                    }),
                )
        };
        let heading = |s: String| {
            div()
                .px(px(10.))
                .pt(px(4.))
                .pb(px(2.))
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT_4))
                .child(s)
        };
        let mut list: Div = div()
            .w(px(220.))
            .p(px(4.))
            .rounded(px(8.))
            .bg(rgb(CARD))
            .border_1()
            .border_color(rgb(LINE_STRONG))
            .shadow_lg()
            .text_size(px(12.))
            .on_mouse_down_out(cx.listener(|live, _, _, cx| live.close_menu(cx)));
        match m {
            Menu::Track(id) => {
                if !self.lib.crates.is_empty() {
                    list = list.child(heading("ADD TO".into()));
                }
                for (k, c) in self.lib.crates.iter().enumerate() {
                    let has = c.tracks.contains(&id);
                    list = list.child(item(
                        if has {
                            format!("{} ✓", c.name)
                        } else {
                            c.name.clone()
                        },
                        cx,
                        Box::new(move |live, _, _| {
                            if let Some(c) = live.lib.crates.get_mut(k)
                                && !c.tracks.contains(&id)
                            {
                                c.tracks.push(id);
                                live.crates_changed();
                            }
                        }),
                    ));
                }
                list = list.child(item(
                    "New crate with this track".into(),
                    cx,
                    Box::new(move |live, window, cx| {
                        let n = live.lib.crates.len() + 1;
                        live.lib.crates.push(Crate {
                            name: format!("Crate {n}"),
                            tracks: vec![id],
                        });
                        live.crates_changed();
                        live.start_rename(n - 1, window, cx);
                    }),
                ));
                if let Source::Crate(k) = self.browser.source {
                    list = list.child(item(
                        "Take out of this crate".into(),
                        cx,
                        Box::new(move |live, _, _| {
                            if let Some(c) = live.lib.crates.get_mut(k) {
                                c.tracks.retain(|t| *t != id);
                                live.crates_changed();
                            }
                        }),
                    ));
                }
            }
            Menu::Crate(k) => {
                list = list
                    .child(item(
                        "Rename".into(),
                        cx,
                        Box::new(move |live, window, cx| live.start_rename(k, window, cx)),
                    ))
                    .child(item(
                        "Delete crate".into(),
                        cx,
                        Box::new(move |live, _, _| {
                            if k < live.lib.crates.len() {
                                live.lib.crates.remove(k);
                                if matches!(live.browser.source, Source::Crate(_)) {
                                    live.browser.source = Source::All;
                                }
                                live.crates_changed();
                            }
                        }),
                    ));
            }
        }
        Some(deferred(anchored().position(at).snap_to_window().child(list)).with_priority(2))
    }
}
