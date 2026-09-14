//! Exact-body candidate discovery from accepted Rust symbol anchors.

use super::AuthorityBoundary;
use reviewgraphen_core::{
    ContentHash, ProgramSpace, RUST_SYMBOL_ANCHOR_SYN_VERSION_V1, RustSymbolKindV1, StableId,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const EXACT_BODY_REPORT_SCHEMA: &str = "reviewgraphen.benchmark.exact_body_candidate_report.v2";
pub const EXACT_BODY_EXTRACTOR_ID: &str = "reviewgraphen.benchmark.accepted-rust-exact-body@2";
pub const NEAR_BODY_REPORT_SCHEMA: &str = "reviewgraphen.benchmark.near_body_candidate_report.v1";
pub const NEAR_BODY_EXTRACTOR_ID: &str = "reviewgraphen.benchmark.accepted-rust-near-body@1";
pub const RESPONSIBILITY_SIGNAL_REPORT_SCHEMA: &str =
    "reviewgraphen.benchmark.responsibility_signal_candidate_report.v1";
pub const RESPONSIBILITY_SIGNAL_PAIR_EXTRACTOR_ID: &str =
    "reviewgraphen.benchmark.rust-responsibility-signal-pairs@1";
const TEST_SCOPE_EXTRACTOR_V1: &str = "reviewgraphen.ingest.rust-test-scope@1";
const RESPONSIBILITY_SHAPE_EXTRACTOR_V1: &str = "reviewgraphen.ingest.rust-responsibility-shape@1";
const RESPONSIBILITY_SIGNALS_EXTRACTOR_V1: &str =
    "reviewgraphen.ingest.rust-responsibility-signals@1";
const MINIMUM_OPERATION_JACCARD_PPM: u64 = 600_000;
const JACCARD_SCALE_PPM: u64 = 1_000_000;

pub type ExactSymbolKind = RustSymbolKindV1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    CandidateOnly,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExactDiscoveryExtractor {
    pub id: &'static str,
    pub rust_anchor_syn_version: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExactDiscoveryProfile {
    pub included_path: &'static str,
    pub included_symbol_kinds: [&'static str; 2],
    pub test_exclusion_fact: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExactDiscoveryDenominator {
    pub accepted_rust_symbols: u64,
    pub eligible_functions: u64,
    pub excluded_non_function_symbols: u64,
    pub excluded_profile_paths: u64,
    pub excluded_test_functions: u64,
    pub excluded_unknown_test_scope: u64,
    pub candidate_groups: u64,
    pub candidate_members: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExactCandidateMember {
    pub artifact_id: StableId,
    pub path: String,
    pub symbol: String,
    pub start_line: u64,
    pub end_line: u64,
    pub symbol_kind: ExactSymbolKind,
    pub signature_shape_hash: ContentHash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExactBodyCandidate {
    pub candidate_id: StableId,
    pub normalized_body_hash: ContentHash,
    pub minimum_member_span_lines: u64,
    pub status: CandidateStatus,
    pub members: Vec<ExactCandidateMember>,
    pub unknowns: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExactBodyCandidateReport {
    pub schema: &'static str,
    pub snapshot_id: StableId,
    pub extractor: ExactDiscoveryExtractor,
    pub profile: ExactDiscoveryProfile,
    pub authority: AuthorityBoundary,
    pub denominator: ExactDiscoveryDenominator,
    pub candidates: Vec<ExactBodyCandidate>,
    pub information_loss: Vec<String>,
    pub unknowns: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NearDiscoveryDenominator {
    pub accepted_rust_symbols: u64,
    pub eligible_functions: u64,
    pub excluded_non_function_symbols: u64,
    pub excluded_profile_paths: u64,
    pub excluded_test_functions: u64,
    pub excluded_unknown_test_scope: u64,
    pub excluded_missing_shape_fact: u64,
    pub excluded_exact_only_groups: u64,
    pub candidate_groups: u64,
    pub candidate_members: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NearCandidateMember {
    pub artifact_id: StableId,
    pub path: String,
    pub symbol: String,
    pub start_line: u64,
    pub end_line: u64,
    pub symbol_kind: ExactSymbolKind,
    pub signature_shape_hash: ContentHash,
    pub normalized_body_hash: ContentHash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NearBodyCandidate {
    pub candidate_id: StableId,
    pub responsibility_shape_hash: ContentHash,
    pub distinct_exact_body_hashes: u64,
    pub minimum_member_span_lines: u64,
    pub status: CandidateStatus,
    pub members: Vec<NearCandidateMember>,
    pub unknowns: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NearBodyCandidateReport {
    pub schema: &'static str,
    pub snapshot_id: StableId,
    pub extractor: ExactDiscoveryExtractor,
    pub profile: ExactDiscoveryProfile,
    pub authority: AuthorityBoundary,
    pub denominator: NearDiscoveryDenominator,
    pub candidates: Vec<NearBodyCandidate>,
    pub information_loss: Vec<String>,
    pub unknowns: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ResponsibilitySignals {
    pub callable: Vec<String>,
    pub signature: Vec<String>,
    pub operation: Vec<String>,
}

#[derive(Clone, Debug)]
struct ResponsibilitySignalFact {
    artifact_id: StableId,
    path: String,
    symbol: String,
    start_line: u64,
    end_line: u64,
    symbol_kind: ExactSymbolKind,
    signature_shape_hash: ContentHash,
    normalized_body_hash: ContentHash,
    test_scope: Option<TestScopeFact>,
    responsibility_shape_hash: Option<ContentHash>,
    responsibility_signals_extractor: Option<String>,
    responsibility_signals: Option<ResponsibilitySignals>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResponsibilitySignalSelection {
    pub signal_fact_extractor: &'static str,
    pub minimum_callable_or_signature_terms: u64,
    pub minimum_operation_terms: u64,
    pub minimum_operation_jaccard_ppm: u64,
    pub max_selective_term_frequency: u64,
    pub frequency_denominator: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResponsibilitySignalDenominator {
    pub accepted_rust_symbols: u64,
    pub eligible_functions: u64,
    pub excluded_non_function_symbols: u64,
    pub excluded_profile_paths: u64,
    pub excluded_test_functions: u64,
    pub excluded_unknown_test_scope: u64,
    pub excluded_missing_shape_fact: u64,
    pub excluded_unknown_signal_fact: u64,
    pub excluded_same_shape_pairs: u64,
    pub evaluated_distinct_shape_pairs: u64,
    pub candidate_pairs: u64,
    pub candidate_members: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResponsibilitySignalCandidateMember {
    pub artifact_id: StableId,
    pub path: String,
    pub symbol: String,
    pub start_line: u64,
    pub end_line: u64,
    pub symbol_kind: ExactSymbolKind,
    pub signature_shape_hash: ContentHash,
    pub normalized_body_hash: ContentHash,
    pub responsibility_shape_hash: ContentHash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResponsibilitySignalCandidate {
    pub candidate_id: StableId,
    pub rank: u64,
    pub status: CandidateStatus,
    pub matched_callable: Vec<String>,
    pub matched_signature: Vec<String>,
    pub matched_operation: Vec<String>,
    pub operation_jaccard_ppm: u64,
    pub members: Vec<ResponsibilitySignalCandidateMember>,
    pub unknowns: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResponsibilitySignalCandidateReport {
    pub schema: &'static str,
    pub snapshot_id: StableId,
    pub extractor: ExactDiscoveryExtractor,
    pub profile: ExactDiscoveryProfile,
    pub authority: AuthorityBoundary,
    pub selection: ResponsibilitySignalSelection,
    pub denominator: ResponsibilitySignalDenominator,
    pub ignored_high_frequency_terms: ResponsibilitySignals,
    pub candidates: Vec<ResponsibilitySignalCandidate>,
    pub information_loss: Vec<String>,
    pub unknowns: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TestScopeFact {
    Production,
    Test,
}

#[derive(Clone, Debug)]
struct ExactFunctionFact {
    artifact_id: StableId,
    path: String,
    symbol: String,
    start_line: u64,
    end_line: u64,
    symbol_kind: ExactSymbolKind,
    signature_shape_hash: ContentHash,
    normalized_body_hash: ContentHash,
    test_scope: Option<TestScopeFact>,
    responsibility_shape_hash: Option<ContentHash>,
}

#[derive(Debug, Error)]
pub enum ExactDiscoveryError {
    #[error("accepted Rust symbol anchors are unavailable")]
    MissingAcceptedAnchors,
    #[error("invalid exact-body discovery fact closure: {0}")]
    InvalidFacts(&'static str),
    #[error("exact-body discovery count overflow")]
    CountOverflow,
    #[error("exact-body candidate identity failed: {0}")]
    Identity(#[from] reviewgraphen_core::DomainError),
}

pub type Result<T> = std::result::Result<T, ExactDiscoveryError>;

/// Enumerate candidate groups from already accepted, snapshot-bound anchors.
pub fn discover_exact_bodies(program: &ProgramSpace) -> Result<ExactBodyCandidateReport> {
    let anchors = program
        .accepted_rust_symbol_anchors()
        .ok_or(ExactDiscoveryError::MissingAcceptedAnchors)?;
    let artifacts = program
        .artifacts()
        .iter()
        .map(|artifact| (&artifact.id, artifact))
        .collect::<BTreeMap<_, _>>();
    let mut facts = Vec::new();
    for (artifact_id, anchor) in anchors {
        if !matches!(
            anchor.symbol_kind(),
            RustSymbolKindV1::Function | RustSymbolKindV1::Method
        ) {
            continue;
        }
        let artifact = artifacts
            .get(artifact_id)
            .ok_or(ExactDiscoveryError::InvalidFacts(
                "accepted anchor has no matching artifact",
            ))?;
        let location = artifact
            .location
            .as_ref()
            .ok_or(ExactDiscoveryError::InvalidFacts(
                "accepted function anchor has no location",
            ))?;
        let test_scope = match (
            artifact
                .attributes
                .get("test_scope")
                .and_then(Value::as_str),
            artifact
                .attributes
                .get("test_scope_extractor")
                .and_then(Value::as_str),
        ) {
            (Some("production"), Some(TEST_SCOPE_EXTRACTOR_V1)) => Some(TestScopeFact::Production),
            (Some("test"), Some(TEST_SCOPE_EXTRACTOR_V1)) => Some(TestScopeFact::Test),
            _ => None,
        };
        let responsibility_shape_hash = match (
            artifact
                .attributes
                .get("responsibility_shape_hash")
                .and_then(Value::as_str),
            artifact
                .attributes
                .get("responsibility_shape_extractor")
                .and_then(Value::as_str),
        ) {
            (Some(hash), Some(RESPONSIBILITY_SHAPE_EXTRACTOR_V1)) => {
                Some(ContentHash::parse(hash).map_err(|_| {
                    ExactDiscoveryError::InvalidFacts("invalid responsibility shape hash")
                })?)
            }
            _ => None,
        };
        facts.push(ExactFunctionFact {
            artifact_id: artifact_id.clone(),
            path: location.path.clone(),
            symbol: artifact.label.clone(),
            start_line: location
                .start_line
                .ok_or(ExactDiscoveryError::InvalidFacts(
                    "function range is absent",
                ))?,
            end_line: location.end_line.ok_or(ExactDiscoveryError::InvalidFacts(
                "function range is absent",
            ))?,
            symbol_kind: anchor.symbol_kind(),
            signature_shape_hash: anchor.signature_shape_hash().clone(),
            normalized_body_hash: anchor.normalized_body_hash().clone(),
            test_scope,
            responsibility_shape_hash,
        });
    }
    discover_facts(
        program.snapshot_id().clone(),
        RUST_SYMBOL_ANCHOR_SYN_VERSION_V1.to_owned(),
        facts,
        anchors.len(),
    )
}

fn discover_facts(
    snapshot_id: StableId,
    rust_anchor_syn_version: String,
    mut facts: Vec<ExactFunctionFact>,
    accepted_rust_symbols: usize,
) -> Result<ExactBodyCandidateReport> {
    if snapshot_id.kind() != "snapshot"
        || rust_anchor_syn_version.is_empty()
        || accepted_rust_symbols < facts.len()
    {
        return Err(ExactDiscoveryError::InvalidFacts("invalid report input"));
    }
    facts.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    if facts
        .windows(2)
        .any(|pair| pair[0].artifact_id == pair[1].artifact_id)
    {
        return Err(ExactDiscoveryError::InvalidFacts(
            "duplicate function artifact",
        ));
    }

    let excluded_non_function_symbols = accepted_rust_symbols - facts.len();
    let mut excluded_profile_paths = 0_usize;
    let mut excluded_test_functions = 0_usize;
    let mut excluded_unknown_test_scope = 0_usize;
    let mut eligible = Vec::new();
    for fact in facts {
        if !production_rust_path(&fact.path) {
            excluded_profile_paths += 1;
        } else if fact.test_scope == Some(TestScopeFact::Test) {
            excluded_test_functions += 1;
        } else if fact.test_scope.is_none() {
            excluded_unknown_test_scope += 1;
        } else if fact.start_line == 0 || fact.end_line < fact.start_line {
            return Err(ExactDiscoveryError::InvalidFacts(
                "invalid function source range",
            ));
        } else {
            eligible.push(fact);
        }
    }

    let mut by_body = BTreeMap::<ContentHash, Vec<ExactFunctionFact>>::new();
    for fact in eligible.iter().cloned() {
        by_body
            .entry(fact.normalized_body_hash.clone())
            .or_default()
            .push(fact);
    }
    let mut candidates = Vec::new();
    for (body_hash, group) in by_body {
        if group.len() < 2 {
            continue;
        }
        let candidate_id = StableId::derived(
            "responsibility-family-candidate-signal",
            &BTreeMap::from([
                (
                    "extractor".to_owned(),
                    Value::String(EXACT_BODY_EXTRACTOR_ID.to_owned()),
                ),
                (
                    "normalized_body_hash".to_owned(),
                    Value::String(body_hash.to_string()),
                ),
                (
                    "snapshot_id".to_owned(),
                    Value::String(snapshot_id.to_string()),
                ),
            ]),
        )?;
        candidates.push(ExactBodyCandidate {
            candidate_id,
            normalized_body_hash: body_hash,
            minimum_member_span_lines: group
                .iter()
                .map(|fact| fact.end_line - fact.start_line + 1)
                .min()
                .expect("candidate groups contain at least two members"),
            status: CandidateStatus::CandidateOnly,
            members: group
                .into_iter()
                .map(|fact| ExactCandidateMember {
                    artifact_id: fact.artifact_id,
                    path: fact.path,
                    symbol: fact.symbol,
                    start_line: fact.start_line,
                    end_line: fact.end_line,
                    symbol_kind: fact.symbol_kind,
                    signature_shape_hash: fact.signature_shape_hash,
                })
                .collect(),
            unknowns: vec![
                "body equality does not establish a common contract".to_owned(),
                "signature compatibility is not established".to_owned(),
            ],
        });
    }
    candidates.sort_by(|left, right| {
        right
            .minimum_member_span_lines
            .cmp(&left.minimum_member_span_lines)
            .then_with(|| right.members.len().cmp(&left.members.len()))
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
    });
    let candidate_members = candidates.iter().map(|group| group.members.len()).sum();
    Ok(ExactBodyCandidateReport {
        schema: EXACT_BODY_REPORT_SCHEMA,
        snapshot_id,
        extractor: ExactDiscoveryExtractor {
            id: EXACT_BODY_EXTRACTOR_ID,
            rust_anchor_syn_version,
        },
        profile: ExactDiscoveryProfile {
            included_path: "crates/*/src/**/*.rs | rust/*/src/**/*.rs",
            included_symbol_kinds: ["function", "method"],
            test_exclusion_fact: "artifact.attributes.test_scope=test@reviewgraphen.ingest.rust-test-scope@1",
        },
        authority: AuthorityBoundary::non_authority(),
        denominator: ExactDiscoveryDenominator {
            accepted_rust_symbols: count(accepted_rust_symbols)?,
            eligible_functions: count(eligible.len())?,
            excluded_non_function_symbols: count(excluded_non_function_symbols)?,
            excluded_profile_paths: count(excluded_profile_paths)?,
            excluded_test_functions: count(excluded_test_functions)?,
            excluded_unknown_test_scope: count(excluded_unknown_test_scope)?,
            candidate_groups: count(candidates.len())?,
            candidate_members: count(candidate_members)?,
        },
        candidates,
        information_loss: vec![
            "function names and signature differences are not used for grouping".to_owned(),
            "only exact normalized body hashes are grouped; near clones are omitted".to_owned(),
            "test functions and paths outside the declared production profile are excluded"
                .to_owned(),
            "symbols without an accepted test-scope fact are excluded".to_owned(),
            "only accepted test-scope facts are used; missing or unsupported facts are excluded"
                .to_owned(),
        ],
        unknowns: vec![
            "macro-expanded functions are not observed".to_owned(),
            "responsibility and common change reason require decision-stage evidence".to_owned(),
            "conditional compilation other than cfg(test) is not evaluated".to_owned(),
        ],
    })
}

/// Enumerate Type-2-like candidates from accepted responsibility-shape facts.
/// A group containing only one exact body hash is omitted because exact
/// discovery already owns that candidate signal.
pub fn discover_near_bodies(program: &ProgramSpace) -> Result<NearBodyCandidateReport> {
    let anchors = program
        .accepted_rust_symbol_anchors()
        .ok_or(ExactDiscoveryError::MissingAcceptedAnchors)?;
    let artifacts = program
        .artifacts()
        .iter()
        .map(|artifact| (&artifact.id, artifact))
        .collect::<BTreeMap<_, _>>();
    let mut facts = Vec::new();
    for (artifact_id, anchor) in anchors {
        if !matches!(
            anchor.symbol_kind(),
            RustSymbolKindV1::Function | RustSymbolKindV1::Method
        ) {
            continue;
        }
        let artifact = artifacts
            .get(artifact_id)
            .ok_or(ExactDiscoveryError::InvalidFacts(
                "accepted anchor has no matching artifact",
            ))?;
        let location = artifact
            .location
            .as_ref()
            .ok_or(ExactDiscoveryError::InvalidFacts(
                "accepted function anchor has no location",
            ))?;
        let test_scope = match (
            artifact
                .attributes
                .get("test_scope")
                .and_then(Value::as_str),
            artifact
                .attributes
                .get("test_scope_extractor")
                .and_then(Value::as_str),
        ) {
            (Some("production"), Some(TEST_SCOPE_EXTRACTOR_V1)) => Some(TestScopeFact::Production),
            (Some("test"), Some(TEST_SCOPE_EXTRACTOR_V1)) => Some(TestScopeFact::Test),
            _ => None,
        };
        let responsibility_shape_hash = match (
            artifact
                .attributes
                .get("responsibility_shape_hash")
                .and_then(Value::as_str),
            artifact
                .attributes
                .get("responsibility_shape_extractor")
                .and_then(Value::as_str),
        ) {
            (Some(hash), Some(RESPONSIBILITY_SHAPE_EXTRACTOR_V1)) => {
                Some(ContentHash::parse(hash).map_err(|_| {
                    ExactDiscoveryError::InvalidFacts("invalid responsibility shape hash")
                })?)
            }
            _ => None,
        };
        facts.push(ExactFunctionFact {
            artifact_id: artifact_id.clone(),
            path: location.path.clone(),
            symbol: artifact.label.clone(),
            start_line: location
                .start_line
                .ok_or(ExactDiscoveryError::InvalidFacts(
                    "function range is absent",
                ))?,
            end_line: location.end_line.ok_or(ExactDiscoveryError::InvalidFacts(
                "function range is absent",
            ))?,
            symbol_kind: anchor.symbol_kind(),
            signature_shape_hash: anchor.signature_shape_hash().clone(),
            normalized_body_hash: anchor.normalized_body_hash().clone(),
            test_scope,
            responsibility_shape_hash,
        });
    }
    discover_near_facts(
        program.snapshot_id().clone(),
        RUST_SYMBOL_ANCHOR_SYN_VERSION_V1.to_owned(),
        facts,
        anchors.len(),
    )
}

/// Enumerate structurally different two-member candidates from accepted,
/// snapshot-bound syntax signal facts. The result is candidate-only: shared
/// vocabulary is never promoted to a responsibility or family decision.
pub fn discover_responsibility_signals(
    program: &ProgramSpace,
) -> Result<ResponsibilitySignalCandidateReport> {
    let anchors = program
        .accepted_rust_symbol_anchors()
        .ok_or(ExactDiscoveryError::MissingAcceptedAnchors)?;
    let artifacts = program
        .artifacts()
        .iter()
        .map(|artifact| (&artifact.id, artifact))
        .collect::<BTreeMap<_, _>>();
    let mut facts = Vec::new();
    for (artifact_id, anchor) in anchors {
        if !matches!(
            anchor.symbol_kind(),
            RustSymbolKindV1::Function | RustSymbolKindV1::Method
        ) {
            continue;
        }
        let artifact = artifacts
            .get(artifact_id)
            .ok_or(ExactDiscoveryError::InvalidFacts(
                "accepted anchor has no matching artifact",
            ))?;
        let location = artifact
            .location
            .as_ref()
            .ok_or(ExactDiscoveryError::InvalidFacts(
                "accepted function anchor has no location",
            ))?;
        let test_scope = match (
            artifact
                .attributes
                .get("test_scope")
                .and_then(Value::as_str),
            artifact
                .attributes
                .get("test_scope_extractor")
                .and_then(Value::as_str),
        ) {
            (Some("production"), Some(TEST_SCOPE_EXTRACTOR_V1)) => Some(TestScopeFact::Production),
            (Some("test"), Some(TEST_SCOPE_EXTRACTOR_V1)) => Some(TestScopeFact::Test),
            _ => None,
        };
        let responsibility_shape_hash = match (
            artifact
                .attributes
                .get("responsibility_shape_hash")
                .and_then(Value::as_str),
            artifact
                .attributes
                .get("responsibility_shape_extractor")
                .and_then(Value::as_str),
        ) {
            (Some(hash), Some(RESPONSIBILITY_SHAPE_EXTRACTOR_V1)) => {
                Some(ContentHash::parse(hash).map_err(|_| {
                    ExactDiscoveryError::InvalidFacts("invalid responsibility shape hash")
                })?)
            }
            _ => None,
        };
        let signal_extractor = artifact
            .attributes
            .get("responsibility_signals_extractor")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let signals = artifact
            .attributes
            .get("responsibility_signals")
            .and_then(parse_responsibility_signals);
        facts.push(ResponsibilitySignalFact {
            artifact_id: artifact_id.clone(),
            path: location.path.clone(),
            symbol: artifact.label.clone(),
            start_line: location
                .start_line
                .ok_or(ExactDiscoveryError::InvalidFacts(
                    "function range is absent",
                ))?,
            end_line: location.end_line.ok_or(ExactDiscoveryError::InvalidFacts(
                "function range is absent",
            ))?,
            symbol_kind: anchor.symbol_kind(),
            signature_shape_hash: anchor.signature_shape_hash().clone(),
            normalized_body_hash: anchor.normalized_body_hash().clone(),
            test_scope,
            responsibility_shape_hash,
            responsibility_signals_extractor: signal_extractor,
            responsibility_signals: signals,
        });
    }
    discover_signal_facts(
        program.snapshot_id().clone(),
        RUST_SYMBOL_ANCHOR_SYN_VERSION_V1.to_owned(),
        facts,
        anchors.len(),
    )
}

fn parse_responsibility_signals(value: &Value) -> Option<ResponsibilitySignals> {
    let object = value.as_object()?;
    if object.len() != 3 {
        return None;
    }
    let parse = |name: &str| {
        object
            .get(name)?
            .as_array()?
            .iter()
            .map(|value| value.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
    };
    Some(ResponsibilitySignals {
        callable: parse("callable")?,
        signature: parse("signature")?,
        operation: parse("operation")?,
    })
}

fn discover_signal_facts(
    snapshot_id: StableId,
    rust_anchor_syn_version: String,
    mut facts: Vec<ResponsibilitySignalFact>,
    accepted_rust_symbols: usize,
) -> Result<ResponsibilitySignalCandidateReport> {
    if snapshot_id.kind() != "snapshot"
        || rust_anchor_syn_version.is_empty()
        || accepted_rust_symbols < facts.len()
    {
        return Err(ExactDiscoveryError::InvalidFacts("invalid report input"));
    }
    facts.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    if facts
        .windows(2)
        .any(|pair| pair[0].artifact_id == pair[1].artifact_id)
    {
        return Err(ExactDiscoveryError::InvalidFacts(
            "duplicate function artifact",
        ));
    }

    let excluded_non_function_symbols = accepted_rust_symbols - facts.len();
    let mut excluded_profile_paths = 0usize;
    let mut excluded_test_functions = 0usize;
    let mut excluded_unknown_test_scope = 0usize;
    let mut excluded_missing_shape_fact = 0usize;
    let mut excluded_unknown_signal_fact = 0usize;
    let mut eligible = Vec::new();
    for fact in facts {
        if !production_rust_path(&fact.path) {
            excluded_profile_paths += 1;
        } else if fact.test_scope == Some(TestScopeFact::Test) {
            excluded_test_functions += 1;
        } else if fact.test_scope.is_none() {
            excluded_unknown_test_scope += 1;
        } else if fact.responsibility_shape_hash.is_none() {
            excluded_missing_shape_fact += 1;
        } else if fact.responsibility_signals_extractor.as_deref()
            != Some(RESPONSIBILITY_SIGNALS_EXTRACTOR_V1)
        {
            excluded_unknown_signal_fact += 1;
        } else if let Some(signals) = fact.responsibility_signals.as_ref() {
            if fact.start_line == 0 || fact.end_line < fact.start_line {
                return Err(ExactDiscoveryError::InvalidFacts(
                    "invalid function source range",
                ));
            }
            if !valid_signal_terms(&signals.callable)
                || !valid_signal_terms(&signals.signature)
                || !valid_signal_terms(&signals.operation)
            {
                return Err(ExactDiscoveryError::InvalidFacts(
                    "responsibility signal terms are not sorted unique strings",
                ));
            }
            eligible.push(fact);
        } else {
            excluded_unknown_signal_fact += 1;
        }
    }

    let max_frequency = 32usize.max(eligible.len().div_ceil(5));
    let callable_frequency = term_frequencies(&eligible, |signals| &signals.callable);
    let signature_frequency = term_frequencies(&eligible, |signals| &signals.signature);
    let operation_frequency = term_frequencies(&eligible, |signals| &signals.operation);
    let ignored_high_frequency_terms = ResponsibilitySignals {
        callable: high_frequency_terms(&callable_frequency, max_frequency),
        signature: high_frequency_terms(&signature_frequency, max_frequency),
        operation: high_frequency_terms(&operation_frequency, max_frequency),
    };

    let mut excluded_same_shape_pairs = 0usize;
    let mut evaluated_distinct_shape_pairs = 0usize;
    let mut candidates = Vec::new();
    for left_index in 0..eligible.len() {
        for right_index in (left_index + 1)..eligible.len() {
            let left = &eligible[left_index];
            let right = &eligible[right_index];
            if left.responsibility_shape_hash == right.responsibility_shape_hash {
                excluded_same_shape_pairs += 1;
                continue;
            }
            evaluated_distinct_shape_pairs += 1;
            let left_signals = left.responsibility_signals.as_ref().expect("eligible fact");
            let right_signals = right
                .responsibility_signals
                .as_ref()
                .expect("eligible fact");
            let matched_callable = selective_intersection(
                &left_signals.callable,
                &right_signals.callable,
                &callable_frequency,
                max_frequency,
            );
            let matched_signature = selective_intersection(
                &left_signals.signature,
                &right_signals.signature,
                &signature_frequency,
                max_frequency,
            );
            let matched_operation = selective_intersection(
                &left_signals.operation,
                &right_signals.operation,
                &operation_frequency,
                max_frequency,
            );
            if matched_callable.len() + matched_signature.len() < 1 || matched_operation.len() < 2 {
                continue;
            }
            let operation_jaccard_ppm = selective_operation_jaccard_ppm(
                &left_signals.operation,
                &right_signals.operation,
                &operation_frequency,
                max_frequency,
                matched_operation.len(),
            )?;
            if operation_jaccard_ppm < MINIMUM_OPERATION_JACCARD_PPM {
                continue;
            }
            let member_ids = vec![
                Value::String(left.artifact_id.to_string()),
                Value::String(right.artifact_id.to_string()),
            ];
            let candidate_id = StableId::derived(
                "responsibility-family-candidate-signal",
                &BTreeMap::from([
                    (
                        "extractor".to_owned(),
                        Value::String(RESPONSIBILITY_SIGNALS_EXTRACTOR_V1.to_owned()),
                    ),
                    ("members".to_owned(), Value::Array(member_ids)),
                    (
                        "rule".to_owned(),
                        Value::String(RESPONSIBILITY_SIGNAL_PAIR_EXTRACTOR_ID.to_owned()),
                    ),
                    (
                        "snapshot_id".to_owned(),
                        Value::String(snapshot_id.to_string()),
                    ),
                ]),
            )?;
            candidates.push(ResponsibilitySignalCandidate {
                candidate_id,
                rank: 0,
                status: CandidateStatus::CandidateOnly,
                matched_callable,
                matched_signature,
                matched_operation,
                operation_jaccard_ppm,
                members: vec![signal_member(left), signal_member(right)],
                unknowns: vec![
                    "shared syntax vocabulary does not establish semantic equivalence".to_owned(),
                    "a common contract and common change reason require decision-stage evidence"
                        .to_owned(),
                ],
            });
        }
    }
    candidates.sort_by(|left, right| {
        signal_match_count(right)
            .cmp(&signal_match_count(left))
            .then_with(|| {
                right
                    .matched_operation
                    .len()
                    .cmp(&left.matched_operation.len())
            })
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
    });
    for (index, candidate) in candidates.iter_mut().enumerate() {
        candidate.rank = count(index + 1)?;
    }
    let candidate_members = candidates
        .iter()
        .map(|candidate| candidate.members.len())
        .sum();
    Ok(ResponsibilitySignalCandidateReport {
        schema: RESPONSIBILITY_SIGNAL_REPORT_SCHEMA,
        snapshot_id,
        extractor: ExactDiscoveryExtractor {
            id: RESPONSIBILITY_SIGNAL_PAIR_EXTRACTOR_ID,
            rust_anchor_syn_version,
        },
        profile: ExactDiscoveryProfile {
            included_path: "crates/*/src/**/*.rs | rust/*/src/**/*.rs",
            included_symbol_kinds: ["function", "method"],
            test_exclusion_fact: "artifact.attributes.test_scope=test@reviewgraphen.ingest.rust-test-scope@1",
        },
        authority: AuthorityBoundary::non_authority(),
        selection: ResponsibilitySignalSelection {
            signal_fact_extractor: RESPONSIBILITY_SIGNALS_EXTRACTOR_V1,
            minimum_callable_or_signature_terms: 1,
            minimum_operation_terms: 2,
            minimum_operation_jaccard_ppm: MINIMUM_OPERATION_JACCARD_PPM,
            max_selective_term_frequency: count(max_frequency)?,
            frequency_denominator: "eligible_functions",
        },
        denominator: ResponsibilitySignalDenominator {
            accepted_rust_symbols: count(accepted_rust_symbols)?,
            eligible_functions: count(eligible.len())?,
            excluded_non_function_symbols: count(excluded_non_function_symbols)?,
            excluded_profile_paths: count(excluded_profile_paths)?,
            excluded_test_functions: count(excluded_test_functions)?,
            excluded_unknown_test_scope: count(excluded_unknown_test_scope)?,
            excluded_missing_shape_fact: count(excluded_missing_shape_fact)?,
            excluded_unknown_signal_fact: count(excluded_unknown_signal_fact)?,
            excluded_same_shape_pairs: count(excluded_same_shape_pairs)?,
            evaluated_distinct_shape_pairs: count(evaluated_distinct_shape_pairs)?,
            candidate_pairs: count(candidates.len())?,
            candidate_members: count(candidate_members)?,
        },
        ignored_high_frequency_terms,
        candidates,
        information_loss: vec![
            "comments, formatting and literal values do not contribute signals".to_owned(),
            "type paths and calls are syntactic terminals without name or dispatch resolution"
                .to_owned(),
            "pairs with equal responsibility-shape hashes are omitted".to_owned(),
            "high-frequency vocabulary is excluded by the declared frequency rule".to_owned(),
        ],
        unknowns: vec![
            "missing or unsupported responsibility-signal facts are excluded from the eligible denominator"
                .to_owned(),
            "macro expansion, aliases, dynamic dispatch and semantic equivalence are not observed"
                .to_owned(),
            "responsibility and common change reason require decision-stage evidence".to_owned(),
        ],
    })
}

fn valid_signal_terms(terms: &[String]) -> bool {
    terms.iter().all(|term| !term.is_empty()) && terms.windows(2).all(|pair| pair[0] < pair[1])
}

fn term_frequencies(
    facts: &[ResponsibilitySignalFact],
    select: impl Fn(&ResponsibilitySignals) -> &[String],
) -> BTreeMap<String, usize> {
    let mut frequencies = BTreeMap::new();
    for fact in facts {
        let signals = fact.responsibility_signals.as_ref().expect("eligible fact");
        for term in select(signals) {
            *frequencies.entry(term.clone()).or_insert(0) += 1;
        }
    }
    frequencies
}

fn high_frequency_terms(frequencies: &BTreeMap<String, usize>, maximum: usize) -> Vec<String> {
    frequencies
        .iter()
        .filter_map(|(term, frequency)| (*frequency > maximum).then_some(term.clone()))
        .collect()
}

fn selective_intersection(
    left: &[String],
    right: &[String],
    frequencies: &BTreeMap<String, usize>,
    maximum: usize,
) -> Vec<String> {
    let right = right.iter().collect::<BTreeSet<_>>();
    left.iter()
        .filter(|term| {
            right.contains(term)
                && frequencies
                    .get(*term)
                    .is_some_and(|frequency| (2..=maximum).contains(frequency))
        })
        .cloned()
        .collect()
}

fn selective_operation_jaccard_ppm(
    left: &[String],
    right: &[String],
    frequencies: &BTreeMap<String, usize>,
    maximum: usize,
    intersection_len: usize,
) -> Result<u64> {
    let union_len = left
        .iter()
        .chain(right)
        .filter(|term| {
            frequencies
                .get(*term)
                .is_some_and(|frequency| (2..=maximum).contains(frequency))
        })
        .collect::<BTreeSet<_>>()
        .len();
    let intersection = count(intersection_len)?;
    let union = count(union_len)?;
    if union == 0 || intersection > union {
        return Err(ExactDiscoveryError::InvalidFacts(
            "invalid selective operation term closure",
        ));
    }
    intersection
        .checked_mul(JACCARD_SCALE_PPM)
        .ok_or(ExactDiscoveryError::CountOverflow)
        .map(|scaled| scaled / union)
}

fn signal_member(fact: &ResponsibilitySignalFact) -> ResponsibilitySignalCandidateMember {
    ResponsibilitySignalCandidateMember {
        artifact_id: fact.artifact_id.clone(),
        path: fact.path.clone(),
        symbol: fact.symbol.clone(),
        start_line: fact.start_line,
        end_line: fact.end_line,
        symbol_kind: fact.symbol_kind,
        signature_shape_hash: fact.signature_shape_hash.clone(),
        normalized_body_hash: fact.normalized_body_hash.clone(),
        responsibility_shape_hash: fact
            .responsibility_shape_hash
            .clone()
            .expect("eligible fact"),
    }
}

fn signal_match_count(candidate: &ResponsibilitySignalCandidate) -> usize {
    candidate.matched_callable.len()
        + candidate.matched_signature.len()
        + candidate.matched_operation.len()
}

fn discover_near_facts(
    snapshot_id: StableId,
    rust_anchor_syn_version: String,
    mut facts: Vec<ExactFunctionFact>,
    accepted_rust_symbols: usize,
) -> Result<NearBodyCandidateReport> {
    if snapshot_id.kind() != "snapshot"
        || rust_anchor_syn_version.is_empty()
        || accepted_rust_symbols < facts.len()
    {
        return Err(ExactDiscoveryError::InvalidFacts("invalid report input"));
    }
    facts.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    if facts
        .windows(2)
        .any(|pair| pair[0].artifact_id == pair[1].artifact_id)
    {
        return Err(ExactDiscoveryError::InvalidFacts(
            "duplicate function artifact",
        ));
    }
    let excluded_non_function_symbols = accepted_rust_symbols - facts.len();
    let mut excluded_profile_paths = 0usize;
    let mut excluded_test_functions = 0usize;
    let mut excluded_unknown_test_scope = 0usize;
    let mut excluded_missing_shape_fact = 0usize;
    let mut eligible = Vec::new();
    for fact in facts {
        if !production_rust_path(&fact.path) {
            excluded_profile_paths += 1;
        } else if fact.test_scope == Some(TestScopeFact::Test) {
            excluded_test_functions += 1;
        } else if fact.test_scope.is_none() {
            excluded_unknown_test_scope += 1;
        } else if fact.responsibility_shape_hash.is_none() {
            excluded_missing_shape_fact += 1;
        } else if fact.start_line == 0 || fact.end_line < fact.start_line {
            return Err(ExactDiscoveryError::InvalidFacts(
                "invalid function source range",
            ));
        } else {
            eligible.push(fact);
        }
    }
    let mut groups = BTreeMap::<ContentHash, Vec<ExactFunctionFact>>::new();
    for fact in eligible.iter().cloned() {
        groups
            .entry(
                fact.responsibility_shape_hash
                    .clone()
                    .expect("checked above"),
            )
            .or_default()
            .push(fact);
    }
    let mut excluded_exact_only_groups = 0usize;
    let mut candidates = Vec::new();
    for (shape_hash, group) in groups {
        if group.len() < 2 {
            continue;
        }
        let distinct = group
            .iter()
            .map(|fact| fact.normalized_body_hash.clone())
            .collect::<std::collections::BTreeSet<_>>();
        if distinct.len() < 2 {
            excluded_exact_only_groups += 1;
            continue;
        }
        let candidate_id = StableId::derived(
            "responsibility-family-candidate-signal",
            &BTreeMap::from([
                (
                    "extractor".to_owned(),
                    Value::String(NEAR_BODY_EXTRACTOR_ID.to_owned()),
                ),
                (
                    "responsibility_shape_hash".to_owned(),
                    Value::String(shape_hash.to_string()),
                ),
                (
                    "snapshot_id".to_owned(),
                    Value::String(snapshot_id.to_string()),
                ),
            ]),
        )?;
        candidates.push(NearBodyCandidate {
            candidate_id,
            responsibility_shape_hash: shape_hash,
            distinct_exact_body_hashes: count(distinct.len())?,
            minimum_member_span_lines: group
                .iter()
                .map(|fact| fact.end_line - fact.start_line + 1)
                .min()
                .expect("nonempty group"),
            status: CandidateStatus::CandidateOnly,
            members: group
                .into_iter()
                .map(|fact| NearCandidateMember {
                    artifact_id: fact.artifact_id,
                    path: fact.path,
                    symbol: fact.symbol,
                    start_line: fact.start_line,
                    end_line: fact.end_line,
                    symbol_kind: fact.symbol_kind,
                    signature_shape_hash: fact.signature_shape_hash,
                    normalized_body_hash: fact.normalized_body_hash,
                })
                .collect(),
            unknowns: vec![
                "structural shape equality does not establish semantic equivalence".to_owned(),
                "identifier and literal roles require responsibility-stage evidence".to_owned(),
            ],
        });
    }
    candidates.sort_by(|left, right| {
        right
            .minimum_member_span_lines
            .cmp(&left.minimum_member_span_lines)
            .then_with(|| right.members.len().cmp(&left.members.len()))
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
    });
    let candidate_members = candidates
        .iter()
        .map(|candidate| candidate.members.len())
        .sum();
    Ok(NearBodyCandidateReport {
        schema: NEAR_BODY_REPORT_SCHEMA,
        snapshot_id,
        extractor: ExactDiscoveryExtractor {
            id: NEAR_BODY_EXTRACTOR_ID,
            rust_anchor_syn_version,
        },
        profile: ExactDiscoveryProfile {
            included_path: "crates/*/src/**/*.rs | rust/*/src/**/*.rs",
            included_symbol_kinds: ["function", "method"],
            test_exclusion_fact: "artifact.attributes.test_scope=test@reviewgraphen.ingest.rust-test-scope@1",
        },
        authority: AuthorityBoundary::non_authority(),
        denominator: NearDiscoveryDenominator {
            accepted_rust_symbols: count(accepted_rust_symbols)?,
            eligible_functions: count(eligible.len())?,
            excluded_non_function_symbols: count(excluded_non_function_symbols)?,
            excluded_profile_paths: count(excluded_profile_paths)?,
            excluded_test_functions: count(excluded_test_functions)?,
            excluded_unknown_test_scope: count(excluded_unknown_test_scope)?,
            excluded_missing_shape_fact: count(excluded_missing_shape_fact)?,
            excluded_exact_only_groups: count(excluded_exact_only_groups)?,
            candidate_groups: count(candidates.len())?,
            candidate_members: count(candidate_members)?,
        },
        candidates,
        information_loss: vec![
            "non-keyword identifiers and literal values are erased by the accepted shape fact"
                .to_owned(),
            "exact-only groups are omitted because exact discovery owns them".to_owned(),
            "macro expansion, types, name binding and semantic equivalence are not established"
                .to_owned(),
        ],
        unknowns: vec![
            "Type-3 clones with inserted, removed or reordered statements are not observed"
                .to_owned(),
            "responsibility and common change reason require decision-stage evidence".to_owned(),
        ],
    })
}

fn production_rust_path(path: &str) -> bool {
    let Some(after_crates) = path
        .strip_prefix("crates/")
        .or_else(|| path.strip_prefix("rust/"))
    else {
        return false;
    };
    let Some((crate_name, within_crate)) = after_crates.split_once('/') else {
        return false;
    };
    !crate_name.is_empty() && within_crate.starts_with("src/") && path.ends_with(".rs")
}

fn count(value: usize) -> Result<u64> {
    u64::try_from(value).map_err(|_| ExactDiscoveryError::CountOverflow)
}

#[cfg(test)]
mod tests;
