//! Gesture timing shared by reusable native display clocks.

use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct FrameClockState {
    generation: u64,
    last_frame: Option<Instant>,
    pending: Duration,
}

impl FrameClockState {
    /// Starting an already active clock must preserve other held gestures.
    pub(crate) fn start(&mut self, now: Instant) {
        if self.last_frame.is_none() {
            self.generation = self.generation.wrapping_add(1);
            self.last_frame = Some(now);
            self.pending = Duration::ZERO;
        }
    }

    pub(crate) fn pause(&mut self) {
        self.last_frame = None;
        self.pending = Duration::ZERO;
    }

    pub(crate) fn generation(&self) -> Option<u64> {
        self.last_frame.map(|_| self.generation)
    }

    /// Reject a native wait from a previous gesture and coalesce elapsed time.
    /// Only an empty-to-ready transition needs to wake the engine.
    pub(crate) fn publish(&mut self, generation: u64, now: Instant) -> bool {
        if self.generation() != Some(generation) {
            return false;
        }
        let Some(previous) = self.last_frame.replace(now) else {
            return false;
        };
        let was_empty = self.pending.is_zero();
        self.pending = self
            .pending
            .saturating_add(now.saturating_duration_since(previous));
        was_empty && !self.pending.is_zero()
    }

    pub(crate) fn take_elapsed(&mut self) -> Option<Duration> {
        let elapsed = std::mem::take(&mut self.pending);
        (!elapsed.is_zero()).then_some(elapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesced_frames_preserve_every_elapsed_interval_with_one_wake() {
        let mut clock = FrameClockState::default();
        let start = Instant::now();
        clock.start(start);
        let generation = clock.generation().unwrap();
        assert!(clock.publish(generation, start + Duration::from_millis(16)));
        assert!(!clock.publish(generation, start + Duration::from_millis(33)));
        assert!(!clock.publish(generation, start + Duration::from_millis(64)));
        assert_eq!(clock.take_elapsed(), Some(Duration::from_millis(64)));
        assert_eq!(clock.take_elapsed(), None);
        assert!(clock.publish(generation, start + Duration::from_millis(80)));
        assert_eq!(clock.take_elapsed(), Some(Duration::from_millis(16)));
    }

    #[test]
    fn restarting_discards_pending_and_in_flight_frames_and_idle_time() {
        let mut clock = FrameClockState::default();
        let start = Instant::now();
        clock.start(start);
        let previous = clock.generation().unwrap();
        clock.publish(previous, start + Duration::from_millis(16));
        clock.pause();
        assert_eq!(clock.take_elapsed(), None);
        assert!(!clock.publish(previous, start + Duration::from_millis(32)));
        let resumed = start + Duration::from_secs(60);
        clock.start(resumed);
        let current = clock.generation().unwrap();
        assert_ne!(previous, current);
        assert!(!clock.publish(previous, resumed + Duration::from_millis(1)));
        assert_eq!(clock.take_elapsed(), None);
        assert!(clock.publish(current, resumed + Duration::from_millis(5)));
        assert_eq!(clock.take_elapsed(), Some(Duration::from_millis(5)));
    }

    #[test]
    fn repeated_start_keeps_the_shared_gesture_interval() {
        let mut clock = FrameClockState::default();
        let start = Instant::now();
        clock.start(start);
        let generation = clock.generation().unwrap();
        clock.publish(generation, start + Duration::from_millis(16));
        clock.start(start + Duration::from_millis(20));
        assert_eq!(clock.generation(), Some(generation));
        assert!(!clock.publish(generation, start + Duration::from_millis(32)));
        assert_eq!(clock.take_elapsed(), Some(Duration::from_millis(32)));
    }
}
