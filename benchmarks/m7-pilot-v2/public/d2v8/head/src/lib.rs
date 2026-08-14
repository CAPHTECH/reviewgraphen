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
        worker.run()?;
        self.phase = Phase::Done;
        Ok(())
    }
}
