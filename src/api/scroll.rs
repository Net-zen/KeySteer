//! Cancellable display-frame scrolling, independent of pointer movement.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Owned by a mode gesture. Cancellation never waits for native input.
#[derive(Debug, Default)]
pub struct ScrollSession(Arc<AtomicU64>);

impl ScrollSession {
    pub fn frame(&self, dx: f64, dy: f64) -> ScrollFrame {
        ScrollFrame {
            dx,
            dy,
            generation: self.0.load(Ordering::Acquire),
            session: self.0.clone(),
        }
    }

    /// Invalidate frames on release, direction change, or session cleanup.
    /// A native call that has already started may still complete.
    pub fn cancel(&self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}

impl Drop for ScrollSession {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// One display interval, not a distance to accumulate behind native work.
#[derive(Debug, Clone)]
pub struct ScrollFrame {
    pub dx: f64,
    pub dy: f64,
    generation: u64,
    session: Arc<AtomicU64>,
}

impl ScrollFrame {
    pub fn is_current(&self) -> bool {
        self.session.load(Ordering::Acquire) == self.generation
    }
}

impl PartialEq for ScrollFrame {
    fn eq(&self, other: &Self) -> bool {
        self.dx == other.dx
            && self.dy == other.dy
            && self.generation == other.generation
            && Arc::ptr_eq(&self.session, &other.session)
    }
}
