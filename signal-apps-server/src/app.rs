use std::io;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::pin_mut;
use serde_json;
use tokio::io::{split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, Mutex};

use crate::appstate::AppMsg;

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

    let mut content = Vec::with_capacity(length);
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

#[async_trait]
pub trait App {
    async fn get_description(
        app_dir: &str,
        name: &str,
    ) -> io::Result<String>;

    fn new(
        id: u64,
        name: &str,
        user: &str,
        control: mpsc::Sender<AppMsg>,
    ) -> Self;

    fn get_id(&self) -> u64;

    fn get_name(&self) -> &str;

    async fn start(&mut self, app_dir: &str, name: &str) -> io::Result<()>;

    async fn send(&mut self, msg: &str);

    async fn stop(&mut self);
}

pub struct UnixStreamApp {
    id: u64,
    name: String,
    user: String,
    control: mpsc::Sender<AppMsg>,
    tx: Option<Arc<Mutex<mpsc::Sender<String>>>>,
    writer: Option<WriteHalf<UnixStream>>,
}

impl UnixStreamApp {
    async fn open_app_socket(
        app_dir: &str,
        name: &str,
    ) -> io::Result<UnixStream> {
        let path = Path::new(app_dir).join(name);
        Ok(UnixStream::connect(path).await?)
    }

    async fn send_bytes(&mut self, bytes: &[u8]) -> bool {
        if let Err(_) = self.writer.as_mut().unwrap().write_all(bytes).await {
            self.control
                .send(AppMsg::EndMsg(self.user.clone(), self.id))
                .await
                .expect("Sending control msg failed!");

            return false;
        }
        true
    }
}

#[async_trait]
impl App for UnixStreamApp {
    async fn get_description(
        app_dir: &str,
        name: &str,
    ) -> io::Result<String> {
        eprintln!("opened socket");
        let stream = Self::open_app_socket(app_dir, name).await?;
        let (mut sr, mut sw) = split(stream);

        // write 1 char
        let query = serde_json::json!({ "type": "query" }).to_string();
        sw.write_all(&(query.len() as u32).to_be_bytes()).await?;
        sw.write_all(query.as_bytes()).await?;
        eprintln!("queried socket");

        if let Ok(desc) = read_msg_from_stream(&mut sr).await {
            if let Ok(desc) = serde_json::from_str::<serde_json::Value>(&desc) {
                if let Some(desc) = desc["value"].as_str() {
                    return Ok(desc.into());
                }
            }
        }

        Ok("".into())
    }

    fn new(
        id: u64,
        name: &str,
        user: &str,
        control: mpsc::Sender<AppMsg>,
    ) -> Self {
        let name = name.into();
        let user = user.into();
        UnixStreamApp {
            id,
            name,
            user,
            control,
            tx: None,
            writer: None,
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
        app_dir: &str,
        name: &str,
    ) -> io::Result<()> {
        let stream = Self::open_app_socket(app_dir, name).await?;
        let (mut sr, sw) = split(stream);

        let (tx, mut rx) = mpsc::channel(100);
        let tx = Arc::new(Mutex::new(tx));

        self.tx = Some(tx.clone());
        self.writer = Some(sw);

        let canceler = Arc::new(Mutex::new(false));
        let id = self.id;

        {
            let user = self.user.clone();
            let control = self.control.clone();
            let canceler = canceler.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                while let Ok(content) = loop {
                    // Read, pausing every 500ms to check if we should cancel instead
                    let reader = read_msg_from_stream(&mut sr);
                    pin_mut!(reader);
                    break loop {
                        match tokio::time::timeout(
                            Duration::from_millis(500),
                            &mut reader,
                        )
                        .await
                        {
                            Ok(content) => {
                                break content;
                            }
                            Err(_) => {
                                if *canceler.lock().await.deref() {
                                    break Err(io::Error::new(
                                        io::ErrorKind::Other,
                                        "Cancelling producer",
                                    ));
                                }
                                continue;
                            }
                        }
                    };
                } {
                    if let Err(_) = tx.lock().await.send(content).await {
                        break;
                    }
                }
                eprintln!("Closed app response producer");
                control
                    .send(AppMsg::EndMsg(user, id))
                    .await
                    .expect("Sending control msg failed!");
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
                            msg["value"].as_str().unwrap_or("").into(),
                        ),
                        _ => AppMsg::EndMsg(user.clone(), id),
                    };
                    control
                        .send(msg)
                        .await
                        .expect("Sending control msg failed!");
                }
            }

            eprintln!("Closed app response consumer");
            *canceler.lock().await.deref_mut() = true;
        });

        Ok(())
    }

    async fn send(&mut self, msg: &str) {
        eprintln!("Sending msg {:?}", msg);
        let _ = self.send_bytes(&(msg.len() as u32).to_be_bytes()).await
            && self.send_bytes(msg.as_bytes()).await;
        eprintln!("Sent msg {:?}", msg);
    }

    async fn stop(&mut self) {
        match self.tx.take() {
            Some(tx) => {
                let _ = tx.lock().await.send("".into()).await;
            }
            None => {}
        }
    }
}
