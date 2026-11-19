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
        for _ in 0..workers.max(1) {
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
        let found: Vec<Found> = self.rx.try_iter().collect();
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
