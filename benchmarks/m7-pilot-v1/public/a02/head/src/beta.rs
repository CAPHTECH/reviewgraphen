use std::{collections::BTreeSet, sync::Mutex};

#[derive(Default)]
pub struct Bridge {
    seen: Mutex<BTreeSet<[u8; 8]>>,
}

impl Bridge {
    pub fn send(&self, _value: u64, token: [u8; 8]) -> bool {
        self.seen.lock().expect("lock").insert(token)
    }
}
