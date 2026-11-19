//! Pina Live in a browser: the files it's given (dropped onto the page, or picked with "Add
//! music…"), and the heavy work on them done in Web Workers, a browser's background threads. The
//! page has one thread, which draws, plays and listens: a track decoded or analysed on it stops
//! all three for seconds. Each worker (web/worker.js) runs this same module, keeps the files as
//! the browser gave them, and answers a request at a time: one decodes tracks for the decks, one
//! analyses them, so a deck never waits behind the library's analysis.
//!
//! The sound starts on the first click (a browser plays nothing before one).

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::{Path, PathBuf},
};

use js_sys::{Float32Array, Object, Promise, Reflect, Uint8Array};
use pina_live_engine::{
    analyze::{Analysis, Tags, Wave},
    audio::Audio,
    memfs,
};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, MessageEvent, Worker, WorkerOptions, WorkerType};

const DECODE: usize = 0;
const ANALYSE: usize = 1;

thread_local! {
    /// Files given and not yet taken into the library: their names.
    static ARRIVED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static WORKERS: [Worker; 2] = [worker(), worker()];
    /// Requests sent and not yet answered: each its promise's `resolve`.
    static WAITING: RefCell<HashMap<u32, js_sys::Function>> = RefCell::default();
    static NEXT: Cell<u32> = const { Cell::new(0) };
}

/// Where a file the browser gave is in the library: its name, under here.
pub fn path_of(name: &str) -> PathBuf {
    PathBuf::from("/Pina Live").join(name)
}

/// The names of the files given since last asked.
pub fn take_arrived() -> Vec<String> {
    ARRIVED.with(|a| std::mem::take(&mut *a.borrow_mut()))
}

/// A worker, its answers passed to whoever's waiting for them.
fn worker() -> Worker {
    let options = WorkerOptions::new();
    options.set_type(WorkerType::Module);
    let w = Worker::new_with_options("worker.js", &options).expect("a Web Worker");
    let answer = Closure::<dyn FnMut(MessageEvent)>::new(|e: MessageEvent| {
        let data = e.data();
        let id = Reflect::get(&data, &"id".into())
            .ok()
            .and_then(|v| v.as_f64());
        let resolve = id.and_then(|id| WAITING.with(|w| w.borrow_mut().remove(&(id as u32))));
        if let Some(resolve) = resolve {
            let _ = resolve.call1(&JsValue::NULL, &data);
        }
    });
    w.set_onmessage(Some(answer.as_ref().unchecked_ref()));
    answer.forget();
    // One that couldn't start (or died) answers everything waiting with why, so no deck waits
    // for ever.
    let failed = Closure::<dyn FnMut(web_sys::ErrorEvent)>::new(|e: web_sys::ErrorEvent| {
        let why = Object::new();
        let _ = Reflect::set(&why, &"err".into(), &e.message().into());
        let waiting: Vec<_> = WAITING.with(|w| w.borrow_mut().drain().collect());
        for (_, resolve) in waiting {
            let _ = resolve.call1(&JsValue::NULL, &why);
        }
    });
    w.set_onerror(Some(failed.as_ref().unchecked_ref()));
    failed.forget();
    w
}

fn message(op: &str, path: &Path) -> Object {
    let m = Object::new();
    let _ = Reflect::set(&m, &"op".into(), &op.into());
    let _ = Reflect::set(&m, &"path".into(), &path.to_string_lossy().as_ref().into());
    m
}

/// `msg` sent to a worker; its answer's `out`, or why there's none.
async fn ask(worker: usize, msg: Object) -> Result<JsValue, String> {
    let id = NEXT.with(|n| {
        n.set(n.get().wrapping_add(1));
        n.get()
    });
    let _ = Reflect::set(&msg, &"id".into(), &id.into());
    let answer = Promise::new(&mut |resolve, _| {
        WAITING.with(|w| w.borrow_mut().insert(id, resolve));
    });
    if let Err(e) = WORKERS.with(|w| w[worker].post_message(&msg)) {
        WAITING.with(|w| w.borrow_mut().remove(&id));
        return Err(format!("{e:?}"));
    }
    let data = JsFuture::from(answer).await.map_err(|e| format!("{e:?}"))?;
    match Reflect::get(&data, &"err".into()) {
        Ok(why) if !why.is_undefined() => Err(why.as_string().unwrap_or_default()),
        _ => Reflect::get(&data, &"out".into()).map_err(|e| format!("{e:?}")),
    }
}

/// The track at `path` decoded for a deck at `rate`, by a worker.
pub async fn decode(path: PathBuf, rate: f64) -> Result<Audio, String> {
    let msg = message("decode", &path);
    let _ = Reflect::set(&msg, &"rate".into(), &rate.into());
    let mut l = ask(DECODE, msg)
        .await?
        .unchecked_into::<Float32Array>()
        .to_vec();
    let r = l.split_off(l.len() / 2);
    Ok(Audio::new(l, r))
}

/// The track at `path` analysed, by a worker.
pub async fn analyze(path: PathBuf) -> Result<(Analysis, Tags, Wave), String> {
    let out = ask(ANALYSE, message("analyze", &path)).await?;
    pina_live_engine::analyze::unpack(&out.unchecked_into::<Uint8Array>().to_vec())
}

/// In a worker: `bytes`, the track at `path`, decoded at `rate`: its left side, then its right.
#[wasm_bindgen]
pub fn worker_decode(path: String, bytes: Vec<u8>, rate: f64) -> Result<Vec<f32>, String> {
    let path = PathBuf::from(path);
    memfs::put(&path, bytes);
    let audio = Audio::load(&path, rate);
    memfs::forget(&path);
    let Audio { mut l, r } = audio?;
    l.extend(r);
    Ok(l)
}

/// In a worker: `bytes`, the track at `path`, analysed (`analyze::pack`).
#[wasm_bindgen]
pub fn worker_analyze(path: String, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let path = PathBuf::from(path);
    memfs::put(&path, bytes);
    let found = pina_live_engine::analyze::analyze(&path);
    memfs::forget(&path);
    Ok(pina_live_engine::analyze::pack(&found?))
}

/// A file the browser gave: handed to both workers (who read it when asked to), and named for
/// the library.
fn arrived(name: String, file: &Blob) {
    let msg = message("put", &path_of(&name));
    let _ = Reflect::set(&msg, &"blob".into(), file);
    WORKERS.with(|w| {
        for w in w {
            let _ = w.post_message(&msg);
        }
    });
    ARRIVED.with(|a| a.borrow_mut().push(name));
}

fn arrived_all(files: Option<web_sys::FileList>) {
    let Some(files) = files else { return };
    for i in 0..files.length() {
        if let Some(f) = files.get(i) {
            arrived(f.name(), &f);
        }
    }
}

/// Files dropped anywhere on the page, taken. (GPUI's own drop handler keeps the browser from
/// opening them; it passes nothing on, since a browser gives files, not paths.)
pub fn listen() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let drop = Closure::<dyn FnMut(web_sys::DragEvent)>::new(|e: web_sys::DragEvent| {
        e.prevent_default();
        arrived_all(e.data_transfer().and_then(|d| d.files()));
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

/// The browser's file picker, for audio: what's picked, taken.
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
    let change = Closure::<dyn FnMut()>::new(move || arrived_all(picked.files()));
    input.set_onchange(Some(change.as_ref().unchecked_ref()));
    change.forget();
    input.click();
}

/// Pina Studio's own songs, from beside the page, taken as if they'd been dropped.
pub fn demos() {
    for name in ["Corner Store Blues.m4a", "Midnight Drive.m4a"] {
        wasm_bindgen_futures::spawn_local(async move {
            let Some(window) = web_sys::window() else {
                return;
            };
            let Ok(resp) = JsFuture::from(window.fetch_with_str(&format!("demo/{name}"))).await
            else {
                return;
            };
            let resp: web_sys::Response = resp.unchecked_into();
            let Ok(blob) = resp.blob() else { return };
            let Ok(blob) = JsFuture::from(blob).await else {
                return;
            };
            arrived(name.to_string(), blob.unchecked_ref());
        });
    }
}
