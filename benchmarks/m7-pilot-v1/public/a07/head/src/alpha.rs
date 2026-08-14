use std::sync::{
    Arc, Barrier,
    atomic::{AtomicU8, Ordering},
};

use crate::Bridge;

pub struct Entry {
    bridge: Arc<Bridge>,
    join: Arc<Barrier>,
    stage: AtomicU8,
}

impl Entry {
    pub fn new(join: Arc<Barrier>, bridge: Arc<Bridge>) -> Self {
        Self {
            bridge,
            join,
            stage: AtomicU8::new(0),
        }
    }

    pub fn run(&self, value: u64) {
        self.join.wait();
        if self
            .stage
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        self.bridge.send(value);
    }
}
