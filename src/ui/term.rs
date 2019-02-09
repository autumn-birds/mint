
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

pub mod screen;
// TODO: We should just scrape the `Command' type out. It's pointless indirection and introduces
// confusion as to what Commands even are, plus the possibility to break stuff less-obviously by
// changing it.
use crate::meta::{Event, EventSource, UserInterface, Command};


enum TermEvent {
    Resize,
    Input { key: Key },
    TextForWindow { window: String, text: String },
}

pub struct TermUiManager {
    tx: Sender<TermEvent>,
    rx: Option<Receiver<TermEvent>>,
}

struct TuiListener {
    tx: Sender<TermEvent>,
    rx: Receiver<TermEvent>,
}

impl TermUiManager {
    pub fn new() -> TermUiManager {
        let (tx, rx) = mpsc::channel();
        TermUiManager {
            tx, rx: Some(rx)
        }
    }
}

impl UserInterface for TermUiManager {
    fn push_to_window(&mut self, window: String, line: String) -> Result<(), ()> {
        match self.tx.send(TermEvent::TextForWindow { window: window, text: line }) {
            Ok(_) => Ok(()),
            Err(_) => Err(()),
        }
    }

    fn register_command(&mut self, c: Command) {
        // TODO
    }

    fn listener(&mut self) -> Box<EventSource + Send> {
        match self.rx.take() {
            Some(rx) => Box::new(TuiListener { rx, tx: self.tx.clone() }),
            None => panic!("Cannot call listener() on UserInterface more than once."),
        }
    }
}

impl EventSource for TuiListener {
    fn run(&mut self, channel: Sender<Event>) {
        let stdin_tx = self.tx.clone();
        thread::spawn(move || {
            let stdin = stdin();
            for c in stdin.keys() {
                // TODO: In the future, when we have better error handling for EventManaged
                // threads, bounce this back to the parent thread and let it crash properly.
                let c = c.expect("Couldn't read from stdin?!");
                stdin_tx.send(TermEvent::Input { key: c });
            }
        });

        let sig_tx = self.tx.clone();
        let sigs = Signals::new(&[libc::SIGWINCH]).expect("Couldn't create Signals iterator");
        thread::spawn(move || {
            for signal in sigs.forever() {
                sig_tx.send(TermEvent::Resize);
            }
        });

        let mut stdout = AlternateScreen::from(stdout().into_raw_mode().unwrap());
        write!(stdout, "{}{}", termion::clear::All, termion::cursor::Hide).unwrap();
        stdout.flush().unwrap();

        match self.do_ui(&mut stdout, &channel) {
            Ok(_) => { },
            Err(e) => { println!("ERROR: In TuiListener::do_ui(): {:?}", e); },
        }

        write!(stdout, "{}", termion::cursor::Show).unwrap();
        stdout.flush().unwrap();
        channel.send(Event::QuitRequest);
    }
}

impl TuiListener {
    fn do_ui(&mut self, mut stdout: &mut impl Write, channel: &Sender<Event>) -> std::io::Result<()> {
        let (term_w, term_h) = termion::terminal_size()?;
        let (term_w, term_h) = (term_w as usize, term_h as usize);

        let mut db = screen::DamageBuffer::new(term_w, term_h);
        let mut view = screen::WrappedView::new(term_w, term_h);

        'outer: loop {
            let next = match self.rx.recv() {
                Ok(e) => e,
                Err(_) => return Err(io::Error::new(io::ErrorKind::Other, "Couldn't receive next terminal event")),
            };
            match next {
                TermEvent::Resize => {
                    let (term_w, term_h) = termion::terminal_size()?;
                    let (term_w, term_h) = (term_w as usize, term_h as usize);
                    db.resize(term_w, term_h);
                    view.resize(term_w, term_h);
                    for (y, line) in view.render().into_iter().enumerate(){
                        db.write_string(0, y, line);
                    }
                    db.redraw(&mut stdout)?;
                },
                TermEvent::TextForWindow { text: txt, window: _ } => {
                    view.push(txt);
                    for (y, line) in view.render().into_iter().enumerate() {
                        db.write_string(0, y, line);
                    }
                    db.redraw(&mut stdout)?;
                },
                TermEvent::Input { key: k } => {
                    match k {
                        Key::Ctrl('c') => { return Ok(()); },
                        _ => { },
                    };
                },
            }
        }
    }
}

