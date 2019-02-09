
use mint;
use mint::meta::*;
use mint::events::ThreadedManager;

use mint::net::tcp::TcpConnectionManager;
use mint::ui::term::TermUiManager;

use std::env;

fn main() {
    let address: String;
    if let Some(arg1) = env::args().nth(1) {
        address = arg1;
    } else {
        panic!("Expected at least one command line argument (ip:port)");
    }

    let mut manager = ThreadedManager::new();

    let mut tcp = TcpConnectionManager::new();
    manager.start_source(tcp.listener());
    tcp.start_connection(address.to_string())
         .unwrap();

    let mut tui = TermUiManager::new();
    manager.start_source(tui.listener());

    let mut event = manager.next_event();
    loop {
        match event.expect("Error in next_event()") {
            Event::ServerText { line: l, which: c } => {
                tui.push_to_window("default".to_string(), l);
            },
            Event::QuitRequest => {
                break;
            },
            ref event => { println!("Unhandled event: {:?}", event); },
        }
        event = manager.next_event();
    }

    // TODO: We need to partially re-work the Manager things so that there's a way to tell all the
    // threads to wind down, release their resources, finish up etc. once the program needs to
    // close...
    println!("At end of main() due to QuitRequest (probably.)");
}

