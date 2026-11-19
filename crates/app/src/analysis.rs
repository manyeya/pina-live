//! Tracks analysed in the background, a few at a time: a queue the decks can jump (a track just
//! loaded goes first), workers that take from it, and what they find, picked up by the UI.

use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread,
    time::Duration,
};

use pina_live_engine::analyze::{Analysis, Tags, Wave, analyze};

pub type Found = (u64, Result<(Analysis, Tags, Wave), String>);

pub struct Analyzer {
    #[cfg(target_family = "wasm")]
    tx: Sender<Found>,
    /// In a browser: the track its worker's analysing.
    #[cfg(target_family = "wasm")]
    asking: Option<u64>,
    queue: Arc<Mutex<VecDeque<(u64, PathBuf)>>>,
    rx: Receiver<Found>,
    /// Queued or being analysed.
    pending: HashSet<u64>,
    /// Since the queue was last empty: how many were asked for, and done.
    pub total: usize,
    pub done: usize,
    stop: Arc<AtomicBool>,
}

impl Analyzer {
    pub fn new(workers: usize) -> Analyzer {
        let queue: Arc<Mutex<VecDeque<(u64, PathBuf)>>> = Arc::default();
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        // A browser's page has one thread: there, `poll` hands a track at a time to a worker.
        let workers = if cfg!(target_family = "wasm") {
            0
        } else {
            workers.max(1)
        };
        for _ in 0..workers {
            let (queue, tx, stop): (_, Sender<Found>, _) =
                (queue.clone(), tx.clone(), stop.clone());
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let next = queue.lock().ok().and_then(|mut q| q.pop_front());
                    match next {
                        Some((id, path)) => {
                            if tx.send((id, analyze(&path))).is_err() {
                                return;
                            }
                        }
                        None => thread::sleep(Duration::from_millis(150)),
                    }
                }
            });
        }
        Analyzer {
            #[cfg(target_family = "wasm")]
            tx,
            #[cfg(target_family = "wasm")]
            asking: None,
            queue,
            rx,
            pending: HashSet::new(),
            total: 0,
            done: 0,
            stop,
        }
    }

    /// Queued; `first`, ahead of everything (a deck's waiting for it).
    pub fn push(&mut self, id: u64, path: PathBuf, first: bool) {
        let Ok(mut q) = self.queue.lock() else { return };
        if self.pending.contains(&id) {
            if first && let Some(i) = q.iter().position(|(q, _)| *q == id) {
                let job = q.remove(i).unwrap();
                q.push_front(job);
            }
            return;
        }
        if self.pending.is_empty() {
            (self.total, self.done) = (0, 0);
        }
        self.pending.insert(id);
        self.total += 1;
        match first {
            true => q.push_front((id, path)),
            false => q.push_back((id, path)),
        }
    }

    pub fn pending(&self, id: u64) -> bool {
        self.pending.contains(&id)
    }

    pub fn busy(&self) -> bool {
        !self.pending.is_empty()
    }

    /// What's been found since last asked.
    pub fn poll(&mut self) -> Vec<Found> {
        #[cfg(target_family = "wasm")]
        if self.asking.is_none()
            && let Some((id, path)) = self.queue.lock().ok().and_then(|mut q| q.pop_front())
        {
            self.asking = Some(id);
            let tx = self.tx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = tx.send((id, crate::web::analyze(path).await));
            });
        }
        let found: Vec<Found> = self.rx.try_iter().collect();
        #[cfg(target_family = "wasm")]
        if found.iter().any(|(id, _)| Some(*id) == self.asking) {
            self.asking = None;
        }
        for (id, _) in &found {
            if self.pending.remove(id) {
                self.done += 1;
            }
        }
        found
    }

    /// The queue emptied (what's being analysed now finishes).
    pub fn stop(&mut self) {
        if let Ok(mut q) = self.queue.lock() {
            for (id, _) in q.drain(..) {
                self.pending.remove(&id);
            }
        }
        self.total = self.done + self.pending.len();
    }
}

impl Drop for Analyzer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
