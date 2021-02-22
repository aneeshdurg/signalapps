use std::fs;

use clap::clap_app;
use futures::{join, stream::StreamExt};
use serde_json;
use signal_hook::consts::signal::SIGINT;
use signal_hook_tokio::Signals;

mod receiver;
mod sender;
mod signalcli;

use crate::receiver::Receiver;
use crate::sender::{InternalSender, Sender};
use crate::signalcli::SignalCliDaemon;

struct MainApp<R, S, IS>
where R: Receiver, S: Sender, IS: InternalSender
{
    recv: R,
    send: S,
    control: IS,
}

impl<R, S, IS> MainApp<R, S, IS>
where R: Receiver, S: Sender, IS: InternalSender
{
    fn new(recv: R, send: S, control: IS) -> Self {
        MainApp { recv, send, control }
    }

    async fn main_loop(&mut self) {
        let signals = Signals::new(&[SIGINT]).unwrap();
        let handle = signals.handle();

        let recv = &mut self.recv;
        let sender = &self.send;
        let main_thread = async {
            println!("Setup main_thread");
            loop {
                let msg = recv.get_msg().await;
                if let None = msg {
                    break;
                }
                let msg = msg.unwrap();

                println!("got msg {:?}", msg);
                if msg.len() == 0 {
                    break;
                }

                println!("begin send");
                // TODO don't just send, read the message as json.
                Sender::send(
                    sender,
                    "+15123006857",
                    "hi"
                );
                println!("done send");
            }

            eprintln!("Exiting main thread");
        };

        let control = &mut self.control;
        let signal_thread = async {
            let mut signals = signals.fuse();
            loop {
                eprintln!("Waiting for signals");
                if let Some(_) = signals.next().await {
                    break;
                }
            }
            handle.close();
            eprintln!("Got exit signal");
            InternalSender::insert_msg(control, "");
            eprintln!("sent sentinel");
        };

        join!(main_thread, signal_thread);
        control.stop();
    }
}

#[tokio::main]
async fn main() {
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

    let user = config["username"]
        .as_str()
        .expect("config json needs a username key");
    eprintln!("Starting as user {:?}", user);

    let (daemon, recv, send) = SignalCliDaemon::new(user);
    MainApp::new(recv, send, daemon).main_loop().await;
}
