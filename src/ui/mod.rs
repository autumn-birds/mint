// TODO: Consider how specification of the arguments for commands ought to work, or if it ought
// to be a thing in the first place.
pub type Command = String;

/// UserInterface trait: This object type knows about the logistical details of handling UI, like drawing to the screens.
///
pub trait UserInterface {
    /// The way windows work is that any unique named window you try to send text to should be
    /// created by the UI code. Which windows are visible at any given time, and how that activity
    /// is surface to the user, is the UI code's business.
    fn push_to_window(&mut self, window: String, line: String) -> Result<(), ()>;
    fn register_command(&mut self, c: Command);
}

pub mod term;
