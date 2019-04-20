
pub type ConnectionID = usize;

/// This type of object knows about servers and contains the low-level logic for connecting and
/// listening to a particular sort of MUD server.  It returns a secondary object instead of directly
/// impl'ing EventSource for the reasons listed above.
///
/// The `address' is provided in a single String with an implementation-defined format to
/// accomodate those types of server that may not be able to be satisfied with a traditional
/// host/port pair.
pub trait ConnectionInterface {
    fn start_connection(&mut self, address: String) -> Result<ConnectionID, String>;
    fn stop_connection(&mut self, which: ConnectionID) -> Result<(), ()>;
    fn write_to_connection(&mut self, which: ConnectionID, what: String) -> Result<(), ()>;
}

pub mod tcp;
