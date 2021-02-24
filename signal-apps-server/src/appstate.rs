use std::collections::HashMap;
use std::io;

use async_std::fs;
use futures::StreamExt;
use serde_json;
use tokio::sync::mpsc;

use crate::app;
use crate::comm::Sender;

#[derive(Debug)]
pub enum AppMsg {
    InMsg(String, String),
    EndMsg(String, u64),    // ends the given appid
    OutMsg(String, String), // Allows access to sender
    Finish,
}

struct AppInfo {
    name: String,
    desc: String,
}

pub struct AppState<App: app::App, S: Sender> {
    // config: serde_json::Value, this could allow querying config from apps
    app_dir: String, // TODO turn this into ref
    app_id: u64,
    sender: S,
    running_apps: HashMap<String, App>,
    app_cache: HashMap<String, AppInfo>,
    task_receiver: mpsc::Receiver<AppMsg>,
    incoming: mpsc::Sender<AppMsg>,
}

impl<App: app::App, S: Sender> AppState<App, S> {
    pub fn new(
        config: serde_json::Value,
        sender: S,
    ) -> (Self, mpsc::Sender<AppMsg>) {
        let app_dir = config["appdir"]
            .as_str()
            .expect("Config is missing appdir")
            .into();
        let (task_sender, task_receiver) = mpsc::channel(100);
        let incoming = task_sender.clone();
        (
            AppState {
                app_dir,
                app_id: 0,
                sender,
                running_apps: HashMap::new(),
                app_cache: HashMap::new(),
                task_receiver,
                incoming,
            },
            task_sender,
        )
    }

    pub async fn process_queue(&mut self) {
        // TODO consider running each run_action in it's own async ctx
        // use FutureUnordered?
        while let Some(msg) = self.task_receiver.recv().await {
            match msg {
                AppMsg::InMsg(source, msg) => {
                    self.run_action(source, msg).await
                }
                AppMsg::EndMsg(source, appid) => {
                    self.endapp(&source, Some(appid)).await;
                }
                AppMsg::OutMsg(source, msg) => self.sender.send(&source, &msg),
                AppMsg::Finish => {
                    break;
                }
            }
        }
        eprintln!("done processing queue");
    }

    async fn populate_app_cache(&mut self, name: &str) -> io::Result<()> {
        if self.app_cache.contains_key(name) {
            eprintln!("Found app inside cache");
            return Ok(());
        }

        eprintln!("Found app outside cache, opening socket");
        let desc = App::get_description(&self.app_dir, name).await?;
        self.app_cache.insert(
            name.to_string(),
            AppInfo {
                name: name.to_string(),
                desc,
            },
        );
        Ok(())
    }

    fn get_id(&mut self) -> u64 {
        let id = self.app_id;
        self.app_id += 1;
        id
    }

    async fn run_action(&mut self, source: String, msg: String) {
        // TODO always send read receipt - that requires more info
        // Maybe eventually this method should take the json obj
        let msg_lower = msg.to_lowercase();
        let cmd: Vec<&str> = msg_lower.split(" ").collect();
        match cmd[0] {
            "startapp" => {
                // Start an app
                match self.running_apps.get(&source) {
                    Some(_) => self.sender.send(
                        &source,
                        "You are already running an app. See `currentapp` for more info."
                    ),
                    None => {
                        if cmd.len() != 2 {
                            self.sender.send(
                                &source,
                                "Malformed startapp request, Expected `startapp <app>`."
                            );
                            return;
                        }

                        let app_name = cmd[1];
                        self.startapp(source, app_name).await;
                    },
                }
            }
            "listapps" => {
                self.listapps(&source).await;
            }
            "currentapp" => {
                // If there's a running app return it's name
                match self.running_apps.get(&source) {
                    None => self.send_no_apps(&source),
                    Some(app) => self.sender.send(&source, app.get_name()),
                }
            }
            "endapp" => {
                self.endapp(&source, None).await;
            }
            "help" => {
                self.send_help(&source);
            }
            _ => {
                match self.running_apps.get_mut(&source) {
                    None => self.send_help(&source),
                    Some(app) => {
                        let msg = serde_json::json!({
                            "type": "msg",
                            "data": &msg
                        })
                        .to_string();
                        app.send(&msg).await
                    }
                };
            }
        }
    }

    async fn startapp(&mut self, source: String, app_name: &str) {
        // We're about to yield so make sure we insert first to prevent other tasks
        // from racing here.
        {
            let ident = source.clone();
            let id = self.get_id();
            // TODO use an enum placeholder instead of all these options and combine new w/ start
            self.running_apps.insert(
                ident,
                App::new(id, app_name, &source, self.incoming.clone()),
            );
        }

        if let Err(_) = self.populate_app_cache(app_name).await {
            self.sender.send(
                &source,
                "Could not find app, please contact your admin if you believe this is in error."
            );
            return;
        }

        // The app might have been removed by endapp during the appinfo fetch
        // above.
        if let Some(mut app) = self.running_apps.remove(&source) {
            if let Err(_) = app.start(&self.app_dir, app_name).await {
                self.sender.send(
                    &source,
                    "Could not start app, please notify your admin.",
                );

            // TODO remove it from app_cache
            } else {
                let start_msg = serde_json::json!({
                    "type": "start",
                    "user": &source,
                })
                .to_string();
                app.send(&start_msg).await;
                self.running_apps.insert(source, app);
            }
        }
    }

    async fn listapps(&mut self, source: &str) {
        // List all installed apps.
        // Do a listdir on the directory containing apps
        // for a known app, check the cache, otherwise populate it
        eprintln!("Reading appdir");
        let mut entries = fs::read_dir(&self.app_dir)
            .await
            .expect("Failed to read app_dir!");
        while let Some(entry) = entries.next().await {
            let entry = entry.expect("Failed to read app_dir!");
            let name = entry.file_name();
            // Ignore any errors here
            let _ = self
                .populate_app_cache(name.to_str().expect("Invalid utf8 name"))
                .await;
        }

        eprintln!("building resp");
        let mut lines = vec![
            "You currently have the following apps installed.".into(),
            "To install more, please contact your admin.".into(),
        ];
        for info in self.app_cache.values() {
            lines.push(format!("{:?} - {:?}\n", info.name, info.desc));
        }
        // TODO cache this?
        let infostr = lines.join("\n");
        eprintln!("sent resp");
        self.sender.send(&source, &infostr);
    }

    async fn endapp(&mut self, source: &str, appid: Option<u64>) {
        // If there's a running app, terminate it.
        if let Some(_) = appid {
            let foundid = self.running_apps.get(source).map(|app| app.get_id());
            if appid != foundid {
                return;
            }
        }

        match self.running_apps.remove(source) {
            None => self.send_no_apps(source),
            Some(mut app) => {
                app.stop().await;
                self.sender.send(source, "Stopped app");
            }
        }
    }

    fn send_help(&self, dest: &str) {
        // TODO better msg
        self.sender.send(dest, "Welcome to signal-apps!");
    }

    fn send_no_apps(&self, dest: &str) {
        self.sender
            .send(dest, "You have no running apps. Send `help` to learn more.");
    }
}
