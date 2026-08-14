use std::collections::BTreeSet;
use std::sync::Mutex;

pub struct Registry {
    keys: Mutex<BTreeSet<String>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            keys: Mutex::new(BTreeSet::new()),
        }
    }

    fn available(&self, key: &str) -> bool {
        !self.keys.lock().expect("registry lock").contains(key)
    }

    pub fn register(&self, key: &str) -> bool {
        if !self.available(key) {
            return false;
        }
        self.keys
            .lock()
            .expect("registry lock")
            .insert(key.to_owned());
        true
    }
}
