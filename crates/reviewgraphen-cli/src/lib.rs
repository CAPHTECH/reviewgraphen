//! Closed CLI surface for schema inspection and generic review orchestration.
//!
//! Generic review is not exposed until its input, isolation, replay, and
//! non-authority contracts are implemented. Fixed-fixture review is never a
//! product command.

use jsonschema::{Resource, Validator};
use reviewgraphen_core::{ContentHash, canonical_json};
use reviewgraphen_runtime::diagnostics::{
    GenericReviewDiagnosticsCollector, GenericReviewStage, GenericReviewStageEvent,
    GenericReviewStageObserver, MonotonicMicrosecondClock,
};
use reviewgraphen_runtime::generic::{
    GENERIC_REVIEW_REQUEST_V2_SCHEMA, GENERIC_REVIEW_REQUEST_V3_SCHEMA,
    GENERIC_REVIEW_REQUEST_V4_SCHEMA, GenericReviewRequest, GenericReviewRequestV2,
    GenericReviewRequestV3, GenericReviewRequestV4, admit_fresh_generic_review_artifact_root_v2,
    run_generic_review, run_generic_review_v2_with_observer, run_generic_review_v3_with_observer,
    run_generic_review_v4_with_observer,
};
#[cfg(unix)]
use rustix::fs::{self, FileType, Mode, OFlags};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fs as stdfs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
    time::Instant,
};

const MAX_INPUT_BYTES: u64 = 128 * 1024 * 1024;
const V3_URI: &str = "https://capht.tech/schemas/reviewgraphen/review-report.v3.schema.json";
const V4_URI: &str = "https://capht.tech/schemas/reviewgraphen/review-report.v4.schema.json";

pub struct CommandOutcome {
    pub exit_code: u8,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl CommandOutcome {
    fn success(stdout: Vec<u8>, stderr: impl Into<String>) -> Self {
        Self {
            exit_code: 0,
            stdout,
            stderr: stderr.into(),
        }
    }

    fn failure(exit_code: u8, stderr: impl Into<String>) -> Self {
        Self {
            exit_code,
            stdout: Vec::new(),
            stderr: stderr.into(),
        }
    }
}

/// Executes the closed command surface. Unknown review forms, including every
/// fixed-fixture form, are rejected before filesystem or report work starts.
pub fn run(arguments: Vec<String>) -> CommandOutcome {
    match arguments.as_slice() {
        [flag] if flag == "--version" || flag == "-V" => CommandOutcome::success(
            format!("reviewgraphen {}\n", env!("CARGO_PKG_VERSION")).into_bytes(),
            String::new(),
        ),
        [
            command,
            request_flag,
            request_path,
            artifacts_flag,
            artifact_root,
        ] if command == "review"
            && request_flag == "--request"
            && artifacts_flag == "--artifacts" =>
        {
            generic_review(Path::new(request_path), Path::new(artifact_root))
        }
        [command, subcommand] if command == "schema" && subcommand == "list" => schema_list(),
        [command, subcommand, name] if command == "schema" && subcommand == "print" => {
            schema_print(name)
        }
        [command, subcommand, path] if command == "schema" && subcommand == "validate" => {
            schema_validate(Path::new(path))
        }
        _ => CommandOutcome::failure(2, usage()),
    }
}

/// Executes the product CLI, including the optional operational diagnostic.
pub fn run_binary(arguments: Vec<String>) -> CommandOutcome {
    let (review_arguments, diagnostic_argument) = match split_diagnostics_argument(arguments) {
        Ok(parsed) => parsed,
        Err(error) => return CommandOutcome::failure(2, error),
    };
    if !is_generic_review_command(&review_arguments) {
        return run(review_arguments);
    }
    let diagnostic_path = match diagnostic_argument {
        Some(path) => match validate_diagnostic_path(&review_arguments, &path) {
            Ok(path) => Some(path),
            Err(error) => return CommandOutcome::failure(2, error),
        },
        None => None,
    };
    let mut trace = StageTrace::new();
    let outcome = run_review_with_observer(&review_arguments, &mut trace);
    if outcome.exit_code != 0 {
        trace.close_failure();
    }
    if let Some(path) = diagnostic_path
        && write_diagnostic(&path, &outcome, trace.collector).is_err()
    {
        return CommandOutcome::failure(2, "unable to write generic review diagnostics");
    }
    outcome
}

fn run_review_with_observer(
    arguments: &[String],
    observer: &mut dyn GenericReviewStageObserver,
) -> CommandOutcome {
    let [_, _, request_path, _, artifact_root] = arguments else {
        return CommandOutcome::failure(2, usage());
    };
    generic_review_with_observer(Path::new(request_path), Path::new(artifact_root), observer)
}

fn split_diagnostics_argument(
    mut arguments: Vec<String>,
) -> Result<(Vec<String>, Option<PathBuf>), &'static str> {
    let positions = arguments
        .iter()
        .enumerate()
        .filter_map(|(index, value)| (value == "--diagnostics").then_some(index))
        .collect::<Vec<_>>();
    if positions.is_empty() {
        return Ok((arguments, None));
    }
    if positions.len() != 1 || positions[0] + 1 >= arguments.len() {
        return Err("invalid --diagnostics usage");
    }
    let index = positions[0];
    let path = PathBuf::from(arguments.remove(index + 1));
    arguments.remove(index);
    if !is_generic_review_command(&arguments) {
        return Err("--diagnostics is only valid for review");
    }
    Ok((arguments, Some(path)))
}

fn validate_diagnostic_path(
    review_arguments: &[String],
    diagnostic_argument: &Path,
) -> Result<PathBuf, &'static str> {
    let [_, _, _, _, artifact_argument] = review_arguments else {
        return Err("invalid --diagnostics usage");
    };
    if diagnostic_argument.as_os_str().is_empty() {
        return Err("generic review diagnostic path is empty");
    }
    let cwd = canonical_invocation_root().map_err(|_| "invalid generic review diagnostic cwd")?;
    let artifact_root = resolve_artifact_root(&cwd, Path::new(artifact_argument))
        .map_err(|_| "generic review diagnostic/artifact path overlap")?;
    let diagnostic = if diagnostic_argument.is_absolute() {
        diagnostic_argument.to_path_buf()
    } else {
        cwd.join(diagnostic_argument)
    };
    let parent = diagnostic
        .parent()
        .ok_or("generic review diagnostic parent missing")?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| "generic review diagnostic parent invalid")?;
    if canonical_parent != parent {
        return Err("generic review diagnostic parent contains symlink");
    }
    let diagnostic = canonical_parent.join(
        diagnostic
            .file_name()
            .ok_or("generic review diagnostic filename missing")?,
    );
    if diagnostic == artifact_root
        || diagnostic.starts_with(&artifact_root)
        || artifact_root.starts_with(&diagnostic)
    {
        return Err("generic review diagnostic/artifact path overlap");
    }
    match stdfs::symlink_metadata(&diagnostic) {
        Ok(_) => Err("generic review diagnostic path already exists"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(diagnostic),
        Err(_) => Err("unable to inspect generic review diagnostic path"),
    }
}

struct DiagnosticClock(Instant);

impl MonotonicMicrosecondClock for DiagnosticClock {
    fn now_microseconds(&mut self) -> u64 {
        u64::try_from(self.0.elapsed().as_micros()).unwrap_or(u64::MAX)
    }
}

fn write_diagnostic(
    path: &Path,
    outcome: &CommandOutcome,
    collector: GenericReviewDiagnosticsCollector<DiagnosticClock>,
) -> Result<(), &'static str> {
    use std::io::Write as _;
    let bindings = serde_json::from_slice::<Value>(&outcome.stdout).ok();
    let binding = |field: &str| {
        bindings
            .as_ref()
            .and_then(|value| value.get(field))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let diagnostic = collector
        .finish(
            binding("request_id"),
            binding("run_id"),
            outcome.exit_code.to_string(),
        )
        .map_err(|_| "unable to finish generic review diagnostics")?;
    let bytes = diagnostic
        .canonical_bytes()
        .map_err(|_| "unable to serialize generic review diagnostics")?;
    let mut file = stdfs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "unable to create generic review diagnostics")?;
    file.write_all(&bytes)
        .map_err(|_| "unable to write generic review diagnostics")
}

fn is_generic_review_command(arguments: &[String]) -> bool {
    matches!(
        arguments,
        [command, request_flag, _, artifacts_flag, _]
            if command == "review"
                && request_flag == "--request"
                && artifacts_flag == "--artifacts"
    )
}

struct StageTrace {
    collector: GenericReviewDiagnosticsCollector<DiagnosticClock>,
    active: Option<GenericReviewStage>,
    last_terminal: Option<GenericReviewStage>,
    last_terminal_failed: bool,
}

impl GenericReviewStageObserver for StageTrace {
    fn observe(&mut self, event: GenericReviewStageEvent) {
        StageTrace::observe(self, event);
    }
}

impl StageTrace {
    fn new() -> Self {
        Self {
            collector: GenericReviewDiagnosticsCollector::new(DiagnosticClock(Instant::now())),
            active: None,
            last_terminal: None,
            last_terminal_failed: false,
        }
    }

    fn observe(&mut self, event: GenericReviewStageEvent) {
        match event {
            GenericReviewStageEvent::Begin(stage) => self.active = Some(stage),
            GenericReviewStageEvent::Completed(stage) => {
                self.active = None;
                self.last_terminal = Some(stage);
                self.last_terminal_failed = false;
            }
            GenericReviewStageEvent::Failed(stage) => {
                self.active = None;
                self.last_terminal = Some(stage);
                self.last_terminal_failed = true;
            }
            GenericReviewStageEvent::Skipped(stage) => {
                self.last_terminal = Some(stage);
                self.last_terminal_failed = false;
            }
        }
        self.collector.observe(event);
    }

    fn close_failure(&mut self) {
        let already_failed = self.active.is_none() && self.last_terminal_failed;
        let failed = self.active.unwrap_or_else(|| {
            if already_failed {
                return self.last_terminal.expect("failed terminal has a stage");
            }
            let next = self
                .last_terminal
                .and_then(next_stage)
                .unwrap_or(GenericReviewStage::Ingest);
            self.observe(GenericReviewStageEvent::Begin(next));
            next
        });
        if !already_failed {
            self.observe(GenericReviewStageEvent::Failed(failed));
        }
        let mut after_failed = false;
        for stage in GenericReviewStage::ORDERED {
            if after_failed {
                self.observe(GenericReviewStageEvent::Skipped(stage));
            }
            if stage == failed {
                after_failed = true;
            }
        }
    }
}

fn next_stage(stage: GenericReviewStage) -> Option<GenericReviewStage> {
    GenericReviewStage::ORDERED
        .into_iter()
        .skip_while(|candidate| *candidate != stage)
        .nth(1)
}

fn usage() -> &'static str {
    "usage: reviewgraphen [--version] | review --request <request.json> --artifacts <fresh-dir> [--diagnostics <fresh-file>] | schema list|print <schema-id>|validate <json-file>"
}

fn generic_review(request_path: &Path, artifact_root: &Path) -> CommandOutcome {
    let mut observer = reviewgraphen_runtime::diagnostics::NoopGenericReviewStageObserver;
    generic_review_with_observer(request_path, artifact_root, &mut observer)
}

fn generic_review_with_observer(
    request_path: &Path,
    artifact_root: &Path,
    observer: &mut dyn GenericReviewStageObserver,
) -> CommandOutcome {
    let bytes = match bounded_regular_file(request_path) {
        Ok(bytes) => bytes,
        Err(error) => return CommandOutcome::failure(3, error),
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return CommandOutcome::failure(3, "invalid generic review request JSON"),
    };
    match value.get("schema").and_then(Value::as_str) {
        Some(GENERIC_REVIEW_REQUEST_V2_SCHEMA) => {
            generic_review_v2(&bytes, value, artifact_root, observer)
        }
        Some(GENERIC_REVIEW_REQUEST_V3_SCHEMA) => {
            generic_review_v3(&bytes, value, artifact_root, observer)
        }
        Some(GENERIC_REVIEW_REQUEST_V4_SCHEMA) => {
            generic_review_v4(&bytes, value, artifact_root, observer)
        }
        _ => {
            let request: GenericReviewRequest = match serde_json::from_value(value) {
                Ok(request) => request,
                Err(_) => {
                    return CommandOutcome::failure(3, "invalid generic review request JSON");
                }
            };
            match run_generic_review(&request, artifact_root).and_then(|run| run.canonical_bytes())
            {
                Ok(bytes) => CommandOutcome::success(bytes, String::new()),
                Err(error) => CommandOutcome::failure(20, error.to_string()),
            }
        }
    }
}

fn generic_review_v4(
    request_bytes: &[u8],
    mut request_value: Value,
    artifact_argument: &Path,
    observer: &mut dyn GenericReviewStageObserver,
) -> CommandOutcome {
    let cwd = match canonical_invocation_root() {
        Ok(path) => path,
        Err(error) => return CommandOutcome::failure(3, error),
    };
    if !v4_request_is_valid(&request_value) || !uses_dot_admission_roots(&request_value) {
        return CommandOutcome::failure(3, "invalid generic review v4 request");
    }
    let artifact_root = match resolve_artifact_root(&cwd, artifact_argument) {
        Ok(path) => path,
        Err(error) => return CommandOutcome::failure(20, error),
    };
    if let Err(error) = require_absent_artifact_root(&artifact_root) {
        return CommandOutcome::failure(20, error);
    }
    let root = match cwd.to_str() {
        Some(root) => root,
        None => return CommandOutcome::failure(3, "canonical invocation cwd is not UTF-8"),
    };
    let Some(request) = request_value.as_object_mut() else {
        return CommandOutcome::failure(3, "invalid generic review v4 request");
    };
    request.insert(
        "workspace_admission_root".to_owned(),
        Value::String(root.to_owned()),
    );
    request.insert(
        "repository_admission_root".to_owned(),
        Value::String(root.to_owned()),
    );
    let request: GenericReviewRequestV4 = match serde_json::from_value(request_value) {
        Ok(request) => request,
        Err(_) => return CommandOutcome::failure(3, "invalid generic review v4 request"),
    };
    let run = match run_generic_review_v4_with_observer(&request, observer) {
        Ok(run) => run,
        Err(error) => return CommandOutcome::failure(20, error.to_string()),
    };
    if let Err(error) = admit_fresh_generic_review_artifact_root_v2(&artifact_root) {
        return CommandOutcome::failure(20, error.to_string());
    }
    observer.observe(GenericReviewStageEvent::Begin(GenericReviewStage::Report));
    let audit_bytes = match run.canonical_bytes() {
        Ok(bytes) => bytes,
        Err(error) => {
            observer.observe(GenericReviewStageEvent::Failed(GenericReviewStage::Report));
            return CommandOutcome::failure(20, error.to_string());
        }
    };
    let human_report = match reviewgraphen_report::generate_generic_human_report_v4(&run) {
        Ok(report) => report,
        Err(error) => {
            observer.observe(GenericReviewStageEvent::Failed(GenericReviewStage::Report));
            return CommandOutcome::failure(20, error.to_string());
        }
    };
    observer.observe(GenericReviewStageEvent::Completed(
        GenericReviewStage::Report,
    ));
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        "audit.run.v4.json".to_owned(),
        ("audit".to_owned(), audit_bytes.clone()),
    );
    artifacts.insert(
        "human-report.manifest.v3.json".to_owned(),
        (
            "human_report_manifest".to_owned(),
            human_report.manifest_bytes,
        ),
    );
    artifacts.insert(
        "human-report.md".to_owned(),
        (
            "human_report_markdown".to_owned(),
            human_report.markdown_bytes,
        ),
    );
    let Some(request_id) = run.value()["request_id"].as_str() else {
        return CommandOutcome::failure(20, "validated v4 run is missing request_id");
    };
    let Some(run_id) = run.value()["run_id"].as_str() else {
        return CommandOutcome::failure(20, "validated v4 run is missing run_id");
    };
    let Some(snapshot_id) = run.value()["legacy_ingestion"]["snapshot_id"].as_str() else {
        return CommandOutcome::failure(20, "validated v4 run is missing snapshot_id");
    };
    let Some(universe_id) = run.value()["plan"]["universe_id"].as_str() else {
        return CommandOutcome::failure(20, "validated v4 run is missing universe_id");
    };
    let manifest = artifact_manifest(
        request_bytes,
        request_id,
        run_id,
        snapshot_id,
        universe_id,
        &artifacts,
    );
    let manifest_bytes = match canonical_json(&manifest) {
        Ok(bytes) => bytes,
        Err(error) => return CommandOutcome::failure(20, error.to_string()),
    };
    observer.observe(GenericReviewStageEvent::Begin(
        GenericReviewStage::ArtifactWrite,
    ));
    if let Err(error) = write_artifacts(&artifact_root, &artifacts, &manifest_bytes) {
        observer.observe(GenericReviewStageEvent::Failed(
            GenericReviewStage::ArtifactWrite,
        ));
        return CommandOutcome::failure(20, error);
    }
    observer.observe(GenericReviewStageEvent::Completed(
        GenericReviewStage::ArtifactWrite,
    ));
    CommandOutcome::success(audit_bytes, String::new())
}

fn generic_review_v3(
    request_bytes: &[u8],
    mut request_value: Value,
    artifact_argument: &Path,
    observer: &mut dyn GenericReviewStageObserver,
) -> CommandOutcome {
    let cwd = match canonical_invocation_root() {
        Ok(path) => path,
        Err(error) => return CommandOutcome::failure(3, error),
    };
    if !v3_request_is_valid(&request_value) || !uses_dot_admission_roots(&request_value) {
        return CommandOutcome::failure(3, "invalid generic review v3 request");
    }
    let artifact_root = match resolve_artifact_root(&cwd, artifact_argument) {
        Ok(path) => path,
        Err(error) => return CommandOutcome::failure(20, error),
    };
    if let Err(error) = require_absent_artifact_root(&artifact_root) {
        return CommandOutcome::failure(20, error);
    }
    let root = match cwd.to_str() {
        Some(root) => root,
        None => return CommandOutcome::failure(3, "canonical invocation cwd is not UTF-8"),
    };
    let Some(request) = request_value.as_object_mut() else {
        return CommandOutcome::failure(3, "invalid generic review v3 request");
    };
    request.insert(
        "workspace_admission_root".to_owned(),
        Value::String(root.to_owned()),
    );
    request.insert(
        "repository_admission_root".to_owned(),
        Value::String(root.to_owned()),
    );
    let request: GenericReviewRequestV3 = match serde_json::from_value(request_value) {
        Ok(request) => request,
        Err(_) => return CommandOutcome::failure(3, "invalid generic review v3 request"),
    };
    let run = match run_generic_review_v3_with_observer(&request, None, observer) {
        Ok(run) => run,
        Err(error) => return CommandOutcome::failure(20, error.to_string()),
    };
    if let Err(error) = admit_fresh_generic_review_artifact_root_v2(&artifact_root) {
        return CommandOutcome::failure(20, error.to_string());
    }
    observer.observe(GenericReviewStageEvent::Begin(GenericReviewStage::Report));
    let audit_bytes = match run.canonical_bytes() {
        Ok(bytes) => bytes,
        Err(error) => {
            observer.observe(GenericReviewStageEvent::Failed(GenericReviewStage::Report));
            return CommandOutcome::failure(20, error.to_string());
        }
    };
    let human_report = match reviewgraphen_report::generate_generic_human_report_v3(&run) {
        Ok(report) => report,
        Err(error) => {
            observer.observe(GenericReviewStageEvent::Failed(GenericReviewStage::Report));
            return CommandOutcome::failure(20, error.to_string());
        }
    };
    observer.observe(GenericReviewStageEvent::Completed(
        GenericReviewStage::Report,
    ));
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        "audit.run.v3.json".to_owned(),
        ("audit".to_owned(), audit_bytes.clone()),
    );
    artifacts.insert(
        "human-report.manifest.v2.json".to_owned(),
        (
            "human_report_manifest".to_owned(),
            human_report.manifest_bytes,
        ),
    );
    artifacts.insert(
        "human-report.md".to_owned(),
        (
            "human_report_markdown".to_owned(),
            human_report.markdown_bytes,
        ),
    );
    for record in run.provider_free_record_artifacts() {
        let packet_path = format!("records/{}", record.reviewer_packet_filename());
        let output_path = format!(
            "records/{}",
            record.deterministic_observer_output_filename()
        );
        let packet = match record.reviewer_packet.canonical_bytes() {
            Ok(bytes) => bytes,
            Err(error) => return CommandOutcome::failure(20, error.to_string()),
        };
        let output = match record.deterministic_observer_output.canonical_bytes() {
            Ok(bytes) => bytes,
            Err(error) => return CommandOutcome::failure(20, error.to_string()),
        };
        artifacts.insert(packet_path, ("reviewer_packet".to_owned(), packet));
        artifacts.insert(
            output_path,
            ("deterministic_observer_output".to_owned(), output),
        );
    }
    let Some(request_id) = run.value()["request_id"].as_str() else {
        return CommandOutcome::failure(20, "validated v3 run is missing request_id");
    };
    let Some(run_id) = run.value()["run_id"].as_str() else {
        return CommandOutcome::failure(20, "validated v3 run is missing run_id");
    };
    let Some(snapshot_id) = run.value()["legacy_ingestion"]["snapshot_id"].as_str() else {
        return CommandOutcome::failure(20, "validated v3 run is missing snapshot_id");
    };
    let Some(universe_id) = run.value()["plan"]["universe_id"].as_str() else {
        return CommandOutcome::failure(20, "validated v3 run is missing universe_id");
    };
    let manifest = artifact_manifest(
        request_bytes,
        request_id,
        run_id,
        snapshot_id,
        universe_id,
        &artifacts,
    );
    let manifest_bytes = match canonical_json(&manifest) {
        Ok(bytes) => bytes,
        Err(error) => return CommandOutcome::failure(20, error.to_string()),
    };
    observer.observe(GenericReviewStageEvent::Begin(
        GenericReviewStage::ArtifactWrite,
    ));
    if let Err(error) = write_artifacts(&artifact_root, &artifacts, &manifest_bytes) {
        observer.observe(GenericReviewStageEvent::Failed(
            GenericReviewStage::ArtifactWrite,
        ));
        return CommandOutcome::failure(20, error);
    }
    observer.observe(GenericReviewStageEvent::Completed(
        GenericReviewStage::ArtifactWrite,
    ));
    CommandOutcome::success(audit_bytes, String::new())
}

fn generic_review_v2(
    request_bytes: &[u8],
    mut request_value: Value,
    artifact_argument: &Path,
    observer: &mut dyn GenericReviewStageObserver,
) -> CommandOutcome {
    let cwd = match canonical_invocation_root() {
        Ok(path) => path,
        Err(error) => return CommandOutcome::failure(3, error),
    };
    if !v2_request_is_valid(&request_value) || !uses_dot_admission_roots(&request_value) {
        return CommandOutcome::failure(3, "invalid generic review v2 request");
    }
    let artifact_root = match resolve_artifact_root(&cwd, artifact_argument) {
        Ok(path) => path,
        Err(error) => return CommandOutcome::failure(20, error),
    };
    if let Err(error) = require_absent_artifact_root(&artifact_root) {
        return CommandOutcome::failure(20, error);
    }
    let root = match cwd.to_str() {
        Some(root) => root,
        None => return CommandOutcome::failure(3, "canonical invocation cwd is not UTF-8"),
    };
    let Some(request) = request_value.as_object_mut() else {
        return CommandOutcome::failure(3, "invalid generic review v2 request");
    };
    request.insert(
        "workspace_admission_root".to_owned(),
        Value::String(root.to_owned()),
    );
    request.insert(
        "repository_admission_root".to_owned(),
        Value::String(root.to_owned()),
    );
    let request: GenericReviewRequestV2 = match serde_json::from_value(request_value) {
        Ok(request) => request,
        Err(_) => return CommandOutcome::failure(3, "invalid generic review v2 request"),
    };
    let run = match run_generic_review_v2_with_observer(&request, observer) {
        Ok(run) => run,
        Err(error) => return CommandOutcome::failure(20, error.to_string()),
    };
    if let Err(error) = admit_fresh_generic_review_artifact_root_v2(&artifact_root) {
        return CommandOutcome::failure(20, error.to_string());
    }
    observer.observe(GenericReviewStageEvent::Begin(GenericReviewStage::Report));
    let audit_bytes = match run.canonical_bytes() {
        Ok(bytes) => bytes,
        Err(error) => {
            observer.observe(GenericReviewStageEvent::Failed(GenericReviewStage::Report));
            return CommandOutcome::failure(20, error.to_string());
        }
    };
    let human_report = match reviewgraphen_report::generate_generic_human_report(&audit_bytes) {
        Ok(report) => report,
        Err(error) => {
            observer.observe(GenericReviewStageEvent::Failed(GenericReviewStage::Report));
            return CommandOutcome::failure(20, error.to_string());
        }
    };
    observer.observe(GenericReviewStageEvent::Completed(
        GenericReviewStage::Report,
    ));
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        "audit.run.v2.json".to_owned(),
        ("audit".to_owned(), audit_bytes.clone()),
    );
    artifacts.insert(
        "human-report.manifest.v1.json".to_owned(),
        (
            "human_report_manifest".to_owned(),
            human_report.manifest_bytes,
        ),
    );
    artifacts.insert(
        "human-report.md".to_owned(),
        (
            "human_report_markdown".to_owned(),
            human_report.markdown_bytes,
        ),
    );
    for record in run.provider_free_record_artifacts() {
        let packet_path = format!("records/{}", record.reviewer_packet_filename());
        let output_path = format!(
            "records/{}",
            record.deterministic_observer_output_filename()
        );
        let packet = match record.reviewer_packet.canonical_bytes() {
            Ok(bytes) => bytes,
            Err(error) => return CommandOutcome::failure(20, error.to_string()),
        };
        let output = match record.deterministic_observer_output.canonical_bytes() {
            Ok(bytes) => bytes,
            Err(error) => return CommandOutcome::failure(20, error.to_string()),
        };
        artifacts.insert(packet_path, ("reviewer_packet".to_owned(), packet));
        artifacts.insert(
            output_path,
            ("deterministic_observer_output".to_owned(), output),
        );
    }
    let manifest = artifact_manifest(
        request_bytes,
        &run.request_id.to_string(),
        &run.run_id.to_string(),
        &run.legacy_ingestion.snapshot_id.to_string(),
        &run.plan.universe_id.to_string(),
        &artifacts,
    );
    let manifest_bytes = match canonical_json(&manifest) {
        Ok(bytes) => bytes,
        Err(error) => return CommandOutcome::failure(20, error.to_string()),
    };
    observer.observe(GenericReviewStageEvent::Begin(
        GenericReviewStage::ArtifactWrite,
    ));
    if let Err(error) = write_artifacts(&artifact_root, &artifacts, &manifest_bytes) {
        observer.observe(GenericReviewStageEvent::Failed(
            GenericReviewStage::ArtifactWrite,
        ));
        return CommandOutcome::failure(20, error);
    }
    observer.observe(GenericReviewStageEvent::Completed(
        GenericReviewStage::ArtifactWrite,
    ));
    CommandOutcome::success(audit_bytes, String::new())
}

fn canonical_invocation_root() -> Result<PathBuf, &'static str> {
    let cwd = env::current_dir().map_err(|_| "unable to resolve invocation cwd")?;
    let metadata = stdfs::symlink_metadata(&cwd).map_err(|_| "unable to resolve invocation cwd")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("canonical invocation cwd must be a non-symlink directory");
    }
    cwd.canonicalize()
        .map_err(|_| "unable to resolve invocation cwd")
}

fn resolve_artifact_root(cwd: &Path, argument: &Path) -> Result<PathBuf, &'static str> {
    if argument.as_os_str().is_empty()
        || argument.is_absolute()
        || argument.components().any(|component| {
            matches!(
                component,
                Component::CurDir | Component::ParentDir | Component::RootDir
            )
        })
    {
        return Err("generic review artifact root path traversal");
    }
    Ok(cwd.join(argument))
}

fn require_absent_artifact_root(root: &Path) -> Result<(), &'static str> {
    match stdfs::symlink_metadata(root) {
        Ok(_) => Err("generic review artifact root already exists"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("unable to inspect generic review artifact root"),
    }
}

fn v2_request_is_valid(value: &Value) -> bool {
    let Ok(schema) = serde_json::from_str::<Value>(include_str!(
        "../../../schemas/reviewgraphen.generic_review_request.v2.schema.json"
    )) else {
        return false;
    };
    jsonschema::validator_for(&schema).is_ok_and(|validator| validator.is_valid(value))
}

fn v3_request_is_valid(value: &Value) -> bool {
    let Ok(schema) = serde_json::from_str::<Value>(include_str!(
        "../../../schemas/reviewgraphen.generic_review_request.v3.schema.json"
    )) else {
        return false;
    };
    jsonschema::validator_for(&schema).is_ok_and(|validator| validator.is_valid(value))
}

fn v4_request_is_valid(value: &Value) -> bool {
    let Ok(schema) = serde_json::from_str::<Value>(include_str!(
        "../../../schemas/reviewgraphen.generic_review_request.v4.schema.json"
    )) else {
        return false;
    };
    jsonschema::validator_for(&schema).is_ok_and(|validator| validator.is_valid(value))
}

fn uses_dot_admission_roots(value: &Value) -> bool {
    matches!(
        value
            .get("workspace_admission_root")
            .and_then(Value::as_str),
        Some(".")
    ) && matches!(
        value
            .get("repository_admission_root")
            .and_then(Value::as_str),
        Some(".")
    )
}

fn artifact_manifest(
    request_bytes: &[u8],
    request_id: &str,
    run_id: &str,
    snapshot_id: &str,
    universe_id: &str,
    artifacts: &BTreeMap<String, (String, Vec<u8>)>,
) -> Value {
    let artifact_rows = artifacts
        .iter()
        .map(|(path, (role, bytes))| {
            json!({
                "path": path,
                "role": role,
                "byte_length": bytes.len(),
                "sha256": ContentHash::sha256(bytes).to_string()
            })
        })
        .collect::<Vec<_>>();
    json!({
        "schema": "reviewgraphen.generic_review_artifact_manifest.v1",
        "request_sha256": ContentHash::sha256(request_bytes).to_string(),
        "request_id": request_id,
        "run_id": run_id,
        "snapshot_id": snapshot_id,
        "universe_id": universe_id,
        "artifacts": artifact_rows
    })
}

fn write_artifacts(
    root: &Path,
    artifacts: &BTreeMap<String, (String, Vec<u8>)>,
    manifest_bytes: &[u8],
) -> Result<(), &'static str> {
    let records = root.join("records");
    stdfs::create_dir(&records).map_err(|_| "unable to create generic review records")?;
    for (path, (_, bytes)) in artifacts {
        let destination = root.join(path);
        stdfs::write(destination, bytes).map_err(|_| "unable to write generic review artifact")?;
    }
    stdfs::write(root.join("artifact-manifest.v1.json"), manifest_bytes)
        .map_err(|_| "unable to write generic review artifact manifest")
}

fn schema_list() -> CommandOutcome {
    let values = json!([
        "reviewgraphen.review.report.v1",
        "reviewgraphen.review.report.v2",
        "reviewgraphen.review.report.v3",
        "reviewgraphen.review.report.v4",
        "reviewgraphen.review.report.v5",
        "reviewgraphen.generic_review_request.v1",
        "reviewgraphen.reviewer_output.v1",
        "reviewgraphen.reviewer_output.v2",
        "reviewgraphen.process_reviewer_record.v1",
        "reviewgraphen.generic_review_run.v1",
        "reviewgraphen.generic_review_request.v2",
        "reviewgraphen.generic_review_run.v2",
        "reviewgraphen.generic_review_human_report.v1",
        "reviewgraphen.generic_review_request.v3",
        "reviewgraphen.generic_review_run.v3",
        "reviewgraphen.generic_review_human_report.v2",
        "reviewgraphen.generic_review_request.v4",
        "reviewgraphen.generic_review_run.v4",
        "reviewgraphen.generic_review_human_report.v3",
        "reviewgraphen.generic_review_diagnostics.v1",
        "reviewgraphen.responsibility_family_state.v1"
    ]);
    CommandOutcome::success(canonical_json(&values).unwrap_or_default(), String::new())
}

fn schema_print(name: &str) -> CommandOutcome {
    match schema_source(name) {
        Some(source) => CommandOutcome::success(source.as_bytes().to_vec(), String::new()),
        None => CommandOutcome::failure(2, "unknown schema ID"),
    }
}

fn schema_validate(path: &Path) -> CommandOutcome {
    let bytes = match bounded_regular_file(path) {
        Ok(bytes) => bytes,
        Err(error) => return CommandOutcome::failure(3, error),
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return validation_failure("invalid_json"),
    };
    let schema_name = match value.get("schema").and_then(Value::as_str) {
        Some(name) => name,
        None => return validation_failure("missing_schema"),
    };
    if !semantic_validation_available(schema_name) {
        return validation_failure("unsupported_platform");
    }
    let validator = match validator_for(schema_name) {
        Ok(validator) => validator,
        Err(_) => return validation_failure("unsupported_schema"),
    };
    if validator.is_valid(&value) && semantic_validation(schema_name, &value).is_ok() {
        CommandOutcome::success(
            canonical_json(&json!({"schema":schema_name,"valid":true})).unwrap_or_default(),
            String::new(),
        )
    } else {
        validation_failure("schema_invalid")
    }
}

fn validation_failure(reason: &str) -> CommandOutcome {
    CommandOutcome {
        exit_code: 3,
        stdout: canonical_json(&json!({"valid":false,"reason":reason})).unwrap_or_default(),
        stderr: String::new(),
    }
}

fn validator_for(name: &str) -> Result<Validator, ()> {
    let source = schema_source(name).ok_or(())?;
    let schema: Value = serde_json::from_str(source).map_err(|_| ())?;
    let v3: Value =
        serde_json::from_str(schema_source("reviewgraphen.review.report.v3").ok_or(())?)
            .map_err(|_| ())?;
    let v4: Value =
        serde_json::from_str(schema_source("reviewgraphen.review.report.v4").ok_or(())?)
            .map_err(|_| ())?;
    jsonschema::options()
        .with_resource(V3_URI, Resource::from_contents(v3).map_err(|_| ())?)
        .with_resource(V4_URI, Resource::from_contents(v4).map_err(|_| ())?)
        .build(&schema)
        .map_err(|_| ())
}

fn semantic_validation(name: &str, report: &Value) -> Result<(), ()> {
    match name {
        "reviewgraphen.review.report.v4" => {
            #[cfg(target_os = "linux")]
            {
                reviewgraphen_report::validate_v4_semantics(report).map_err(|_| ())
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = report;
                Err(())
            }
        }
        "reviewgraphen.review.report.v5" => {
            #[cfg(target_os = "linux")]
            {
                reviewgraphen_report::validate_v5_semantics(report).map_err(|_| ())
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = report;
                Err(())
            }
        }
        "reviewgraphen.generic_review_run.v1" => {
            reviewgraphen_runtime::generic::validate_generic_review_run_semantics(report)
                .map_err(|_| ())
        }
        "reviewgraphen.generic_review_run.v2" => {
            reviewgraphen_runtime::generic::validate_generic_review_run_v2_semantics(report)
                .map_err(|_| ())
        }
        "reviewgraphen.generic_review_run.v3" => {
            reviewgraphen_runtime::generic::validate_generic_review_run_v3_wire_structure(report)
                .map_err(|_| ())
        }
        "reviewgraphen.generic_review_run.v4" => {
            reviewgraphen_runtime::generic::validate_generic_review_run_v4_wire_structure(report)
                .map_err(|_| ())
        }
        "reviewgraphen.responsibility_family_state.v1" => {
            let state: reviewgraphen_core::AcceptedResponsibilityFamilyStateV1 =
                serde_json::from_value(report.clone()).map_err(|_| ())?;
            state.validate().map_err(|_| ())
        }
        _ => Ok(()),
    }
}

fn semantic_validation_available(name: &str) -> bool {
    cfg!(target_os = "linux")
        || !matches!(
            name,
            "reviewgraphen.review.report.v4" | "reviewgraphen.review.report.v5"
        )
}

#[cfg(unix)]
fn bounded_regular_file(path: &Path) -> Result<Vec<u8>, &'static str> {
    // The descriptor is opened with NOFOLLOW, then checked after opening. This
    // binds the file type and byte limit to the object actually read, rather
    // than to an earlier pathname lookup.
    let descriptor = fs::open(
        path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| "unable to read input file")?;
    let metadata = fs::fstat(&descriptor).map_err(|_| "unable to read input file")?;
    if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile
        || metadata.st_size < 0
        || u64::try_from(metadata.st_size)
            .ok()
            .is_none_or(|size| size > MAX_INPUT_BYTES)
    {
        return Err("input must be a bounded regular file");
    }
    let mut file = std::fs::File::from(descriptor);
    let declared_size = usize::try_from(metadata.st_size).map_err(|_| "input is too large")?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(declared_size)
        .map_err(|_| "input is too large")?;
    file.by_ref()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => "unable to read input file",
            _ => "unable to read input file",
        })?;
    if bytes.len() > usize::try_from(MAX_INPUT_BYTES).unwrap_or(usize::MAX) {
        return Err("input must be a bounded regular file");
    }
    Ok(bytes)
}

#[cfg(not(unix))]
fn bounded_regular_file(_path: &Path) -> Result<Vec<u8>, &'static str> {
    // The documented CLI contract requires descriptor-safe path handling.
    Err("descriptor-safe file reads are unsupported on this platform")
}

fn schema_source(name: &str) -> Option<&'static str> {
    match name {
        "reviewgraphen.review.report.v1" => Some(include_str!(
            "../../../schemas/reviewgraphen.report.schema.json"
        )),
        "reviewgraphen.review.report.v2" => Some(include_str!(
            "../../../schemas/reviewgraphen.report.v2.schema.json"
        )),
        "reviewgraphen.review.report.v3" => Some(include_str!(
            "../../../schemas/reviewgraphen.report.v3.schema.json"
        )),
        "reviewgraphen.review.report.v4" => Some(include_str!(
            "../../../schemas/reviewgraphen.report.v4.schema.json"
        )),
        "reviewgraphen.review.report.v5" => Some(include_str!(
            "../../../schemas/reviewgraphen.report.v5.schema.json"
        )),
        "reviewgraphen.generic_review_request.v1" => Some(include_str!(
            "../../../schemas/reviewgraphen.generic_review_request.v1.schema.json"
        )),
        "reviewgraphen.reviewer_output.v1" => Some(include_str!(
            "../../../schemas/reviewgraphen.reviewer_output.v1.schema.json"
        )),
        "reviewgraphen.reviewer_output.v2" => Some(include_str!(
            "../../../schemas/reviewgraphen.reviewer_output.v2.schema.json"
        )),
        "reviewgraphen.process_reviewer_record.v1" => Some(include_str!(
            "../../../schemas/reviewgraphen.process_reviewer_record.v1.schema.json"
        )),
        "reviewgraphen.generic_review_run.v1" => Some(include_str!(
            "../../../schemas/reviewgraphen.generic_review_run.v1.schema.json"
        )),
        "reviewgraphen.generic_review_request.v2" => Some(include_str!(
            "../../../schemas/reviewgraphen.generic_review_request.v2.schema.json"
        )),
        "reviewgraphen.generic_review_run.v2" => Some(include_str!(
            "../../../schemas/reviewgraphen.generic_review_run.v2.schema.json"
        )),
        "reviewgraphen.generic_review_human_report.v1" => Some(include_str!(
            "../../../schemas/reviewgraphen.generic_review_human_report.v1.schema.json"
        )),
        "reviewgraphen.generic_review_request.v3" => Some(include_str!(
            "../../../schemas/reviewgraphen.generic_review_request.v3.schema.json"
        )),
        "reviewgraphen.generic_review_run.v3" => Some(include_str!(
            "../../../schemas/reviewgraphen.generic_review_run.v3.schema.json"
        )),
        "reviewgraphen.generic_review_human_report.v2" => Some(include_str!(
            "../../../schemas/reviewgraphen.generic_review_human_report.v2.schema.json"
        )),
        "reviewgraphen.generic_review_request.v4" => Some(include_str!(
            "../../../schemas/reviewgraphen.generic_review_request.v4.schema.json"
        )),
        "reviewgraphen.generic_review_run.v4" => Some(include_str!(
            "../../../schemas/reviewgraphen.generic_review_run.v4.schema.json"
        )),
        "reviewgraphen.generic_review_human_report.v3" => Some(include_str!(
            "../../../schemas/reviewgraphen.generic_review_human_report.v3.schema.json"
        )),
        "reviewgraphen.generic_review_diagnostics.v1" => Some(include_str!(
            "../../../schemas/reviewgraphen.generic_review_diagnostics.v1.schema.json"
        )),
        "reviewgraphen.responsibility_family_state.v1" => Some(include_str!(
            "../../../schemas/reviewgraphen.responsibility_family_state.v1.schema.json"
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn bounded_input_reader_accepts_regular_files_and_rejects_unsafe_types() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let regular = directory.path().join("input.json");
        std::fs::write(&regular, b"{}\n").unwrap();
        assert_eq!(bounded_regular_file(&regular).unwrap(), b"{}\n");

        let link = directory.path().join("input-link.json");
        symlink(&regular, &link).unwrap();
        assert_eq!(
            bounded_regular_file(&link),
            Err("unable to read input file")
        );
        assert_eq!(
            bounded_regular_file(directory.path()),
            Err("input must be a bounded regular file")
        );

        let oversized = directory.path().join("oversized.json");
        std::fs::File::create(&oversized)
            .unwrap()
            .set_len(MAX_INPUT_BYTES + 1)
            .unwrap();
        assert_eq!(
            bounded_regular_file(&oversized),
            Err("input must be a bounded regular file")
        );
    }

    #[test]
    fn durable_report_semantics_are_not_silently_skipped_off_linux() {
        assert_eq!(
            semantic_validation_available("reviewgraphen.review.report.v4"),
            cfg!(target_os = "linux")
        );
        assert_eq!(
            semantic_validation_available("reviewgraphen.review.report.v5"),
            cfg!(target_os = "linux")
        );
        assert!(semantic_validation_available(
            "reviewgraphen.generic_review_run.v4"
        ));
    }

    #[test]
    fn schema_surface_is_closed_and_canonical() {
        let list = run(vec!["schema".into(), "list".into()]);
        assert_eq!(list.exit_code, 0);
        assert_eq!(
            serde_json::from_slice::<Value>(&list.stdout).unwrap(),
            json!([
                "reviewgraphen.review.report.v1",
                "reviewgraphen.review.report.v2",
                "reviewgraphen.review.report.v3",
                "reviewgraphen.review.report.v4",
                "reviewgraphen.review.report.v5",
                "reviewgraphen.generic_review_request.v1",
                "reviewgraphen.reviewer_output.v1",
                "reviewgraphen.reviewer_output.v2",
                "reviewgraphen.process_reviewer_record.v1",
                "reviewgraphen.generic_review_run.v1",
                "reviewgraphen.generic_review_request.v2",
                "reviewgraphen.generic_review_run.v2",
                "reviewgraphen.generic_review_human_report.v1",
                "reviewgraphen.generic_review_request.v3",
                "reviewgraphen.generic_review_run.v3",
                "reviewgraphen.generic_review_human_report.v2",
                "reviewgraphen.generic_review_request.v4",
                "reviewgraphen.generic_review_run.v4",
                "reviewgraphen.generic_review_human_report.v3",
                "reviewgraphen.generic_review_diagnostics.v1",
                "reviewgraphen.responsibility_family_state.v1"
            ])
        );
        let printed = run(vec![
            "schema".into(),
            "print".into(),
            "reviewgraphen.review.report.v5".into(),
        ]);
        assert_eq!(printed.exit_code, 0);
        assert_eq!(
            serde_json::from_slice::<Value>(&printed.stdout)
                .unwrap()
                .get("$id")
                .and_then(Value::as_str),
            Some("https://capht.tech/schemas/reviewgraphen/review-report.v5.schema.json")
        );
        assert_eq!(run(vec!["review".into()]).exit_code, 2);

        let diagnostic_example: Value = serde_json::from_str(include_str!(
            "../../../schemas/reviewgraphen.generic_review_diagnostics.v1.example.json"
        ))
        .unwrap();
        assert!(
            validator_for("reviewgraphen.generic_review_diagnostics.v1")
                .unwrap()
                .is_valid(&diagnostic_example)
        );

        let family_example: Value = serde_json::from_str(include_str!(
            "../../../schemas/reviewgraphen.responsibility_family_state.v1.example.json"
        ))
        .unwrap();
        assert!(
            validator_for("reviewgraphen.responsibility_family_state.v1")
                .unwrap()
                .is_valid(&family_example)
        );
        assert!(
            semantic_validation(
                "reviewgraphen.responsibility_family_state.v1",
                &family_example
            )
            .is_ok()
        );
    }

    #[test]
    fn version_is_the_package_version_and_has_no_diagnostics_side_effect() {
        for flag in ["--version", "-V"] {
            let outcome = run_binary(vec![flag.to_owned()]);
            assert_eq!(outcome.exit_code, 0);
            assert_eq!(
                outcome.stdout,
                format!("reviewgraphen {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
            );
            assert!(outcome.stderr.is_empty());
        }
    }

    #[test]
    fn detached_report_gate_command_is_not_supported() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            br#"{"schema":"reviewgraphen.review.report.v5","gate":{"status":"pass"}}"#,
        )
        .unwrap();
        let result = run(vec!["gate".into(), file.path().display().to_string()]);
        assert_eq!(result.exit_code, 2);
        assert!(result.stdout.is_empty());
        assert!(!result.stderr.contains("gate <report.json>"));

        let nonexistent = run(vec![
            "gate".into(),
            "/path/that/must/not/be/read.json".into(),
        ]);
        assert_eq!(nonexistent.exit_code, 2);
        assert_eq!(nonexistent.stderr, result.stderr);
    }

    #[test]
    fn fixed_fixture_review_is_completely_rejected_before_input_access() {
        let baseline = run(vec!["review".into()]);
        assert_eq!(baseline.exit_code, 2);
        assert!(baseline.stdout.is_empty());
        assert!(!baseline.stderr.contains("fixture"));

        for arguments in [
            vec!["review".into(), "--fixture".into(), "double-submit".into()],
            vec!["review".into(), "--fixture".into(), "anything".into()],
            vec![
                "review".into(),
                "--fixture".into(),
                "/path/that/must/not/be/read".into(),
            ],
        ] {
            let rejected = run(arguments);
            assert_eq!(rejected.exit_code, 2);
            assert!(rejected.stdout.is_empty());
            assert_eq!(rejected.stderr, baseline.stderr);
        }
    }
}
