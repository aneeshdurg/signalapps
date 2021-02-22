use std::collections::HashMap;

use async_std::fs;
use futures::StreamExt;
use serde_json;
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use crate::comm::Sender;

struct App<'a> {
    name: &'a str,
    stream: UnixStream,
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
        while let Some((source, msg)) = self.task_receiver.recv().await {
            self.run_action(source, msg).await;
        }
    }

    pub async fn run_action(&self, source: String, msg: String) {
        // TODO always send read receipt - that requires more info
        // Maybe eventually this method should take the json obj
        let msg_lower = msg.to_lowercase();
        let cmd: Vec<&str> = msg_lower.split(" ").collect();
        match cmd[0] {
            "startapp" => {
                // Start an app
                // This requires access to the config to find the app dir
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
                    println!("{}", entry.file_name().to_string_lossy());
                }
            }
            "currentapp" => {
                // If there's a running app return it's name
            }
            "endapp" => {
                // If there's a running app, terminate it.
            }
            "help" => {
                self.send_help(&source);
            }
            _ => {
                // If there's a running app send msg to the app
                // other wise send_help
            }
        }
    }

    fn send_help(&self, dest: &str) {
        // TODO
        self.sender.send(dest, "Welcome to signal-apps!");
    }

    pub async fn drain(&self) {
        // TODO close all current apps
    }
}
