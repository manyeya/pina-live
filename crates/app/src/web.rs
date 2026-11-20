//! Pina Live in a browser: the folders and files it's given (picked with "Add a folder…", or
//! dropped onto the page), and the heavy work on them done in Web Workers, a browser's background threads. The
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
    /// Files given and not yet taken into the library: each its path in what was given
    /// ("Crate/Artist - Song.mp3", or a lone file's name).
    static ARRIVED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Folders given and not yet taken: their names.
    static FOLDERS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static WORKERS: [Worker; 2] = [worker(), worker()];
    /// Requests sent and not yet answered: each its promise's `resolve`.
    static WAITING: RefCell<HashMap<u32, js_sys::Function>> = RefCell::default();
    static NEXT: Cell<u32> = const { Cell::new(0) };
}

/// Where a file or folder the browser gave is in the library: its path in what was given, under
/// here.
pub fn path_of(name: &str) -> PathBuf {
    PathBuf::from("/Pina Live").join(name)
}

/// The files and the folders given since last asked.
pub fn take_arrived() -> (Vec<String>, Vec<String>) {
    let files = ARRIVED.with(|a| std::mem::take(&mut *a.borrow_mut()));
    (
        files,
        FOLDERS.with(|f| std::mem::take(&mut *f.borrow_mut())),
    )
}

// What was dropped or picked, folders walked into: { files: [[path, File]…], folders: [name…] }.
// (A dropped folder's entries are read by callbacks, so this part's JavaScript; hidden files and
// folders are passed over, as a scan on disk passes them over.)
#[wasm_bindgen(inline_js = r#"
const hidden = (path) => path.split("/").some((s) => s.startsWith("."));

export function dropped(dt) {
  // Read now, while the drop's data is still there; walked after.
  const entries = [...dt.items].map((i) => i.webkitGetAsEntry && i.webkitGetAsEntry()).filter(Boolean);
  const loose = [...dt.files];
  return (async () => {
    const files = [], folders = [];
    const file = (e) => new Promise((ok) => e.file(ok, () => ok(null)));
    const children = (dir) => new Promise((ok) => {
      const reader = dir.createReader(), all = [];
      const next = () => reader.readEntries((batch) => {
        if (!batch.length) return ok(all);
        all.push(...batch);
        next();
      }, () => ok(all));
      next();
    });
    const walk = async (e, at) => {
      const path = at + e.name;
      if (hidden(path)) return;
      if (e.isFile) {
        const f = await file(e);
        if (f) files.push([path, f]);
      } else if (e.isDirectory) {
        for (const c of await children(e)) await walk(c, path + "/");
      }
    };
    if (!entries.length) loose.forEach((f) => files.push([f.name, f]));
    for (const e of entries) {
      if (e.isDirectory) folders.push(e.name);
      await walk(e, "");
    }
    return { files, folders };
  })();
}

export function picked(input) {
  const files = [], folders = new Set();
  for (const f of input.files) {
    const path = f.webkitRelativePath || f.name;
    if (hidden(path)) continue;
    files.push([path, f]);
    if (path.includes("/")) folders.add(path.split("/")[0]);
  }
  return { files, folders: [...folders] };
}
"#)]
extern "C" {
    fn dropped(dt: &web_sys::DataTransfer) -> Promise;
    fn picked(input: &web_sys::HtmlInputElement) -> JsValue;
}

/// What `dropped` or `picked` found, taken: each file handed to the workers, each folder noted.
fn took(found: &JsValue) {
    let list = |key: &str| {
        Reflect::get(found, &key.into())
            .ok()
            .map(|v| js_sys::Array::from(&v))
    };
    for pair in list("files").iter().flat_map(|a| a.iter()) {
        let pair = js_sys::Array::from(&pair);
        if let Some(path) = pair.get(0).as_string() {
            arrived(path, pair.get(1).unchecked_ref());
        }
    }
    for name in list("folders").iter().flat_map(|a| a.iter()) {
        if let Some(name) = name.as_string() {
            FOLDERS.with(|f| f.borrow_mut().push(name));
        }
    }
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
/// the library by its path in what was given.
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

/// Folders and files dropped anywhere on the page, taken. (GPUI's own drop handler keeps the
/// browser from opening them; it passes nothing on, since a browser gives files, not paths.)
pub fn listen() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let drop = Closure::<dyn FnMut(web_sys::DragEvent)>::new(|e: web_sys::DragEvent| {
        e.prevent_default();
        let Some(dt) = e.data_transfer() else { return };
        let walked = dropped(&dt);
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(found) = JsFuture::from(walked).await {
                took(&found);
            }
        });
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

/// The browser's folder picker: the folder picked, and everything in it, taken.
pub fn pick() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Ok(input) = document.create_element("input") else {
        return;
    };
    let input: web_sys::HtmlInputElement = input.unchecked_into();
    input.set_type("file");
    // A folder, as the desktop's "Add a folder…" takes one (every browser has this, prefixed).
    let _ = Reflect::set(&input, &"webkitdirectory".into(), &true.into());
    let chosen = input.clone();
    let change = Closure::<dyn FnMut()>::new(move || {
        took(&picked(&chosen));
        chosen.remove();
    });
    input.set_onchange(Some(change.as_ref().unchecked_ref()));
    change.forget();
    // In the page while it's open, out of sight: a real element (tests reach it there too).
    input.set_hidden(true);
    if let Some(body) = document.body() {
        let _ = body.append_child(&input);
    }
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
