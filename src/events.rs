
use crate::meta::{Event, EventSource, EventManager};

use std::thread;
use std::thread::JoinHandle;
use std::sync::mpsc;

pub struct ThreadedManager {
    channel_rx: mpsc::Receiver<Event>,
    channel_tx: mpsc::Sender<Event>,
    threads: Vec<JoinHandle<()>>,
}

impl ThreadedManager {
    fn new() -> ThreadedManager {
        let (tx, rx) = mpsc::channel::<Event>();
        ThreadedManager {
            channel_rx: rx,
            channel_tx: tx,
            threads: vec![],
        }
    }
}

impl EventManager for ThreadedManager {
    fn start_source(&mut self, mut src: Box<EventSource + Send>) {
        let their_tx = self.channel_tx.clone();
        let them = thread::spawn(move || {
            src.run(their_tx);
        });
        self.threads.push(them);
    }

    fn next_event(&mut self) -> Result<Event, String> {
        // TODO: In the future it would be good to have some kind of check on whether the threads
        // in question are still running.  Unfortunately I'm...not really clear on how I'd do that
        // right now.
        if self.threads.len() < 1 {
            Err("No threads are running; would block forever".to_string())
        } else {
            match self.channel_rx.recv() {
                Ok(x) => Ok(x),
                Err(_) => Err("recv() error (no more messages ever?)".to_string()),
            }
        }
    }
}

