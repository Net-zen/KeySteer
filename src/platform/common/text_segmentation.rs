//! One executing system tokenizer and one replaceable request. Native calls are
//! bounded by chunks; cancellation is checked between chunks and before publish.
use crate::api::text_select::{Request, Segmented};
use crate::support::worker::WorkerJoin;
use std::{
    ops::Range,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

#[derive(Default)]
struct State {
    latest: Option<Request>,
    epoch: u64,
    stop: bool,
}

pub(crate) struct Worker {
    shared: Arc<(Mutex<State>, Condvar)>,
    join: WorkerJoin,
}

impl Worker {
    pub(crate) fn new(
        mut segment: impl FnMut(&str) -> Vec<Range<usize>> + Send + 'static,
        publish: impl Fn(Segmented) + Send + 'static,
    ) -> Result<Self, String> {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let background = shared.clone();
        let join = WorkerJoin::spawn(
            "text-segmentation",
            std::thread::Builder::new().name("text-segmentation".into()),
            move || loop {
                let (request, epoch) = {
                    let (lock, wake) = &*background;
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    while state.latest.is_none() && !state.stop {
                        state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
                    }
                    if state.stop {
                        break;
                    }
                    let Some(request) = state.latest.take() else {
                        continue;
                    };
                    (request, state.epoch)
                };
                let current = || {
                    let state = background.0.lock().unwrap_or_else(|e| e.into_inner());
                    !state.stop && state.epoch == epoch
                };
                let mut words = Vec::new();
                let mut start = 0;
                while start < request.text.len() && current() {
                    let tail = &request.text[start..];
                    let end = tail.char_indices().nth(2048).map_or(tail.len(), |(i, _)| i);
                    let end = if end < tail.len() {
                        tail[..end]
                            .rfind(char::is_whitespace)
                            .filter(|i| *i > end / 2)
                            .unwrap_or(end)
                    } else {
                        end
                    };
                    words.extend(
                        segment(&tail[..end])
                            .into_iter()
                            .filter(|r| {
                                r.start < r.end
                                    && r.end <= end
                                    && tail.is_char_boundary(r.start)
                                    && tail.is_char_boundary(r.end)
                            })
                            .map(|r| (start + r.start)..(start + r.end)),
                    );
                    start += end;
                }
                if current() {
                    publish(Segmented {
                        id: request.id,
                        words,
                    });
                }
            },
        )?;
        Ok(Self { shared, join })
    }
    pub(crate) fn submit(&self, request: Option<Request>) {
        let mut state = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        state.epoch = state.epoch.wrapping_add(1);
        state.latest = request;
        self.shared.1.notify_one();
    }
    pub(crate) fn request_stop(&self) {
        let mut state = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        state.stop = true;
        state.latest = None;
        self.shared.1.notify_one();
    }
    pub(crate) fn stop_until(&mut self, deadline: Instant) -> Result<(), String> {
        self.request_stop();
        self.join.join_until(deadline)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.request_stop();
        if !self.join.shutdown_failure_was_returned()
            && let Err(error) = self.stop_until(Instant::now() + Duration::from_millis(500))
        {
            crate::support::logging::report_error("text-segmentation", error);
        }
    }
}

/// Reject surrogate interiors and malformed native UTF-16 ranges.
pub(crate) fn utf16_ranges(
    text: &str,
    ranges: impl IntoIterator<Item = Range<usize>>,
) -> Vec<Range<usize>> {
    let mut map = Vec::with_capacity(text.len() + 1);
    for (offset, c) in text.char_indices() {
        map.push(Some(offset));
        if c.len_utf16() == 2 {
            map.push(None);
        }
    }
    map.push(Some(text.len()));
    ranges
        .into_iter()
        .filter_map(|r| {
            let start = (*map.get(r.start)?)?;
            let end = (*map.get(r.end)?)?;
            (start < end).then_some(start..end)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn surrogate_boundaries_are_checked_before_conversion() {
        assert_eq!(
            utf16_ranges("a🦀中", [0..1, 1..3, 3..4, 2..3, 3..9]),
            [0..1, 1..5, 5..8]
        );
    }
    #[test]
    fn cancellation_discards_running_and_replaced_requests() {
        let (started, ready) = std::sync::mpsc::channel();
        let (resume, waiting) = std::sync::mpsc::channel();
        let (publish, result) = std::sync::mpsc::channel();
        let mut worker = Worker::new(
            move |text| {
                if text == "first" {
                    started.send(()).unwrap();
                    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
                }
                std::iter::once(0..text.len()).collect()
            },
            move |segmented| {
                publish.send(segmented).unwrap();
            },
        )
        .unwrap();
        worker.submit(Some(Request {
            id: 1,
            text: "first".into(),
        }));
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.submit(Some(Request {
            id: 2,
            text: "replaced".into(),
        }));
        worker.submit(None);
        worker.submit(Some(Request {
            id: 3,
            text: "current".into(),
        }));
        resume.send(()).unwrap();
        assert_eq!(result.recv_timeout(Duration::from_secs(2)).unwrap().id, 3);
        assert!(result.try_recv().is_err());
        worker
            .stop_until(Instant::now() + Duration::from_secs(1))
            .unwrap();
    }
}
