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

use crate::app::UnixStreamApp;
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

async fn main_loop<App, C, R, S>(
    control: C,
    mut recv: R,
    sender: S,
    config: serde_json::Value,
) where
    App: app::App,
    C: Control,
    R: Receiver,
    S: Sender + Send + Sync,
{
    let new_app = AppState::new(config, sender);
    let mut state: AppState<App, _> = new_app.0;
    let state_queue = new_app.1;

    let main_thread = async {
        println!("Setup main_thread");
        loop {
            let msg = recv.get_msg().await;
            let msg = match msg.as_ref().map(String::as_str) {
                None => {
                    eprintln!("No further msgs");
                    break;
                }
                Some("") => {
                    eprintln!("got sentinel");
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
    main_loop::<UnixStreamApp, _, _, _>(control, recv, send, config).await;

    Ok(())
}

#[cfg(test)]
mod test {
    use std::fs::File;
    use std::io;
    use std::process;

    use async_trait::async_trait;
    use tempdir::TempDir;
    use tokio::sync::mpsc;
    use tokio::time::{sleep, Duration};

    use super::*;

    pub struct MockSender {
        channel: mpsc::UnboundedSender<(String, String)>,
    }

    impl MockSender {
        fn new() -> (Self, mpsc::UnboundedReceiver<(String, String)>) {
            let (channel, recv) = mpsc::unbounded_channel();
            (MockSender { channel }, recv)
        }
    }

    impl Sender for MockSender {
        fn send(&self, dest: &str, msg: &str) {
            let sender = self.channel.clone();
            sender.send((dest.to_string(), msg.to_string())).unwrap()
        }
    }

    pub struct MockControl {
        channel: mpsc::UnboundedSender<String>,
    }

    impl MockControl {
        fn new(channel: mpsc::UnboundedSender<String>) -> Self {
            MockControl { channel }
        }
    }

    #[async_trait]
    impl Control for MockControl {
        async fn insert_msg(&self, msg: &str) {
            self.channel.send(msg.into()).unwrap();
        }
    }

    pub struct MockReceiver {
        channel: mpsc::UnboundedReceiver<String>,
    }

    impl MockReceiver {
        fn new() -> (Self, MockControl) {
            let (send, channel) = mpsc::unbounded_channel();
            (MockReceiver { channel }, MockControl::new(send))
        }
    }

    #[async_trait]
    impl Receiver for MockReceiver {
        async fn get_msg(&mut self) -> Option<String> {
            self.channel.recv().await
        }
    }

    struct MockApp {
        id: u64,
        name: String,
        messages: Vec<String>,
    }

    #[async_trait]
    impl app::App for MockApp {
        async fn get_description(_: &str, _: &str) -> io::Result<String> {
            Ok("".into())
        }

        fn new(id: u64, name: &str, _: &str, _: mpsc::Sender<AppMsg>) -> Self {
            MockApp {
                id,
                name: name.into(),
                messages: vec![],
            }
        }

        fn get_id(&self) -> u64 {
            self.id
        }

        fn get_name(&self) -> &str {
            &self.name
        }

        async fn start(&mut self, _: &str, _: &str) -> io::Result<()> {
            Ok(())
        }

        async fn send(&mut self, msg: &str) {
            self.messages.push(msg.into());
        }

        async fn stop(&mut self) {}
    }

    #[tokio::test]
    async fn test_sigint_stops_during_running_app() {
        let tmp_dir = TempDir::new("apps").expect("create tempdir failed!");
        let file_path = tmp_dir.path().join("app");
        File::create(file_path).expect("create app failed!");

        let (recv, control) = MockReceiver::new();
        let channel = control.channel.clone();

        let t1 = async move {
            let (send, _) = MockSender::new();
            let config = serde_json::json!({
                "appdir": tmp_dir.path().to_str()
            });
            main_loop::<MockApp, _, _, _>(control, recv, send, config).await;
        };

        let t2 = async {
            channel.send("startapp app".into()).unwrap();
            sleep(Duration::from_millis(250)).await;
            unsafe { libc::kill(process::id() as i32, libc::SIGINT); }
        };

        join!(t1, t2);
    }
}
