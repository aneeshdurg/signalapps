use std::io::Result;
use std::str;
use std::sync::Arc;
use std::process;

use async_process::{Child, Command, Stdio};
use async_trait::async_trait;
use futures_lite::{io::BufReader, prelude::*};
use tokio::sync::mpsc;

use crate::comm::{Control, Receiver, Sender};

static SIGNALCLI_PATH: &str =
    "../signal-cli/build/install/signal-cli/bin/signal-cli";

pub struct SignalCliReciever {
    recv_chan: mpsc::Receiver<String>,
}

pub struct SignalCliSender {
    user: String,
}

pub struct SignalCliDaemon {
    _daemon: Child,
    _recvproc: Child,
    send_chan: Arc<mpsc::Sender<String>>,
}

impl SignalCliDaemon {
    pub fn new(
        user: &str,
    ) -> Result<(Self, SignalCliReciever, SignalCliSender)> {
        let _daemon = Command::new(SIGNALCLI_PATH)
            .arg("daemon")
            .stdout(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;

        let (tx, rx) = mpsc::channel(100);

        let recv_chan = rx;
        let send_chan = Arc::new(tx);

        let mut _recvproc = Command::new(SIGNALCLI_PATH)
            .arg("--dbus")
            .arg("--output=json")
            .arg("-u")
            .arg(user)
            .arg("receive")
            .arg("--timeout")
            .arg("-1") /* disable timeout */
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let recvout = _recvproc.stdout.take().unwrap();

        {
            let send_chan = send_chan.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(recvout).lines();
                // TODO max size for line?
                while let Some(Ok(line)) = lines.next().await {
                    if line.len() == 0 {
                        // weird empty lines sometimes.
                        continue;
                    }
                    send_chan.send(line).await.expect("Sending line failed!");
                }
            });
        }

        Ok((
            SignalCliDaemon {
                _daemon,
                _recvproc,
                send_chan,
            },
            SignalCliReciever { recv_chan },
            SignalCliSender {
                user: user.to_string(),
            },
        ))
    }
}

#[async_trait]
impl Control for SignalCliDaemon {
    async fn insert_msg(&self, msg: &str) {
        self.send_chan
            .send(msg.to_string())
            .await
            .expect("Sending control msg failed!");
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
        process::Command::new(SIGNALCLI_PATH)
            .args(&["--dbus", "-u", &user, "send", "-m", &msg, &dest])
            .output()
            .expect("Send failed!");
        eprintln!("Finished send proc");
    }
}
