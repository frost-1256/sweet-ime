//! Background refinement of live conversion with the neural reranker.
//!
//! The UI thread shows the lattice result immediately and submits the
//! reading here; a single worker thread runs `convert_best` (which may take
//! tens of milliseconds) on the most recent request only and posts the
//! result back to the requesting thread's window.

use std::sync::{Condvar, Mutex, OnceLock};

pub struct Job {
    pub reading: String,
    pub left_context: String,
    /// Called on the worker thread with the refined surface.
    pub done: Box<dyn FnOnce(String) + Send>,
}

struct Queue {
    slot: Mutex<Option<Job>>,
    ready: Condvar,
}

fn queue() -> &'static Queue {
    static Q: OnceLock<Queue> = OnceLock::new();
    Q.get_or_init(|| {
        let _ = std::thread::Builder::new()
            .name("sweet-ime-live".into())
            .spawn(worker);
        Queue {
            slot: Mutex::new(None),
            ready: Condvar::new(),
        }
    })
}

/// Queues `job`, replacing any request that has not started yet.
pub fn submit(job: Job) {
    let q = queue();
    *q.slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(job);
    q.ready.notify_one();
}

fn worker() {
    let q = queue();
    loop {
        let job = {
            let mut slot = q.slot.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(j) = slot.take() {
                    break j;
                }
                slot = q.ready.wait(slot).unwrap_or_else(|e| e.into_inner());
            }
        };
        let Some(conv) = crate::converter() else {
            continue;
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            conv.convert_best(&job.reading, &[], &job.left_context)
                .first()
                .map(|c| c.surface())
        }));
        if let Ok(Some(surface)) = result {
            (job.done)(surface);
        }
    }
}
