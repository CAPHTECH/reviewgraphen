pub trait Sink {
    fn append(&mut self, value: &[u8]) -> Result<(), String>;
}

pub trait Receipt {
    fn observed(&mut self) -> bool;
}

pub fn submit(
    sink: &mut impl Sink,
    receipt: &mut impl Receipt,
    value: &[u8],
) -> Result<(), String> {
    for _ in 0..2 {
        sink.append(value)?;
        if receipt.observed() {
            return Ok(());
        }
    }
    Err("receipt unavailable".to_owned())
}
