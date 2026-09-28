//! Coalescing delivery of native notifications. Only adjacent, replaceable
//! updates coalesce: completions and commands are FIFO barriers and never drop.
//! Physical input and scan batches use their existing dedicated mailboxes.
//! Producers never wait for capacity; the short queue lock contains no native I/O.

use crate::api::{BackendEvent, UpdateProgress};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, Mutex};

struct State {
    events: VecDeque<BackendEvent>,
    connected: bool,
}
struct Shared {
    state: Mutex<State>,
    pending: AtomicBool,
    #[cfg(test)]
    ready: std::sync::Condvar,
}

#[derive(Clone)]
pub(crate) struct Sender(Arc<Shared>);
pub(crate) struct Receiver(Arc<Shared>);

pub(crate) fn channel() -> (Sender, Receiver) {
    let shared = Arc::new(Shared {
        pending: AtomicBool::new(false),
        state: Mutex::new(State {
            events: VecDeque::new(),
            connected: true,
        }),
        #[cfg(test)]
        ready: std::sync::Condvar::new(),
    });
    (Sender(shared.clone()), Receiver(shared))
}

fn replaces(previous: &BackendEvent, next: &BackendEvent) -> bool {
    match (previous, next) {
        (
            BackendEvent::TextPromptChanged { id: a, .. },
            BackendEvent::TextPromptChanged { id: b, .. },
        ) => a == b,
        (
            BackendEvent::UpdateProgress(UpdateProgress::Downloading { latest: a, .. }),
            BackendEvent::UpdateProgress(UpdateProgress::Downloading { latest: b, .. }),
        ) => a == b,
        _ => false,
    }
}

impl Sender {
    /// Returns whether the native event loop needs waking. A nonempty queue is
    /// already runnable; its receiver checks it before entering a native wait.
    pub(crate) fn send(&self, event: BackendEvent) -> Result<bool, ()> {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.connected {
            return Err(());
        }
        let wake = state.events.is_empty();
        let replaced = if state
            .events
            .back()
            .is_some_and(|last| replaces(last, &event))
        {
            state.events.pop_back()
        } else {
            None
        };
        state.events.push_back(event);
        self.0.pending.store(true, Ordering::Release);
        drop(state);
        // Potentially large strings are freed outside the producer lock.
        drop(replaced);
        #[cfg(test)]
        self.0.ready.notify_one();
        Ok(wake)
    }
}

impl Receiver {
    pub(crate) fn try_recv(&self) -> Result<BackendEvent, TryRecvError> {
        if !self.0.pending.load(Ordering::Acquire) {
            return Err(if Arc::strong_count(&self.0) == 1 {
                TryRecvError::Disconnected
            } else {
                TryRecvError::Empty
            });
        }
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        let event = state.events.pop_front();
        if state.events.is_empty() {
            self.0.pending.store(false, Ordering::Release);
        }
        // Keep the small steady-state buffer; relinquish burst capacity once
        // drained, never shrink a live queue on each event.
        let retired = if state.events.is_empty() && state.events.capacity() > 16 {
            Some(std::mem::take(&mut state.events))
        } else {
            None
        };
        drop(state);
        drop(retired);
        event.ok_or_else(|| {
            if Arc::strong_count(&self.0) == 1 {
                TryRecvError::Disconnected
            } else {
                TryRecvError::Empty
            }
        })
    }

    #[cfg(test)]
    pub(crate) fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<BackendEvent, std::sync::mpsc::RecvTimeoutError> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match self.try_recv() {
                Ok(event) => return Ok(event),
                Err(TryRecvError::Disconnected) => {
                    return Err(std::sync::mpsc::RecvTimeoutError::Disconnected);
                }
                Err(TryRecvError::Empty) => {}
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(std::sync::mpsc::RecvTimeoutError::Timeout);
            }
            let state = self.0.state.lock().unwrap();
            if state.events.is_empty() {
                drop(self.0.ready.wait_timeout(state, remaining).unwrap());
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn recv(&self) -> Result<BackendEvent, std::sync::mpsc::RecvTimeoutError> {
        self.recv_timeout(std::time::Duration::from_secs(5))
    }
}

/// Bound background work between display opportunities without changing key
/// priority, starting a timer, or assigning deadlines to reliable results.
#[derive(Default)]
pub(crate) struct BackgroundBudget(u8);

impl BackgroundBudget {
    pub(crate) fn record(&mut self) {
        self.0 = self.0.saturating_add(1);
    }
    pub(crate) fn reset(&mut self) {
        self.0 = 0;
    }
    pub(crate) fn yield_due(&mut self) -> bool {
        if self.0 < 8 {
            return false;
        }
        self.reset();
        true
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        state.connected = false;
        let events = std::mem::take(&mut state.events);
        drop(state);
        drop(events);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_bursts_offer_a_frame_every_eight_events() {
        let mut budget = BackgroundBudget::default();
        for _ in 0..100 {
            for _ in 0..8 {
                assert!(!budget.yield_due());
                budget.record();
            }
            assert!(budget.yield_due());
        }
        budget.record();
        budget.reset();
        assert!(!budget.yield_due());
    }

    #[test]
    fn concurrent_producers_preserve_each_producers_completions() {
        let (tx, rx) = channel();
        let workers: Vec<_> = (0..4)
            .map(|producer| {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    for sequence in 0..1000 {
                        tx.send(BackendEvent::FocusedWindowBounds {
                            id: producer * 1000 + sequence,
                            bounds: Ok(None),
                        })
                        .unwrap();
                    }
                })
            })
            .collect();
        let mut next = [0; 4];
        for _ in 0..4000 {
            let BackendEvent::FocusedWindowBounds { id, .. } = rx.recv().unwrap() else {
                panic!("wrong event")
            };
            let producer = (id / 1000) as usize;
            assert_eq!(id % 1000, next[producer]);
            next[producer] += 1;
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(next, [1000; 4]);
    }

    #[test]
    fn progress_is_coalesced_without_overtaking_completion() {
        let (tx, rx) = channel();
        for percent in 0..=100 {
            tx.send(BackendEvent::UpdateProgress(UpdateProgress::Downloading {
                latest: "1".into(),
                percent,
            }))
            .unwrap();
        }
        tx.send(BackendEvent::UpdateChecked(
            crate::api::UpdateCheckResult::UpToDate {
                current: "1".into(),
            },
        ))
        .unwrap();
        tx.send(BackendEvent::UpdateProgress(UpdateProgress::Checking))
            .unwrap();
        assert!(matches!(
            rx.try_recv(),
            Ok(BackendEvent::UpdateProgress(UpdateProgress::Downloading {
                percent: 100,
                ..
            }))
        ));
        assert!(matches!(rx.try_recv(), Ok(BackendEvent::UpdateChecked(_))));
        assert!(matches!(
            rx.try_recv(),
            Ok(BackendEvent::UpdateProgress(UpdateProgress::Checking))
        ));
    }

    fn changed(id: u64, text: &str) -> BackendEvent {
        BackendEvent::TextPromptChanged {
            id,
            text: text.into(),
        }
    }

    #[test]
    fn transient_burst_keeps_latest_and_one_wakeup() {
        let (tx, rx) = channel();
        assert_eq!(tx.send(changed(1, "first")), Ok(true));
        for _ in 0..10_000 {
            assert_eq!(tx.send(changed(1, "latest")), Ok(false));
        }
        assert_eq!(rx.0.state.lock().unwrap().events.len(), 1);
        assert!(
            matches!(rx.try_recv(), Ok(BackendEvent::TextPromptChanged { text, .. }) if text == "latest")
        );
        assert_eq!(tx.send(BackendEvent::Quit), Ok(true));
    }

    #[test]
    fn completion_and_session_boundaries_preserve_order() {
        let (tx, rx) = channel();
        tx.send(changed(1, "a")).unwrap();
        tx.send(BackendEvent::TextPromptResult {
            id: 1,
            value: Ok(Some("a".into())),
        })
        .unwrap();
        tx.send(changed(1, "b")).unwrap();
        tx.send(changed(2, "c")).unwrap();
        assert!(
            matches!(rx.try_recv(), Ok(BackendEvent::TextPromptChanged { id: 1, text }) if text == "a")
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(BackendEvent::TextPromptResult { id: 1, .. })
        ));
        assert!(
            matches!(rx.try_recv(), Ok(BackendEvent::TextPromptChanged { id: 1, text }) if text == "b")
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(BackendEvent::TextPromptChanged { id: 2, .. })
        ));
    }

    #[test]
    fn reliable_burst_is_lossless_and_releases_peak_storage() {
        let (tx, rx) = channel();
        for id in 0..1000 {
            tx.send(BackendEvent::FocusedWindowBounds {
                id,
                bounds: Ok(None),
            })
            .unwrap();
        }
        for expected in 0..1000 {
            assert!(
                matches!(rx.try_recv(), Ok(BackendEvent::FocusedWindowBounds { id, .. }) if id == expected)
            );
        }
        assert_eq!(rx.0.state.lock().unwrap().events.capacity(), 0);
        drop(rx);
        assert!(tx.send(BackendEvent::Quit).is_err());
    }
}
