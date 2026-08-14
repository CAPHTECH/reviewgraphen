use std::{collections::BTreeSet, sync::Mutex};

#[derive(Default)]
pub struct Bridge {
    seen: Mutex<BTreeSet<u64>>,
}

impl Bridge {
    pub fn send(&self, _group: u8, value: u64) -> bool {
        self.seen.lock().expect("lock").insert(value)
    }
}
