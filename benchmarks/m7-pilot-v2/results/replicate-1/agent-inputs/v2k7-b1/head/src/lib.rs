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

    fn insert_available(keys: &mut BTreeSet<String>, key: &str) -> bool {
        if keys.contains(key) {
            return false;
        }
        keys.insert(key.to_owned());
        true
    }

    pub fn register(&self, key: &str) -> bool {
        let mut keys = self.keys.lock().expect("registry lock");
        Self::insert_available(&mut keys, key)
    }
}
