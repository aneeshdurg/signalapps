use std::fs;
use std::io::Result;

use clap::clap_app;
use futures::{join, stream::StreamExt};
use serde_json;
use signal_hook::consts::signal::SIGINT;
use signal_hook_tokio::Signals;

mod app;
mod appstate;
mod comm;
mod signalcli;

use crate::appstate::{AppMsg, AppState};
use crate::comm::{Control, Receiver, Sender};
use crate::signalcli::SignalCliDaemon;

fn get_msg(msg: &serde_json::Value) -> Option<(&str, &str)> {
    let envelope = &msg["envelope"];
    if let Some(source) = envelope["source"].as_str() {
        if let Some(content) = envelope["dataMessage"]["message"].as_str() {
            return Some((source, content));
        }
    }

    None
}

async fn signal_handler<C: Control>(control: C) {
    let signals = Signals::new(&[SIGINT]).unwrap();
    let handle = signals.handle();

    let mut signals = signals.fuse();
    eprintln!("Waiting for signals");
    while let None = signals.next().await {}
    eprintln!("Got exit signal");
    handle.close();

    control.insert_msg("").await;
    eprintln!("sent sentinel");
}

async fn main_loop<C, R, S>(
    control: C,
    mut recv: R,
    sender: S,
    config: serde_json::Value,
) where
    C: Control,
    R: Receiver,
    S: Sender + Send + Sync,
{
    let (mut state, state_queue) = AppState::new(config, sender);

    let main_thread = async {
        println!("Setup main_thread");
        loop {
            let msg = recv.get_msg().await;
            let msg = match msg.as_ref().map(String::as_str) {
                None | Some("") => {
                    break;
                }
                Some(msg) => msg,
            };

            if let Ok(Some((source, msg))) =
                serde_json::from_str::<serde_json::Value>(msg)
                    .as_ref()
                    .map(get_msg)
            {
                let source = source.to_string();
                let msg = msg.to_string();
                state_queue
                    .send(AppMsg::InMsg(source, msg))
                    .await
                    .expect("enqueing task failed!");
            }
        }

        // Notify the receiver that we have no more messages to send explicitly
        state_queue
            .send(AppMsg::Finish)
            .await
            .expect("enqueing task failed!");

        eprintln!("Exiting main thread");
    };

    join!(main_thread, signal_handler(control), state.process_queue());
    // TODO prevent drop of control until after the join completes.
    //   We want to know that the daemon lives during state.drain.
}

#[tokio::main]
async fn main() -> Result<()> {
    let matches = clap_app!(SignalApps =>
        (version: "0.0")
        (author: "Aneesh Durg <aneeshdurg17@gmail.com>")
        (about: "Run a signal app server")
        (@arg CONFIG: -c --config +required +takes_value "Path to config json")
    )
    .get_matches();

    let config = matches.value_of("CONFIG").unwrap();
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(config)?)?;

    let user = config["username"]
        .as_str()
        .expect("config json needs a username key");
    eprintln!("Starting as user {:?}", user);

    let (control, recv, send) = SignalCliDaemon::new(user)?;
    main_loop(control, recv, send, config).await;

    Ok(())
}
