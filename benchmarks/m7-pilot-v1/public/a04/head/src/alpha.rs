use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use crate::Bridge;

pub struct Entry {
    bridge: Arc<Bridge>,
    sequence: AtomicU8,
}

impl Entry {
    pub fn new(bridge: Arc<Bridge>) -> Self {
        Self {
            bridge,
            sequence: AtomicU8::new(0),
        }
    }

    pub fn run(&self, _group: u8, value: u64) {
        let group = self.sequence.fetch_add(1, Ordering::SeqCst);
        self.bridge.send(group, value);
    }
}
