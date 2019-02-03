use crate::meta::{Event, ConnectionInterface, EventSource, ConnectionID};

extern crate mio;
use mio::{Events, Poll, Ready, PollOpt, Token};
use mio::net::TcpStream;

use std::net::SocketAddr;
use std::collections::HashMap;
use std::sync::mpsc;

use std::os::unix::io::RawFd;

pub struct TcpConnectionManager {
    links: HashMap<ConnectionID, TcpStream>,
    last_connection_id: ConnectionID,

    // We need a way to register sockets with the Poll object, but mio's Poll objects aren't so
    // easy to share across threads or data structures, and our thread owns the Poll object (see
    // the TcpListener just below.)  To solve this problem, we send RawFds for each new socket we
    // want to register along a channel, and use mio's Registraton/SetReadiness mechanism to alert
    // the polling loop.

    socketreg_tx: mpsc::Sender<RawFd>,
    // This is wrapped in an Option because we want to create it when calling new(), but it does
    // need to be moved into a struct later.  (Ultimately, it is moved across thread boundaries and
    // the reader thread registers it to a Poll instance.)
    socketreg_rx: Option<mpsc::Receiver<RawFd>>,

    socketreg_sr: mio::SetReadiness,
    // This is in an Option for the same reason.
    socketreg_alert: Option<mio::Registration>,
}

// This struct (and its implementation) is (the data used by) what runs in another thread to listen
// for events. See the ThreadedManager code in events.rs.  It has to be a separate struct because
// otherwise any EventManager objects would want to take ownership of the entire
// TcpConnectionManager object and move it into a thread, and we can't have that...
struct TcpListener {
    socketreg_rx: mpsc::Receiver<RawFd>,
    socketreg_alert: mio::Registration,
}

impl TcpConnectionManager {
    pub fn new() -> TcpConnectionManager {
        let (registration, set_readiness) = mio::Registration::new2();
        let (tx, rx) = mpsc::channel::<RawFd>();

        return TcpConnectionManager {
            links: HashMap::new(),
            last_connection_id: 0,

            socketreg_tx: tx,
            socketreg_rx: Some(rx),
            socketreg_sr: set_readiness,
            socketreg_alert: Some(registration),
        }
    }
}

impl ConnectionInterface for TcpConnectionManager {
    fn start_connection(&mut self, address: String) -> ConnectionID {
        self.last_connection_id
    }

    fn stop_connection(&mut self, which: ConnectionID) -> Result<(), ()> {
        Ok(())
    }

    fn listener(&mut self) -> Box<EventSource + Send> {
        match (self.socketreg_rx.take(), self.socketreg_alert.take()) {
            (Some(rx), Some(alert)) => Box::new(TcpListener {
                socketreg_rx: rx,
                socketreg_alert: alert,
            }),
            _ => { panic!("Cannot call listener() on ConnectionInterface more than once.") }
        }
    }
}

impl EventSource for TcpListener {
    fn run(&mut self, channel: std::sync::mpsc::Sender<Event>) {
    }
}
