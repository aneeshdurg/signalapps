use async_trait::async_trait;

#[async_trait]
pub trait Receiver {
    async fn get_msg(&mut self) -> Option<String>;
}

pub trait Control {
    fn insert_msg(&self, msg: &str);
    fn stop(&mut self);
}

pub trait Sender {
    fn send(&self, dest: &str, msg: &str);
}

pub struct NullSender {}

impl Sender for NullSender {
    fn send(&self, _dest: &str, _msg: &str) {}
}
