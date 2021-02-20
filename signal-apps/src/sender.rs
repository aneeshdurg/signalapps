pub trait Sender {
    fn send(&self, dest: &str, msg: &str);
}

pub struct NullSender { }

impl Sender for NullSender {
    fn send(&self, _dest: &str, _msg: &str) { }
}
