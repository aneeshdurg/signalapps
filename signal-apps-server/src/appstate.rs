use std::collections::HashMap;
use std::io;

use async_std::fs;
use bytes::BufMut;
use futures::StreamExt;
use serde_json;
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use crate::comm::Sender;

struct App<'a> {
    name: &'a str,
    stream: UnixStream,
}

impl App<'_> {
    async fn send(&self, msg: &str) {}
    async fn stop(&self) {}
}

struct AppInfo {
    name: String,
    desc: String,
}

pub struct AppState<'a, S: Sender> {
    config: serde_json::Value,
    app_dir: String, // TODO turn this into ref
    sender: S,
    running_apps: HashMap<String, App<'a>>,
    app_cache: HashMap<String, AppInfo>,
    task_receiver: mpsc::Receiver<(String, String)>,
}

impl<S: Sender> AppState<'_, S> {
    pub fn new(
        config: serde_json::Value,
        sender: S,
    ) -> (Self, mpsc::Sender<(String, String)>) {
        let app_dir = config["appdir"]
            .as_str()
            .expect("Config is missing appdir")
            .to_string();
        let (task_sender, task_receiver) = mpsc::channel(100);
        (
            AppState {
                config,
                app_dir,
                sender,
                running_apps: HashMap::new(),
                app_cache: HashMap::new(),
                task_receiver,
            },
            task_sender,
        )
    }

    pub async fn process_queue(&mut self) {
        // TODO consider running each run_action in it's own async ctx
        while let Some((source, msg)) = self.task_receiver.recv().await {
            self.run_action(source, msg).await;
        }
    }

    pub async fn run_action(&mut self, source: String, msg: String) {
        // TODO always send read receipt - that requires more info
        // Maybe eventually this method should take the json obj
        let msg_lower = msg.to_lowercase();
        let cmd: Vec<&str> = msg_lower.split(" ").collect();
        match cmd[0] {
            "startapp" => {
                // Start an app
            }
            "listapps" => {
                // List all installed apps.
                // Do a listdir on the directory containing apps
                // for a known app, check the cache, otherwise populate it
                let mut entries = fs::read_dir(&self.app_dir)
                    .await
                    .expect("Failed to read app_dir!");
                while let Some(entry) = entries.next().await {
                    let entry = entry.expect("Failed to read app_dir!");
                    let name = entry
                        .file_name()
                        .into_string()
                        .expect("Invalid utf8 name");
                    // TODO lookup in cache before connecting

                    let stream = UnixStream::connect(entry.path())
                        .await
                        .expect("failed to open unix socket");
                    stream.writable().await.expect("Could not write to socket");

                    stream.try_write(b"?").unwrap();

                    let mut desc = vec![];
                    loop {
                        stream
                            .readable()
                            .await
                            .expect("Could not read from socket");
                        match stream.try_read_buf(&mut desc) {
                            Ok(0) => {
                                break;
                            }
                            Ok(_) => {
                                continue;
                            }
                            Err(ref e)
                                if e.kind() == io::ErrorKind::WouldBlock =>
                            {
                                continue;
                            }
                            Err(_) => {
                                break;
                            }
                        }
                    }
                    let desc = String::from_utf8(desc).unwrap_or("".to_owned());

                    let ident = name.clone();
                    self.app_cache.insert(ident, AppInfo { name, desc });
                }
            }
            "currentapp" => {
                // If there's a running app return it's name
                match self.running_apps.get(&source) {
                    None => self.send_no_apps(&source),
                    Some(app) => self.sender.send(&source, app.name),
                }
            }
            "endapp" => {
                // If there's a running app, terminate it.
                match self.running_apps.get(&source) {
                    None => self.send_no_apps(&source),
                    Some(app) => {
                        app.stop().await;
                        self.running_apps.remove(&source);
                    }
                }
            }
            "help" => {
                self.send_help(&source);
            }
            _ => {
                match self.running_apps.get(&source) {
                    None => self.send_help(&source),
                    Some(app) => app.send(&msg).await,
                };
            }
        }
    }

    fn send_help(&self, dest: &str) {
        // TODO
        self.sender.send(dest, "Welcome to signal-apps!");
    }

    fn send_no_apps(&self, dest: &str) {
        self.sender
            .send(dest, "You have no running apps. Send `help` to learn more.");
    }

    pub async fn drain(&self) {
        // TODO close all current apps
    }
}
