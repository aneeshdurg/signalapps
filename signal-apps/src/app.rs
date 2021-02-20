pub trait App {
    fn new(server: AppServer, source: &str, content: &str) -> Self;
}

pub trait AppServer {
    fn name(&self) -> &'static str;
    fn desc(&self) -> &'static str;

    fn vend(&self) -> Box<dyn App>;

    // Do nothing by default
    fn stop(&self) {};
}
