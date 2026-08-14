use std::sync::Arc;

use crate::Bridge;

pub struct Entry {
    bridge: Arc<Bridge>,
}

impl Entry {
    pub fn new(bridge: Arc<Bridge>) -> Self {
        Self { bridge }
    }

    pub fn run(&self, value: u64) {
        let token = value.to_be_bytes();
        self.bridge.send(value, token);
    }
}
