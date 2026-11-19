//! A single fixed watchdog terminates the sidecar when an in-flight frame expires.

use std::io;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

enum Event {
    Start(Instant),
    Finish(Instant),
}

pub struct Watchdog {
    sender: SyncSender<Event>,
}

impl Watchdog {
    pub fn new() -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("prism-deadline".into())
            .stack_size(64 * 1024)
            .spawn(move || {
                while let Ok(event) = receiver.recv() {
                    let Event::Start(deadline) = event else {
                        std::process::exit(124);
                    };
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    match receiver.recv_timeout(remaining) {
                        Ok(Event::Finish(finished)) if finished <= deadline => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                        _ => std::process::exit(124),
                    }
                }
            })?;
        Ok(Self { sender })
    }
    pub fn start(&self, timeout: Duration) -> io::Result<Instant> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| io::Error::other("invalid deadline"))?;
        self.sender
            .send(Event::Start(deadline))
            .map_err(|_| io::Error::other("deadline monitor unavailable"))?;
        Ok(deadline)
    }
    pub fn finish(&self) -> io::Result<()> {
        self.sender
            .send(Event::Finish(Instant::now()))
            .map_err(|_| io::Error::other("deadline monitor unavailable"))
    }
}
