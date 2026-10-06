//! One executing sample and one replaceable request, with generation cancellation.
use crate::api::{
    Color,
    point_sample::{Request, Sample},
};
use crate::support::worker::WorkerJoin;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Default)]
struct State {
    latest: Option<Option<Request>>,
    epoch: u64,
    stop: bool,
}

pub(crate) struct Worker {
    shared: Arc<(Mutex<State>, Condvar)>,
    join: WorkerJoin,
}

impl Worker {
    pub(crate) fn new<S: FnMut(Option<Request>) -> Option<Color> + 'static>(
        create: impl FnOnce() -> S + Send + 'static,
        publish: impl Fn(Sample) + Send + 'static,
    ) -> Result<Self, String> {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let background = shared.clone();
        let join = WorkerJoin::spawn(
            "point-sample",
            std::thread::Builder::new().name("point-sample".into()),
            move || {
                let mut sample = create();
                loop {
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
                    let color = sample(request);
                    if let Some(request) = request {
                        let current = {
                            let state = background.0.lock().unwrap_or_else(|e| e.into_inner());
                            !state.stop && state.epoch == epoch
                        };
                        if current {
                            publish(Sample { request, color });
                        }
                    }
                }
            },
        )?;
        Ok(Self { shared, join })
    }

    pub(crate) fn submit(&self, request: Option<Request>) {
        let mut state = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        state.epoch = state.epoch.wrapping_add(1);
        state.latest = Some(request);
        self.shared.1.notify_one();
    }

    pub(crate) fn stop(&mut self) -> Result<(), String> {
        {
            let mut state = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
            state.stop = true;
            state.latest = None;
        }
        self.shared.1.notify_one();
        self.join.join_timeout(Duration::from_millis(500))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            crate::support::logging::report_error("point-sample", error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn samples_coalesce_cancel_and_release_resources_off_the_caller() {
        let (started, received) = std::sync::mpsc::channel();
        let (resume, wait) = std::sync::mpsc::channel();
        let (publish, result) = std::sync::mpsc::channel();
        let mut worker = Worker::new(
            move || {
                move |request: Option<Request>| {
                    started.send(request.map(|r| r.id)).unwrap();
                    if request.is_some_and(|r| r.id == 1) {
                        wait.recv_timeout(Duration::from_secs(2)).unwrap();
                    }
                    Some(Color::rgb(1, 2, 3))
                }
            },
            move |sample| {
                publish.send(sample).unwrap();
            },
        )
        .unwrap();
        let request = |id| {
            Some(Request {
                id,
                point: Default::default(),
            })
        };
        worker.submit(request(1));
        assert_eq!(
            received.recv_timeout(Duration::from_secs(2)).unwrap(),
            Some(1)
        );
        for id in 2..100 {
            worker.submit(request(id));
        }
        worker.submit(None);
        resume.send(()).unwrap();
        assert_eq!(received.recv_timeout(Duration::from_secs(2)).unwrap(), None);
        assert!(result.try_recv().is_err());
        worker.submit(request(100));
        assert_eq!(
            result
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .request
                .id,
            100
        );
        worker.stop().unwrap();
    }
}
