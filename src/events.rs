
use crate::meta::{Event, EventSource, EventManager, Channel};

use std::thread;
use std::thread::JoinHandle;
use std::sync::mpsc;

/// Implementation of Channel using std::mpsc and tags of type `usize`.
struct DataChannel<T> {
    rx: Option<mpsc::Receiver<(usize, T)>>,
    tx: mpsc::Sender<(usize, T)>,
    tag: usize,
    is_rx: bool,
}

impl<T> Channel<usize, T> for DataChannel<T> {
    fn send(&mut self, value: T) -> Result<(), ()> {
        if self.is_rx {
            panic!("Tried to send() down RX end of a Channel.");
        }

        self.tx.send((self.tag, value));
    }

    fn recv(&mut self, value: T) -> (usize, T) {
        match self.rx {
            Some(rx) => rx.recv(),
            None => { panic!("Tried to recv() from TX end of a Channel."); }
        }
    }

    fn new(&mut self, tag: usize) -> DataChannel {
        let (tx, rx) = mpsc::channel::<(usize, T)>();
        DataChannel {
            rx: Some(rx),
            tx,
            tag,
            is_rx: true
        }
    }

    fn get_tx(&mut self) -> Channel {
        DataChannel {
            rx: None,
            tx: self.tx.clone(),
            tag: self.tag,
            is_rx: false
        }
    }
}

/// System to manage threads listening for data, process the data in an orderly fashion and return
/// Events to the caller.
pub struct ThreadedManager {
    channel_rx: mpsc::Receiver<Event>,
    threads: Vec<JoinHandle<()>>,
}

impl ThreadedManager {
    pub fn new() -> ThreadedManager {
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
        //
        // (Probably requires a second thread started to block on join()ing the first thread and
        // then send a special Event type back to indicate that the first thread died.)
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

