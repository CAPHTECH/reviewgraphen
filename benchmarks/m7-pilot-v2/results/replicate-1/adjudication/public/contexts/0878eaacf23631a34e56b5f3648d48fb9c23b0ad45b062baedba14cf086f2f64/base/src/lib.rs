pub trait Sink {
    fn append(&mut self, value: &[u8]) -> Result<(), String>;
}

pub fn submit(sink: &mut impl Sink, value: &[u8]) -> Result<(), String> {
    sink.append(value)
}
