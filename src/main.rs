use mint;
use mint::meta::*;
use mint::events::ThreadedManager;
use mint::plaintcp::TcpConnectionManager;

fn main() {
    let mut manager = ThreadedManager::new();
    let mut tcp = TcpConnectionManager::new();
    manager.start_source(tcp.listener());
    tcp.start_connection("".to_string());

    let mut event = manager.next_event();
    loop {
        match event.expect("Error in next_event()") {
            Event::ServerText { line: l, which: c } => { println!("## {} ==> {}", c, l); },
            _ => { println!("Other random event"); },
        }
        event = manager.next_event();
    }
}
