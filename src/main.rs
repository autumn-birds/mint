
use mint;
use mint::meta::*;
use mint::events::ThreadedManager;

use mint::net::tcp::TcpConnectionManager;
use mint::ui::term::screen::WrappedView;

use std::env;

fn main() {
//    let address: String;
//    if let Some(arg1) = env::args().nth(1) {
//        address = arg1;
//    } else {
//        panic!("Expected at least one command line argument (ip:port)");
//    }
//
//    let mut manager = ThreadedManager::new();
//    let mut tcp = TcpConnectionManager::new();
//    manager.start_source(tcp.listener());
//    tcp.start_connection(address.to_string())
//         .unwrap();
//
//    let mut event = manager.next_event();
//    loop {
//        match event.expect("Error in next_event()") {
//            Event::ServerText { line: l, which: c } => { println!("## {} ==> {}", c, l); },
//            ref event => { println!("Unhandled event: {:?}", event); },
//        }
//        event = manager.next_event();
//    }

    let mut v: WrappedView = WrappedView::new(30, 30);
    for l in v.render() {
        println!("0>{}<", l);
    }
    for n in 1..100 {
        v.push(format!("Hello world.  I am a string.  I am very long.  I am the {}th out of {} strings, so far as I know.", n, n));
        for l in v.render() {
            println!("{}>{}<", n, l);
        }
    }
}

