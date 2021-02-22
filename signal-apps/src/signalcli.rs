use std::fs::File;
use std::str;
use std::sync::Arc;

use async_process::{Child, Command, Stdio};
use async_trait::async_trait;
use futures_lite::{io::BufReader, prelude::*};
use subprocess::{Popen, PopenConfig, Redirection};
use tokio::sync::mpsc;

use crate::receiver::Receiver;
use crate::sender::{InternalSender, Sender};

static SIGNALCLI_PATH: &str = "../signal-cli/build/install/signal-cli/bin/signal-cli";

pub struct SignalCliReciever {
    recv_chan: mpsc::Receiver<String>,
}

pub struct SignalCliSender {
    user: String,
}

pub struct SignalCliDaemon {
    daemon: Popen,
    recvproc: Option<Child>,
    send_chan: Arc<mpsc::Sender<String>>,
}

impl SignalCliDaemon {
    pub fn new(user: &str) -> (Self, SignalCliReciever, SignalCliSender) {
        // TODO legit error handling
        let devnull = File::create("/dev/null").unwrap();
        let daemon = Popen::create(
            &[SIGNALCLI_PATH, "daemon"],
            PopenConfig {
                stdout: Redirection::File(devnull),
                ..Default::default()
            },
        )
        .unwrap();

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
            SignalCliDaemon { daemon, recvproc, send_chan },
            SignalCliReciever { recv_chan },
            SignalCliSender { user: user.to_string() }
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
        eprintln!("terminating daemon");
        self.daemon.terminate().unwrap();
        self.daemon.wait().unwrap();

        let mut recvproc = self.recvproc.take().unwrap();
        tokio::spawn(async move {
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
        eprintln!("Starting send proc");
        Popen::create(
            &[
                SIGNALCLI_PATH,
                "--dbus",
                "-u",
                &self.user,
                "send",
                "-m",
                msg,
                dest,
            ],
            PopenConfig {
                stdout: Redirection::File(File::create("/dev/null").unwrap()),
                ..Default::default()
            },
        )
        .unwrap()
        .wait()
        .expect("Send failed!");
        eprintln!("Finished send proc");
    }
}
