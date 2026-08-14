use std::sync::{
    Arc, Barrier,
    atomic::{AtomicBool, Ordering},
};

use crate::Bridge;

pub struct Entry {
    active: AtomicBool,
    bridge: Arc<Bridge>,
    join: Arc<Barrier>,
}

impl Entry {
    pub fn new(join: Arc<Barrier>, bridge: Arc<Bridge>) -> Self {
        Self {
            active: AtomicBool::new(false),
            bridge,
            join,
        }
    }

    pub fn run(&self, value: u64) {
        if self.active.load(Ordering::SeqCst) {
            return;
        }
        self.join.wait();
        self.active.store(true, Ordering::SeqCst);
        self.bridge.send(value);
        self.active.store(false, Ordering::SeqCst);
    }
}
