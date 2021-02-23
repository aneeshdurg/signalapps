use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Arc;

use async_std::fs;
use futures::StreamExt;
use serde_json;
use tokio::io::{split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, Mutex};

use crate::comm::Sender;

async fn read_msg_from_stream(
    stream: &mut ReadHalf<UnixStream>,
) -> io::Result<String> {
    let mut length = [0u8, 0, 0, 0];
    let mut read = 0;
    let mut failed = false;
    loop {
        match stream.read(&mut length[read..]).await {
            Ok(0) => {
                failed = true;
                break;
            }
            Ok(n) => {
                eprintln!("l read {} bytes", n);
                read += n;
                if read == 4 {
                    break;
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(_) => {
                failed = true;
                break;
            }
        }
    }
    if failed {
        return Err(io::Error::new(io::ErrorKind::Other, "Failed to read"));
    }

    let length = u32::from_be_bytes(length) as usize;
    eprintln!("Expecting {} bytes", length);

    let mut content = vec![];
    loop {
        match stream.read_buf(&mut content).await {
            Ok(0) => {
                failed = true;
                break;
            }
            Ok(n) => {
                eprintln!("read {} bytes", n);
                if content.len() == length {
                    break;
                }
                continue;
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(_) => {
                failed = true;
                break;
            }
        }
    }
    if failed {
        return Err(io::Error::new(io::ErrorKind::Other, "Failed to read"));
    }

    String::from_utf8(content)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid utf-8"))
}

#[derive(Debug)]
pub enum AppMsg {
    InMsg(String, String),
    OutMsg(String, String), // Allows access to sender
    Finish,
}

struct App {
    name: String,
    user: String,
    control: mpsc::Sender<AppMsg>,
    tx: Option<Arc<Mutex<mpsc::Sender<String>>>>,
    writer: Option<WriteHalf<UnixStream>>,
    // TODO take in a ref to AppState and create a way for async messages to be sent to the State,
    // also need to take in a Sender (ref?) to send msgs to client directly.
    //
    // Maybe Sender should implement Clone?
    //
    // TODO document protocol!
}

impl App {
    fn new(name: &str, user: &str, control: mpsc::Sender<AppMsg>) -> Self {
        let name = name.to_string();
        let user = user.to_string();
        // TODO using an enum placeholder instead of all these options and combine w/ start_stream
        App {
            name,
            user,
            control,
            tx: None,
            writer: None,
        }
    }

    fn start_stream(&mut self, stream: UnixStream) {
        let (mut sr, sw) = split(stream);

        let (tx, mut rx) = mpsc::channel(100);
        let tx = Arc::new(Mutex::new(tx));

        self.tx = Some(tx.clone());
        self.writer = Some(sw);

        {
            let tx = tx.clone();
            tokio::spawn(async move {
                while let Ok(content) = read_msg_from_stream(&mut sr).await {
                    if let Err(_) = tx.lock().await.send(content).await {
                        break;
                    }
                }
            });
        }

        let user = self.user.clone();
        let control = self.control.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if msg.len() == 0 {
                    break;
                }

                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&msg)
                {
                    let msg = match msg["type"].as_str() {
                        Some("response") => AppMsg::OutMsg(
                            user.clone(),
                            msg["value"].as_str().unwrap_or("").to_string(),
                        ),
                        _ => AppMsg::InMsg(user.clone(), "endapp".to_string()),
                    };
                    control
                        .send(msg)
                        .await
                        .expect("Sending control msg failed!");
                }
            }
        });
    }

    async fn cancel(&mut self) {
        self.control
            .send(AppMsg::InMsg(self.user.clone(), "endapp".to_string()))
            .await
            .expect("Sending control msg failed!");
    }

    async fn send(&mut self, msg: &str) {
        eprintln!("Sending msg {:?}", msg);
        // TODO convert msg to json
        if let Err(_) = self
            .writer
            .as_mut()
            .unwrap()
            .write_all(&(msg.len() as u32).to_be_bytes())
            .await
        {
            self.cancel().await;
            return;
        }

        if let Err(_) = self
            .writer
            .as_mut()
            .unwrap()
            .write_all(msg.as_bytes())
            .await
        {
            self.cancel().await;
            return;
        }

        eprintln!("Sent msg {:?}", msg);
    }

    async fn stop(&mut self) {
        match self.tx.take() {
            Some(tx) => tx.lock().await.send("".to_string()).await.unwrap(),
            None => {}
        }
    }
}

struct AppInfo {
    name: String,
    desc: String,
}

pub struct AppState<S: Sender> {
    config: serde_json::Value,
    app_dir: String, // TODO turn this into ref
    sender: S,
    running_apps: HashMap<String, App>,
    app_cache: HashMap<String, AppInfo>,
    task_receiver: mpsc::Receiver<AppMsg>,
    incoming: mpsc::Sender<AppMsg>,
}

impl<S: Sender> AppState<S> {
    pub fn new(
        config: serde_json::Value,
        sender: S,
    ) -> (Self, mpsc::Sender<AppMsg>) {
        let app_dir = config["appdir"]
            .as_str()
            .expect("Config is missing appdir")
            .to_string();
        let (task_sender, task_receiver) = mpsc::channel(100);
        let incoming = task_sender.clone();
        (
            AppState {
                config,
                app_dir,
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
                AppMsg::OutMsg(source, msg) => self.sender.send(&source, &msg),
                AppMsg::Finish => {
                    break;
                }
            }
        }
        eprintln!("done processing queue");
    }

    async fn open_app_socket(&self, name: &str) -> io::Result<UnixStream> {
        let path = Path::new(&self.app_dir).join(name);
        Ok(UnixStream::connect(path).await?)
    }

    async fn populate_app_cache(&mut self, name: &str) -> io::Result<()> {
        if self.app_cache.contains_key(name) {
            eprintln!("Found app inside cache");
            return Ok(());
        }

        eprintln!("Found app outside cache, opening socket");
        eprintln!("opened socket");
        let stream = self.open_app_socket(name).await?;
        stream.writable().await.expect("Could not write to socket");
        eprintln!("writable socket");

        // write 1 char
        let bytes = 1u32.to_be_bytes();
        stream.try_write(&bytes).unwrap();
        stream.try_write(&['?' as u8]).unwrap();

        eprintln!("queried socket");

        let mut length = [0u8, 0, 0, 0];
        let mut read = 0;
        let mut failed = false;
        loop {
            match stream.try_read(&mut length[read..]) {
                Ok(0) => {
                    failed = true;
                    break;
                }
                Ok(n) => {
                    eprintln!("l read {} bytes", n);
                    read += n;
                    if read == 4 {
                        break;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    continue;
                }
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        if failed {
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to read"));
        }

        let length = u32::from_be_bytes(length) as usize;
        eprintln!("Expecting {} bytes", length);

        let mut desc = vec![];
        loop {
            stream.readable().await.expect("Could not read from socket");
            eprintln!("socket was readable!");

            match stream.try_read_buf(&mut desc) {
                Ok(0) => {
                    failed = true;
                    break;
                }
                Ok(n) => {
                    eprintln!("read {} bytes", n);
                    if desc.len() == length {
                        break;
                    }
                    continue;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    continue;
                }
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        if failed {
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to read"));
        }

        eprintln!("got response");

        let desc = String::from_utf8(desc).unwrap_or("".to_owned());
        if let Ok(desc) = serde_json::from_str::<serde_json::Value>(&desc) {
            if let Some(desc) = desc["value"].as_str() {
                let name = name.to_string();
                let ident = name.clone();
                let desc = desc.to_string();
                self.app_cache.insert(ident, AppInfo { name, desc });
            }
        }

        Ok(())
    }

    pub async fn run_action(&mut self, source: String, msg: String) {
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
                        // We're about to yield so make sure we insert first to prevent other tasks
                        // from racing here.
                        {
                            let ident = source.clone();
                            self.running_apps.insert(
                                ident,
                                App::new(app_name, &source, self.incoming.clone())
                            );
                        }

                        if let Err(_) =  self.populate_app_cache(app_name).await {
                            self.sender.send(
                                &source,
                                "Could not find app, please contact your admin if you believe this is in error."
                            );
                            return;
                        }

                        // The app might have been removed by endapp during the appinfo fetch
                        // above.
                        if let Some(mut app) = self.running_apps.remove(&source) {
                            match self.open_app_socket(app_name).await {
                                Ok(stream) => {
                                    app.start_stream(stream);
                                    self.running_apps.insert(source, app);
                                },
                                Err(_) => {
                                    self.sender.send(
                                        &source,
                                        "Could not start app, please notify your admin."
                                    );
                                }
                            }
                        }
                    },
                }
            }
            "listapps" => {
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
                        .populate_app_cache(
                            name.to_str().expect("Invalid utf8 name"),
                        )
                        .await;
                }

                eprintln!("building resp");
                let mut lines = vec![
                    "You currently have the following apps installed."
                        .to_string(),
                    "To install more, please contact your admin.".to_string(),
                ];
                for info in self.app_cache.values() {
                    lines.push(format!("{:?} - {:?}\n", info.name, info.desc));
                }
                // TODO cache this?
                let infostr = lines.join("\n");
                eprintln!("sent resp");
                self.sender.send(&source, &infostr);
            }
            "currentapp" => {
                // If there's a running app return it's name
                match self.running_apps.get(&source) {
                    None => self.send_no_apps(&source),
                    Some(app) => self.sender.send(&source, &app.name),
                }
            }
            "endapp" => {
                // If there's a running app, terminate it.
                match self.running_apps.remove(&source) {
                    None => self.send_no_apps(&source),
                    Some(mut app) => {
                        app.stop().await;
                        self.running_apps.remove(&source);
                    }
                }
            }
            "help" => {
                self.send_help(&source);
            }
            _ => {
                match self.running_apps.get_mut(&source) {
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
