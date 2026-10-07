//! Latest-frame scrolling on an independent native executor. No polling timer,
//! completion handshake, or queue of historical continuous-scroll distances.

use crate::api::{BackendEvent, scroll::ScrollFrame};
use crate::support::worker::WorkerJoin;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct State {
    latest: Option<ScrollFrame>,
    stopped: bool,
}

#[derive(Default)]
struct Mailbox {
    state: Mutex<State>,
    ready: Condvar,
}

pub(crate) struct ScrollWorker {
    mailbox: Arc<Mailbox>,
    worker: WorkerJoin,
}

impl ScrollWorker {
    pub(crate) fn start(
        mut scroll: impl FnMut(f64, f64) -> Result<(), String> + Send + 'static,
        report: impl Fn(BackendEvent) + Send + 'static,
    ) -> Result<Self, String> {
        let mailbox = Arc::new(Mailbox::default());
        let shared = mailbox.clone();
        let worker = WorkerJoin::spawn(
            "continuous scrolling",
            std::thread::Builder::new().name("keysteer-scroll".into()),
            move || loop {
                let frame = {
                    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                    while !state.stopped && state.latest.is_none() {
                        state = shared.ready.wait(state).unwrap_or_else(|e| e.into_inner());
                    }
                    if state.stopped {
                        break;
                    }
                    let Some(frame) = state.latest.take() else {
                        continue;
                    };
                    frame
                };
                // Native work never holds the mailbox lock. The engine can
                // publish the newest frame or cancel a gesture immediately.
                if frame.is_current()
                    && let Err(error) = scroll(frame.dx, frame.dy)
                {
                    report(BackendEvent::InputInjectionFailed(format!(
                        "continuous scroll dx={} dy={}: {error}",
                        frame.dx, frame.dy
                    )));
                }
            },
        )?;
        Ok(Self { mailbox, worker })
    }

    pub(crate) fn submit(&self, frame: ScrollFrame) -> Result<(), String> {
        if !frame.is_current() {
            return Ok(());
        }
        let mut state = self.mailbox.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped {
            return Err("continuous scroll worker has stopped".into());
        }
        // Replace an obsolete interval; do not add its distance to the new
        // one. Latency takes precedence over catching up after native stalls.
        state.latest = Some(frame);
        drop(state);
        self.mailbox.ready.notify_one();
        Ok(())
    }

    pub(crate) fn request_stop(&self) {
        let mut state = self.mailbox.state.lock().unwrap_or_else(|e| e.into_inner());
        state.stopped = true;
        state.latest = None;
        drop(state);
        self.mailbox.ready.notify_one();
    }

    pub(crate) fn stop_until(&mut self, deadline: Instant) -> Result<(), String> {
        self.request_stop();
        self.worker.join_until(deadline)
    }
}

impl Drop for ScrollWorker {
    fn drop(&mut self) {
        self.request_stop();
        if !self.worker.shutdown_failure_was_returned()
            && let Err(error) = self.worker.join_timeout(Duration::from_secs(2))
        {
            crate::support::logging::report_error("scroll-worker", error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::scroll::ScrollSession;
    use std::sync::mpsc;

    #[test]
    fn busy_native_scroll_keeps_only_the_latest_frame_without_waiting_or_adding_distance() {
        let (injected, observed) = mpsc::channel();
        let (resume, gate) = mpsc::channel();
        let mut worker = ScrollWorker::start(
            move |dx, dy| {
                injected.send((dx, dy)).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            },
            |_| panic!("unexpected error"),
        )
        .unwrap();
        let session = ScrollSession::default();
        worker.submit(session.frame(0.0, 1.0)).unwrap();
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(2)).unwrap(),
            (0.0, 1.0)
        );
        for n in 2..=1000 {
            worker.submit(session.frame(0.0, n as f64)).unwrap();
        }
        resume.send(()).unwrap();
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(2)).unwrap(),
            (0.0, 1000.0)
        );
        // Neither 32-slot overflow nor 999 delayed native calls can occur.
        worker.request_stop();
        resume.send(()).unwrap();
        worker
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert!(observed.try_recv().is_err());
    }

    #[test]
    fn release_cancels_a_queued_frame_and_a_new_gesture_can_start_immediately() {
        let (injected, observed) = mpsc::channel();
        let (resume, gate) = mpsc::channel();
        let mut worker = ScrollWorker::start(
            move |dx, dy| {
                injected.send((dx, dy)).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            },
            |_| panic!("unexpected error"),
        )
        .unwrap();
        let session = ScrollSession::default();
        worker.submit(session.frame(0.0, 1.0)).unwrap();
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
        let stale = session.frame(0.0, 2.0);
        worker.submit(stale.clone()).unwrap();
        session.cancel();
        assert!(!stale.is_current());
        worker.submit(session.frame(0.0, -3.0)).unwrap();
        // A late publication from the old gesture cannot replace the new one.
        worker.submit(stale).unwrap();
        resume.send(()).unwrap();
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(2)).unwrap(),
            (0.0, -3.0)
        );
        worker.request_stop();
        resume.send(()).unwrap();
        worker
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn native_failure_is_reported_and_shutdown_rejects_new_frames() {
        let (sent, received) = mpsc::channel();
        let mut worker = ScrollWorker::start(
            |_, _| Err("native failure".into()),
            move |event| {
                sent.send(event).unwrap();
            },
        )
        .unwrap();
        let session = ScrollSession::default();
        worker.submit(session.frame(1.0, 2.0)).unwrap();
        assert!(
            matches!(received.recv_timeout(Duration::from_secs(2)).unwrap(),
            BackendEvent::InputInjectionFailed(error) if error.contains("native failure"))
        );
        worker
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert!(worker.submit(session.frame(1.0, 2.0)).is_err());
    }
}
