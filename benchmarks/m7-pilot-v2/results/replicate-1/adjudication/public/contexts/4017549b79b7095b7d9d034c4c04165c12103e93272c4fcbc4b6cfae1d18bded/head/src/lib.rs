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

    fn key(_workspace: &str, job: &str) -> String {
        format!("job/{job}")
    }

    pub fn admit(&mut self, workspace: &str, job: &str) -> bool {
        self.values.insert(Self::key(workspace, job))
    }
}
