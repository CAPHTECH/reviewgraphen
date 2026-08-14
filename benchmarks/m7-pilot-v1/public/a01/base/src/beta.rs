use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
pub struct Bridge {
    seen: AtomicUsize,
}

impl Bridge {
    pub fn send(&self, _value: u64) {
        self.seen.fetch_add(1, Ordering::SeqCst);
    }

    pub fn seen(&self) -> usize {
        self.seen.load(Ordering::SeqCst)
    }
}
