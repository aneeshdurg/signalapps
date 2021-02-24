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

        let mut apps: Vec<_> = self.app_cache.keys().into_iter().collect();
        apps.sort();
        for app in apps {
            let info = self.app_cache.get(app).unwrap();
            lines.push(format!("{} - {}", info.name, info.desc));
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

#[cfg(test)]
mod test {
    use std::cell::RefCell;
    use std::fs::File;

    use async_trait::async_trait;
    use tempdir::TempDir;

    use super::*;

    const SOURCE: &'static str = "+15555555";

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

    struct MockApp {
        id: u64,
        name: String,
        messages: Vec<String>,
    }

    thread_local!(static DESCRIPTIONQUERIES: RefCell<usize> = RefCell::new(0));

    #[async_trait]
    impl app::App for MockApp {
        async fn get_description(
            _app_dir: &str,
            name: &str,
        ) -> io::Result<String> {
            DESCRIPTIONQUERIES.with(|d| {
                *d.borrow_mut() += 1;
            });
            Ok(format!("mockapp {}", name).into())
        }

        fn new(
            id: u64,
            name: &str,
            _user: &str,
            _control: mpsc::Sender<AppMsg>,
        ) -> Self {
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

        async fn start(
            &mut self,
            _app_dir: &str,
            _name: &str,
        ) -> io::Result<()> {
            Ok(())
        }

        async fn send(&mut self, msg: &str) {
            self.messages.push(msg.into());
        }

        async fn stop(&mut self) {}
    }

    #[tokio::test]
    async fn test_finish() {
        let (sender, _) = MockSender::new();
        let config = serde_json::json!({
            "appdir": "/tmp/test"
        });
        let new_app = AppState::new(config, sender);
        let mut state: AppState<MockApp, MockSender> = new_app.0;
        let state_queue = new_app.1;
        state_queue
            .send(AppMsg::Finish)
            .await
            .expect("Failed to send finish");
        // This should exit immediately
        state.process_queue().await;
    }

    #[tokio::test]
    async fn test_currentapp_noapp() {
        let (sender, mut sent) = MockSender::new();
        let config = serde_json::json!({
            "appdir": "/tmp/test"
        });
        let new_app = AppState::new(config, sender);
        let mut state: AppState<MockApp, MockSender> = new_app.0;

        state.run_action(SOURCE.into(), "currentapp".into()).await;

        let msg = sent.recv().await.expect("Found no sent messages");
        assert_eq!(SOURCE, msg.0);
        assert_eq!(
            "You have no running apps. Send `help` to learn more.",
            msg.1
        );

        // Check that there's no additional messages
        drop(state);
        assert_eq!(None, sent.recv().await);
    }

    #[tokio::test]
    async fn test_endapp_noapp() {
        let (sender, mut sent) = MockSender::new();
        let config = serde_json::json!({
            "appdir": "/tmp/test"
        });
        let new_app = AppState::new(config, sender);
        let mut state: AppState<MockApp, MockSender> = new_app.0;

        state.endapp(SOURCE, None).await;
        let msg = sent.recv().await.expect("Found no sent messages");
        assert_eq!(SOURCE, msg.0);
        assert_eq!(
            "You have no running apps. Send `help` to learn more.",
            msg.1
        );

        // This variant should be internal only, so we expect no output
        state.endapp(SOURCE, Some(1)).await;
        drop(state);
        assert_eq!(None, sent.recv().await);
    }

    #[tokio::test]
    async fn test_listapps_empty() {
        let tmp_dir = TempDir::new("apps").expect("create tempdir failed!");
        let (sender, mut sent) = MockSender::new();
        let config = serde_json::json!({
            "appdir": tmp_dir.path().to_str()
        });
        let new_app = AppState::new(config, sender);
        let mut state: AppState<MockApp, MockSender> = new_app.0;
        state.listapps(SOURCE).await;

        let msg = sent.recv().await.expect("Found no sent messages");
        let expected: Vec<&'static str> = vec![
            "You currently have the following apps installed.",
            "To install more, please contact your admin.",
        ];
        let expected = expected.join("\n");

        assert_eq!(SOURCE, msg.0);
        assert_eq!(expected, msg.1);

        // Check that there's no additional messages
        drop(state);
        assert_eq!(None, sent.recv().await);
    }

    #[tokio::test]
    async fn test_listapps_some_apps() {
        let tmp_dir = TempDir::new("apps").expect("create tempdir failed!");
        for i in 0..2 {
            let file_path = tmp_dir.path().join(format!("app{}", i));
            File::create(file_path).expect("create app failed!");
        }

        let (sender, mut sent) = MockSender::new();
        let config = serde_json::json!({
            "appdir": tmp_dir.path().to_str()
        });
        let new_app = AppState::new(config, sender);
        let mut state: AppState<MockApp, MockSender> = new_app.0;
        state.listapps(SOURCE).await;

        let msg = sent.recv().await.expect("Found no sent messages");
        let expected: Vec<&'static str> = vec![
            "You currently have the following apps installed.",
            "To install more, please contact your admin.",
            "app0 - mockapp app0",
            "app1 - mockapp app1",
        ];
        let expected = expected.join("\n");

        assert_eq!(SOURCE, msg.0);
        assert_eq!(expected, msg.1);

        // Check that there's no additional messages
        drop(state);
        assert_eq!(None, sent.recv().await);
    }

    #[tokio::test]
    async fn test_listapps_cached() {
        let tmp_dir = TempDir::new("apps").expect("create tempdir failed!");
        for i in 0..2 {
            let file_path = tmp_dir.path().join(format!("app{}", i));
            File::create(file_path).expect("create app failed!");
        }

        let (sender, mut sent) = MockSender::new();
        let config = serde_json::json!({
            "appdir": tmp_dir.path().to_str()
        });
        let new_app = AppState::new(config, sender);
        let mut state: AppState<MockApp, MockSender> = new_app.0;
        state.listapps(SOURCE).await;
        state.listapps(SOURCE).await;

        for _ in 0u8..2 {
            let msg = sent.recv().await.expect("Found no sent messages");
            let expected: Vec<&'static str> = vec![
                "You currently have the following apps installed.",
                "To install more, please contact your admin.",
                "app0 - mockapp app0",
                "app1 - mockapp app1",
            ];
            let expected = expected.join("\n");

            assert_eq!(SOURCE, msg.0);
            assert_eq!(expected, msg.1);
        }

        DESCRIPTIONQUERIES.with(|d| {
            assert_eq!(*d.borrow(), 2);
        });

        // Check that there's no additional messages
        drop(state);
        assert_eq!(None, sent.recv().await);
    }

    #[tokio::test]
    async fn test_startapp_sends_query() {
        let tmp_dir = TempDir::new("apps").expect("create tempdir failed!");
        let file_path = tmp_dir.path().join("app");
        File::create(file_path).expect("create app failed!");

        let (sender, _) = MockSender::new();
        let config = serde_json::json!({
            "appdir": tmp_dir.path().to_str()
        });
        let new_app = AppState::new(config, sender);
        let mut state: AppState<MockApp, MockSender> = new_app.0;

        // TODO test that a query happened
        state.startapp(SOURCE.into(), "app").await;
        DESCRIPTIONQUERIES.with(|d| {
            assert_eq!(*d.borrow(), 1);
        });

        // TODO test that no query happened
        state.startapp("+other".into(), "app").await;
        DESCRIPTIONQUERIES.with(|d| {
            assert_eq!(*d.borrow(), 1);
        });
    }

    #[tokio::test]
    async fn test_startapp_sends_start_msg() {
        let tmp_dir = TempDir::new("apps").expect("create tempdir failed!");
        let file_path = tmp_dir.path().join("app");
        File::create(file_path).expect("create app failed!");

        let (sender, _) = MockSender::new();
        let config = serde_json::json!({
            "appdir": tmp_dir.path().to_str()
        });
        let new_app = AppState::new(config, sender);
        let mut state: AppState<MockApp, MockSender> = new_app.0;

        state.startapp(SOURCE.into(), "app").await;

        let expected =
            serde_json::json!({"type": "start", "user": SOURCE}).to_string();
        assert_eq!(
            vec![expected],
            state.running_apps.get(SOURCE).unwrap().messages
        );
    }
}
