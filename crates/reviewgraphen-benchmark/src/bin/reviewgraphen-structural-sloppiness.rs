//! Narrow CLI for the experimental structural-sloppiness benchmark contract.
//!
//! It accepts only already-serialized ProgramSpace input and never executes
//! repository or target code.

use reviewgraphen_benchmark::structural_sloppiness::{
    analyze, flat_projection_canonical_bytes, validate_report,
};
use reviewgraphen_core::{ProgramSpace, canonical_json};
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use thiserror::Error;

const USAGE: &str = "Usage:\n  reviewgraphen-structural-sloppiness analyze --input FILE --output FRESH_FILE\n  reviewgraphen-structural-sloppiness validate --input FILE --report FILE\n  reviewgraphen-structural-sloppiness flat --input FILE --output FRESH_FILE\n\nThe command validates ProgramSpace input only; it does not execute target code.\nA successful analysis is a completed non-authoritative observation, not a clean or safe sign-off.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("reviewgraphen-structural-sloppiness: {error}");
            error.exit_code()
        }
    }
}

fn run() -> Result<(), CliError> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.is_empty() || has_help(&args) {
        println!("{USAGE}");
        return Ok(());
    }
    let command = os_text(&args[0], "command")?;
    let options = parse_options(&args[1..])?;
    match command.as_str() {
        "analyze" => {
            let (input, output) = input_and_output(&options)?;
            let report = analyze(&read_program(&input)?)?;
            write_fresh(&output, &canonical_json(&report)?)
        }
        "validate" => {
            require_exact_options(&options, &["input", "report"])?;
            let input = option_path(&options, "input")?;
            let report_path = option_path(&options, "report")?;
            let program = read_program(&input)?;
            let report_bytes = fs::read(&report_path).map_err(|source| CliError::ReportRead {
                path: report_path.clone(),
                source,
            })?;
            let report = serde_json::from_slice(&report_bytes).map_err(|source| {
                CliError::MalformedReport {
                    path: report_path,
                    source,
                }
            })?;
            validate_report(&program, &report).map_err(CliError::StaleReport)?;
            println!("structural-sloppiness report validates against the supplied ProgramSpace");
            Ok(())
        }
        "flat" => {
            let (input, output) = input_and_output(&options)?;
            write_fresh(
                &output,
                &flat_projection_canonical_bytes(&read_program(&input)?)?,
            )
        }
        _ => Err(CliError::Usage(format!("unknown command `{command}`"))),
    }
}

fn has_help(args: &[OsString]) -> bool {
    args.iter().any(|arg| arg == "--help" || arg == "-h")
}

fn os_text(value: &OsString, field: &'static str) -> Result<String, CliError> {
    value
        .clone()
        .into_string()
        .map_err(|_| CliError::Usage(format!("{field} must be valid UTF-8")))
}

fn parse_options(args: &[OsString]) -> Result<BTreeMap<String, PathBuf>, CliError> {
    let mut result = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let name = os_text(&args[index], "option")?;
        if !name.starts_with("--") {
            return Err(CliError::Usage(format!("expected option, found `{name}`")));
        }
        let Some(value) = args.get(index + 1) else {
            return Err(CliError::Usage(format!("option `{name}` requires a path")));
        };
        if result
            .insert(
                name.trim_start_matches("--").to_owned(),
                PathBuf::from(value),
            )
            .is_some()
        {
            return Err(CliError::Usage(format!(
                "option `{name}` was supplied more than once"
            )));
        }
        index += 2;
    }
    Ok(result)
}

fn input_and_output(options: &BTreeMap<String, PathBuf>) -> Result<(PathBuf, PathBuf), CliError> {
    require_exact_options(options, &["input", "output"])?;
    Ok((
        option_path(options, "input")?,
        option_path(options, "output")?,
    ))
}

fn require_exact_options(
    options: &BTreeMap<String, PathBuf>,
    expected: &[&str],
) -> Result<(), CliError> {
    if options.len() != expected.len() || expected.iter().any(|name| !options.contains_key(*name)) {
        return Err(CliError::Usage(format!(
            "expected exactly: {}",
            expected
                .iter()
                .map(|name| format!("--{name} FILE"))
                .collect::<Vec<_>>()
                .join(" ")
        )));
    }
    Ok(())
}

fn option_path(
    options: &BTreeMap<String, PathBuf>,
    name: &'static str,
) -> Result<PathBuf, CliError> {
    options
        .get(name)
        .cloned()
        .ok_or_else(|| CliError::Usage(format!("missing --{name}")))
}

fn read_program(path: &Path) -> Result<ProgramSpace, CliError> {
    let bytes = fs::read(path).map_err(|source| CliError::InputRead {
        path: path.to_owned(),
        source,
    })?;
    ProgramSpace::from_json_slice(&bytes).map_err(|source| CliError::MalformedInput {
        path: path.to_owned(),
        source,
    })
}

fn write_fresh(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let mut output = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(CliError::OutputExists(path.to_owned()));
        }
        Err(source) => {
            return Err(CliError::OutputOpen {
                path: path.to_owned(),
                source,
            });
        }
    };
    output
        .write_all(bytes)
        .map_err(|source| CliError::OutputWrite {
            path: path.to_owned(),
            source,
        })
}

#[derive(Debug, Error)]
enum CliError {
    #[error("usage error: {0}\n\n{USAGE}")]
    Usage(String),
    #[error("cannot read ProgramSpace input `{path}`: {source}")]
    InputRead {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("malformed ProgramSpace input `{path}`: {source}")]
    MalformedInput {
        path: PathBuf,
        source: reviewgraphen_core::DomainError,
    },
    #[error("cannot read report `{path}`: {source}")]
    ReportRead {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("malformed structural-sloppiness report `{path}`: {source}")]
    MalformedReport {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("stale or tampered report: {0}")]
    StaleReport(reviewgraphen_benchmark::structural_sloppiness::StructuralSloppinessError),
    #[error("refusing to overwrite existing output `{0}`")]
    OutputExists(PathBuf),
    #[error("cannot create output `{path}`: {source}")]
    OutputOpen {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot write output `{path}`: {source}")]
    OutputWrite {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("analysis failed: {0}")]
    Analysis(#[from] reviewgraphen_benchmark::structural_sloppiness::StructuralSloppinessError),
    #[error("cannot serialize report: {0}")]
    Canonical(#[from] reviewgraphen_core::DomainError),
}

impl CliError {
    fn exit_code(&self) -> ExitCode {
        match self {
            Self::Usage(_) => ExitCode::from(2),
            Self::InputRead { .. }
            | Self::MalformedInput { .. }
            | Self::ReportRead { .. }
            | Self::MalformedReport { .. } => ExitCode::from(3),
            Self::StaleReport(_) => ExitCode::from(4),
            Self::OutputExists(_) => ExitCode::from(5),
            Self::OutputOpen { .. }
            | Self::OutputWrite { .. }
            | Self::Analysis(_)
            | Self::Canonical(_) => ExitCode::from(6),
        }
    }
}
