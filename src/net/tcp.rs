
use crate::meta::{Event, ConnectionInterface, EventSource, ConnectionID, ReadinessPager, Listener};

use mio::{Events, Poll, Ready, PollOpt, Token};
use mio::net::TcpStream;
use std::net::{SocketAddr, ToSocketAddrs};
use std::io::{Read, Write};

use std::collections::HashMap;

use std::sync::mpsc;

const BUFFER_SIZE: usize = 4096;
// 10 is ASCII newline
const LINE_SEPARATOR: u8 = 10;

/// Internal event type for events sent back from the listening thread.
enum LinkEvt {
    Established(ConnectionID, TcpStream),
    CouldntEstablish(ConnectionID),
    Data(ConnectionID, Vec<u8>),
    Error(ConnectionID, String),
    Eof(ConnectionID),
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

    socketreg_tx: mpsc::Sender<ConnectionRequest>,
    // This is wrapped in an Option because we want to create it when calling new(), but it does
    // need to be moved into a struct later.  (Ultimately, it is moved across thread boundaries and
    // the reader thread registers it to a Poll instance.)
    socketreg_rx: Option<mpsc::Receiver<ConnectionRequest>>,

    socketreg_sr: mio::SetReadiness,
    // This is in an Option for the same reason.
    socketreg_alert: Option<mio::Registration>,

    // This allows us to receive raw data to process from our Listener.
    listener_rx: mpsc::Receiver<LinkEvt>,
    listener_tx: mpsc::Sender<LinkEvt>,

    // A HashMap of vec<u8> used for buffering input from remote servers.
    input_buffers: HashMap<ConnectionID, Vec<u8>>
}

/// This struct represents a request to the listening thread that a new connection be started.
/// It's done in a thread because, despite the ADDITIONAL back and forth complexity, we get
/// multiple results from parsing any given address and we need to try all of them in case the
/// first one doesn't work.  (`localhost` did not work in my initial "just use the first one and
/// hope" test, I suspect because my dummy server was probably only listening on IPv4 or
/// something...?)
struct ConnectionRequest {
    addrs: Vec<SocketAddr>,
    cid: ConnectionID,
}

impl TcpConnectionManager {
    pub fn new() -> TcpConnectionManager {
        let (registration, set_readiness) = mio::Registration::new2();
        let (tx, rx) = mpsc::channel::<ConnectionRequest>();
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

        let addrs: Vec<SocketAddr> = match address.as_str().to_socket_addrs() {
            Ok(results) => results.collect(),
            Err(_) => { return Err(format!("Couldn't get address for {}", address)) },
        };

        // I consider it OKAY-ISH to panic here? and in similar cases? because if the threads are
        // unwinding in that way it means something is pretty seriously wrong with the entire
        // program. IT MIGHT BE A TERRIBLE IDEA.  This might be able to be turned into a ? some
        // day, when we get to issue 9.
        self.socketreg_tx.send(ConnectionRequest {
            addrs,
            cid: self.last_connection_id,
        }).expect("TcpConnectionManager internal error: Couldn't send() fd to reader for registration");

        self.socketreg_sr.set_readiness(Ready::readable())
              .expect("TcpConnectionManager internal error: Couldn't set_readiness() for socket registration");

        self.last_connection_id += 1;
        Ok(cid)
    }

    fn stop_connection(&mut self, _which: ConnectionID) -> Result<(), ()> {
        // TODO: Implement this
        Ok(())
    }

    fn write_to_connection(&mut self, which: ConnectionID, what: String) -> Result<(), ()> {
        // TODO: Error handling here should probably be better; it ought to return a type that
        // allows using the ? operator on I/O most likely
        match self.links.get_mut(&which) {
            Some(link) => {
                match link.write(what.as_bytes()) {
                    Err(_) => Err(()),
                    Ok(_) => Ok(()),
                }
            },
            None => Err(()),
        }
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
                pending_requests: HashMap::new(),
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
                    let buffer = self.input_buffers.entry(cid).or_insert(Vec::new());
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
                Ok(LinkEvt::Error(cid, msg)) => {
                    queue.push(Event::ConnectionEnd {
                        which: cid,
                        reason: format!("Link error: {}", msg),
                    });
                    self.links.remove(&cid); // We...probably don't care if this fails? XXX
                },
                Ok(LinkEvt::Established(cid, stream)) => {
                    queue.push(Event::ConnectionStart {
                        which: cid,
                    });
                    self.links.insert(cid, stream);
                },
                Ok(LinkEvt::CouldntEstablish(cid)) => {
                    // TODO: Should this have its own event?
                    queue.push(Event::ConnectionEnd {
                        which: cid,
                        reason: "Could not establish connection".to_string(),
                    });
                },
                Ok(LinkEvt::Eof(cid)) => {
                    queue.push(Event::ConnectionEnd {
                        which: cid,
                        reason: format!("End of connection"),
                    });
                    self.links.remove(&cid);
                },
                Err(_) => break,
            }
        }

        queue
    }
}



/// Listener impl for TcpConnectionManager; data/object for the listener thread for TCP
/// connections.
struct TcpListener {
    socketreg_rx: mpsc::Receiver<ConnectionRequest>,
    socketreg_alert: mio::Registration,
    data_tx: mpsc::Sender<LinkEvt>,

    // This is a list of pending connection requests.  We need it because any given address string,
    // when parsed/resolved, can yield a number of different possible SocketAddr's that might not
    // all be the correct one (e.g., when something is listening on IPv4 but not IPv6.)  The
    // HashMap lets us check if the connection is part of a pending request when something fails;
    // when a read or write on a connection succeeds, we remove it from pending_requests if it's
    // there.
    pending_requests: HashMap<ConnectionID, Vec<SocketAddr>>,
}


impl TcpListener {
    /// Try to connect to the next option available 
    fn try_request(&mut self, req: ConnectionID) -> Option<TcpStream> {
        if let Some(opts_left) = self.pending_requests.get_mut(&req) {
            while opts_left.len() > 0 {
                // Can unwrap() here because we know len > 0.
                let address_to_try = opts_left.pop().unwrap();

                match TcpStream::connect(&address_to_try) {
                    Ok(stream) => return Some(stream),
                    Err(_) => { },
                };
            }
        }

        // At this point, we know that either opts_left.len() == 0 (no more addresses to try, if
        // there ever were) or the request no longer exists at all.  (The loop above should always
        // have drained everything before the code gets to this point.)  We can safely try to
        // remove it from pending_requests.
        self.pending_requests.remove(&req);

        None
    }

    /// Deal with trying a connection request and taking the appropriate actions.  Called
    /// internally.
    fn handle_request(&mut self, poll: &mio::Poll, links: &mut HashMap<ConnectionID, TcpStream>, flag: &mut Box<ReadinessPager>, cid: ConnectionID) {
        match self.try_request(cid) {
            Some(stream) => {
                // We don't send Established here; it would be premature.  It can fail
                // on a read() still.
                poll.register(&stream, Token(cid), Ready::readable(), PollOpt::level()).unwrap();
                links.insert(cid, stream);
            },
            None => {
                self.data_tx.send(LinkEvt::CouldntEstablish(cid))
                    .expect("Couldn't send() LinkEvt");
                flag.ok();
            }
        }
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
                    // A ConnectionRequest has arrived.  Deal with it.
                    let request: ConnectionRequest = self.socketreg_rx.recv().unwrap();
                    let cid = request.cid;
                    self.pending_requests.insert(cid, request.addrs);
                    self.handle_request(&poll, &mut links, &mut flag, cid);
                } else {
                    // Read from a socket.  Full disclosure: This code is heavily based on an
                    // example I found randomly in mio's Token documentation.
                    //
                    // TODO: These indents are excessive, figure out how to factor out some of
                    // this.
                    let cid: usize = event.token().0;
                    let mut buffer = [0u8; BUFFER_SIZE];
                    loop {
                        // TODO: IMPORTANT -- Don't panic if it doesn't exist in the links.  Do
                        // something else, like sending an internal error and closing/deregistering
                        // or whatever seems most appropriate.
                        match links.get_mut(&cid).expect("links.get_mut").read(&mut buffer) {
                            Ok(0) => {
                                // End of the link.  Drop it on this end.  When we send the Error
                                // event, the code that owns the other copy of the connection
                                // should also drop it.
                                poll.deregister(links.get(&cid).expect("links.get"))
                                    .expect("deregister");

                                // We PROBABLY don't want to try the next address in a pending
                                // request here ... if it immediately closed the connection, it's
                                // likely whoever set up the server doesn't want us there?  In that
                                // case, it's rude to instantly poke them on another IP.  If
                                // there's a real error case where this is the very first result,
                                // that would change.

                                links.remove(&cid);
                                self.data_tx.send(LinkEvt::Eof(cid))
                                    .expect("Couldn't send Eof back to main thread");
                                flag.ok();
                                break;
                            },
                            Ok(num_bytes) => {
                                // The buffering is done after it's sent across the thread (see
                                // above code.) XXX Maybe we should have the buffering-for-lines
                                // here anyway; think about it. (For example, a misbehaving server
                                // could send us a tremendous amount of data and clog up the main
                                // thread with buffering, whereas if it clogged up the TCP I/O
                                // thread, the user might be able to notice in some cases and close
                                // the link, which would call close() on the main-thread side and
                                // put a stop to it.)
                                let mut vec = Vec::new();
                                vec.extend_from_slice(&buffer[..num_bytes]);

                                // See the comment on pending_requests for explanation.
                                if let Some(_) = self.pending_requests.get(&cid) {
                                    let new_link = links.get_mut(&cid).expect("links.get_mut")
                                        .try_clone().expect("clone link");
                                    self.data_tx.send(LinkEvt::Established(cid, new_link))
                                        .expect("Couldn't send LinkEvt::Established");
                                    self.pending_requests.remove(&cid);
                                }

                                self.data_tx.send(LinkEvt::Data(cid, vec))
                                    .expect("Couldn't send LinkEvt::Data");

                                flag.ok();
                            },
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                // The socket is not ready anymore, stop reading
                                break;
                            },
                            Err(ref e) => {
                                // We assume the link wrapped up here--that an error means we
                                // probably can't keep using it.  TODO: Do we need to (or should
                                // we) do anything to make sure e.g. close()ing?
                                poll.deregister(links.get(&cid).expect("links.get")).expect("deregister");
                                links.remove(&cid);

                                // Let the main thread know things went sideways.
                                self.data_tx.send(LinkEvt::Error(cid, format!("Problem calling read(): {}", e)))
                                    .expect("Couldn't send Error back to main thread");

                                // If there are more addresses in a pending_request, we'll try the
                                // next one of those.
                                self.handle_request(&poll, &mut links, &mut flag, cid);

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

