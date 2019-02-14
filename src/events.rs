
use crate::meta::{Event, EventSource, EventManager, ReadinessPager};

use std::thread;
use std::sync::mpsc;
use std::collections::VecDeque;
use std::rc::Rc;
use std::cell::RefCell;

/// A notice sent by a child thread, either 'data is ready' or 'fatal error.'
enum StateNotice {
    Ready,
    Error(String),
}

/// Listener for readiness/error notices sent out by child threads.  The type parameter `I`
/// indicates what type of unique ID to use for disambiguating the events.
struct Listener<I>
    where I: Sync + Send {
    rx: mpsc::Receiver<(I, StateNotice)>,
    tx: mpsc::Sender<(I, StateNotice)>,
}

impl<I> Listener<I>
    where I: Sync + Send {
    fn new() -> Listener<I> {
        let (rx, tx) = mpsc::channel::<I>();
        Listener { 
            rx, tx
        }
    }

    fn clone_tx(&self, tag: I) -> Pager<I> {
        Pager {
            tx: self.tx.clone(),
            tag: tag,
        }
    }

    fn recv(&self) -> (I, StateNotice) {
        self.rx.recv().expect("Couldn't receive any more readiness signals")
    }
}

/// Implementation of ReadinessPager for the ThreadedManager.
struct Pager<I>
    where I: Sync + Send {
    tx: mpsc::Sender<(I, StateNotice)>,
    tag: I,
}

impl<I> ReadinessPager for Pager<I>
    where I: Sync + Send {
    fn ok(&mut self) {
        self.tx.send((self.tag, StateNotice::Ready)).expect("Error send()ing to notify EventManager of readiness");
    }

    fn err(&mut self, why: String) {
        self.tx.send((self.tag, StateNotice::Error(why))).expect("Error send()ing to notify EventManager of error");
    }
}

type ESrc = Box<EventSource>;

/// System to manage threads listening for data, process the data in an orderly fashion and return
/// Events to the caller.
pub struct ThreadedManager {
    // We're using usize to disambiguate between the EventSources--since we're going to be pushing them
    // onto a Vec anyway and we don't plan to ever toss out any old entries.
    endpoint: Listener<usize>,
    sources: Vec<Rc<RefCell<ESrc>>>,
    // Has a fatal error occurred?  (If so, we want to refuse to do anything.)
    poisoned: bool,
    // Any time we receive more than one event, we 'cache' the events so that we can return one at
    // a time to the caller.  Hopefully it's fast about consuming them.
    events_waiting: VecDeque<Event>,
}

impl ThreadedManager {
    pub fn new() -> ThreadedManager {
        let (tx, rx) = mpsc::channel::<Event>();
        ThreadedManager {
            endpoint: Listener::new::<usize>(),
            sources: vec![],
            poisoned: false,
            events_waiting: VecDeque::new(),
        }
    }
}

impl EventManager for ThreadedManager {
    /// Add a source.  This takes Rc<RefCell<Box<EventSource>>> instead of consuming the value and
    /// wrapping it internally because the caller needs to maintain a handle to the *specific*
    /// implementation in some cases, and if the only remaining reference is a dyn EventSource-type
    /// object, you won't be able to access anything that isn't a generic EventSource method.
    fn start_source(&mut self, mut src: Rc<RefCell<Box<EventSource>>>) {
        let new_id: usize = self.sources.len();
        // Note that len = index of last element + 1 (since indexes start at zero) and so is also
        // the index of the next element we'll insert into any given list.
        let listeners_pager = self.endpoint.clone_tx(new_id);
        let listener = src.borrow_mut().get_listener();

        let citizen = thread::spawn(move || {
            listener.run(listeners_pager);
        });

        // Check for a badly behaved thread dying in the case that it doesn't actually call err()
        // on its pager.
        let police_pager = self.endpoint.clone_tx(new_id);
        let police = thread::spawn(move || {
            match citizen.join() {
                // TODO: This is going to be troublesome if/when threads die because we may not
                // know which thread died from this alone.
                Ok(_) => { police_pager.err("A thread that should run forever returned!".to_string()); },
                Err(_) => { police_pager.err("A thread that should not have died died!".to_string()); }
            }
        });

        self.sources.push(src);
    }

    /// Return the next Event.  This will return any Events that are queued up, but if the queue is empty
    /// it will wait for an Event to arrive.
    fn next_event(&mut self) -> Result<Event, String> {
        while self.events_waiting.len() < 1 {
            if self.src.len() < 1 {
                return Err("No threads are running; would block forever".to_string());
            } else if self.poisoned {
                return Err("A fatal error has already occurred".to_string());
            } else {
                let next_sig = match self.endpoint.recv() {
                    Ok(x) => x,
                    Err(_) => return Err("recv() error (no more messages ever?)".to_string()),
                };

                match next_sig {
                    (id, StateNotice::Ready) => {
                        let mut results = self.sources[id].borrow_mut().process();
                        results.for_each(|result| self.events_waiting.push_back(result));
                    },
                    (id, StateNotice::Error(bad_things)) => {
                        self.poisoned = true;
                        self.events_waiting.push_back(Event::InternalError { what: bad_things });
                    },
                }
            }
        }

        Ok(self.events_waiting.pop_front())
    }
}

