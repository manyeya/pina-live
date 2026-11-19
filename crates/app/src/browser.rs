//! The library: where to look on the left (every track, what's new, the history, each folder and
//! crate), a search that takes words and `bpm:` and `key:` together, and the tracks in rows, a
//! list that stays quick at a hundred thousand. A track's row says where it's loaded, whether
//! it's been played tonight, and whether its key mixes with the deck that's leading.

use gpui::{
    Context, Div, Entity, ExternalPaths, Focusable, FontWeight, MouseButton, MouseDownEvent,
    UniformListScrollHandle, Window, div, prelude::*, rgb, uniform_list,
};
use pina_live_engine::{
    analyze,
    library::{Library, Query, Track},
};

use crate::{
    decks::{clock, decks_with},
    live::{DECK_COLORS, DECK_NAMES, Live},
    ui::{scale::px, text::TextInput, theme::*},
};

pub const ROW_H: f32 = 27.0;

/// A track being dragged out of the library: what it is, drawn under the pointer.
#[derive(Clone)]
pub struct TrackDrag {
    pub id: u64,
    pub title: String,
    pub detail: String,
}

impl gpui::Render for TrackDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .py(px(7.))
            .rounded(px(8.))
            .bg(alpha(FIELD, 0.96))
            .border_1()
            .border_color(rgb(LINE_STRONG))
            .shadow_lg()
            .font_family(SANS)
            .text_size(px(12.))
            .text_color(rgb(TEXT))
            .child(icon(Icon::Wave, 14.).text_color(rgb(TEXT_3)))
            .child(self.title.clone())
            .child(
                div()
                    .font_family(MONO)
                    .text_size(px(11.))
                    .text_color(rgb(TEXT_3))
                    .child(self.detail.clone()),
            )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    All,
    Recent,
    History,
    Folder(usize),
    Crate(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sort {
    Added,
    Title,
    Artist,
    Bpm,
    Key,
    Time,
    Genre,
    Plays,
}

pub struct Browser {
    pub search: Entity<TextInput>,
    text: String,
    pub source: Source,
    pub sort: Sort,
    pub desc: bool,
    /// The tracks shown, as indices into the library.
    pub rows: Vec<usize>,
    pub selected: Option<u64>,
    pub list: UniformListScrollHandle,
    /// Only what mixes with the leading deck: its tempo near (or half or double), its key's.
    pub matching: bool,
    /// What matching matched against last: the tempo and the key.
    pub near: Option<(f64, String)>,
    /// Something to say over the list for a moment (a preview with no headphones set up).
    pub note: Option<String>,
    pub heads_at: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
}

impl Browser {
    pub fn new(_: &mut Window, cx: &mut Context<Live>) -> Browser {
        Browser {
            search: cx.new(|cx| TextInput::search("Search   bpm:120-128   key:~8A", cx)),
            text: String::new(),
            source: Source::All,
            sort: Sort::Added,
            desc: true,
            rows: Vec::new(),
            selected: None,
            list: UniformListScrollHandle::new(),
            matching: false,
            near: None,
            note: None,
            heads_at: Default::default(),
        }
    }

    pub fn focus_search(&self, window: &mut Window, cx: &mut Context<Live>) {
        let h = self.search.read(cx).focus_handle(cx);
        window.focus(&h, cx);
    }

    /// The rows worked out again: the source, the search, the sort.
    pub fn refresh(&mut self, lib: &Library) {
        let q = Query::parse(&self.text);
        let month = pina_live_engine::library::now().saturating_sub(30 * 24 * 3600);
        let mut rows: Vec<usize> = match &self.source {
            Source::History => {
                let mut seen = std::collections::HashSet::new();
                lib.history
                    .iter()
                    .rev()
                    .filter(|p| seen.insert(p.track))
                    .filter_map(|p| lib.tracks.iter().position(|t| t.id == p.track))
                    .collect()
            }
            Source::Crate(c) => lib.crates.get(*c).map_or(Vec::new(), |c| {
                c.tracks
                    .iter()
                    .filter_map(|id| lib.tracks.iter().position(|t| t.id == *id))
                    .collect()
            }),
            Source::Folder(f) => match lib.folders.get(*f) {
                Some(dir) => (0..lib.tracks.len())
                    .filter(|&i| lib.tracks[i].path.starts_with(dir))
                    .collect(),
                None => Vec::new(),
            },
            Source::Recent => (0..lib.tracks.len())
                .filter(|&i| lib.tracks[i].added >= month)
                .collect(),
            Source::All => (0..lib.tracks.len()).collect(),
        };
        rows.retain(|&i| lib.tracks[i].matches(&q));
        if self.matching
            && let Some((bpm, key)) = &self.near
        {
            rows.retain(|&i| mixes_with(&lib.tracks[i], *bpm, key));
        }
        // The history keeps its order (latest first); everything else sorts.
        if self.source != Source::History {
            let t = &lib.tracks;
            let key = |i: usize| -> (f64, String) {
                let x = &t[i];
                match self.sort {
                    Sort::Added => (x.added as f64, String::new()),
                    Sort::Title => (0.0, x.tags.title.to_lowercase()),
                    Sort::Artist => (0.0, x.tags.artist.to_lowercase()),
                    Sort::Bpm => (x.bpm().unwrap_or(0.0), String::new()),
                    Sort::Key => (camelot_order(x.camelot()), String::new()),
                    Sort::Time => (x.seconds as f64, String::new()),
                    Sort::Genre => (0.0, x.tags.genre.to_lowercase()),
                    Sort::Plays => (x.plays as f64, String::new()),
                }
            };
            rows.sort_by(|&a, &b| {
                let o = key(a)
                    .partial_cmp(&key(b))
                    .unwrap_or(std::cmp::Ordering::Equal);
                if self.desc { o.reverse() } else { o }
            });
        }
        self.rows = rows;
    }
}

/// Whether a track mixes with what's leading: its tempo within 6% (or at half or double), and its
/// key on the wheel next to the leader's (when both are known).
pub fn mixes_with(t: &Track, bpm: f64, key: &str) -> bool {
    let tempo = t.bpm().is_some_and(|b| {
        [0.5, 1.0, 2.0]
            .iter()
            .any(|m| ((b * m) / bpm - 1.0).abs() < 0.06)
    });
    let k = t.camelot();
    tempo && (key.is_empty() || k.is_empty() || analyze::compatible(k, key))
}

/// A key's place on the Camelot wheel as a hue (its neighbours, the keys that mix with it,
/// the hues next to it), the major (B) a shade lighter than the minor (A).
pub fn key_hue(k: &str) -> Option<gpui::Hsla> {
    let i = k.len().checked_sub(1)?;
    let (n, l) = k.split_at(i);
    let n: f32 = n.parse().ok().filter(|n| (1.0..=12.0).contains(n))?;
    Some(gpui::hsla(
        (n - 1.0) / 12.0,
        0.62,
        if l == "B" { 0.68 } else { 0.56 },
        1.0,
    ))
}

/// Keys round the wheel in order: 1A, 1B, 2A … 12B; no key last.
fn camelot_order(k: &str) -> f64 {
    let Some((n, l)) = k.len().checked_sub(1).map(|i| k.split_at(i)) else {
        return 99.0;
    };
    n.parse::<f64>()
        .map_or(99.0, |n| n * 2.0 + if l == "B" { 1.0 } else { 0.0 })
}

impl Live {
    pub fn browser_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // Typing searches as it goes.
        let typed = self.browser.search.read(cx).text().to_string();
        let near = self.leader().and_then(|l| {
            let s = self.status().decks[l];
            (s.bpm > 0.0).then(|| {
                let key = self.decks[l]
                    .analysis
                    .as_ref()
                    .map_or(String::new(), |a| a.camelot.clone());
                ((s.bpm * 10.0).round() / 10.0, key)
            })
        });
        if typed != self.browser.text || (self.browser.matching && near != self.browser.near) {
            self.browser.text = typed;
            self.browser.near = near;
            self.browser.refresh(&self.lib);
        }
        let _ = window;
        div()
            .id("browser")
            .relative()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .child(crate::probe::probe(&self.probes, "lib"))
            .border_t_1()
            .border_color(rgb(LINE))
            .on_drop(
                cx.listener(|live, paths: &ExternalPaths, _, cx| live.dropped(None, paths, cx)),
            )
            .child(self.sidebar(cx))
            .child(self.grip(crate::layout::Edge::Side, cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(self.search_bar(cx))
                    .child(self.column_heads(cx))
                    .child(self.track_list(cx)),
            )
    }

    fn side_row(
        &self,
        icon_: Icon,
        label: String,
        count: Option<usize>,
        on: bool,
        src: Source,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .h(px(26.))
            .mx(px(6.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(6.))
            .cursor_pointer()
            .bg(rgb(if on { RAISED } else { HEADERS }))
            .text_color(rgb(if on { TEXT } else { TEXT_2 }))
            .hover(|s| s.bg(rgb(if on { RAISED } else { CARD })))
            .child(icon(icon_, 14.).text_color(rgb(if on { TEXT } else { TEXT_3 })))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(label),
            )
            .when_some(count, |d, n| {
                d.child(
                    div()
                        .font_family(MONO)
                        .text_size(px(10.5))
                        .text_color(rgb(TEXT_4))
                        .child(n.to_string()),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    live.browser.source = src.clone();
                    live.browser.refresh(&live.lib);
                    live.browser
                        .list
                        .scroll_to_item(0, gpui::ScrollStrategy::Top);
                    cx.notify();
                }),
            )
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let src = self.browser.source.clone();
        let lib = &self.lib;
        let heading = |s: &'static str| {
            div()
                .px(px(14.))
                .pt(px(12.))
                .pb(px(4.))
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT_4))
                .child(s)
        };
        let mut col = div()
            .id("sidebar")
            .w(px(self.settings.layout.side))
            .flex_none()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .bg(rgb(HEADERS))
            .border_r_1()
            .border_color(rgb(LINE))
            .pt(px(4.))
            .child(heading("LIBRARY"))
            .child(self.side_row(
                Icon::List,
                "All tracks".into(),
                Some(lib.tracks.len()),
                src == Source::All,
                Source::All,
                cx,
            ))
            .child(self.side_row(
                Icon::Plus,
                "Added this month".into(),
                None,
                src == Source::Recent,
                Source::Recent,
                cx,
            ))
            .child(self.side_row(
                Icon::Cycle,
                "History".into(),
                None,
                src == Source::History,
                Source::History,
                cx,
            ))
            .child(heading("FOLDERS"));
        for (k, f) in lib.folders.iter().enumerate() {
            let name = f.file_name().map_or(f.display().to_string(), |n| {
                n.to_string_lossy().into_owned()
            });
            col = col.child(self.side_row(
                Icon::Folder,
                name,
                None,
                src == Source::Folder(k),
                Source::Folder(k),
                cx,
            ));
        }
        col = col.child(
            div()
                .h(px(26.))
                .mx(px(6.))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(6.))
                .cursor_pointer()
                .text_color(rgb(TEXT_3))
                .hover(|s| s.bg(rgb(CARD)).text_color(rgb(TEXT_2)))
                .child(icon(Icon::Plus, 14.))
                .child("Add a folder…")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(crate::live::AddFolder), cx);
                    }),
                ),
        );
        col = col.child(heading("CRATES"));
        if lib.crates.is_empty() {
            col = col.child(
                div()
                    .px(px(14.))
                    .text_size(px(11.))
                    .text_color(rgb(TEXT_4))
                    .child("Right-click a track to start one."),
            );
        }
        for (k, c) in lib.crates.iter().enumerate() {
            let row = match self.renaming_field(k) {
                Some(field) => div()
                    .h(px(26.))
                    .mx(px(6.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(6.))
                    .bg(rgb(FIELD))
                    .child(icon(Icon::Stack, 14.).text_color(rgb(TEXT_3)))
                    .child(div().flex_1().child(field)),
                None => self
                    .side_row(
                        Icon::Stack,
                        c.name.clone(),
                        Some(c.tracks.len()),
                        src == Source::Crate(k),
                        Source::Crate(k),
                        cx,
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                            live.open_menu(crate::crates::Menu::Crate(k), e.position, cx)
                        }),
                    ),
            };
            col = col.child(row);
        }
        col
    }

    fn search_bar(&self, cx: &mut Context<Self>) -> Div {
        let analysing = self.analyzer.busy();
        let n = self.browser.rows.len();
        let full = self.settings.layout.library == crate::layout::LibView::Full;
        div()
            .relative()
            .h(px(42.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .child(crate::probe::probe(&self.probes, "lib.search"))
            .border_b_1()
            .border_color(rgb(LINE_SOFT))
            .child(
                div()
                    .w(px(360.))
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .px(px(9.))
                    .rounded(px(7.))
                    .bg(rgb(CARD))
                    .border_1()
                    .border_color(rgb(LINE))
                    .child(icon(Icon::Search, 14.).text_color(rgb(TEXT_3)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(self.browser.search.clone()),
                    ),
            )
            .child(self.match_chip(cx))
            .child(
                div()
                    .font_family(MONO)
                    .text_size(px(11.))
                    .text_color(rgb(TEXT_3))
                    .child(format!("{n} track{}", if n == 1 { "" } else { "s" })),
            )
            .when_some(self.browser.note.clone(), |d, note| {
                d.child(div().text_size(px(11.)).text_color(rgb(AMBER)).child(note))
            })
            .child(div().flex_1())
            .when(analysing, |d| d.child(self.analysis_progress(cx)))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(TEXT_4))
                    .min_w(px(0.))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child("Drag a track onto a deck · double-click loads the free one"),
            )
            // The library as big as the window, or shut (⌘⇧L, ⌘L).
            .child(icon_seg(Icon::Expand, full, 26.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, _, _, cx| {
                    let v = match full {
                        true => crate::layout::LibView::Open,
                        false => crate::layout::LibView::Full,
                    };
                    live.set_library(v, cx)
                }),
            ))
            .child(icon_seg(Icon::Close, false, 26.0).on_mouse_down(
                MouseButton::Left,
                cx.listener(|live, _, _, cx| live.set_library(crate::layout::LibView::Closed, cx)),
            ))
            .child(crate::probe::mark(&self.probes, "lib.search.end"))
    }

    /// Match: only what mixes with the leading deck, said against what.
    fn match_chip(&self, cx: &mut Context<Self>) -> Div {
        let on = self.browser.matching;
        let label = match self.leader() {
            Some(l) => format!("Match {}", crate::live::DECK_NAMES[l]),
            None => "Match".into(),
        };
        div()
            .h(px(26.))
            .px(px(9.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(6.))
            .border_1()
            .cursor_pointer()
            .font_weight(FontWeight::MEDIUM)
            .border_color(if on {
                alpha(ACCENT, 0.45)
            } else {
                rgb(LINE).into()
            })
            .when(on, |d| d.bg(alpha(ACCENT, 0.12)))
            .text_color(rgb(if on { ACCENT_TEXT } else { TEXT_3 }))
            .hover(|s| s.text_color(rgb(TEXT_2)))
            .child(icon(Icon::Pulse, 13.).text_color(rgb(if on { ACCENT_TEXT } else { TEXT_3 })))
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|live, _, _, cx| {
                    live.browser.matching = !live.browser.matching;
                    live.browser.near = None;
                    live.browser.refresh(&live.lib);
                    cx.notify();
                }),
            )
    }

    /// Analysis at work: how far, and Stop.
    fn analysis_progress(&self, cx: &mut Context<Self>) -> Div {
        let (done, total) = (self.analyzer.done, self.analyzer.total.max(1));
        let t = done as f32 / total as f32;
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(TEXT_2))
                    .child(format!("Analysing {} of {}", (done + 1).min(total), total)),
            )
            .child(
                div()
                    .w(px(90.))
                    .h(px(4.))
                    .rounded(px(2.))
                    .bg(rgb(RAISED))
                    .child(
                        div()
                            .h_full()
                            .w(px(90.0 * t))
                            .rounded(px(2.))
                            .bg(rgb(ACCENT)),
                    ),
            )
            .child(button("Stop", false).h(px(22.)).on_mouse_down(
                MouseButton::Left,
                cx.listener(|live, _, _, cx| {
                    live.analyzer.stop();
                    cx.notify();
                }),
            ))
    }

    fn column_heads(&self, cx: &mut Context<Self>) -> Div {
        let (sort, desc) = (self.browser.sort, self.browser.desc);
        let head =
            |label: &'static str, s: Sort, w: Option<f32>, right: bool, cx: &mut Context<Self>| {
                let on = sort == s;
                div()
                    .when_some(w, |d, w| d.w(px(w)).flex_none())
                    .when(w.is_none(), |d| d.flex_1().min_w(px(0.)))
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .when(right, |d| d.justify_end())
                    .cursor_pointer()
                    .text_color(rgb(if on { TEXT } else { TEXT_3 }))
                    .hover(|st| st.text_color(rgb(TEXT_2)))
                    .child(label)
                    .when(on, |d| d.child(if desc { "↓" } else { "↑" }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            let b = &mut live.browser;
                            if b.sort == s {
                                b.desc = !b.desc;
                            } else {
                                b.sort = s;
                                b.desc = matches!(s, Sort::Added | Sort::Plays);
                            }
                            live.browser.refresh(&live.lib);
                            cx.notify();
                        }),
                    )
            };
        let at = self.browser.heads_at.clone();
        div()
            .h(px(26.))
            .flex_none()
            .relative()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .text_size(px(10.5))
            // Where the heads are, for the rows under them (tests find a row by it).
            .child(
                gpui::canvas(move |b, _, _| at.set(b), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .font_weight(FontWeight::SEMIBOLD)
            .border_b_1()
            .border_color(rgb(LINE_SOFT))
            .child(div().w(px(20.)).flex_none())
            .child(div().w(px(30.)).flex_none())
            .child(head("TITLE", Sort::Title, None, false, cx))
            .child(head("ARTIST", Sort::Artist, Some(200.0), false, cx))
            .child(head("BPM", Sort::Bpm, Some(50.0), true, cx))
            .child(head("KEY", Sort::Key, Some(40.0), true, cx))
            .child(head("TIME", Sort::Time, Some(44.0), true, cx))
            .child(head("GENRE", Sort::Genre, Some(110.0), false, cx))
            .child(head("PLAYS", Sort::Plays, Some(40.0), true, cx))
            .child(div().w(px(52.)).flex_none())
    }

    fn track_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let n = self.browser.rows.len();
        if self.lib.tracks.is_empty() {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Your music goes here"))
                .child(
                    div()
                        .text_color(rgb(TEXT_3))
                        .child("Add the folders you keep it in: every track's tempo, key and phrases are found as it comes in."),
                )
                .child(
                    button("Add a folder…", true).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, window, cx| window.dispatch_action(Box::new(crate::live::AddFolder), cx)),
                    ),
                )
                .child(div().text_size(px(11.)).text_color(rgb(TEXT_4)).child("or drop folders and files anywhere here"))
                .into_any_element();
        }
        if n == 0 {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(TEXT_3))
                .child("Nothing here matches")
                .into_any_element();
        }
        uniform_list(
            "tracks",
            n,
            cx.processor(|live, range: std::ops::Range<usize>, _, cx| {
                range.map(|k| live.track_row(k, cx)).collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.browser.list)
        .flex_1()
        .into_any_element()
    }

    fn track_row(&self, k: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t: &Track = &self.lib.tracks[self.browser.rows[k]];
        let id = t.id;
        let on = self.browser.selected == Some(id);
        let played = self
            .lib
            .history
            .iter()
            .rev()
            .take(200)
            .any(|p| p.track == id);
        let on_decks = decks_with(self, id);
        let pending = self.analyzer.pending(id);
        let leading_key = self
            .leader()
            .and_then(|l| self.decks[l].analysis.as_ref())
            .map(|a| a.camelot.clone());
        let fits = leading_key
            .as_deref()
            .is_some_and(|k| analyze::compatible(t.camelot(), k));
        let mono = |s: String, w: f32, color: u32| {
            div()
                .w(px(w))
                .flex_none()
                .text_right()
                .font_family(MONO)
                .text_size(px(11.))
                .text_color(rgb(color))
                .child(s)
        };
        let shown = self.shown();
        let previewing = self.preview.as_ref().is_some_and(|(p, _)| *p == id);
        let drag = TrackDrag {
            id,
            title: t.tags.title.clone(),
            detail: match (t.bpm(), t.camelot()) {
                (Some(b), k) if !k.is_empty() => format!("{b:.1} · {k}"),
                (Some(b), _) => format!("{b:.1}"),
                _ => String::new(),
            },
        };
        div()
            .id(("row", k))
            .on_drag(drag, |d, _, _, cx| cx.new(|_| d.clone()))
            .w_full()
            .h(px(ROW_H))
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .border_b_1()
            .border_color(rgb(LINE_SOFT))
            .bg(rgb(if on { RAISED } else { BG }))
            .when(!on, |d| d.hover(|s| s.bg(rgb(CARD))))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                    live.browser.selected = Some(id);
                    live.open_menu(crate::crates::Menu::Track(id), e.position, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |live, e: &MouseDownEvent, _, cx| {
                    live.browser.selected = Some(id);
                    if e.click_count >= 2 {
                        let d = live.free_deck();
                        live.load(d, id, cx);
                    }
                    cx.notify();
                }),
            )
            // Heard in the headphones: from its first drop (or a quarter in).
            .child(
                div()
                    .size(px(20.))
                    .flex_none()
                    .rounded(px(10.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .when(previewing, |d| d.bg(alpha(ACCENT, 0.2)))
                    .hover(|s| s.bg(rgb(RAISED)))
                    .child(
                        icon(if previewing { Icon::Pause } else { Icon::Play }, 11.)
                            .text_color(rgb(if previewing { ACCENT_TEXT } else { TEXT_4 })),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |live, _, _, cx| {
                            cx.stop_propagation();
                            live.toggle_preview(id, cx);
                        }),
                    ),
            )
            .child(div().w(px(30.)).flex_none().flex().gap(px(2.)).children(
                on_decks.into_iter().map(|d| {
                    div()
                        .size(px(14.))
                        .rounded(px(3.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(alpha(DECK_COLORS[d], 0.2))
                        .text_color(rgb(DECK_COLORS[d]))
                        .text_size(px(9.))
                        .font_weight(FontWeight::BOLD)
                        .child(DECK_NAMES[d])
                }),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(rgb(if previewing {
                        ACCENT_TEXT
                    } else if played {
                        TEXT_3
                    } else {
                        TEXT
                    }))
                    .child(t.tags.title.clone()),
            )
            .child(
                div()
                    .w(px(200.))
                    .flex_none()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(rgb(TEXT_2))
                    .child(t.tags.artist.clone()),
            )
            .child(match (t.bpm(), pending, &t.error) {
                (Some(b), _, _) => mono(format!("{b:.1}"), 50.0, TEXT_2),
                (None, true, _) => mono("···".into(), 50.0, TEXT_4),
                (None, _, Some(_)) => div()
                    .w(px(50.))
                    .flex_none()
                    .flex()
                    .child(chip("Can't read", RECORD)),
                _ => mono(String::new(), 50.0, TEXT_4),
            })
            .child(
                div()
                    .w(px(40.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .children(
                        key_hue(t.camelot())
                            .map(|c| div().size(px(6.)).flex_none().rounded(px(3.)).bg(c)),
                    )
                    .child(mono(
                        t.camelot().to_string(),
                        28.0,
                        if fits { ACCENT_TEXT } else { TEXT_2 },
                    )),
            )
            .child(mono(
                if t.seconds > 0.0 {
                    clock(t.seconds as f64, false)
                } else {
                    String::new()
                },
                44.0,
                TEXT_3,
            ))
            .child(
                div()
                    .w(px(110.))
                    .flex_none()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(rgb(TEXT_3))
                    .child(t.tags.genre.clone()),
            )
            .child(mono(
                if t.plays > 0 {
                    t.plays.to_string()
                } else {
                    String::new()
                },
                40.0,
                TEXT_4,
            ))
            .child(
                div()
                    .w(px(52.))
                    .flex_none()
                    .flex()
                    .gap(px(4.))
                    .justify_end()
                    .children((0..shown).map(|d| {
                        let deck = [0usize, 1, 2, 3][d];
                        div()
                            .size(px(20.))
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(9.5))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(TEXT_4))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(RAISED)).text_color(rgb(DECK_COLORS[deck])))
                            .child(DECK_NAMES[deck])
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |live, _, _, cx| {
                                    cx.stop_propagation();
                                    live.browser.selected = Some(id);
                                    live.load(deck, id, cx);
                                }),
                            )
                    })),
            )
    }

    /// The selected track onto `deck`.
    pub fn load_selected(&mut self, deck: usize, cx: &mut Context<Self>) {
        if let Some(id) = self.browser.selected {
            self.load(deck, id, cx);
        }
    }

    /// The selection moved up or down the list.
    pub fn browse_by(&mut self, by: isize, cx: &mut Context<Self>) {
        let rows = &self.browser.rows;
        if rows.is_empty() {
            return;
        }
        let at = self
            .browser
            .selected
            .and_then(|id| rows.iter().position(|&i| self.lib.tracks[i].id == id));
        let next = match at {
            Some(k) => (k as isize + by).clamp(0, rows.len() as isize - 1) as usize,
            None => 0,
        };
        self.browser.selected = Some(self.lib.tracks[rows[next]].id);
        self.browser
            .list
            .scroll_to_item(next, gpui::ScrollStrategy::Center);
        cx.notify();
    }
}
