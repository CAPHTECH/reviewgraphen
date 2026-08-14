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
        if self.stage.load(Ordering::SeqCst) != 0 {
            return;
        }
        self.join.wait();
        self.bridge.send(value);
        self.stage.store(1, Ordering::SeqCst);
    }
}
