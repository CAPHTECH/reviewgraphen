use std::collections::BTreeSet;

pub struct Seen {
    values: BTreeSet<String>,
}

impl Seen {
    pub fn new() -> Self {
        Self {
            values: BTreeSet::new(),
        }
    }

    pub fn admit(&mut self, workspace: &str, job: &str) -> bool {
        self.values.insert(format!("{workspace}/{job}"))
    }
}
