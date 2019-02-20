
use std::sync::mpsc;
use std::sync::mpsc::{Sender, Receiver};
use std::io::{Write, stdout, stdin};
use std::io;
use std::thread;

use termion::event::Key;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use termion::input::TermRead;

use signal_hook::iterator::Signals;

mod input;

/// Trait for objects that can be conceptualized as a rectangle on a grid of characters and drawn
/// on screen.
pub trait Window {
    fn draw(&self) -> Vec<String>;
    
    /// Return (width, height).
    fn get_size(&self) -> (usize, usize);

    /// Return (x, y), 1-indexed to harmonise with termion's expectations.  Unless the user will be
    /// interacting with this in a way that requires cursor positioning/movement, it's safe to just
    /// return (1,1) here.
    fn get_cursor_pos(&self) -> (usize, usize);

    // These are implemented as two functions because some objects (e.g. the input line) will want
    // to dynamically decide one of these values (in the input line's case, based on how much text
    // is present and width it's been asked to render to), so those objects are allowed to just
    // panic!() if you try to set a dynamic value.
    fn set_width(&mut self, new_w: usize);
    fn set_height(&mut self, new_h: usize);
}

pub mod screen;
// TODO: We should just scrape the `Command' type out. It's pointless indirection and introduces
// confusion as to what Commands even are, plus the possibility to break stuff less-obviously by
// changing it.
use crate::meta::{Event, EventSource, UserInterface, Command, ReadinessPager, Listener};

/// Source for events (e.g. a line of text input) originating from a terminal-based user interface,
/// and high-level implementation of that interface.
pub struct TermUiManager {
    tx_template: Sender<TermEvent>,
    rx: Receiver<TermEvent>,

    term_size: (usize, usize),
    stdout: AlternateScreen<termion::raw::RawTerminal<io::Stdout>>,

    // The DamageBuffer here is used as an abstraction on the entire terminal; we only need one of
    // these.  It allows us to think about the terminal as more of a grid buffer than a stream
    // with external state on the other end.
    db: screen::DamageBuffer,

    // Eventually, we're going to want multiple views, an input line and something like window
    // management. Placeholder:
    view: screen::WrappedView,
}

impl TermUiManager {
    /// Create a new TermUiManager.  NB: This will expect to be the only TermUiManager, and to be
    /// able to grab a stdout() instance, write to that instance (clearing/setting up the terminal)
    /// and construct the TermUiManager object with ownership of it.
    pub fn new() -> TermUiManager {
        let (tx, rx) = mpsc::channel();

        let (term_w, term_h) = termion::terminal_size().unwrap();

        let mut stdout = AlternateScreen::from(stdout().into_raw_mode().unwrap());
        write!(stdout, "{}{}", termion::clear::All, termion::cursor::Hide).unwrap();
        stdout.flush().unwrap();

        TermUiManager {
            stdout,
            rx,
            tx_template: tx,
            term_size: (term_w as usize, term_h as usize),
            db: screen::DamageBuffer::new(term_w as usize, term_h as usize),
            view: screen::WrappedView::new(term_w as usize, term_h as usize),
        }
    }
}

/// Clean up the terminal when the TermUiManager is dropped.
impl Drop for TermUiManager {
    fn drop(&mut self) {
        write!(self.stdout, "{}", termion::cursor::Show).unwrap();
        self.stdout.flush().unwrap();
    }
}

impl EventSource for TermUiManager {
    fn get_listeners(&mut self) -> Vec<Box<Listener>> {
        vec![
            Box::new(TermionListener {
                tx: self.tx_template.clone(),
            }),
            Box::new(ResizeListener {
                tx: self.tx_template.clone(),
            }),
        ]
    }

    fn process(&mut self) -> Vec<Event> {
        // The events from the thread in this case will be either terminal resize or some kind of
        // event from Termion---key, maybe eventually mouse, whatever.  So, when this is called
        // we'll deal with as many as we can read right now.
        let mut out = vec![];

        loop {
            match self.rx.try_recv() {
                Err(_) => {
                    break;
                }
                Ok(TermEvent::Resize) => {
                    let (term_w, term_h) = termion::terminal_size().unwrap();
                    let (term_w, term_h) = (term_w as usize, term_h as usize);

                    self.db.resize(term_w, term_h);
                    self.view.resize(term_w, term_h);

                    // Render the whole view and just write it wholesale to the damage buffer.
                    // Underlying assumption: CPU is much cheaper than I/O to the terminal for the
                    // costs we care about.
                    for (y, line) in self.view.render().into_iter().enumerate() {
                        self.db.write_string(0, y, line);
                    }

                    // Tell the damage buffer to terminal-update.
                    self.db.redraw(&mut self.stdout).unwrap();
                },
                Ok(TermEvent::Input { key: k }) => {
                    match k {
                        Key::Ctrl('c') => { out.push(Event::QuitRequest) },
                        // Obviously, huge TODO here.
                        _ => { },
                    };
                },
            }
        }

        out
    }
}

/// Implements the public API for adding new text data to windows in the user interface.
impl UserInterface for TermUiManager {
    fn push_to_window(&mut self, _window: String, line: String) -> Result<(), ()> {
        // Just for testing, we throw everything into a single view...  We could
        // ultimately make it a single view that drew in variously filtered ways as
        // well, or multiple views on the same text that could be filtered however you
        // like.
        self.view.push(line);

        // We could potentially refactor this into a self.redraw()? (repeated from
        // above)
        for (y, line) in self.view.render().into_iter().enumerate() {
            self.db.write_string(0, y, line);
        }

        self.db.redraw(&mut self.stdout).unwrap();
        Ok(())
    }

    fn register_command(&mut self, _c: Command) {
        // TODO
    }
}

/// Event type used internally for communication between threads.
enum TermEvent {
    Resize,
    Input { key: Key },
}

/// Listener for terminal resize events.
struct ResizeListener {
    tx: Sender<TermEvent>,
}

impl Listener for ResizeListener {
    fn run(&mut self, mut flag: Box<ReadinessPager>) {
        let sigs = Signals::new(&[libc::SIGWINCH]).expect("Couldn't create Signals iterator");
        for _signal in sigs.forever() {
            self.tx.send(TermEvent::Resize).expect("error sending TermEvent::Resize");
            flag.ok();
        }
    }
}

/// Listener for termion (e.g., key, mouse, etc.) events.
struct TermionListener {
    tx: Sender<TermEvent>,
}
impl Listener for TermionListener {
    fn run(&mut self, mut flag: Box<ReadinessPager>) {
        let stdin = stdin();
        for c in stdin.keys() {
            // TODO: In the future, when we have better error handling for EventManaged
            // threads, bounce this back to the parent thread and let it crash properly....?
            let c = c.expect("Couldn't read from stdin?!");
            self.tx.send(TermEvent::Input { key: c }).expect("error sending TermEvent::Input");
            flag.ok();
        }
    }
}
