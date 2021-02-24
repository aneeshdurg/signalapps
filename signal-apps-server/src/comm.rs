use async_trait::async_trait;

#[async_trait]
pub trait Receiver {
    async fn get_msg(&mut self) -> Option<String>;
}

#[async_trait]
pub trait Control {
    async fn insert_msg(&self, msg: &str);
}

pub trait Sender {
    fn send(&self, dest: &str, msg: &str);
}
