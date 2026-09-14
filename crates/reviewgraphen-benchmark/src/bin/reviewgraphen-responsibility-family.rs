//! Narrow CLI for experimental responsibility-family reinspection planning.

use reviewgraphen_benchmark::responsibility_family::decision::{
    DecisionAssessment, DecisionAssessmentInput, DecisionProposal, ResponsibilityFamilyCandidate,
    build_assessment, obligations, propose, validate_proposal,
};
use reviewgraphen_benchmark::responsibility_family::discovery::{
    ExactBodyCandidateReport, NearBodyCandidateReport, ResponsibilitySignalCandidateReport,
    discover_exact_bodies, discover_near_bodies, discover_responsibility_signals,
};
use reviewgraphen_benchmark::responsibility_family::search::{
    PlannedResponsibilityContract, ResponsibilitySearchError, ResponsibilitySearchReport,
    search_responsibility,
};
use reviewgraphen_benchmark::responsibility_family::{
    ReinspectionPlan, ResponsibilityFamilyState, plan, validate_plan,
};
use reviewgraphen_core::{ProgramSpace, canonical_json};
use reviewgraphen_ingest::{IngestRequest, ingest_v2};
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const USAGE: &str = "Usage:\n  reviewgraphen-responsibility-family snapshot --workspace DIR --repository DIR --identity TEXT --base COMMIT --target COMMIT --output FRESH_FILE\n  reviewgraphen-responsibility-family discover-exact --program-space FILE --output FRESH_FILE\n  reviewgraphen-responsibility-family discover-near --program-space FILE --output FRESH_FILE\n  reviewgraphen-responsibility-family discover-signals --program-space FILE --output FRESH_FILE\n  reviewgraphen-responsibility-family search-responsibility --program-space FILE --contract FILE --output FRESH_FILE\n  reviewgraphen-responsibility-family decision-obligations --candidate FILE --output FRESH_FILE\n  reviewgraphen-responsibility-family decision-assess --candidate FILE --input FILE --output FRESH_FILE\n  reviewgraphen-responsibility-family decision-propose --candidate FILE --assessment FILE --output FRESH_FILE\n  reviewgraphen-responsibility-family decision-validate --candidate FILE --assessment FILE --proposal FILE\n  reviewgraphen-responsibility-family plan --before FILE --after FILE --output FRESH_FILE\n  reviewgraphen-responsibility-family validate --before FILE --after FILE --plan FILE\n\nSnapshot performs bounded read-only ingestion with Cargo disabled. Discovery and planned-responsibility search consume accepted snapshot-bound ProgramSpace facts and emit candidate-only reports. Other inputs are externally declared candidates, assessments, and family states. Outputs are non-authoritative; this command accepts no family, executes no target code, and grants no sign-off.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("reviewgraphen-responsibility-family: {error}");
            error.exit_code()
        }
    }
}

fn run() -> Result<(), CliError> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    let command = os_text(&args[0], "command")?;
    let options = parse_options(&args[1..])?;
    match command.as_str() {
        "snapshot" => {
            require_exact_options(
                &options,
                &[
                    "base",
                    "identity",
                    "output",
                    "repository",
                    "target",
                    "workspace",
                ],
            )?;
            let request = IngestRequest::new(
                option_path(&options, "workspace")?,
                option_path(&options, "repository")?,
                option_text(&options, "identity")?,
                option_text(&options, "base")?,
                option_text(&options, "target")?,
            );
            let ingested = ingest_v2(&request)?;
            write_fresh(
                &option_path(&options, "output")?,
                &canonical_json(&ingested.legacy.program_space)?,
            )
        }
        "discover-exact" => {
            require_exact_options(&options, &["output", "program-space"])?;
            let program = read_program_space(&option_path(&options, "program-space")?)?;
            let report: ExactBodyCandidateReport = discover_exact_bodies(&program)?;
            write_fresh(&option_path(&options, "output")?, &canonical_json(&report)?)
        }
        "discover-near" => {
            require_exact_options(&options, &["output", "program-space"])?;
            let program = read_program_space(&option_path(&options, "program-space")?)?;
            let report: NearBodyCandidateReport = discover_near_bodies(&program)?;
            write_fresh(&option_path(&options, "output")?, &canonical_json(&report)?)
        }
        "discover-signals" => {
            require_exact_options(&options, &["output", "program-space"])?;
            let program = read_program_space(&option_path(&options, "program-space")?)?;
            let report: ResponsibilitySignalCandidateReport =
                discover_responsibility_signals(&program)?;
            write_fresh(&option_path(&options, "output")?, &canonical_json(&report)?)
        }
        "search-responsibility" => {
            require_exact_options(&options, &["contract", "output", "program-space"])?;
            let program = read_program_space(&option_path(&options, "program-space")?)?;
            let contract =
                read_planned_responsibility_contract(&option_path(&options, "contract")?)?;
            let report: ResponsibilitySearchReport = search_responsibility(&program, &contract)?;
            write_fresh(&option_path(&options, "output")?, &canonical_json(&report)?)
        }
        "decision-obligations" => {
            require_exact_options(&options, &["candidate", "output"])?;
            let candidate = read_candidate(&option_path(&options, "candidate")?)?;
            let universe = obligations(&candidate)?;
            write_fresh(
                &option_path(&options, "output")?,
                &canonical_json(&universe)?,
            )
        }
        "decision-assess" => {
            require_exact_options(&options, &["candidate", "input", "output"])?;
            let candidate = read_candidate(&option_path(&options, "candidate")?)?;
            let input = read_assessment_input(&option_path(&options, "input")?)?;
            let assessment = build_assessment(&candidate, &input)?;
            write_fresh(
                &option_path(&options, "output")?,
                &canonical_json(&assessment)?,
            )
        }
        "decision-propose" => {
            require_exact_options(&options, &["assessment", "candidate", "output"])?;
            let candidate = read_candidate(&option_path(&options, "candidate")?)?;
            let assessment = read_assessment(&option_path(&options, "assessment")?)?;
            let proposal = propose(&candidate, &assessment)?;
            write_fresh(
                &option_path(&options, "output")?,
                &canonical_json(&proposal)?,
            )
        }
        "decision-validate" => {
            require_exact_options(&options, &["assessment", "candidate", "proposal"])?;
            let candidate = read_candidate(&option_path(&options, "candidate")?)?;
            let assessment = read_assessment(&option_path(&options, "assessment")?)?;
            let proposal = read_decision_proposal(&option_path(&options, "proposal")?)?;
            validate_proposal(&candidate, &assessment, &proposal)?;
            println!("responsibility-family decision proposal validates");
            Ok(())
        }
        "plan" => {
            require_exact_options(&options, &["after", "before", "output"])?;
            let before = read_state(&option_path(&options, "before")?)?;
            let after = read_state(&option_path(&options, "after")?)?;
            let planned = plan(&before, &after)?;
            write_fresh(
                &option_path(&options, "output")?,
                &canonical_json(&planned)?,
            )
        }
        "validate" => {
            require_exact_options(&options, &["after", "before", "plan"])?;
            let before = read_state(&option_path(&options, "before")?)?;
            let after = read_state(&option_path(&options, "after")?)?;
            let planned = read_plan(&option_path(&options, "plan")?)?;
            validate_plan(&before, &after, &planned)?;
            println!("responsibility-family reinspection plan validates");
            Ok(())
        }
        _ => Err(CliError::Usage(format!("unknown command `{command}`"))),
    }
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

fn option_text(
    options: &BTreeMap<String, PathBuf>,
    name: &'static str,
) -> Result<String, CliError> {
    options
        .get(name)
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .ok_or_else(|| CliError::Usage(format!("--{name} must be valid UTF-8")))
}

fn read_state(path: &Path) -> Result<ResponsibilityFamilyState, CliError> {
    read_json(path, "state")
}

fn read_program_space(path: &Path) -> Result<ProgramSpace, CliError> {
    let bytes = fs::read(path).map_err(|source| CliError::Read {
        kind: "ProgramSpace",
        path: path.to_owned(),
        source,
    })?;
    ProgramSpace::from_json_slice(&bytes).map_err(CliError::ProgramSpace)
}

fn read_planned_responsibility_contract(
    path: &Path,
) -> Result<PlannedResponsibilityContract, CliError> {
    read_json(path, "planned responsibility contract")
}

fn read_candidate(path: &Path) -> Result<ResponsibilityFamilyCandidate, CliError> {
    read_json(path, "candidate")
}

fn read_assessment(path: &Path) -> Result<DecisionAssessment, CliError> {
    read_json(path, "assessment")
}

fn read_assessment_input(path: &Path) -> Result<DecisionAssessmentInput, CliError> {
    read_json(path, "assessment input")
}

fn read_decision_proposal(path: &Path) -> Result<DecisionProposal, CliError> {
    read_json(path, "decision proposal")
}

fn read_plan(path: &Path) -> Result<ReinspectionPlan, CliError> {
    read_json(path, "plan")
}

fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
    kind: &'static str,
) -> Result<T, CliError> {
    let bytes = fs::read(path).map_err(|source| CliError::Read {
        kind,
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| CliError::Malformed {
        kind,
        path: path.to_owned(),
        source,
    })
}

fn write_fresh(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let (temporary_path, mut output) = create_fresh_temporary_output(path)?;
    let write_result = output
        .write_all(bytes)
        .and_then(|()| output.flush())
        .and_then(|()| output.sync_all());
    drop(output);
    if let Err(source) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(CliError::Write {
            path: path.to_owned(),
            source,
        });
    }

    match fs::hard_link(&temporary_path, path) {
        Ok(()) => {
            let _ = fs::remove_file(&temporary_path);
            Ok(())
        }
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temporary_path);
            Err(CliError::OutputExists(path.to_owned()))
        }
        Err(source) => {
            let _ = fs::remove_file(&temporary_path);
            Err(CliError::Write {
                path: path.to_owned(),
                source,
            })
        }
    }
}

fn create_fresh_temporary_output(path: &Path) -> Result<(PathBuf, fs::File), CliError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().ok_or_else(|| CliError::Write {
        path: path.to_owned(),
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "output path must name a file",
        ),
    })?;

    for _ in 0..256 {
        let temporary_path = parent.join(temporary_output_name(file_name));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => return Ok((temporary_path, file)),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(CliError::Write {
                    path: path.to_owned(),
                    source,
                });
            }
        }
    }

    Err(CliError::Write {
        path: path.to_owned(),
        source: std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate a fresh temporary output",
        ),
    })
}

fn temporary_output_name(file_name: &OsStr) -> OsString {
    let mut name = OsString::from(".");
    name.push(file_name);
    name.push(".reviewgraphen-");
    name.push(std::process::id().to_string());
    name.push("-");
    name.push(
        TEMP_FILE_SEQUENCE
            .fetch_add(1, Ordering::Relaxed)
            .to_string(),
    );
    name.push(".tmp");
    name
}

#[derive(Debug, Error)]
enum CliError {
    #[error("usage error: {0}\n\n{USAGE}")]
    Usage(String),
    #[error("cannot read {kind} `{path}`: {source}")]
    Read {
        kind: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("malformed {kind} `{path}`: {source}")]
    Malformed {
        kind: &'static str,
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("refusing to overwrite existing output `{0}`")]
    OutputExists(PathBuf),
    #[error("cannot write output `{path}`: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("planning failed: {0}")]
    Planning(#[from] reviewgraphen_benchmark::responsibility_family::ResponsibilityFamilyError),
    #[error("decision support failed: {0}")]
    Decision(#[from] reviewgraphen_benchmark::responsibility_family::decision::DecisionError),
    #[error("exact-body discovery failed: {0}")]
    Discovery(
        #[from] reviewgraphen_benchmark::responsibility_family::discovery::ExactDiscoveryError,
    ),
    #[error("planned responsibility search failed: {0}")]
    Search(#[from] ResponsibilitySearchError),
    #[error("invalid ProgramSpace: {0}")]
    ProgramSpace(reviewgraphen_core::DomainError),
    #[error("snapshot ingestion failed: {0}")]
    Ingest(#[from] reviewgraphen_ingest::IngestError),
    #[error("cannot serialize plan: {0}")]
    Canonical(#[from] reviewgraphen_core::DomainError),
}

impl CliError {
    fn exit_code(&self) -> ExitCode {
        match self {
            Self::Usage(_) => ExitCode::from(2),
            Self::Read { .. } | Self::Malformed { .. } => ExitCode::from(3),
            Self::Planning(_)
            | Self::Decision(_)
            | Self::Discovery(_)
            | Self::Search(_)
            | Self::ProgramSpace(_)
            | Self::Ingest(_) => ExitCode::from(4),
            Self::OutputExists(_) => ExitCode::from(5),
            Self::Write { .. } | Self::Canonical(_) => ExitCode::from(6),
        }
    }
}
