use std::{
    io,
    sync::{Arc, Mutex, mpsc},
    thread,
};

type Job = Box<dyn FnOnce() + Send + 'static>;

/// A fixed set of threads that answer private protocol requests. Artwork and
/// media requests arrive in bursts, and one thread per request would let a
/// cover grid open dozens of simultaneous downloads and file reads.
pub struct ProtocolWorkers {
    sender: mpsc::Sender<Job>,
}

impl ProtocolWorkers {
    pub fn start(name: &'static str, count: usize) -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        for index in 0..count.max(1) {
            let receiver = Arc::clone(&receiver);
            thread::Builder::new()
                .name(format!("{name}-{index}"))
                .spawn(move || {
                    loop {
                        let job = match receiver.lock() {
                            Ok(receiver) => receiver.recv(),
                            Err(_) => return,
                        };
                        match job {
                            Ok(job) => job(),
                            Err(_) => return,
                        }
                    }
                })?;
        }
        Ok(Self { sender })
    }

    /// Queues a request; when the workers are gone the request is dropped and
    /// the webview sees a failed load rather than a hang.
    pub fn run(&self, job: impl FnOnce() + Send + 'static) {
        let _ = self.sender.send(Box::new(job));
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        sync::{Arc, Mutex, mpsc},
        thread,
        time::Duration,
    };

    use super::ProtocolWorkers;

    #[test]
    fn runs_every_job_on_a_bounded_set_of_threads() {
        let workers = ProtocolWorkers::start("test-workers", 3).expect("workers");
        let names = Arc::new(Mutex::new(BTreeSet::new()));
        let (done, finished) = mpsc::channel();
        for _ in 0..24 {
            let names = Arc::clone(&names);
            let done = done.clone();
            workers.run(move || {
                thread::sleep(Duration::from_millis(2));
                names
                    .lock()
                    .expect("names")
                    .insert(thread::current().name().map(str::to_owned));
                done.send(()).expect("report");
            });
        }
        for _ in 0..24 {
            finished
                .recv_timeout(Duration::from_secs(5))
                .expect("every job completes");
        }
        let names = names.lock().expect("names");
        assert_eq!(names.len(), 3);
        assert!(names.iter().all(|name| {
            name.as_deref()
                .is_some_and(|name| name.starts_with("test-workers-"))
        }));
    }
}
