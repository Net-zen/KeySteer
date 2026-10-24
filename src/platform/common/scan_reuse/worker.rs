//! Keep non-Send resources on one source thread; join each job independently.

use super::IdleCache;
use crate::support::worker::WorkerJoin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

type Work<T> = Box<dyn FnOnce(&mut IdleCache<T>) + Send>;
struct Task<T> {
    work: Work<T>,
    finished: JobFinished,
}
struct Thread<T> {
    sender: Option<mpsc::SyncSender<Task<T>>>,
    join: WorkerJoin,
}

pub(crate) struct IdleWorker<T: 'static> {
    thread: Mutex<Option<Thread<T>>>,
    stopping: Arc<AtomicBool>,
}
impl<T: 'static> Default for IdleWorker<T> {
    fn default() -> Self {
        Self {
            thread: Mutex::new(None),
            stopping: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl<T: 'static> IdleWorker<T> {
    /// Submission never waits for active work. The source's coordinator owns
    /// ordering/cancellation, with at most one queued job as a final bound.
    pub(crate) fn submit(
        &self,
        name: &'static str,
        work: impl FnOnce(&mut IdleCache<T>) + Send + 'static,
    ) -> Result<JobJoin, String> {
        let mut thread = self.thread.lock().unwrap_or_else(|e| e.into_inner());
        if self.stopping.load(Ordering::Acquire) {
            return Err(format!("{name} is stopping"));
        }
        if let Some(owner) = thread.as_mut() {
            match owner.join.reap_finished() {
                Ok(false) => {}
                result => {
                    if let Err(error) = result {
                        crate::support::logging::report_error("scan-reuse", error);
                    }
                    *thread = None;
                }
            }
        }
        if thread.is_none() {
            let (sender, receiver) = mpsc::sync_channel::<Task<T>>(1);
            let stopping = Arc::clone(&self.stopping);
            let join = WorkerJoin::spawn(
                name,
                std::thread::Builder::new().name(name.into()),
                move || {
                    // T need not be Send: it is constructed, used and destroyed here.
                    let mut idle = IdleCache::default();
                    idle.set_limit(1, Instant::now());
                    loop {
                        drop(idle.take_expired(Instant::now()));
                        if stopping.load(Ordering::Acquire) {
                            break;
                        }
                        let task = if let Some(until) = idle.next_expiration() {
                            match receiver
                                .recv_timeout(until.saturating_duration_since(Instant::now()))
                            {
                                Ok(task) => task,
                                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            }
                        } else {
                            match receiver.recv() {
                                Ok(task) => task,
                                Err(_) => break,
                            }
                        };
                        if stopping.load(Ordering::Acquire) {
                            drop(task);
                            break;
                        }
                        let Task { work, mut finished } = task;
                        work(&mut idle);
                        finished.complete();
                    }
                },
            )?;
            *thread = Some(Thread {
                sender: Some(sender),
                join,
            });
        }
        let (sender, finished) = mpsc::channel();
        let task = Task {
            work: Box::new(work),
            finished: JobFinished(Some(sender)),
        };
        let result = thread
            .as_ref()
            .and_then(|thread| thread.sender.as_ref())
            .ok_or_else(|| format!("{name} has no request channel"))?
            .try_send(task);
        // Rejected closures can own native inputs; release them outside the lock.
        drop(thread);
        result.map_err(|_| format!("{name} is busy or has stopped"))?;
        Ok(JobJoin {
            name,
            finished,
            result: None,
        })
    }

    pub(crate) fn request_stop(&self) {
        self.stopping.store(true, Ordering::Release);
        let sender = self
            .thread
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
            .and_then(|thread| thread.sender.take());
        drop(sender);
    }

    pub(crate) fn stop_until(&self, deadline: Instant) -> Result<(), String> {
        self.request_stop();
        let mut thread = self.thread.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(owner) = thread.as_mut() {
            owner.join.join_until(deadline)?;
        }
        *thread = None;
        Ok(())
    }

    pub(crate) fn shutdown_failure_was_returned(&self) -> bool {
        self.thread
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|thread| thread.join.shutdown_failure_was_returned())
    }

    pub(crate) fn finish_drop_until(&self, deadline: Instant) {
        self.request_stop();
        if !self.shutdown_failure_was_returned()
            && let Err(error) = self.stop_until(deadline)
        {
            crate::support::logging::report_error("scan-reuse", error);
        }
    }
}

impl<T: 'static> Drop for IdleWorker<T> {
    fn drop(&mut self) {
        self.finish_drop_until(Instant::now() + Duration::from_millis(500));
    }
}

struct JobFinished(Option<mpsc::Sender<Result<(), String>>>);
impl JobFinished {
    fn complete(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(Ok(()));
        }
    }
}
impl Drop for JobFinished {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(Err("scan source stopped before completing its job".into()));
        }
    }
}

/// Completion of native work, independent of the source's idle reuse lifetime.
pub(crate) struct JobJoin {
    name: &'static str,
    finished: mpsc::Receiver<Result<(), String>>,
    result: Option<Result<(), String>>,
}
impl JobJoin {
    pub(crate) fn reap_finished(&mut self) -> Result<bool, String> {
        if self.result.is_none() {
            match self.finished.try_recv() {
                Ok(result) => self.result = Some(result),
                Err(mpsc::TryRecvError::Empty) => return Ok(false),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.result = Some(Err(format!("{} lost its completion owner", self.name)));
                }
            }
        }
        self.result
            .as_ref()
            .map_or(Ok(false), |result| result.clone().map(|_| true))
    }

    pub(crate) fn join_until(&mut self, deadline: Instant) -> Result<(), String> {
        if !self.reap_finished()? {
            self.result = Some(
                self.finished
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .map_err(|_| {
                        format!(
                            "{} did not complete before the shutdown deadline",
                            self.name
                        )
                    })?,
            );
        }
        self.reap_finished().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use std::thread::{self, ThreadId};

    struct NativeOwner {
        thread: ThreadId,
        dropped: mpsc::Sender<ThreadId>,
        _not_send: Rc<()>,
    }
    impl Drop for NativeOwner {
        fn drop(&mut self) {
            assert_eq!(self.thread, thread::current().id());
            let _ = self.dropped.send(self.thread);
        }
    }

    #[test]
    fn completed_jobs_reuse_non_send_owners_and_expire_on_their_source_thread() {
        let worker = IdleWorker::<NativeOwner>::default();
        let (dropped, drops) = mpsc::channel();
        let (observed, observations) = mpsc::channel();
        let first_observed = observed.clone();
        let mut first = worker
            .submit("scan reuse regression", move |idle| {
                let owner = NativeOwner {
                    thread: thread::current().id(),
                    dropped,
                    _not_send: Rc::new(()),
                };
                first_observed.send(owner.thread).unwrap();
                assert!(idle.retain(owner, Instant::now()).is_none());
            })
            .unwrap();
        first
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        let owner_thread = observations.recv().unwrap();
        assert_ne!(owner_thread, thread::current().id());
        assert!(drops.try_recv().is_err());
        let mut second = worker
            .submit("scan reuse regression", move |idle| {
                let (owner, expired) = idle.take(Instant::now());
                assert!(expired.is_empty());
                let owner = owner.expect("completed resource was retained");
                observed.send(owner.thread).unwrap();
                // Exercise the real idle wake without sleeping for ten seconds.
                let near_expiry =
                    Instant::now() - super::super::IDLE_TIMEOUT + Duration::from_millis(30);
                assert!(idle.retain(owner, near_expiry).is_none());
            })
            .unwrap();
        second
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(observations.recv().unwrap(), owner_thread);
        assert_eq!(
            drops.recv_timeout(Duration::from_secs(2)).unwrap(),
            owner_thread
        );
        worker
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn shutdown_keeps_active_owners_and_rejects_queued_work_until_actual_return() {
        let worker = IdleWorker::<NativeOwner>::default();
        let (entered, entry) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let (dropped, drops) = mpsc::channel();
        let mut active = worker
            .submit("blocked scan regression", move |idle| {
                let owner = NativeOwner {
                    thread: thread::current().id(),
                    dropped,
                    _not_send: Rc::new(()),
                };
                entered.send(owner.thread).unwrap();
                blocked.recv().unwrap();
                assert!(idle.retain(owner, Instant::now()).is_none());
            })
            .unwrap();
        let owner_thread = entry.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut queued = worker
            .submit("blocked scan regression", |_| {
                panic!("stopped pending work ran")
            })
            .unwrap();
        assert!(
            worker
                .submit("blocked scan regression", |_| panic!(
                    "queue exceeded its bound"
                ))
                .is_err()
        );
        assert!(worker.stop_until(Instant::now()).is_err());
        assert!(!active.reap_finished().unwrap());
        assert!(drops.try_recv().is_err());
        assert!(worker.submit("blocked scan regression", |_| {}).is_err());
        release.send(()).unwrap();
        active
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        worker
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert!(
            queued
                .join_until(Instant::now() + Duration::from_secs(2))
                .is_err()
        );
        assert_eq!(
            drops.recv_timeout(Duration::from_secs(2)).unwrap(),
            owner_thread
        );
    }
}
