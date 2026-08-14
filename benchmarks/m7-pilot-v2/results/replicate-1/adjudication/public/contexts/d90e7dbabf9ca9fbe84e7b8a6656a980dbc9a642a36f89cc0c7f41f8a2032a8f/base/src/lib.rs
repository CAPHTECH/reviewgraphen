pub trait Channel {
    fn send(&mut self, token: &str, body: &[u8]) -> Result<bool, String>;
}

pub fn transmit(channel: &mut impl Channel, request: &str, body: &[u8]) -> Result<(), String> {
    channel.send(request, body).map(|_| ())
}
