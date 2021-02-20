use std::fs;
use std::marker::{Send, Sync};

use crossbeam_utils::thread;
use clap::clap_app;
use serde_json;
use signal_hook::consts::signal::SIGINT;
use signal_hook::iterator::Signals;

mod receiver;
mod sender;
mod signalcli;

use crate::receiver::Receiver;
use crate::sender::Sender;
use crate::signalcli::SignalCliDaemon;

struct MainApp<I> where I: Receiver + Sender + Send + Sync
{
    interface: I,
}

impl<I> MainApp<I> where I: Receiver + Sender + Send + Sync
{
    fn new(interface: I) -> Self {
        MainApp { interface }
    }

    fn main_loop(&mut self) {
        let recv = &self.interface;

        thread::scope(|s| {
            let mut signals = Signals::new(&[SIGINT]).unwrap();
            let handle = signals.handle();

            let main_thread = s.spawn(|_| {
                loop {
                    let msg = recv.get_msg();
                    println!("got msg {:?}", msg);
                    if msg.len() == 0 {
                        eprintln!("Exiting main_thread");
                        break;
                    }
                    // TODO don't just send, read the message as json.
                    Sender::send(&self.interface, "+15123006857", "hi");
                }
            });

            loop {
                match signals.wait().into_iter().next() {
                    None => {},
                    _ => { break; }
                }
            }
            handle.close();

            Receiver::insert_msg(recv, "");
            main_thread.join().expect("Thread failed to join.");
        }).unwrap();

        Receiver::stop(&mut self.interface);
    }
}


fn main() {
    let matches = clap_app!(SignalApps =>
        (version: "0.0")
        (author: "Aneesh Durg <aneeshdurg17@gmail.com>")
        (about: "Run a signal app server")
        (@arg CONFIG: -c --config +required +takes_value "Path to config json")
    )
    .get_matches();

    let config = matches.value_of("CONFIG").unwrap();
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(config).unwrap()).unwrap();

    let user = config["username"].as_str().expect("config json needs a username key");
    eprintln!("Starting as user {:?}", user);
    let daemon = SignalCliDaemon::new(user);

    MainApp::new(daemon).main_loop();
}
