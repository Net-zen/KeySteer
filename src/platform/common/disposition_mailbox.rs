#![forbid(unsafe_code)]

//! Allocation-free rendezvous for synchronous native input callbacks.
//!
//! Native keyboard callbacks must wait for the engine's consume/forward
//! decision. A generation-tagged reusable slot avoids allocating a one-shot
//! channel for every physical key edge and prevents a late response from a
//! timed-out callback being observed by the next event.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::api::backend::KeyDisposition;

#[derive(Default)]
struct Slot {
    generation: u64,
    disposition: Option<KeyDisposition>,
    forwarded_timeout: Option<u64>,
    #[cfg(any(target_os = "macos", test))]
    closed: bool,
}

#[derive(Default)]
pub(crate) struct DispositionMailbox {
    slot: Mutex<Slot>,
    ready: Condvar,
}

impl DispositionMailbox {
    /// Reserve the reusable slot for one native callback.
    #[cfg(any(not(target_os = "macos"), test))]
    pub(crate) fn begin(&self) -> u64 {
        let mut slot = self.slot.lock().unwrap_or_else(|error| error.into_inner());
        slot.generation = slot.generation.wrapping_add(1);
        slot.disposition = None;
        slot.generation
    }

    /// Reserve the macOS callback slot unless native input capture has entered
    /// its terminal state. The check shares the existing slot lock, so it adds
    /// no atomic, allocation, or second synchronization step to the callback.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn try_begin(&self) -> Option<u64> {
        let mut slot = self.slot.lock().unwrap_or_else(|error| error.into_inner());
        if slot.closed {
            return None;
        }
        slot.generation = slot.generation.wrapping_add(1);
        slot.disposition = None;
        Some(slot.generation)
    }

    /// Complete `generation`; a known forwarded timeout accepts only one late
    /// Forward acknowledgement without changing the current callback slot.
    pub(crate) fn complete(&self, generation: u64, disposition: KeyDisposition) -> bool {
        let mut slot = self.slot.lock().unwrap_or_else(|error| error.into_inner());
        if disposition == KeyDisposition::Forward && slot.forwarded_timeout == Some(generation) {
            slot.forwarded_timeout = None;
            return true;
        }
        if slot.generation != generation || slot.disposition.is_some() {
            return false;
        }
        slot.disposition = Some(disposition);
        self.ready.notify_one();
        true
    }

    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn wait(&self, generation: u64, timeout: Duration) -> Option<KeyDisposition> {
        self.wait_inner(generation, timeout, false)
    }

    /// Supply the actual native timeout fallback, including held-key pairing.
    /// Only an explicitly forwarded timeout accepts a late Forward response;
    /// consumed or unknown fallbacks retain strict error recovery.
    pub(crate) fn wait_with_fallback(
        &self,
        generation: u64,
        timeout: Duration,
        fallback: KeyDisposition,
    ) -> Option<KeyDisposition> {
        self.wait_inner(generation, timeout, fallback == KeyDisposition::Forward)
    }

    fn wait_inner(
        &self,
        generation: u64,
        timeout: Duration,
        forwarding: bool,
    ) -> Option<KeyDisposition> {
        let slot = self.slot.lock().unwrap_or_else(|error| error.into_inner());
        if slot.generation != generation {
            return None;
        }
        let (mut slot, _) = self
            .ready
            .wait_timeout_while(slot, timeout, |slot| {
                slot.generation == generation && slot.disposition.is_none()
            })
            .unwrap_or_else(|error| error.into_inner());
        if slot.generation != generation {
            return None;
        }
        let disposition = slot.disposition;
        if disposition.is_none() {
            if forwarding {
                slot.forwarded_timeout = Some(generation);
            }
            // Retire under the same lock as complete: even before another
            // key arrives, a late conflicting response must report failure.
            slot.generation = slot.generation.wrapping_add(1);
        }
        disposition
    }

    /// Permanently fail open this mailbox. A native callback racing with
    /// shutdown either reserves its generation before this lock and is woken,
    /// or observes `closed` and returns without waiting.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn close(&self) {
        let mut slot = self.slot.lock().unwrap_or_else(|error| error.into_inner());
        slot.closed = true;
        slot.forwarded_timeout = None;
        slot.generation = slot.generation.wrapping_add(1);
        slot.disposition = None;
        self.ready.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_timeout_accepts_only_matching_forward_without_touching_next_key() {
        let mailbox = DispositionMailbox::default();
        let first = mailbox.begin();
        assert_eq!(
            mailbox.wait_with_fallback(first, Duration::ZERO, KeyDisposition::Forward),
            None
        );
        assert!(!mailbox.complete(first, KeyDisposition::Consume));
        assert!(!mailbox.complete(first, KeyDisposition::Defer));
        let second = mailbox.begin();
        assert!(mailbox.complete(first, KeyDisposition::Forward));
        assert!(!mailbox.complete(first, KeyDisposition::Forward));
        assert!(mailbox.complete(second, KeyDisposition::Consume));
        assert_eq!(
            mailbox.wait(second, Duration::ZERO),
            Some(KeyDisposition::Consume)
        );
    }

    #[test]
    fn shutdown_rejects_even_a_previously_forwarded_timeout() {
        let mailbox = DispositionMailbox::default();
        let generation = mailbox.begin();
        mailbox.wait_with_fallback(generation, Duration::ZERO, KeyDisposition::Forward);
        mailbox.close();
        assert!(!mailbox.complete(generation, KeyDisposition::Forward));
    }

    #[test]
    fn late_completion_cannot_pollute_the_next_generation() {
        let mailbox = DispositionMailbox::default();
        let first = mailbox.begin();
        let second = mailbox.begin();

        assert!(!mailbox.complete(first, KeyDisposition::Consume));
        assert!(mailbox.complete(second, KeyDisposition::Forward));
        assert_eq!(
            mailbox.wait(second, Duration::ZERO),
            Some(KeyDisposition::Forward)
        );
    }

    #[test]
    fn timeout_fails_open_without_invalidating_future_events() {
        let mailbox = DispositionMailbox::default();
        let first = mailbox.begin();
        assert_eq!(mailbox.wait(first, Duration::ZERO), None);
        assert!(!mailbox.complete(first, KeyDisposition::Consume));

        let second = mailbox.begin();
        assert!(mailbox.complete(second, KeyDisposition::Consume));
        assert_eq!(
            mailbox.wait(second, Duration::ZERO),
            Some(KeyDisposition::Consume)
        );
    }

    #[test]
    fn close_invalidates_current_and_rejects_future_callbacks() {
        let mailbox = DispositionMailbox::default();
        let generation = mailbox.try_begin().unwrap();
        mailbox.close();

        assert_eq!(mailbox.wait(generation, Duration::ZERO), None);
        assert!(!mailbox.complete(generation, KeyDisposition::Consume));
        assert_eq!(mailbox.try_begin(), None);
    }
}
