use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use crate::Bridge;

pub struct Entry {
    bridge: Arc<Bridge>,
    nonce: AtomicU64,
}

impl Entry {
    pub fn new(bridge: Arc<Bridge>) -> Self {
        Self {
            bridge,
            nonce: AtomicU64::new(1),
        }
    }

    pub fn run(&self, value: u64) {
        let token = self.nonce.fetch_add(1, Ordering::SeqCst).to_be_bytes();
        self.bridge.send(value, token);
    }
}
