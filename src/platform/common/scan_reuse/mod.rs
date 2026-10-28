//! Shared idle reuse, deadline waits and source-job ownership across scan modes.
//! Native creation, liveness checks and destruction stay on the provider thread.

use std::time::{Duration, Instant};

#[cfg(any(target_os = "windows", test))]
mod worker;
#[cfg(target_os = "windows")]
pub(crate) use worker::{IdleWorker, JobJoin};

pub(crate) const IDLE_TIMEOUT: Duration = Duration::from_secs(10);

/// Source queues share deadline waits. Expired/native owners are released by
/// callers after dropping the queue lock; no polling or idle heartbeat.
pub(crate) fn wait_until<'a, T>(
    ready: &std::sync::Condvar,
    state: std::sync::MutexGuard<'a, T>,
    until: Option<Instant>,
) -> std::sync::MutexGuard<'a, T> {
    if let Some(until) = until {
        ready
            .wait_timeout(state, until.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|e| e.into_inner())
            .0
    } else {
        ready.wait(state).unwrap_or_else(|e| e.into_inner())
    }
}

struct Entry<T> {
    resource: T,
    until: Instant,
}

pub(crate) struct IdleCache<T> {
    entries: Vec<Entry<T>>,
    limit: usize,
}

impl<T> Default for IdleCache<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            limit: 0,
        }
    }
}

impl<T> IdleCache<T> {
    /// Return excess or expired owners so callers can release them outside locks.
    pub(crate) fn set_limit(&mut self, limit: usize, now: Instant) -> Vec<T> {
        self.limit = limit;
        self.take_expired(now)
    }

    /// Only completed resources are eligible. A rejected owner remains the caller's.
    pub(crate) fn retain(&mut self, resource: T, now: Instant) -> Option<T> {
        if self.entries.len() >= self.limit {
            return Some(resource);
        }
        self.entries.push(Entry {
            resource,
            until: now + IDLE_TIMEOUT,
        });
        None
    }

    pub(crate) fn take(&mut self, now: Instant) -> (Option<T>, Vec<T>) {
        let expired = self.take_expired(now);
        (self.entries.pop().map(|entry| entry.resource), expired)
    }

    pub(crate) fn take_expired(&mut self, now: Instant) -> Vec<T> {
        let mut expired = Vec::new();
        let mut index = 0;
        while index < self.entries.len() {
            if self.entries[index].until <= now || index >= self.limit {
                expired.push(self.entries.swap_remove(index).resource);
            } else {
                index += 1;
            }
        }
        expired
    }

    pub(crate) fn next_expiration(&self) -> Option<Instant> {
        self.entries.iter().map(|entry| entry.until).min()
    }

    pub(crate) fn drain(&mut self) -> Vec<T> {
        self.entries.drain(..).map(|entry| entry.resource).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuse_after_six_seconds_refreshes_the_ten_second_idle_deadline() {
        let now = Instant::now();
        let mut cache = IdleCache::default();
        assert!(cache.set_limit(1, now).is_empty());
        assert_eq!(cache.retain(7, now), None);
        assert_eq!(cache.next_expiration(), Some(now + Duration::from_secs(10)));

        let next_scan = now + Duration::from_secs(9);
        let (resource, expired) = cache.take(next_scan);
        assert_eq!(resource, Some(7));
        assert!(expired.is_empty());
        assert_eq!(cache.next_expiration(), None);
        assert_eq!(cache.retain(resource.unwrap(), next_scan), None);
        assert!(cache.take_expired(now + Duration::from_secs(18)).is_empty());
        assert_eq!(cache.take_expired(now + Duration::from_secs(19)), [7]);
        assert_eq!(cache.next_expiration(), None);
    }

    #[test]
    fn take_rejects_expired_resources_and_preserves_live_owners() {
        let now = Instant::now();
        let mut cache = IdleCache::default();
        cache.set_limit(2, now);
        assert_eq!(cache.retain(1, now), None);
        assert_eq!(cache.retain(2, now + Duration::from_secs(5)), None);
        let (resource, expired) = cache.take(now + IDLE_TIMEOUT);
        assert_eq!(resource, Some(2));
        assert_eq!(expired, [1]);
        assert_eq!(cache.next_expiration(), None);
    }

    #[test]
    fn capacity_changes_and_shutdown_return_owners_without_dropping_under_the_lock() {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        };
        struct Owner(Arc<AtomicUsize>);
        impl Drop for Owner {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let now = Instant::now();
        let drops = Arc::new(AtomicUsize::new(0));
        const LIMIT: usize = 4;
        let cache = Mutex::new(IdleCache::default());
        let mut held = cache.lock().unwrap();
        held.set_limit(LIMIT, now);
        for _ in 0..LIMIT {
            assert!(held.retain(Owner(drops.clone()), now).is_none());
        }
        let rejected = held.retain(Owner(drops.clone()), now);
        assert!(rejected.is_some());
        let excess = held.set_limit(1, now);
        assert_eq!(excess.len(), LIMIT - 1);
        let remaining = held.drain();
        assert_eq!(remaining.len(), 1);
        assert_eq!(held.next_expiration(), None);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        drop(held);
        drop((rejected, excess, remaining));
        assert_eq!(drops.load(Ordering::Relaxed), LIMIT + 1);
    }
}
