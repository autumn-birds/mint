
pub type ConnectionID = usize;
pub type WindowID = usize;

pub enum Event {
    // We will want to be able to discriminate which _window_ in the UI it came from, not which
    // connection it should go to.  That is, the UI doesn't know anything about the mapping of
    // windows to connections.)
    UserInput { line: String, which: WindowID },
    // Obviously, server text just knows which connection it came from.
    ServerText { line: String, which: ConnectionID },
    // ...
}

pub trait EventSource {
    // Objects that can provide Events impl this.

    fn run(&mut self, channel: std::sync::mpsc::Sender<Event>);
}

pub trait EventManager {
    // This object deals with connections and events. And specifically, running and wrangling the
    // threads that produce them.

    fn add(&mut self, src: EventSource);
    fn next_event(&mut self) -> Event;
}

pub trait Window {
    // This object represents a single 'window', or view on a history of lines of text, which can
    // (through whatever mechanism in the user interface) be specifically sent input (e.g., by
    // having its own input field, or by having "focus" if there is only one input field.)

    fn push(&mut self, line: String);
}

pub trait UserInterface {
    // This object knows about the logistical details of handling UI, like drawing to the screens.
    //
    // It should always impl EventSource.  It needs to generate Events, such as when text input is
    // sent from the input pane.  Technically, it can react to some inputs on its own and only
    // generate Events for what the rest of the system needs to know about.

    fn add_window(&mut self, w: Window) -> WindowID;
    fn remove_window(&mut self, w: WindowID);
    fn push_to_window(&mut self, w: WindowID, line: String) -> Result<(), ()>;
}

pub trait ConnectionInterface {
    // This type of object knows about servers and contains the low-level logic for connecting and
    // listening to a particular sort of MUD server.
    //
    // These objects should always impl EventSource.
    //
    // The `address' is provided in a single String with an implementation-defined format to
    // accomodate those types of server that may not be able to be satisfied with a traditional
    // host/port pair.

    fn start_connection(&mut self, address: String) -> ConnectionID;
    fn stop_connection(&mut self, which: ConnectionID) -> Result<(), ()>;
}

