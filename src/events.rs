
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
    where I: Sync + Send + Copy {
    rx: mpsc::Receiver<(I, StateNotice)>,
    tx: mpsc::Sender<(I, StateNotice)>,
}

impl<I> Listener<I>
    where I: Sync + Send + Copy {
    fn new() -> Listener<I> {
        let (tx, rx) = mpsc::channel::<(I, StateNotice)>();
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
    where I: Sync + Send + Copy {
    tx: mpsc::Sender<(I, StateNotice)>,
    tag: I,
}

impl<I> ReadinessPager for Pager<I>
    where I: Sync + Send + Copy {
    fn ok(&mut self) {
        self.tx.send((self.tag, StateNotice::Ready)).expect("Error send()ing to notify EventManager of readiness");
    }

    fn err(&mut self, why: String) {
        self.tx.send((self.tag, StateNotice::Error(why))).expect("Error send()ing to notify EventManager of error");
    }
}

/// System to manage threads listening for data, process the data in an orderly fashion and return
/// Events to the caller.
pub struct ThreadedManager {
    // We're using usize to disambiguate between the EventSources--since we're going to be pushing them
    // onto a Vec anyway and we don't plan to ever toss out any old entries.
    endpoint: Listener<usize>,
    sources: Vec<Rc<RefCell<EventSource>>>,
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
            endpoint: Listener::new(),
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
    fn start_source(&mut self, mut src: Rc<RefCell<EventSource>>) {
        // Note that len = index of last element + 1 (since indexes start at zero) and so is also
        // the index of the next element we'll insert into any given list.
        let new_id: usize = self.sources.len();

        let listeners = src.borrow_mut().get_listeners();
        for mut listener in listeners {
            let citizen_pager = self.endpoint.clone_tx(new_id);
            let citizen = thread::spawn(move || {
                listener.run(Box::new(citizen_pager));
            });

            // Check for a badly behaved thread dying in the case that it doesn't actually call err()
            // on its pager.
            let mut police_pager = self.endpoint.clone_tx(new_id);
            let police = thread::spawn(move || {
                match citizen.join() {
                    // TODO: This is going to be troublesome if/when threads die because we may not
                    // know which thread died from this alone.
                    Ok(_) => { police_pager.err("A thread that should run forever returned!".to_string()); },
                    Err(_) => { police_pager.err("A thread that should not have died died!".to_string()); }
                }
            });
        }

        self.sources.push(src);
    }

    /// Return the next Event.  This will return any Events that are queued up, but if the queue is empty
    /// it will wait for an Event to arrive.
    fn next_event(&mut self) -> Result<Event, String> {
        while self.events_waiting.len() < 1 {
            if self.sources.len() < 1 {
                return Err("No threads are running; would block forever".to_string());
            } else if self.poisoned {
                return Err("A fatal error has already occurred".to_string());
            } else {
                match self.endpoint.recv() {
                    (id, StateNotice::Ready) => {
                        let mut results = self.sources[id].borrow_mut().process();
                        for result in results {
                            self.events_waiting.push_back(result);
                        }
                    },
                    (id, StateNotice::Error(bad_things)) => {
                        self.poisoned = true;
                        self.events_waiting.push_back(Event::InternalError { what: bad_things });
                    },
                }
            }
        }

        // The loop above should only exit if self.events_waiting.len() is 1 or more, so
        // pop_front() should be guaranteed to function and unwrap() should be safe.
        Ok(self.events_waiting.pop_front().unwrap())
    }
}

