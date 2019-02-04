
use mint;
use mint::meta::*;
use mint::events::ThreadedManager;
use mint::plaintcp::TcpConnectionManager;

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

    let mut event = manager.next_event();
    loop {
        match event.expect("Error in next_event()") {
            Event::ServerText { line: l, which: c } => { println!("## {} ==> {}", c, l); },
            ref event => { println!("Unhandled event: {:?}", event); },
        }
        event = manager.next_event();
    }
}

