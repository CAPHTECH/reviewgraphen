//! Closed v2 call-enumeration facts used by the changed-public-callee slice.

use reviewgraphen_core::{MvpRulePack, canonical_json};
use reviewgraphen_ingest::{
    CallKind, CallObstructionReason, IngestRequest, LatentOccurrenceCount,
    decode_and_validate_ingestion_report_v2, ingest, ingest_v2, ingest_with_sources,
    ingest_with_sources_v2,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const IDENTITY: &str = "reviewgraphen.test/changed-public-callee-facts";
const FIXTURE_GIT_DATE: &str = "2000-01-01T00:00:00Z";
const LIVE_ORACLE_WORKSPACE: &str = "/tmp/reviewgraphen-c1d-live-oracle";

struct Repository {
    workspace_root: PathBuf,
    _workspace: Option<TempDir>,
    root: PathBuf,
    base: String,
    target: String,
}

impl Repository {
    fn request(&self) -> IngestRequest {
        IngestRequest::new(
            &self.workspace_root,
            &self.root,
            IDENTITY,
            &self.base,
            &self.target,
        )
    }
}

fn command<const N: usize>(root: &Path, arguments: [&str; N]) {
    let status = Command::new("git")
        .current_dir(root)
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join(".xdg-config"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", FIXTURE_GIT_DATE)
        .env("GIT_COMMITTER_DATE", FIXTURE_GIT_DATE)
        .args(arguments)
        .status()
        .expect("git launches");
    assert!(status.success(), "git command succeeds");
}

fn output<const N: usize>(root: &Path, arguments: [&str; N]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join(".xdg-config"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", FIXTURE_GIT_DATE)
        .env("GIT_COMMITTER_DATE", FIXTURE_GIT_DATE)
        .args(arguments)
        .output()
        .expect("git launches");
    assert!(output.status.success(), "git command succeeds");
    String::from_utf8(output.stdout)
        .expect("git output is UTF-8")
        .trim()
        .to_owned()
}

fn write(root: &Path, path: &str, bytes: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("path has a parent")).expect("parent exists");
    fs::write(path, bytes).expect("fixture source writes");
}

fn json_contains_string(value: &serde_json::Value, needle: &str) -> bool {
    match value {
        serde_json::Value::String(value) => value == needle,
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| json_contains_string(value, needle)),
        serde_json::Value::Object(values) => values
            .values()
            .any(|value| json_contains_string(value, needle)),
        _ => false,
    }
}

fn repository_with_source(source: &str) -> Repository {
    let workspace = tempfile::tempdir().expect("temporary workspace");
    repository_at(workspace.path().to_owned(), Some(workspace), source)
}

fn live_oracle_repository(source: &str) -> Repository {
    let workspace = PathBuf::from(LIVE_ORACLE_WORKSPACE);
    if workspace.exists() {
        fs::remove_dir_all(&workspace).expect("remove prior fixed oracle workspace");
    }
    repository_at(workspace, None, source)
}

fn repository_at(workspace_root: PathBuf, workspace: Option<TempDir>, source: &str) -> Repository {
    let root = workspace_root.join("fixture");
    fs::create_dir_all(&workspace_root).expect("workspace directory");
    fs::create_dir(&root).expect("repository directory");
    command(&root, ["init", "--quiet", "--object-format=sha1"]);
    command(
        &root,
        ["config", "user.email", "reviewgraphen@example.test"],
    );
    command(&root, ["config", "user.name", "ReviewGraphen test"]);
    write(&root, "src/lib.rs", "pub fn before() {}\n");
    command(&root, ["add", "."]);
    command(&root, ["commit", "--quiet", "-m", "base"]);
    let base = output(&root, ["rev-parse", "HEAD"]);
    write(&root, "src/lib.rs", source);
    command(&root, ["add", "."]);
    command(&root, ["commit", "--quiet", "-m", "unresolved calls"]);
    let target = output(&root, ["rev-parse", "HEAD"]);
    Repository {
        workspace_root,
        _workspace: workspace,
        root,
        base,
        target,
    }
}

fn repository() -> Repository {
    repository_with_source(
        "pub struct Widget;\n\
         impl Widget { pub fn worker(&self) {} }\n\
         fn target() {}\n\
         fn duplicate() {}\n\
         fn duplicate() {}\n\
         pub fn non_path() { (|| {})(); }\n\
         pub fn shadowed(target: impl Fn()) { target(); }\n\
         pub fn unresolved() { nowhere(); }\n\
         pub fn multiple() { duplicate(); }\n\
         pub fn cross_crate() { external::worker(); }\n\
         pub fn ambiguous_path() { Widget::worker(&Widget); }\n\
         pub fn glob_import() { use external::*; globbed(); }\n\
         pub fn method(receiver: &Widget) { receiver.worker(); }\n\
         pub fn macro_call() { unknown_macro!(); }\n",
    )
}

fn assert_closed_occurrence(
    result: &reviewgraphen_ingest::IngestResultV2,
    expected_kind: CallKind,
    expected_reason: CallObstructionReason,
) {
    let observed = result
        .ingestion_report_v2
        .located_call_occurrences()
        .iter()
        .map(|occurrence| {
            let span = occurrence.span();
            assert_eq!(span.path, "src/lib.rs");
            assert!(span.start_line >= 1 && span.end_line >= span.start_line);
            assert!(span.start_column >= 1 && span.end_column >= span.start_column);
            let serialized = serde_json::to_value(occurrence).expect("v2 record serializes");
            assert_eq!(
                serialized["schema"],
                "reviewgraphen.ingestion_obstruction.v2"
            );
            assert_eq!(
                serialized["related_capabilities"],
                serde_json::json!(["direct_calls"])
            );
            (occurrence.call_kind(), occurrence.reason())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        observed,
        vec![(expected_kind, expected_reason)],
        "fixture must retain exactly one closed occurrence of the expected typed kind"
    );
}

fn assert_occurrence_for_source(
    source: &str,
    expected_kind: CallKind,
    expected_reason: CallObstructionReason,
) {
    let repository = repository_with_source(source);
    let result = ingest_v2(&repository.request()).expect("v2 ingestion succeeds");
    assert_closed_occurrence(&result, expected_kind, expected_reason);
}

fn assert_exact_occurrence_span(
    source: &str,
    expected_kind: CallKind,
    expected_reason: CallObstructionReason,
    expected: (u64, u64, u64, u64),
) {
    let repository = repository_with_source(source);
    let result = ingest_v2(&repository.request()).expect("v2 ingestion succeeds");
    let obstruction = result
        .ingestion_report_v2
        .located_call_occurrences()
        .iter()
        .find(|obstruction| {
            obstruction.call_kind() == expected_kind && obstruction.reason() == expected_reason
        })
        .expect("typed occurrence exists");
    let span = obstruction.span();
    assert_eq!(
        (
            span.start_line,
            span.end_line,
            span.start_column,
            span.end_column,
        ),
        expected,
        "span uses exact one-based inclusive coordinates"
    );
}

#[test]
fn accepted_test_scope_covers_cfg_test_functions_modules_and_methods() {
    let repository = repository_with_source(
        "pub fn production() {}\n\
         pub struct Production;\n\
         impl Production { pub fn method(&self) {} }\n\
         #[cfg(test)] fn cfg_helper() {}\n\
         #[cfg(test)] impl Production { fn cfg_method(&self) {} }\n\
         #[cfg(test)] mod tests {\n\
             fn helper() {}\n\
             struct Fixture;\n\
             impl Fixture { fn method(&self) {} }\n\
         }\n",
    );
    let result = ingest_v2(&repository.request()).expect("v2 ingestion succeeds");
    let scopes = result
        .legacy
        .program_space
        .artifacts()
        .iter()
        .filter(|artifact| matches!(artifact.kind.as_str(), "function" | "method"))
        .map(|artifact| {
            (
                artifact.label.as_str(),
                artifact
                    .attributes
                    .get("test_scope")
                    .and_then(serde_json::Value::as_str),
                artifact
                    .attributes
                    .get("test_scope_extractor")
                    .and_then(serde_json::Value::as_str),
            )
        })
        .collect::<Vec<_>>();

    assert!(scopes.iter().any(|(name, scope, extractor)| {
        name.ends_with("::production")
            && *scope == Some("production")
            && *extractor == Some("reviewgraphen.ingest.rust-test-scope@1")
    }));
    assert!(scopes.iter().any(|(name, scope, _)| {
        name.ends_with("::method::Production") && *scope == Some("production")
    }));
    for expected in [
        "cfg_helper",
        "cfg_method::Production",
        "helper",
        "method::Fixture",
    ] {
        assert!(
            scopes
                .iter()
                .any(|(name, scope, _)| name.ends_with(expected) && *scope == Some("test")),
            "{expected} must be an accepted test-scope fact: {scopes:?}"
        );
    }
}

#[test]
fn unresolved_method_call_has_closed_direct_calls_occurrence() {
    assert_occurrence_for_source(
        "pub struct Widget; impl Widget { fn worker(&self) {} } \
         pub fn caller(widget: &Widget) { widget.worker(); }",
        CallKind::Method,
        CallObstructionReason::MethodDispatchUnresolved,
    );
}

#[test]
fn one_line_direct_call_span_uses_literal_inclusive_columns() {
    assert_exact_occurrence_span(
        "pub fn caller() { nowhere(); }",
        CallKind::Direct,
        CallObstructionReason::DirectTargetCountZero,
        (1, 1, 19, 27),
    );
}

#[test]
fn multi_line_direct_call_span_uses_literal_inclusive_columns() {
    assert_exact_occurrence_span(
        "pub fn caller() {\n    nowhere();\n}",
        CallKind::Direct,
        CallObstructionReason::DirectTargetCountZero,
        (2, 2, 5, 13),
    );
}

#[test]
fn non_path_call_span_uses_literal_inclusive_columns() {
    assert_exact_occurrence_span(
        "pub fn caller() { (|| {})(); }",
        CallKind::Direct,
        CallObstructionReason::DirectNonPath,
        (1, 1, 19, 27),
    );
}

#[test]
fn direct_empty_path_reason_remains_a_closed_v2_enum_value() {
    assert_eq!(
        serde_json::to_value(CallObstructionReason::DirectEmptyPath).expect("reason serializes"),
        serde_json::json!("direct_empty_path")
    );
}

#[test]
fn cross_crate_call_has_closed_direct_calls_occurrence() {
    assert_occurrence_for_source(
        "pub fn caller() { external::worker(); }",
        CallKind::Direct,
        CallObstructionReason::DirectTargetCountZero,
    );
}

#[test]
fn macro_invocation_has_closed_direct_calls_occurrence() {
    assert_occurrence_for_source(
        "pub fn caller() { unknown_macro!(); }",
        CallKind::MacroInvocation,
        CallObstructionReason::MacroExpansionUnresolved,
    );
}

#[test]
fn ambiguous_path_with_same_named_targets_has_closed_direct_calls_occurrence() {
    assert_occurrence_for_source(
        "fn duplicate() {} fn duplicate() {} pub fn caller() { crate::duplicate(); }",
        CallKind::Direct,
        CallObstructionReason::DirectTargetCountMultiple,
    );
}

#[test]
fn glob_import_call_has_closed_direct_calls_occurrence() {
    assert_occurrence_for_source(
        "pub fn caller() { use external::*; globbed(); }",
        CallKind::Direct,
        CallObstructionReason::DirectUnresolvedScope,
    );
}

#[test]
fn shadowed_call_has_closed_direct_calls_occurrence() {
    assert_occurrence_for_source(
        "fn target() {} pub fn caller(target: impl Fn()) { target(); }",
        CallKind::Direct,
        CallObstructionReason::DirectShadowedBinding,
    );
}

#[test]
fn zero_target_call_has_closed_direct_calls_occurrence() {
    assert_occurrence_for_source(
        "pub fn caller() { nowhere(); }",
        CallKind::Direct,
        CallObstructionReason::DirectTargetCountZero,
    );
}

#[test]
fn multiple_target_call_has_closed_direct_calls_occurrence() {
    assert_occurrence_for_source(
        "fn duplicate() {} fn duplicate() {} pub fn caller() { duplicate(); }",
        CallKind::Direct,
        CallObstructionReason::DirectTargetCountMultiple,
    );
}

#[test]
fn direct_call_candidate_space_retains_a_global_unknown_macro_cardinality_limitation() {
    let repository = repository();
    let result = ingest_v2(&repository.request()).expect("v2 ingestion succeeds");
    assert_eq!(
        result
            .ingestion_report_v2
            .global_direct_calls_limitation()
            .latent_occurrence_count(),
        LatentOccurrenceCount::Unknown
    );
}

#[test]
fn direct_call_occurrences_do_not_mix_other_capabilities() {
    let repository = repository();
    let result = ingest_v2(&repository.request()).expect("v2 ingestion succeeds");
    let serialized = serde_json::to_value(&result.ingestion_report_v2).expect("sidecar serializes");
    assert_eq!(
        serialized["global_direct_calls_limitation"]["related_capabilities"],
        serde_json::json!(["direct_calls"])
    );
}

#[test]
fn rust_public_attribute_remains_exact_visibility_public_syntax() {
    let repository = repository_with_source(
        "pub fn externally_public() {}\n\
         pub(crate) fn crate_visible() {}\n\
         mod nested {\n\
             pub(super) fn parent_visible() {}\n\
             pub(in crate) fn restricted_visible() {}\n\
         }\n",
    );
    let result = ingest(&repository.request()).expect("ingestion succeeds");
    for (label, expected) in [
        ("crate::externally_public", true),
        ("crate::crate_visible", false),
        ("crate::nested::parent_visible", false),
        ("crate::nested::restricted_visible", false),
    ] {
        let artifact = result
            .program_space
            .artifacts()
            .iter()
            .find(|artifact| artifact.label == label)
            .unwrap_or_else(|| panic!("{label} artifact exists"));
        let value = serde_json::to_value(artifact).expect("artifact serializes");
        assert_eq!(value["attributes"]["public"], expected, "{label}");
    }
}

#[test]
fn direct_calls_capability_remains_partial() {
    let repository = repository_with_source("pub fn resolved() {} pub fn caller() { resolved(); }");
    let result = ingest(&repository.request()).expect("ingestion succeeds");
    assert_eq!(
        result.extraction_report.capabilities["direct_calls"],
        reviewgraphen_ingest::CapabilityState::Partial
    );
}

#[test]
fn accepted_calls_remain_single_target_syntactic_unique_relations() {
    let repository = repository_with_source(
        "fn callee() {} pub fn caller() { callee(); } pub fn external() { ext::callee(); }",
    );
    let result = ingest(&repository.request()).expect("ingestion succeeds");
    let calls = result
        .program_space
        .relations()
        .iter()
        .filter(|relation| relation.kind == "calls")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 1, "only the local unique target is accepted");
    let relation = serde_json::to_value(calls[0]).expect("relation serializes");
    assert_eq!(relation["attributes"]["resolution"], "syntactic_unique");
    assert_eq!(relation["target_ids"].as_array().unwrap().len(), 1);
}

#[test]
fn repeated_ingestion_retains_identical_occurrence_ids_and_canonical_bytes() {
    let repository = repository();
    let first = ingest_v2(&repository.request()).expect("first v2 ingestion succeeds");
    let second = ingest_v2(&repository.request()).expect("second v2 ingestion succeeds");
    assert_eq!(
        first
            .ingestion_report_v2
            .canonical_bytes()
            .expect("first sidecar bytes"),
        second
            .ingestion_report_v2
            .canonical_bytes()
            .expect("second sidecar bytes"),
        "same input retains identical obstruction IDs and canonical bytes"
    );
    let occurrence_ids = |result: &reviewgraphen_ingest::IngestResultV2| {
        result
            .ingestion_report_v2
            .located_call_occurrences()
            .iter()
            .map(|occurrence| occurrence.id().clone())
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(occurrence_ids(&first), occurrence_ids(&second));
}

#[test]
fn v2_sidecar_isolated_from_live_legacy_ingest_and_rejects_semantic_mutations() {
    let repository = live_oracle_repository(
        "fn resolved() {}\n\
         pub struct Widget; impl Widget { fn worker(&self) {} }\n\
         pub fn caller(widget: &Widget) { resolved(); nowhere(); widget.worker(); unknown_macro!(); }\n",
    );
    let request = repository.request();
    let legacy = ingest(&request).expect("legacy ingest succeeds");
    let legacy_sources =
        ingest_with_sources(&request, u64::MAX).expect("legacy source ingest succeeds");
    let v2 = ingest_v2(&request).expect("v2 ingest succeeds");
    let v2_sources = ingest_with_sources_v2(&request, u64::MAX).expect("v2 source ingest succeeds");

    let legacy_bytes = legacy.canonical_output().expect("legacy canonical bytes");
    let legacy_source_bytes = reviewgraphen_ingest::IngestResult {
        program_space: legacy_sources.program_space.clone(),
        extraction_report: legacy_sources.extraction_report.clone(),
    }
    .canonical_output()
    .expect("legacy source canonical bytes");
    let v2_legacy_bytes = v2.legacy.canonical_output().expect("v2 legacy bytes");
    let v2_source_legacy_bytes = reviewgraphen_ingest::IngestResult {
        program_space: v2_sources.legacy.program_space.clone(),
        extraction_report: v2_sources.legacy.extraction_report.clone(),
    }
    .canonical_output()
    .expect("v2 source legacy bytes");
    assert_eq!(legacy_bytes, legacy_source_bytes);
    assert_eq!(legacy_bytes, v2_legacy_bytes);
    assert_eq!(legacy_bytes, v2_source_legacy_bytes);

    let synthesize = |space: &reviewgraphen_core::ProgramSpace| {
        canonical_json(&MvpRulePack::synthesize(space).expect("five legacy rules synthesize"))
            .expect("legacy synthesis canonicalizes")
    };
    let synthesized = [
        synthesize(&legacy.program_space),
        synthesize(&v2.legacy.program_space),
        synthesize(&legacy_sources.program_space),
        synthesize(&v2_sources.legacy.program_space),
    ];
    for rules in &synthesized[1..] {
        assert_eq!(rules, &synthesized[0]);
    }

    let report = &v2.ingestion_report_v2;
    let sidecar_ids = report
        .located_call_occurrences()
        .iter()
        .map(|item| item.id().clone())
        .chain(std::iter::once(
            report.global_direct_calls_limitation().id().clone(),
        ))
        .collect::<BTreeSet<_>>();
    assert!(
        sidecar_ids.is_disjoint(
            &legacy
                .program_space
                .extraction()
                .limitations
                .iter()
                .map(|item| item.id.clone())
                .collect()
        )
    );
    let legacy_synthesis = canonical_json(
        &MvpRulePack::synthesize(&legacy.program_space).expect("legacy rules synthesize"),
    )
    .expect("legacy rules canonicalize");
    let legacy_synthesis: serde_json::Value =
        serde_json::from_slice(&legacy_synthesis).expect("legacy rules JSON");
    for sidecar_id in &sidecar_ids {
        assert!(
            !json_contains_string(&legacy_synthesis, &sidecar_id.to_string()),
            "a v2 sidecar ID must not enter any v1 universe or existing obligation source"
        );
    }
    assert!(
        sidecar_ids.is_disjoint(
            &legacy
                .extraction_report
                .obstructions
                .iter()
                .map(|item| item.id.clone())
                .collect()
        )
    );
    assert_eq!(
        report
            .global_direct_calls_limitation()
            .latent_occurrence_count(),
        LatentOccurrenceCount::Unknown
    );
    assert!(!report.located_call_occurrences().is_empty());

    let bytes = report.canonical_bytes().expect("sidecar canonical bytes");
    assert!(
        decode_and_validate_ingestion_report_v2(
            &bytes,
            &legacy.program_space,
            &legacy.extraction_report
        )
        .is_ok()
    );
    let reject = |mutate: fn(&mut serde_json::Value)| {
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("sidecar JSON");
        mutate(&mut value);
        let bytes = canonical_json(&value).expect("canonical mutation");
        assert!(
            decode_and_validate_ingestion_report_v2(
                &bytes,
                &legacy.program_space,
                &legacy.extraction_report
            )
            .is_err()
        );
    };
    reject(|value| value["schema"] = serde_json::json!("reviewgraphen.ingestion_report.v999"));
    reject(|value| {
        value["located_call_occurrences"][0]["reason"] = serde_json::json!("direct_non_path")
    });
    reject(|value| {
        value["located_call_occurrences"][0]["description"] = serde_json::json!("mutated")
    });
    reject(|value| value["located_call_occurrences"][0]["span"]["extra"] = serde_json::json!(true));
    reject(|value| value["global_direct_calls_limitation"]["extra"] = serde_json::json!(true));
    reject(|value| {
        value["global_direct_calls_limitation"]["kind"] = serde_json::json!("relation_unresolved")
    });
    reject(|value| {
        let global = value["global_direct_calls_limitation"].clone();
        value["located_call_occurrences"]
            .as_array_mut()
            .expect("occurrences are an array")
            .push(global);
    });
    reject(|value| {
        value["global_direct_calls_limitation"] = value["located_call_occurrences"][0].clone();
    });
    reject(|value| {
        value["located_call_occurrences"][0]["latent_occurrence_count"] =
            serde_json::json!("unknown");
    });
    reject(|value| {
        value["global_direct_calls_limitation"]["span"] =
            value["located_call_occurrences"][0]["span"].clone();
    });
}
