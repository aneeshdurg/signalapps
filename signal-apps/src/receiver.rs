use async_trait::async_trait;

#[async_trait]
pub trait Receiver {
    async fn get_msg(&mut self) -> Option<String>;
}
