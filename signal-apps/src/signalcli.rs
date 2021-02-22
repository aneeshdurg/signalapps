use std::str;
use std::sync::Arc;

use async_process::{Child, Command, Stdio};
use async_trait::async_trait;
use futures_lite::{io::BufReader, prelude::*};
use tokio::sync::mpsc;

use crate::comm::{InternalSender, Receiver, Sender};

static SIGNALCLI_PATH: &str = "../signal-cli/build/install/signal-cli/bin/signal-cli";

pub struct SignalCliReciever {
    recv_chan: mpsc::Receiver<String>,
}

pub struct SignalCliSender {
    user: String,
}

pub struct SignalCliDaemon {
    daemon: Option<Child>,
    recvproc: Option<Child>,
    send_chan: Arc<mpsc::Sender<String>>,
}

impl SignalCliDaemon {
    pub fn new(user: &str) -> (Self, SignalCliReciever, SignalCliSender) {
        // TODO legit error handling
        let daemon = Some(
            Command::new(SIGNALCLI_PATH)
                .arg("daemon")
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );

        let (tx, rx) = mpsc::channel(10);

        let recv_chan = rx;
        let send_chan = Arc::new(tx);

        let mut recvproc = Command::new(SIGNALCLI_PATH)
            .arg("--dbus")
            .arg("--output=json")
            .arg("-u")
            .arg(user)
            .arg("receive")
            .arg("--timeout")
            .arg("-1") /* disable timeout */
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let recvout = recvproc.stdout.take().unwrap();
        let recvproc = Some(recvproc);

        {
            let send_chan = send_chan.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(recvout).lines();
                // TODO max size for line?
                while let Some(Ok(line)) = lines.next().await {
                    send_chan
                        .try_send(line)
                        .expect("Sending received msg failed!");
                }
            });
        }

        (
            SignalCliDaemon {
                daemon,
                recvproc,
                send_chan,
            },
            SignalCliReciever { recv_chan },
            SignalCliSender {
                user: user.to_string(),
            },
        )
    }
}

impl InternalSender for SignalCliDaemon {
    fn insert_msg(&self, msg: &str) {
        self.send_chan
            .try_send(msg.to_string())
            .expect("Inserting message failed!");
    }

    fn stop(&mut self) {
        // TODO Maybe stop should actually be a custom drop?
        // investigate kill_on_drop in Command.
        eprintln!("terminating daemon");
        let mut dproc = self.daemon.take().unwrap();
        let mut recvproc = self.recvproc.take().unwrap();
        tokio::spawn(async move {
            dproc.kill().unwrap();
            dproc
                .status()
                .await
                .expect("failed to wait for daemon proc");

            recvproc.kill().unwrap();
            recvproc
                .status()
                .await
                .expect("failed to wait for recvproc");
            eprintln!("done terminating daemon");
        });
    }
}

#[async_trait]
impl Receiver for SignalCliReciever {
    async fn get_msg(&mut self) -> Option<String> {
        self.recv_chan.recv().await
    }
}

impl Sender for SignalCliSender {
    fn send(&self, dest: &str, msg: &str) {
        let dest = dest.to_string();
        let msg = msg.to_string();
        let user = self.user.clone();
        eprintln!("Starting send proc");
        tokio::spawn(async move {
            Command::new(SIGNALCLI_PATH)
                .args(&["--dbus", "-u", &user, "send", "-m", &msg, &dest])
                .stdout(Stdio::null())
                .output()
                .await
                .expect("Send failed!");
            eprintln!("Finished send proc");
        });
    }
}
