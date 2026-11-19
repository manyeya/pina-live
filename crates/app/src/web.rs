//! Pina Live in a browser: the files it's given (dropped onto the page, or picked with "Add
//! music…"), read into memory for the library, since a browser has no folders to scan; and the
//! sound, started on the first click (a browser plays nothing before one).

use std::cell::RefCell;

use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;

thread_local! {
    /// Files read and not yet taken: each its name and bytes.
    static ARRIVED: RefCell<Vec<(String, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
}

/// The files read since last asked.
pub fn take_arrived() -> Vec<(String, Vec<u8>)> {
    ARRIVED.with(|a| std::mem::take(&mut *a.borrow_mut()))
}

/// A file read into memory, then kept for the library.
fn read(file: web_sys::File) {
    wasm_bindgen_futures::spawn_local(async move {
        let Ok(buffer) = JsFuture::from(file.array_buffer()).await else {
            return;
        };
        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
        ARRIVED.with(|a| a.borrow_mut().push((file.name(), bytes)));
    });
}

fn read_all(files: Option<web_sys::FileList>) {
    let Some(files) = files else { return };
    for i in 0..files.length() {
        if let Some(f) = files.get(i) {
            read(f);
        }
    }
}

/// Files dropped anywhere on the page, read. (GPUI's own drop handler keeps the browser from
/// opening them; it passes nothing on, since a browser gives files, not paths.)
pub fn listen() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let drop = Closure::<dyn FnMut(web_sys::DragEvent)>::new(|e: web_sys::DragEvent| {
        e.prevent_default();
        read_all(e.data_transfer().and_then(|d| d.files()));
    });
    let over = Closure::<dyn FnMut(web_sys::DragEvent)>::new(|e: web_sys::DragEvent| {
        e.prevent_default();
    });
    let _ = document.add_event_listener_with_callback("drop", drop.as_ref().unchecked_ref());
    let _ = document.add_event_listener_with_callback("dragover", over.as_ref().unchecked_ref());
    // Listening for as long as the page is open.
    drop.forget();
    over.forget();
}

/// The browser's file picker, for audio: what's picked, read.
pub fn pick() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Ok(input) = document.create_element("input") else {
        return;
    };
    let input: web_sys::HtmlInputElement = input.unchecked_into();
    input.set_type("file");
    input.set_multiple(true);
    input.set_accept("audio/*,.mp3,.wav,.flac,.aiff,.aif,.m4a,.ogg");
    let picked = input.clone();
    let change = Closure::<dyn FnMut()>::new(move || read_all(picked.files()));
    input.set_onchange(Some(change.as_ref().unchecked_ref()));
    change.forget();
    input.click();
}

/// Pina Studio's own songs, from beside the page, read as if they'd been dropped.
pub fn demos() {
    for name in ["Corner Store Blues.m4a", "Midnight Drive.m4a"] {
        wasm_bindgen_futures::spawn_local(async move {
            let Some(window) = web_sys::window() else {
                return;
            };
            let url = format!("demo/{name}");
            let Ok(resp) = JsFuture::from(window.fetch_with_str(&url)).await else {
                return;
            };
            let resp: web_sys::Response = resp.unchecked_into();
            let Ok(buffer) = resp.array_buffer() else {
                return;
            };
            let Ok(buffer) = JsFuture::from(buffer).await else {
                return;
            };
            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
            ARRIVED.with(|a| a.borrow_mut().push((name.to_string(), bytes)));
        });
    }
}
