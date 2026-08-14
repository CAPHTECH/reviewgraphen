pub trait Channel {
    fn send(&mut self, token: &str, body: &[u8]) -> Result<bool, String>;
}

pub fn transmit(channel: &mut impl Channel, request: &str, body: &[u8]) -> Result<(), String> {
    for attempt in 0..3 {
        let token = format!("{request}:{attempt}");
        if channel.send(&token, body)? {
            return Ok(());
        }
    }
    Err("delivery unconfirmed".to_owned())
}
