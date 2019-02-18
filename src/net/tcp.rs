
use crate::meta::{Event, ConnectionInterface, EventSource, ConnectionID, ReadinessPager, Listener};

use mio::{Events, Poll, Ready, PollOpt, Token};
use mio::net::TcpStream;

use std::net::SocketAddr;
use std::collections::HashMap;
use std::sync::mpsc;

use std::io::Read;

const BUFFER_SIZE: usize = 4096;
// 10 is ASCII newline
const LINE_SEPARATOR: u8 = 10;

/// Internal event type for TCP connection data and/or errors.
enum LinkEvt {
    Data(usize, Vec<u8>),
    Error(usize),
    Eof(usize),
}

/// EventSource for TCP connections.
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

    // This allows us to receive raw data to process from our Listener.
    listener_rx: mpsc::Receiver<LinkEvt>,
    listener_tx: mpsc::Sender<LinkEvt>,

    // A HashMap of vec<u8> used for buffering input from remote servers.
    input_buffers: HashMap<ConnectionID, Vec<u8>>
}

/// Listener impl for TcpConnectionManager.
struct TcpListener {
    socketreg_rx: mpsc::Receiver<Connection>,
    socketreg_alert: mio::Registration,
    data_tx: mpsc::Sender<LinkEvt>,
}

/// This struct represents a socket along with its ConnectionID.
struct Connection {
    socket: TcpStream,
    cid: ConnectionID,
}

impl TcpConnectionManager {
    pub fn new() -> TcpConnectionManager {
        let (registration, set_readiness) = mio::Registration::new2();
        let (tx, rx) = mpsc::channel::<Connection>();
        let (tx2, rx2) = mpsc::channel::<LinkEvt>();

        return TcpConnectionManager {
            links: HashMap::new(),
            // We use 1 since the listener thread wants to use 0 for its 'alert me when there's a
            // new socket to register' Token.
            last_connection_id: 1,

            socketreg_tx: tx,
            socketreg_rx: Some(rx),
            socketreg_sr: set_readiness,
            socketreg_alert: Some(registration),

            listener_tx: tx2,
            listener_rx: rx2,

            input_buffers: HashMap::new(),
        }
    }
}

impl ConnectionInterface for TcpConnectionManager {
    fn start_connection(&mut self, address: String) -> Result<ConnectionID, String> {
        let cid = self.last_connection_id;

        // TODO: Support something fancier, e.g., that looks up DNS.
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

    fn stop_connection(&mut self, _which: ConnectionID) -> Result<(), ()> {
        // TODO: Implement this
        Ok(())
    }

    fn write_to_connection(&mut self, _which: ConnectionID, _what: String) -> Result<(), ()> {
        // TODO: Implement this
        Ok(())
    }

}

impl EventSource for TcpConnectionManager {
    fn get_listeners(&mut self) -> Vec<Box<Listener>> {
        // Just return an event listener, but we can only do this once as it's not possible to have
        // two rx ends.  (It would actually be a logical error if this was ever called twice on
        // anything I think? Unless you were restarting it...)
        match (self.socketreg_rx.take(), self.socketreg_alert.take()) {
            (Some(rx), Some(alert)) => vec![Box::new(TcpListener {
                socketreg_rx: rx,
                socketreg_alert: alert,
                data_tx: self.listener_tx.clone(),
            })],
            _ => { panic!("Cannot call listener() on ConnectionInterface more than once.") }
        }
    }

    fn process(&mut self) -> Vec<Event> {
        // Process input from the thread... this mostly just transcribes the input into Events
        // right now, but it needs to get more complicated and handle at least EOL buffering and
        // Telnet, later.
        let mut queue = vec![];

        loop {
            match self.listener_rx.try_recv() {
                Ok(LinkEvt::Data(cid, mut what)) => {
                    let mut buffer = self.input_buffers.entry(cid).or_insert(Vec::new());
                    buffer.append(&mut what);

                    // Drain all the *complete* lines out of the buffer and push them into the
                    // queue as Event::ServerText objects.
                    while buffer.contains(&LINE_SEPARATOR) {
                        let line = buffer.split(|c| *c == LINE_SEPARATOR).next().unwrap();
                        queue.push(Event::ServerText {
                            which: cid,
                            line: String::from_utf8_lossy(&line).to_string(),
                        });
                        buffer.drain(0..line.len() + 1);
                    }
                },
                Ok(LinkEvt::Error(cid)) => {
                    queue.push(Event::ConnectionEnd {
                        which: cid,
                        reason: format!("Error trying to read from the TcpStream"),
                    });
                },
                Ok(LinkEvt::Eof(cid)) => {
                    queue.push(Event::ConnectionEnd {
                        which: cid,
                        reason: format!("End of connection"),
                    });
                },
                Err(_) => break,
            }
        }

        queue
    }
}

impl Listener for TcpListener {
    fn run(&mut self, mut flag: Box<ReadinessPager>) {
        // TODO: See the comment in ThreadedManager (events.rs).  Make this thread return an
        // appropriate Result type to where we can use `?` unstead of unwrap(), and watch for that
        // as noted there.
        let poll = Poll::new().unwrap();
        let mut events = Events::with_capacity(128);
        let mut links = HashMap::new();

        // Register the alert object we're using to wake up when it's time to add a socket to our
        // inventory (e.g. register it with the poll.)
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
                        // TODO: IMPORTANT -- Don't panic if it doesn't exist in the links.  Do
                        // something else, like sending an internal error and closing/deregistering
                        // or whatever seems most appropriate.
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
                                self.data_tx.send(LinkEvt::Eof(cid))
                                    .expect("Couldn't send Eof back to main thread");
                                flag.ok();
                                break;
                            },
                            Ok(num_bytes) => {
                                // TODO: Actual handling of, like, lines (e.g. buffer until \n)
                                let mut vec = Vec::new();
                                vec.extend_from_slice(&buffer[..num_bytes]);
                                self.data_tx.send(LinkEvt::Data(cid, vec))
                                    .expect("Couldn't send Data back to main thread");
                                flag.ok();
                            },
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                // The socket is not ready anymore, stop reading
                                break;
                            },
                            Err(_e) => {
                                // We assume the link wrapped up here--that an error means we
                                // probably can't keep using it.  TODO: Do we need to (or should
                                // we) do anything to make sure e.g. close()ing?
                                poll.deregister(links.get(&cid).expect("links.get"));
                                links.remove(&cid);
                                self.data_tx.send(LinkEvt::Error(cid))
                                    .expect("Couldn't send Error back to main thread");
                                flag.ok();
                                break;
                            },
                        }
                    }
                }
            }
        }
    }
}

