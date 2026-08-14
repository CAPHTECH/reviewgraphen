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
        self.bridge.send(value, value.to_be_bytes());
    }
}
