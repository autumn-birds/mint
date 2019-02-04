
use crate::meta::{Event, ConnectionInterface, EventSource, ConnectionID};

extern crate mio;
use mio::{Events, Poll, Ready, PollOpt, Token};
use mio::net::TcpStream;

use std::net::SocketAddr;
use std::collections::HashMap;
use std::sync::mpsc;

use std::os::unix::io::{RawFd, AsRawFd, FromRawFd};
use std::io::Read;

const BUFFER_SIZE: usize = 4096;

pub struct TcpConnectionManager {
    links: HashMap<ConnectionID, TcpStream>,
    last_connection_id: ConnectionID,

    // We need a way to register sockets with the Poll object, but mio's Poll objects aren't so
    // easy to share across threads or data structures, and our thread owns the Poll object (see
    // the TcpListener just below.)  To solve this problem, we send RawFds for each new socket we
    // want to register along a channel, and use mio's Registraton/SetReadiness mechanism to alert
    // the polling loop.

    socketreg_tx: mpsc::Sender<Connection>,
    // This is wrapped in an Option because we want to create it when calling new(), but it does
    // need to be moved into a struct later.  (Ultimately, it is moved across thread boundaries and
    // the reader thread registers it to a Poll instance.)
    socketreg_rx: Option<mpsc::Receiver<Connection>>,

    socketreg_sr: mio::SetReadiness,
    // This is in an Option for the same reason.
    socketreg_alert: Option<mio::Registration>,
}

// This struct (and its implementation) is (the data used by) what runs in another thread to listen
// for events. See the ThreadedManager code in events.rs.  It has to be a separate struct because
// otherwise any EventManager objects would want to take ownership of the entire
// TcpConnectionManager object and move it into a thread, and we can't have that...
struct TcpListener {
    socketreg_rx: mpsc::Receiver<Connection>,
    socketreg_alert: mio::Registration,
}

// This struct represents a socket along with its ConnectionID.
struct Connection {
    socket: TcpStream,
    cid: ConnectionID,
}

impl TcpConnectionManager {
    pub fn new() -> TcpConnectionManager {
        let (registration, set_readiness) = mio::Registration::new2();
        let (tx, rx) = mpsc::channel::<Connection>();

        return TcpConnectionManager {
            links: HashMap::new(),
            // We use 1 since the listener thread wants to use 0 for its 'alert me when there's a
            // new socket to register' Token.
            last_connection_id: 1,

            socketreg_tx: tx,
            socketreg_rx: Some(rx),
            socketreg_sr: set_readiness,
            socketreg_alert: Some(registration),
        }
    }
}

impl ConnectionInterface for TcpConnectionManager {
    fn start_connection(&mut self, address: String) -> Result<ConnectionID, String> {
        let cid = self.last_connection_id;

        let addr: SocketAddr = match address.as_str().parse() {
            Ok(addr) => addr,
            Err(_) => { return Err(format!("Couldn't parse {} as an address", address)) },
        };

        let stream = match TcpStream::connect(&addr) {
            Ok(stream) => stream,
            Err(_) => { return Err("Couldn't connect".to_string()) },
        };

        // TODO: Revisit this and consider more verbose, "softer" error handling here.
        self.socketreg_tx.send(Connection {
            socket: stream.try_clone().unwrap(),
            cid: cid
        }).expect("TcpConnectionManager internal error: Couldn't send() fd to reader for registration");

        self.socketreg_sr.set_readiness(Ready::readable())
              .expect("TcpConnectionManager internal error: Couldn't set_readiness() for socket registration");

        self.links.insert(cid, stream);
        self.last_connection_id += 1;
        Ok(cid)
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
        // TODO: See the comment in ThreadedManager (events.rs).  Make this thread return an
        // appropriate Result type to where we can use `?` unstead of unwrap(), and watch for that
        // as noted there.
        let mut poll = Poll::new().unwrap();
        let mut events = Events::with_capacity(128);
        let mut links = HashMap::new();

        poll.register(&self.socketreg_alert, Token(0), Ready::readable(), PollOpt::edge()).unwrap();

        loop {
            poll.poll(&mut events, None).unwrap();
            for event in &events {
                if event.token() == Token(0) {
                    // We're being told we have a new socket to register.
                    // TODO: We may want to make sure we don't block forever if this is fired
                    // erroneously, but for now we can hope it's never fired erroneously.
                    let new_link: Connection = self.socketreg_rx.recv().unwrap();
                    let stream = new_link.socket;
                    poll.register(&stream, Token(new_link.cid), Ready::readable(), PollOpt::level()).unwrap();
                    links.insert(new_link.cid, stream);
                } else {
                    // Read from a socket.  Full disclosure: This code is heavily based on an
                    // example I found randomly in mio's Token documentation.
                    let cid: usize = event.token().0;
                    // TODO: We should probably be maintaining actual buffers for this, between
                    // calls.  Or some sort of pending-message storage, in case one call doesn't
                    // produce a complete line.  For now (for testing) this will do.
                    let mut buffer = [0u8; BUFFER_SIZE];
                    loop {
                        // TODO: Again, error handling?
                        match links.get_mut(&cid).expect("links.get_mut").read(&mut buffer) {
                            Ok(0) => {
                                // End of the link.  Drop it on this end.  TODO: The
                                // TcpConnectionManager instance in the main thread will probably
                                // still retain its link, and that's not the best thing.  (We could
                                // require that stop_connection() be called, but that's less than
                                // ideal, because it means we have a non-obvious API that the
                                // outside world needs to honor.  Ideally we'd be able to tend all
                                // of our own internal state here.)
                                poll.deregister(links.get(&cid).expect("links.get"));
                                links.remove(&cid);
                                channel.send(Event::ConnectionEnd {
                                    which: cid,
                                    reason: "Socket closed.".to_string(),
                                }).expect("Couldn't send ConnectionEnd");
                                break;
                            },
                            Ok(num_bytes) => {
                                // TODO: Actual handling of, like, lines (e.g. buffer until \n)
                                channel.send(Event::ServerText {
                                    which: cid,
                                    line: String::from_utf8_lossy(&buffer[..num_bytes]).to_string(),
                                }).expect("Couldn't send ServerText");
                            },
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                // The socket is not ready anymore, stop reading
                                break;
                            },
                            Err(e) => {
                                // TODO: It's probably better to just act like the link ended??  Or
                                // at least assume the worst, *try to* clean up the link by closing
                                // the socket, and then send our disconnection events and drop it
                                // like we normally would...
                                poll.deregister(links.get(&cid).expect("links.get"));
                                links.remove(&cid);
                                channel.send(Event::ConnectionEnd {
                                    which: cid,
                                    reason: format!("Unexpected read error: {:?}", e),
                                }).expect("Couldn't send ConnectionEnd on unexpected read error");
                                break;
                            },
                        }
                    }
                }
            }
        }
    }
}

