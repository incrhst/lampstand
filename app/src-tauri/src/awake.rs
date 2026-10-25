//! Keeping the computer awake from one thread of its own.
//!
//! On Windows the sleep assertion (`SetThreadExecutionState`) belongs to the
//! thread that makes it. `App` asks from whichever thread handled the
//! request: a pool thread that is retired when idle, taking the assertion
//! with it, or one that makes the assertion while another drops it. So a
//! single long-lived thread makes the assertion, holds it and drops it, and
//! everyone else only sends it a message.

use std::sync::mpsc::{self, Sender};

/// The thread that holds the assertion `G` (the keep-awake guard).
pub struct Awake {
    tx: Sender<bool>,
}

impl Awake {
    /// Starts the thread. `make` is called on it when the computer should
    /// be kept awake and returns the guard, or `None` if it could not be
    /// made; dropping the guard lets the computer sleep again. A repeated
    /// request for the state it is already in is ignored. The thread ends,
    /// dropping any guard, when the `Awake` is dropped.
    pub fn spawn<G: 'static>(make: impl Fn() -> Option<G> + Send + 'static) -> Awake {
        let (tx, rx) = mpsc::channel::<bool>();
        let started = std::thread::Builder::new()
            .name("lampstand-awake".into())
            .spawn(move || {
                let mut guard: Option<G> = None;
                let mut on = false;
                while let Ok(want) = rx.recv() {
                    if want == on {
                        continue;
                    }
                    on = want;
                    guard = if want { make() } else { None };
                }
                drop(guard);
            });
        if let Err(e) = started {
            eprintln!("lampstand: cannot keep the computer awake: {e}");
        }
        Awake { tx }
    }

    pub fn set(&self, on: bool) {
        // Nobody listening means the thread could not start: nothing to do.
        let _ = self.tx.send(on);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::thread::ThreadId;
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct Log {
        made: Vec<ThreadId>,
        dropped: Vec<ThreadId>,
    }

    struct Guard(Arc<Mutex<Log>>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0
                .lock()
                .unwrap()
                .dropped
                .push(std::thread::current().id());
        }
    }

    fn wait(log: &Arc<Mutex<Log>>, until: impl Fn(&Log) -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !until(&log.lock().unwrap()) {
            assert!(Instant::now() < end, "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_guard_is_made_and_dropped_on_the_same_thread_whoever_asks() {
        let log = Arc::new(Mutex::new(Log::default()));
        let made = log.clone();
        let awake = Arc::new(Awake::spawn(move || {
            made.lock().unwrap().made.push(std::thread::current().id());
            Some(Guard(made.clone()))
        }));

        // Asked from three different threads, one of which then ends.
        let a = awake.clone();
        std::thread::spawn(move || a.set(true)).join().unwrap();
        wait(&log, |l| l.made.len() == 1);
        // Repeated: ignored.
        awake.set(true);
        let a = awake.clone();
        std::thread::spawn(move || a.set(false)).join().unwrap();
        wait(&log, |l| l.dropped.len() == 1);
        awake.set(false);
        awake.set(true);
        wait(&log, |l| l.made.len() == 2);
        let a = awake.clone();
        std::thread::spawn(move || a.set(false)).join().unwrap();
        wait(&log, |l| l.dropped.len() == 2);

        let l = log.lock().unwrap();
        assert_eq!(l.made.len(), 2, "a repeated true makes nothing");
        let own = l.made[0];
        assert!(l.made.iter().chain(&l.dropped).all(|t| *t == own));
        assert_ne!(own, std::thread::current().id());
    }

    #[test]
    fn a_guard_still_held_is_dropped_when_the_awake_goes() {
        let log = Arc::new(Mutex::new(Log::default()));
        let made = log.clone();
        let awake = Awake::spawn(move || Some(Guard(made.clone())));
        awake.set(true);
        std::thread::sleep(Duration::from_millis(50));
        drop(awake);
        wait(&log, |l| l.dropped.len() == 1);
    }
}
