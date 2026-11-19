//! Pina Live: DJ software on Pina Studio's engine.

mod analysis;
mod browser;
mod controls;
mod decks;
mod fxbar;
mod keys;
mod live;
mod midi;
mod mixer;
mod settings;
mod stems;
#[cfg(test)]
mod tests;
mod top;
mod ui;
mod wave;

use gpui::{App, AppContext, Bounds, Menu, MenuItem, WindowBounds, WindowOptions, px, size};

use live::*;

fn main() {
    gpui_platform::application().run(move |cx: &mut App| {
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
