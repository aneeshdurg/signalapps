pub trait Receiver {
    fn insert_msg(&self, msg: &str);
    fn get_msg(&self) -> String;
    fn stop(&mut self);
}
