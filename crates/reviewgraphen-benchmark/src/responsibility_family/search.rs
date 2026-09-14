//! Deterministic planned-responsibility candidate search.
//!
//! This benchmark-only projection searches accepted Rust syntax facts. It
//! deliberately retains semantic clauses only through the canonical contract
//! hash and never accepts a candidate, verifies a clause, or signs off.

use super::AuthorityBoundary;
use reviewgraphen_core::{ContentHash, ProgramSpace, RustSymbolKindV1, StableId, canonical_json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const PLANNED_RESPONSIBILITY_CONTRACT_SCHEMA: &str =
    "reviewgraphen.benchmark.planned_responsibility_contract.v1";
pub const RESPONSIBILITY_SEARCH_REPORT_SCHEMA: &str =
    "reviewgraphen.benchmark.responsibility_search_report.v1";
pub const RESPONSIBILITY_SEARCH_RULE: &str =
    "reviewgraphen.benchmark.planned-responsibility-search@1";
pub const RESPONSIBILITY_SIGNALS_EXTRACTOR_V1: &str =
    "reviewgraphen.ingest.rust-responsibility-signals@1";

const TEST_SCOPE_EXTRACTOR_V1: &str = "reviewgraphen.ingest.rust-test-scope@1";
const MAX_VALUES: usize = 64;
const MAX_CONTRACT_UNKNOWNS: usize = MAX_VALUES - 2;
const MAX_TEXT_BYTES: usize = 4096;
const MINIMUM_OPERATION_COVERAGE_PPM: u64 = 600_000;
const COVERAGE_SCALE_PPM: u64 = 1_000_000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedResponsibilityContract {
    schema: String,
    contract_id: StableId,
    snapshot_id: StableId,
    signal_extractor: String,
    clauses: Vec<PlannedResponsibilityClause>,
    search_signals: SearchSignals,
    unknowns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PlannedResponsibilityClause {
    clause_id: String,
    kind: PlannedResponsibilityClauseKind,
    statement: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PlannedResponsibilityClauseKind {
    Precondition,
    Postcondition,
    Invariant,
    TypedError,
    Compatibility,
    Performance,
    PurposeConstraint,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SearchSignals {
    callable: Vec<String>,
    signature: Vec<String>,
    operation: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResponsibilitySearchReport {
    schema: &'static str,
    search_id: StableId,
    snapshot_id: StableId,
    contract: ContractBinding,
    program_space: ProgramSpaceBinding,
    rule: &'static str,
    signal_extractor: &'static str,
    selection: ResponsibilitySearchSelection,
    contract_clause_handling: ContractClauseHandling,
    authority: AuthorityBoundary,
    denominator: ResponsibilitySearchDenominator,
    query_signals: QuerySignals,
    candidates: Vec<ResponsibilitySearchCandidate>,
    information_loss: Vec<String>,
    unknowns: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ContractBinding {
    contract_id: StableId,
    canonical_hash: ContentHash,
    clause_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ProgramSpaceBinding {
    profile_id: String,
    profile_version: String,
    rule_set_hash: ContentHash,
    extractor_set_hash: ContentHash,
    policy_version: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ResponsibilitySearchSelection {
    frequency_denominator: &'static str,
    max_selective_term_frequency: u64,
    minimum_callable_or_signature_matches: u64,
    minimum_selective_operation_matches: u64,
    minimum_query_operation_coverage_ppm: u64,
    candidate_selection: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ContractClauseHandling {
    semantic_content_retained_by: &'static str,
    semantic_content_evaluated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ResponsibilitySearchDenominator {
    accepted_rust_symbols: u64,
    eligible_functions: u64,
    excluded_non_function_symbols: u64,
    excluded_profile_paths: u64,
    excluded_test_functions: u64,
    excluded_unknown_test_scope: u64,
    excluded_unknown_signal_fact: u64,
    evaluated_functions: u64,
    candidate_count: u64,
    candidate_clause_obligations: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct QuerySignals {
    selective: SearchSignals,
    absent: SearchSignals,
    high_frequency: SearchSignals,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ResponsibilitySearchCandidate {
    candidate_id: StableId,
    rank: u64,
    artifact_id: StableId,
    path: String,
    symbol: String,
    source_hash: ContentHash,
    member_hash: ContentHash,
    matched_signals: SearchSignals,
    unmatched_signals: SearchSignals,
    coverage: CandidateCoverage,
    unverified_clause_ids: Vec<String>,
    unknowns: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct CandidateCoverage {
    selective_callable_or_signature_matches: u64,
    selective_operation_matches: u64,
    query_operation_terms: u64,
    query_operation_coverage_ppm: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TestScopeFact {
    Production,
    Test,
}

#[derive(Clone, Debug)]
struct SearchFact {
    artifact_id: StableId,
    path: String,
    symbol: String,
    source_hash: ContentHash,
    member_hash: ContentHash,
    test_scope: Option<TestScopeFact>,
    signals_extractor: Option<String>,
    signals_present: bool,
    signals: Option<SearchSignals>,
}

#[derive(Debug, Error)]
pub enum ResponsibilitySearchError {
    #[error("invalid planned responsibility contract: {0}")]
    InvalidContract(&'static str),
    #[error("invalid ProgramSpace for planned responsibility search: {0}")]
    InvalidProgram(&'static str),
    #[error("planned responsibility search count overflow")]
    CountOverflow,
    #[error("planned responsibility search identity failed: {0}")]
    Identity(#[from] reviewgraphen_core::DomainError),
}

pub type Result<T> = std::result::Result<T, ResponsibilitySearchError>;

/// Search accepted Rust function and method artifacts against a validated,
/// user-declared contract. The result is a non-authoritative candidate report.
pub fn search_responsibility(
    program: &ProgramSpace,
    contract: &PlannedResponsibilityContract,
) -> Result<ResponsibilitySearchReport> {
    validate_contract(contract)?;
    if contract.snapshot_id != *program.snapshot_id() {
        return Err(ResponsibilitySearchError::InvalidContract(
            "contract snapshot does not match ProgramSpace",
        ));
    }

    let canonical_contract = canonical_json(contract)?;
    let contract_hash = ContentHash::sha256(&canonical_contract);
    let search_id = StableId::derived(
        "responsibility-search",
        &BTreeMap::from([
            (
                "contract_hash".to_owned(),
                Value::String(contract_hash.to_string()),
            ),
            (
                "rule".to_owned(),
                Value::String(RESPONSIBILITY_SEARCH_RULE.to_owned()),
            ),
            (
                "signal_extractor".to_owned(),
                Value::String(RESPONSIBILITY_SIGNALS_EXTRACTOR_V1.to_owned()),
            ),
            (
                "snapshot_id".to_owned(),
                Value::String(contract.snapshot_id.to_string()),
            ),
            (
                "profile_id".to_owned(),
                Value::String(program.profile_id().to_owned()),
            ),
            (
                "profile_version".to_owned(),
                Value::String(program.profile_version().to_owned()),
            ),
            (
                "rule_set_hash".to_owned(),
                Value::String(program.rule_set_hash().to_string()),
            ),
            (
                "extractor_set_hash".to_owned(),
                Value::String(program.extractor_set_hash().to_string()),
            ),
            (
                "policy_version".to_owned(),
                Value::String(program.policy_version().to_owned()),
            ),
        ]),
    )?;

    let (facts, accepted_rust_symbols) = search_facts(program)?;
    let excluded_non_function_symbols = accepted_rust_symbols.checked_sub(facts.len()).ok_or(
        ResponsibilitySearchError::InvalidProgram(
            "accepted Rust symbol count is smaller than function facts",
        ),
    )?;
    let mut excluded_profile_paths = 0usize;
    let mut excluded_test_functions = 0usize;
    let mut excluded_unknown_test_scope = 0usize;
    let mut excluded_unknown_signal_fact = 0usize;
    let mut eligible = Vec::new();
    for fact in facts {
        if !production_rust_path(&fact.path) {
            excluded_profile_paths += 1;
        } else if fact.test_scope == Some(TestScopeFact::Test) {
            excluded_test_functions += 1;
        } else if fact.test_scope.is_none() {
            excluded_unknown_test_scope += 1;
        } else if fact.signals_extractor.as_deref() == Some(RESPONSIBILITY_SIGNALS_EXTRACTOR_V1)
            && fact.signals_present
            && fact.signals.is_none()
        {
            return Err(ResponsibilitySearchError::InvalidProgram(
                "supported responsibility signal fact is malformed",
            ));
        } else if fact.signals_extractor.as_deref() != Some(RESPONSIBILITY_SIGNALS_EXTRACTOR_V1) {
            excluded_unknown_signal_fact += 1;
        } else if let Some(signals) = fact.signals.as_ref() {
            if !valid_accepted_signal_terms(&signals.callable)
                || !valid_accepted_signal_terms(&signals.signature)
                || !valid_accepted_signal_terms(&signals.operation)
            {
                return Err(ResponsibilitySearchError::InvalidProgram(
                    "responsibility signal terms are not sorted unique strings",
                ));
            }
            eligible.push(fact);
        } else {
            excluded_unknown_signal_fact += 1;
        }
    }

    let maximum_frequency = 32usize.max(eligible.len().div_ceil(5));
    let callable_frequency = term_frequencies(&eligible, |signals| &signals.callable);
    let signature_frequency = term_frequencies(&eligible, |signals| &signals.signature);
    let operation_frequency = term_frequencies(&eligible, |signals| &signals.operation);
    let query_signals = QuerySignals {
        selective: SearchSignals {
            callable: selective_terms(
                &contract.search_signals.callable,
                &callable_frequency,
                maximum_frequency,
            ),
            signature: selective_terms(
                &contract.search_signals.signature,
                &signature_frequency,
                maximum_frequency,
            ),
            operation: selective_terms(
                &contract.search_signals.operation,
                &operation_frequency,
                maximum_frequency,
            ),
        },
        absent: SearchSignals {
            callable: absent_terms(&contract.search_signals.callable, &callable_frequency),
            signature: absent_terms(&contract.search_signals.signature, &signature_frequency),
            operation: absent_terms(&contract.search_signals.operation, &operation_frequency),
        },
        high_frequency: SearchSignals {
            callable: high_frequency_terms(
                &contract.search_signals.callable,
                &callable_frequency,
                maximum_frequency,
            ),
            signature: high_frequency_terms(
                &contract.search_signals.signature,
                &signature_frequency,
                maximum_frequency,
            ),
            operation: high_frequency_terms(
                &contract.search_signals.operation,
                &operation_frequency,
                maximum_frequency,
            ),
        },
    };

    let clause_ids = contract
        .clauses
        .iter()
        .map(|clause| clause.clause_id.clone())
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for fact in &eligible {
        let signals = fact.signals.as_ref().expect("eligible fact");
        let matched_signals = SearchSignals {
            callable: matching_terms(&query_signals.selective.callable, &signals.callable),
            signature: matching_terms(&query_signals.selective.signature, &signals.signature),
            operation: matching_terms(&query_signals.selective.operation, &signals.operation),
        };
        let callable_or_signature_matches = matched_signals
            .callable
            .len()
            .checked_add(matched_signals.signature.len())
            .ok_or(ResponsibilitySearchError::CountOverflow)?;
        let selective_operation_matches = matched_signals.operation.len();
        let query_operation_terms = contract.search_signals.operation.len();
        let coverage_ppm = coverage_ppm(selective_operation_matches, query_operation_terms)?;
        if callable_or_signature_matches < 1
            || selective_operation_matches < 2
            || coverage_ppm < MINIMUM_OPERATION_COVERAGE_PPM
        {
            continue;
        }
        let candidate_id = StableId::derived(
            "responsibility-search-candidate",
            &BTreeMap::from([
                (
                    "artifact_id".to_owned(),
                    Value::String(fact.artifact_id.to_string()),
                ),
                ("search_id".to_owned(), Value::String(search_id.to_string())),
            ]),
        )?;
        candidates.push(ResponsibilitySearchCandidate {
            candidate_id,
            rank: 0,
            artifact_id: fact.artifact_id.clone(),
            path: fact.path.clone(),
            symbol: fact.symbol.clone(),
            source_hash: fact.source_hash.clone(),
            member_hash: fact.member_hash.clone(),
            unmatched_signals: SearchSignals {
                callable: unmatched_terms(
                    &contract.search_signals.callable,
                    &matched_signals.callable,
                ),
                signature: unmatched_terms(
                    &contract.search_signals.signature,
                    &matched_signals.signature,
                ),
                operation: unmatched_terms(
                    &contract.search_signals.operation,
                    &matched_signals.operation,
                ),
            },
            matched_signals,
            coverage: CandidateCoverage {
                selective_callable_or_signature_matches: count(callable_or_signature_matches)?,
                selective_operation_matches: count(selective_operation_matches)?,
                query_operation_terms: count(query_operation_terms)?,
                query_operation_coverage_ppm: coverage_ppm,
            },
            unverified_clause_ids: clause_ids.clone(),
            unknowns: vec!["semantic clauses are not evaluated".to_owned()],
        });
    }
    candidates.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    for (index, candidate) in candidates.iter_mut().enumerate() {
        candidate.rank = count(index + 1)?;
    }

    let candidate_clause_obligations = count(candidates.len())?
        .checked_mul(count(contract.clauses.len())?)
        .ok_or(ResponsibilitySearchError::CountOverflow)?;
    let report_unknowns = sorted_unique(
        contract
            .unknowns
            .iter()
            .cloned()
            .chain(std::iter::once(
                "candidate membership is not responsibility-family acceptance".to_owned(),
            ))
            .chain(std::iter::once(
                "semantic clauses are not evaluated for matching".to_owned(),
            ))
            .collect(),
    );

    Ok(ResponsibilitySearchReport {
        schema: RESPONSIBILITY_SEARCH_REPORT_SCHEMA,
        search_id,
        snapshot_id: program.snapshot_id().clone(),
        contract: ContractBinding {
            contract_id: contract.contract_id.clone(),
            canonical_hash: contract_hash,
            clause_count: count(contract.clauses.len())?,
        },
        program_space: ProgramSpaceBinding {
            profile_id: program.profile_id().to_owned(),
            profile_version: program.profile_version().to_owned(),
            rule_set_hash: program.rule_set_hash().clone(),
            extractor_set_hash: program.extractor_set_hash().clone(),
            policy_version: program.policy_version().to_owned(),
        },
        rule: RESPONSIBILITY_SEARCH_RULE,
        signal_extractor: RESPONSIBILITY_SIGNALS_EXTRACTOR_V1,
        selection: ResponsibilitySearchSelection {
            frequency_denominator: "eligible_functions",
            max_selective_term_frequency: count(maximum_frequency)?,
            minimum_callable_or_signature_matches: 1,
            minimum_selective_operation_matches: 2,
            minimum_query_operation_coverage_ppm: MINIMUM_OPERATION_COVERAGE_PPM,
            candidate_selection: "all_eligible_matching_candidates",
        },
        contract_clause_handling: ContractClauseHandling {
            semantic_content_retained_by: "contract_hash_only",
            semantic_content_evaluated: false,
        },
        authority: AuthorityBoundary::non_authority(),
        denominator: ResponsibilitySearchDenominator {
            accepted_rust_symbols: count(accepted_rust_symbols)?,
            eligible_functions: count(eligible.len())?,
            excluded_non_function_symbols: count(excluded_non_function_symbols)?,
            excluded_profile_paths: count(excluded_profile_paths)?,
            excluded_test_functions: count(excluded_test_functions)?,
            excluded_unknown_test_scope: count(excluded_unknown_test_scope)?,
            excluded_unknown_signal_fact: count(excluded_unknown_signal_fact)?,
            evaluated_functions: count(eligible.len())?,
            candidate_count: count(candidates.len())?,
            candidate_clause_obligations,
        },
        query_signals,
        candidates,
        information_loss: vec![
            "responsibility signals are syntax observations without name or dispatch resolution"
                .to_owned(),
            "semantic clauses and purpose constraints are retained only by the canonical contract hash"
                .to_owned(),
        ],
        unknowns: report_unknowns,
    })
}

fn validate_contract(contract: &PlannedResponsibilityContract) -> Result<()> {
    if contract.schema != PLANNED_RESPONSIBILITY_CONTRACT_SCHEMA
        || contract.contract_id.kind() != "planned-responsibility-contract"
        || contract.snapshot_id.kind() != "snapshot"
        || contract.signal_extractor != RESPONSIBILITY_SIGNALS_EXTRACTOR_V1
        || contract.clauses.len() > MAX_VALUES
        || contract.unknowns.len() > MAX_CONTRACT_UNKNOWNS
        || !valid_terms(&contract.unknowns)
        || !valid_terms(&contract.search_signals.callable)
        || !valid_terms(&contract.search_signals.signature)
        || !valid_terms(&contract.search_signals.operation)
        || contract.search_signals.callable.is_empty()
            && contract.search_signals.signature.is_empty()
        || contract.search_signals.operation.len() < 2
    {
        return Err(ResponsibilitySearchError::InvalidContract(
            "invalid header, bounds, or signal ordering",
        ));
    }
    if !contract
        .clauses
        .windows(2)
        .all(|pair| pair[0].clause_id < pair[1].clause_id)
        || contract
            .clauses
            .iter()
            .any(|clause| !valid_text(&clause.clause_id) || !valid_text(&clause.statement))
    {
        return Err(ResponsibilitySearchError::InvalidContract(
            "clauses must be sorted unique bounded text",
        ));
    }
    Ok(())
}

fn search_facts(program: &ProgramSpace) -> Result<(Vec<SearchFact>, usize)> {
    let anchors =
        program
            .accepted_rust_symbol_anchors()
            .ok_or(ResponsibilitySearchError::InvalidProgram(
                "accepted Rust symbol anchors are unavailable",
            ))?;
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
        let artifact =
            artifacts
                .get(artifact_id)
                .ok_or(ResponsibilitySearchError::InvalidProgram(
                    "accepted function anchor has no matching artifact",
                ))?;
        let location =
            artifact
                .location
                .as_ref()
                .ok_or(ResponsibilitySearchError::InvalidProgram(
                    "accepted function anchor has no location",
                ))?;
        let source_hash =
            artifact
                .content_hash
                .clone()
                .ok_or(ResponsibilitySearchError::InvalidProgram(
                    "accepted function artifact has no source hash",
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
        facts.push(SearchFact {
            artifact_id: artifact_id.clone(),
            path: location.path.clone(),
            symbol: artifact.label.clone(),
            source_hash,
            member_hash: anchor.normalized_body_hash().clone(),
            test_scope,
            signals_extractor: artifact
                .attributes
                .get("responsibility_signals_extractor")
                .and_then(Value::as_str)
                .map(str::to_owned),
            signals_present: artifact.attributes.contains_key("responsibility_signals"),
            signals: artifact
                .attributes
                .get("responsibility_signals")
                .and_then(parse_signals),
        });
    }
    facts.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    if facts
        .windows(2)
        .any(|pair| pair[0].artifact_id == pair[1].artifact_id)
    {
        return Err(ResponsibilitySearchError::InvalidProgram(
            "duplicate accepted function artifact",
        ));
    }
    Ok((facts, anchors.len()))
}

fn parse_signals(value: &Value) -> Option<SearchSignals> {
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
    Some(SearchSignals {
        callable: parse("callable")?,
        signature: parse("signature")?,
        operation: parse("operation")?,
    })
}

fn valid_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_TEXT_BYTES
}

fn valid_terms(terms: &[String]) -> bool {
    terms.len() <= MAX_VALUES
        && terms.iter().all(|term| valid_text(term))
        && terms.windows(2).all(|pair| pair[0] < pair[1])
}

fn valid_accepted_signal_terms(terms: &[String]) -> bool {
    terms.iter().all(|term| valid_text(term)) && terms.windows(2).all(|pair| pair[0] < pair[1])
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

fn term_frequencies(
    facts: &[SearchFact],
    select: impl Fn(&SearchSignals) -> &[String],
) -> BTreeMap<String, usize> {
    let mut frequencies = BTreeMap::new();
    for fact in facts {
        for term in select(fact.signals.as_ref().expect("eligible fact")) {
            *frequencies.entry(term.clone()).or_insert(0) += 1;
        }
    }
    frequencies
}

fn selective_terms(
    query: &[String],
    frequencies: &BTreeMap<String, usize>,
    maximum: usize,
) -> Vec<String> {
    query
        .iter()
        .filter(|term| {
            frequencies
                .get(*term)
                .is_some_and(|frequency| (1..=maximum).contains(frequency))
        })
        .cloned()
        .collect()
}

fn absent_terms(query: &[String], frequencies: &BTreeMap<String, usize>) -> Vec<String> {
    query
        .iter()
        .filter(|term| !frequencies.contains_key(*term))
        .cloned()
        .collect()
}

fn high_frequency_terms(
    query: &[String],
    frequencies: &BTreeMap<String, usize>,
    maximum: usize,
) -> Vec<String> {
    query
        .iter()
        .filter(|term| {
            frequencies
                .get(*term)
                .is_some_and(|frequency| *frequency > maximum)
        })
        .cloned()
        .collect()
}

fn matching_terms(query: &[String], document: &[String]) -> Vec<String> {
    query
        .iter()
        .filter(|term| document.binary_search(term).is_ok())
        .cloned()
        .collect()
}

fn unmatched_terms(query: &[String], matched: &[String]) -> Vec<String> {
    let matched = matched.iter().collect::<BTreeSet<_>>();
    query
        .iter()
        .filter(|term| !matched.contains(term))
        .cloned()
        .collect()
}

fn coverage_ppm(matches: usize, terms: usize) -> Result<u64> {
    let matches = count(matches)?;
    let terms = count(terms)?;
    if terms == 0 {
        return Err(ResponsibilitySearchError::InvalidContract(
            "operation query must not be empty",
        ));
    }
    matches
        .checked_mul(COVERAGE_SCALE_PPM)
        .ok_or(ResponsibilitySearchError::CountOverflow)
        .map(|scaled| scaled / terms)
}

fn sorted_unique(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn count(value: usize) -> Result<u64> {
    u64::try_from(value).map_err(|_| ResponsibilitySearchError::CountOverflow)
}
