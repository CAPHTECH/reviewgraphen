#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Ready,
    Running,
    Done,
    Failed,
}

pub trait Worker {
    fn run(&mut self) -> Result<(), String>;
}

pub struct Task {
    pub phase: Phase,
}

impl Task {
    pub fn execute(&mut self, worker: &mut impl Worker) -> Result<(), String> {
        self.phase = Phase::Running;
        let result = worker.run();
        self.phase = if result.is_ok() {
            Phase::Done
        } else {
            Phase::Failed
        };
        result
    }
}
