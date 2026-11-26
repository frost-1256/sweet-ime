//! Background refinement shared by desktop input frontends.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Condvar, Mutex, Once};
use std::time::{Duration, Instant};

use crate::Converter;

/// Default settle window before running neural live refinement (0 = off).
const DEFAULT_LIVE_DEBOUNCE: Duration = Duration::ZERO;

type Completion = Box<dyn FnOnce(String, String) + Send>;

struct Job {
    converter: &'static Converter,
    reading: String,
    left_context: String,
    debounce: Duration,
    done: Completion,
    submitted_at: Instant,
}

struct PendingState {
    job: Option<Job>,
    generation: u64,
}

/// A process-lifetime worker that retains only the latest pending request.
pub struct LiveConversionWorker {
    started: Once,
    pending: Mutex<PendingState>,
    ready: Condvar,
}

impl Default for LiveConversionWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveConversionWorker {
    pub const fn new() -> Self {
        Self {
            started: Once::new(),
            pending: Mutex::new(PendingState {
                job: None,
                generation: 0,
            }),
            ready: Condvar::new(),
        }
    }

    /// Replaces any pending request using the default debounce window. The
    /// callback receives the reading and refined surface on the worker thread.
    pub fn submit(
        &'static self,
        converter: &'static Converter,
        reading: String,
        left_context: String,
        done: impl FnOnce(String, String) + Send + 'static,
    ) {
        self.submit_with_debounce(converter, reading, left_context, DEFAULT_LIVE_DEBOUNCE, done);
    }

    /// Replaces any pending request with a caller-chosen settle delay.
    /// `Duration::ZERO` runs immediately without waiting.
    pub fn submit_with_debounce(
        &'static self,
        converter: &'static Converter,
        reading: String,
        left_context: String,
        debounce: Duration,
        done: impl FnOnce(String, String) + Send + 'static,
    ) {
        self.started.call_once(|| {
            let _ = std::thread::Builder::new()
                .name("sweet-ime-live".into())
                .spawn(|| self.run());
        });
        let replaced = {
            let mut state = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            state.generation = state.generation.wrapping_add(1);
            state.job.replace(Job {
                converter,
                reading,
                left_context,
                debounce,
                done: Box::new(done),
                submitted_at: Instant::now(),
            })
        };
        self.ready.notify_one();
        drop(replaced);
    }

    fn run(&self) {
        loop {
            let job = {
                let mut state = self.pending.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    state = self
                        .ready
                        .wait_while(state, |s| s.job.is_none())
                        .unwrap_or_else(|e| e.into_inner());
                    let generation = state.generation;
                    let (debounce, elapsed) = {
                        let job = state.job.as_ref().unwrap();
                        (job.debounce, job.submitted_at.elapsed())
                    };
                    if !debounce.is_zero()
                        && let Some(wait) = debounce.checked_sub(elapsed)
                    {
                        let (next_state, _) = self
                            .ready
                            .wait_timeout_while(state, wait, |s| s.generation == generation)
                            .unwrap_or_else(|e| e.into_inner());
                        state = next_state;
                        if state.generation != generation {
                            continue;
                        }
                    }
                    if let Some(job) = state.job.take() {
                        break job;
                    }
                }
            };
            let result = catch_unwind(AssertUnwindSafe(|| {
                job.converter.refine_live(&job.reading, &job.left_context)
            }));
            if let Ok(Some(surface)) = result {
                (job.done)(job.reading, surface);
            }
        }
    }
}

impl Converter {
    /// Refines the whole reading with the current reranker settings.
    pub fn refine_live(&self, reading: &str, left_context: &str) -> Option<String> {
        self.convert_best(reading, &[], left_context)
            .first()
            .map(|conversion| conversion.surface())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    use crate::Dictionary;

    use super::*;

    #[test]
    fn keeps_only_the_latest_pending_reading() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/system.dict");
        let Ok(dictionary) = Dictionary::open(path) else {
            return;
        };
        static WORKER: LiveConversionWorker = LiveConversionWorker::new();
        let converter = Box::leak(Box::new(Converter::new(dictionary)));
        let timeout = Duration::from_secs(5);
        let (started_tx, started_rx) = channel();
        let (release_tx, release_rx) = channel();
        WORKER.submit(converter, "きょう".into(), String::new(), move |_, _| {
            let _ = started_tx.send(());
            let _ = release_rx.recv_timeout(timeout);
        });
        started_rx.recv_timeout(timeout).unwrap();

        let (result_tx, result_rx) = channel();
        let superseded = result_tx.clone();
        WORKER.submit(
            converter,
            "あした".into(),
            String::new(),
            move |reading, surface| {
                let _ = superseded.send((reading, surface));
            },
        );
        let reading = "にほんご";
        let expected = converter.refine_live(reading, "").unwrap();
        WORKER.submit(
            converter,
            reading.into(),
            String::new(),
            move |reading, surface| {
                let _ = result_tx.send((reading, surface));
            },
        );
        release_tx.send(()).unwrap();

        assert_eq!(
            result_rx.recv_timeout(timeout).unwrap(),
            (reading.into(), expected)
        );
        assert!(result_rx.try_recv().is_err());
    }
}
