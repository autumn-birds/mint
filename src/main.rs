
use mint;
use mint::meta::*;
use mint::events::ThreadedManager;

use mint::net::tcp::TcpConnectionManager;
use mint::ui::term::TermUiManager;

use std::env;
use std::{cell::RefCell, rc::Rc};

fn wrap<T>(x: T) -> Rc<RefCell<T>> {
    Rc::new(RefCell::new(x))
}

fn main() {
    let address: String;
    if let Some(arg1) = env::args().nth(1) {
        address = arg1;
    } else {
        panic!("Expected at least one command line argument (ip:port)");
    }

    let mut manager = ThreadedManager::new();

    let tcp = wrap(TcpConnectionManager::new());
    manager.start_source(tcp.clone());
    let cid = tcp.borrow_mut().start_connection(address.to_string())
         .unwrap();

    let tui = wrap(TermUiManager::new());
    manager.start_source(tui.clone());

    let mut event = manager.next_event();
    loop {
        match event.unwrap() {
            Event::ServerText { line: l, which: _c } => {
                tui.borrow_mut().push_to_window("default".to_string(), l);
            },
            Event::QuitRequest => {
                break;
            },
            Event::UserInput { mut line, which: _ } => {
                // Since we don't have real window management or multiple connections yet, we do
                // ...this
                // Obviously needs more error handling too, like everything else in this program.
                line.push_str("\n");
                match tcp.borrow_mut().write_to_connection(cid, line) {
                    Ok(_) => { },
                    Err(_) => { tui.borrow_mut().push_to_window("default".to_string(),
                            format!("Couldn't write to connection")); }
                }
            }
            ref event => {
                tui.borrow_mut().push_to_window("default".to_string(),
                        format!("Unhandled event: {:?}", event));
            },
        }
        event = manager.next_event();
    }

    println!("At end of main() due to QuitRequest (probably.)");
}

