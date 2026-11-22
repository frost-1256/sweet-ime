//! Background refinement shared by desktop input frontends.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Condvar, Mutex, Once};

use crate::Converter;

type Completion = Box<dyn FnOnce(String, String) + Send>;

struct Job {
    converter: &'static Converter,
    reading: String,
    left_context: String,
    done: Completion,
}

/// A process-lifetime worker that retains only the latest pending request.
pub struct LiveConversionWorker {
    started: Once,
    pending: Mutex<Option<Job>>,
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
            pending: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    /// Replaces any pending request. The callback receives the reading and
    /// refined surface on the worker thread.
    pub fn submit(
        &'static self,
        converter: &'static Converter,
        reading: String,
        left_context: String,
        done: impl FnOnce(String, String) + Send + 'static,
    ) {
        self.started.call_once(|| {
            let _ = std::thread::Builder::new()
                .name("sweet-ime-live".into())
                .spawn(|| self.run());
        });
        let replaced = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(Job {
                converter,
                reading,
                left_context,
                done: Box::new(done),
            });
        self.ready.notify_one();
        drop(replaced);
    }

    fn run(&self) {
        loop {
            let job = {
                let pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
                self.ready
                    .wait_while(pending, |pending| pending.is_none())
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                    .unwrap()
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
