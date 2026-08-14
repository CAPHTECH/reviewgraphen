//! Closed CLI surface for schema inspection and generic review orchestration.
//!
//! Generic review is not exposed until its input, isolation, replay, and
//! non-authority contracts are implemented. Fixed-fixture review is never a
//! product command.

use jsonschema::{Resource, Validator};
use reviewgraphen_core::canonical_json;
#[cfg(target_os = "linux")]
use rustix::fs::{self, FileType, Mode, OFlags};
use serde_json::{Value, json};
use std::{
    io::{self, Read},
    path::Path,
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

fn usage() -> &'static str {
    "usage: reviewgraphen schema list|print <schema-id>|validate <report.json>"
}

fn schema_list() -> CommandOutcome {
    let values = json!([
        "reviewgraphen.review.report.v1",
        "reviewgraphen.review.report.v2",
        "reviewgraphen.review.report.v3",
        "reviewgraphen.review.report.v4",
        "reviewgraphen.review.report.v5"
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
            reviewgraphen_report::validate_v4_semantics(report).map_err(|_| ())
        }
        "reviewgraphen.review.report.v5" => {
            reviewgraphen_report::validate_v5_semantics(report).map_err(|_| ())
        }
        _ => Ok(()),
    }
}

#[cfg(target_os = "linux")]
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

#[cfg(not(target_os = "linux"))]
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
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                "reviewgraphen.review.report.v5"
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
