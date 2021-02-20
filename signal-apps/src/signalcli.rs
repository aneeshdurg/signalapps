use std::fs::File;
use std::io::Read;
use std::str;
use std::sync::{Arc, Mutex};
use std::thread::spawn;

extern crate multiqueue;
use subprocess::{Popen, PopenConfig, Redirection};

use crate::receiver::Receiver;
use crate::sender::Sender;

static SIGNALCLI_PATH: &str = "../signal-cli/build/install/signal-cli/bin/signal-cli";

// Max size of buffer for incoming messages
const BUFFER_LEN: usize = 1024;

pub struct SignalCliDaemon {
    user: String,
    daemon: Popen,
    recvproc: Arc<Mutex<Popen>>,
    recv_chan: Mutex<multiqueue::MPMCReceiver<String>>,
    send_chan: Arc<Mutex<multiqueue::MPMCSender<String>>>
}

impl SignalCliDaemon {
    pub fn new(user: &str) -> Self {
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

        let recvproc = Arc::new(Mutex::new(
            Popen::create(
                &[
                    SIGNALCLI_PATH,
                    "--dbus",
                    "--output=json",
                    "-u",
                    user,
                    "receive",
                    "--timeout",
                    "-1", /* disable timeout */
                ],
                PopenConfig {
                    stdout: Redirection::Pipe,
                    ..Default::default()
                },
            )
            .unwrap(),
        ));

        let rproc = recvproc.clone();
        let (tx, rx) = multiqueue::mpmc_queue(10);

        let send_chan = Arc::new(Mutex::new(tx));
        let recv_chan = Mutex::new(rx);

        let tx = send_chan.clone();
        spawn(move || {
            loop {
                let mut msg = [0; BUFFER_LEN];
                // read a line up to 1024 characters. if there are more characters in a line,
                // discard the remainder.
                let mut i = 0;
                let mut error = false;
                loop {
                    let mut buffer = [0; 1];
                    let read_res = {
                        rproc
                            .lock()
                            .unwrap()
                            .stdout
                            .as_ref()
                            .unwrap()
                            .read(&mut buffer)
                    };

                    match read_res {
                        Ok(1) => {
                            if buffer[0] == ('\n' as u8) {
                                break;
                            } else {
                                // we either need this byte in the message or we're discarding this
                                // line.
                                if i < BUFFER_LEN {
                                    msg[i] = buffer[0];
                                    i += 1;
                                }
                            }
                        }
                        _ => {
                            error = true;
                            break;
                        }
                    }
                }

                if error {
                    break;
                }

                let msg = String::from_utf8(msg[0..i].to_vec()).unwrap();
                tx.lock().unwrap().try_send(msg).expect("Sending received msg failed!");
            }
        });

        SignalCliDaemon {
            user: user.to_string(),
            daemon,
            recvproc,
            recv_chan,
            send_chan,
        }
    }
}

impl Receiver for SignalCliDaemon {
    fn insert_msg(&self, msg: &str) {
        self.send_chan.lock().unwrap().try_send(msg.to_string())
            .expect("Inserting message failed!");
    }

    fn get_msg(&self) -> String {
        self.recv_chan.lock().unwrap().recv().unwrap()
    }

    fn stop(&mut self) {
        eprintln!("terminating daemon");
        self.daemon.terminate().unwrap();
        self.daemon.wait().unwrap();

        self.recvproc.lock().unwrap().terminate().unwrap();
        self.recvproc.lock().unwrap().wait().unwrap();
        eprintln!("done terminating daemon");
    }


}

impl Sender for SignalCliDaemon {
    fn send(&self, dest: &str, msg: &str) {
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
        ).unwrap().wait().expect("Send failed!");
    }
}
