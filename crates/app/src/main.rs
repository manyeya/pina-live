//! Pina Live: DJ software on Pina Studio's engine.

// A Windows release opens no console window behind it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod analysis;
mod browser;
mod controls;
mod crates;
mod decks;
mod detail;
mod fxbar;
mod hw;
mod keys;
mod layout;
mod live;
mod midi;
mod mixer;
mod models;
mod probe;
mod settings;
mod stems;
#[cfg(test)]
mod tests;
mod top;
mod ui;
mod wave;
#[cfg(target_family = "wasm")]
mod web;

use gpui::{App, AppContext, Bounds, Menu, MenuItem, WindowBounds, WindowOptions, px, size};

use live::*;

#[cfg(not(target_family = "wasm"))]
fn main() {
    run(gpui_platform::application());
}

/// In a browser, wasm-bindgen starts it (its `main` isn't run there).
#[cfg(target_family = "wasm")]
fn main() {}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    // In a Web Worker (web/worker.js) there's no page: it only answers what's asked (web.rs).
    if web_sys::window().is_none() {
        return;
    }
    gpui_platform::web_init();
    web::listen();
    run(gpui_platform::single_threaded_web());
}

fn run(app: gpui::Application) {
    app.run(move |cx: &mut App| {
        if let Err(e) = cx.text_system().add_fonts(ui::theme::fonts()) {
            eprintln!("fonts: {e}");
        }
        ui::text::bind_keys(cx);
        live::bind_keys(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus([
            Menu::new("Pina Live").items([MenuItem::action("Quit Pina Live", Quit)]),
            Menu::new("Library").items([
                MenuItem::action("Add Music Folder…", AddFolder),
                MenuItem::action("Find", FocusSearch),
            ]),
            Menu::new("View").items([
                MenuItem::action("Two or Four Decks", ToggleFour),
                MenuItem::action("Zoom In", ZoomIn),
                MenuItem::action("Zoom Out", ZoomOut),
            ]),
        ]);
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(MIN_W), px(MIN_H))),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| cx.new(|cx| Live::start(window, cx)))
            .unwrap();
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
