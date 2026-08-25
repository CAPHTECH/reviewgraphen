use std::{
    env,
    io::{self, Write},
    process::ExitCode,
};

fn main() -> ExitCode {
    let outcome = reviewgraphen_cli::run_binary(env::args().skip(1).collect());
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    if !outcome.stdout.is_empty() {
        let _ = stdout.write_all(&outcome.stdout);
    }
    if !outcome.stderr.is_empty() {
        let _ = stderr.write_all(outcome.stderr.as_bytes());
    }
    ExitCode::from(outcome.exit_code)
}
