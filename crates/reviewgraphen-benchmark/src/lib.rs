//! M7 research artifacts.  Nothing in this crate is accepted review state.

pub mod prepare;
pub mod real;
pub mod target_context;

use reviewgraphen_core::{ContentHash, canonical_json};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const TRIAL_MANIFEST_SCHEMA: &str = "reviewgraphen.benchmark.trial_manifest.v1";
pub const CANDIDATE_OUTPUT_SCHEMA: &str = "reviewgraphen.benchmark.candidate_output.v1";
pub const ORACLE_SCHEMA: &str = "reviewgraphen.benchmark.oracle.v1";
pub const SCORE_SCHEMA: &str = "reviewgraphen.benchmark.score.v1";
pub const TRIAL_INVENTORY_SCHEMA: &str = "reviewgraphen.benchmark.trial_inventory.v1";
pub const COLLECTION_SCHEMA: &str = "reviewgraphen.benchmark.collection.v1";
pub const RUN_SUMMARY_SCHEMA: &str = "reviewgraphen.benchmark.run_summary.v1";
pub const MECHANISM_ONTOLOGY_VERSION: &str = "reviewgraphen.benchmark.mechanism_ontology.v1";
pub const PROTOCOL_VERSION: &str = "reviewgraphen.benchmark.protocol.v2";
pub const MAX_CANDIDATE_LOCATIONS: usize = 16;
pub const MAX_MECHANISM_TAGS: usize = 8;
pub const MAX_SOURCE_LINE: u32 = 1_000_000;

#[derive(Debug, Error)]
pub enum BenchmarkError {
    #[error("benchmark validation failed: {0}")]
    Validation(&'static str),
    #[error("benchmark JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("benchmark canonicalization failed: {0}")]
    Canonical(#[from] reviewgraphen_core::DomainError),
    #[error("candidate trial does not match manifest")]
    TrialMismatch,
    #[error("oracle unit does not match manifest")]
    UnitMismatch,
    #[error("candidate violates the trial protocol: {0}")]
    ProtocolInvalid(&'static str),
    #[error(
        "paired aggregate has incomplete or duplicate arms for {unit_id} replicate {replicate}"
    )]
    InvalidPair { unit_id: String, replicate: u32 },
}

pub type Result<T> = std::result::Result<T, BenchmarkError>;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Arm {
    B1FreeForm,
    G3Proxy,
    FullReviewGraphen,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Structured,
    Abstained,
    ParseFailure,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MechanismId {
    CheckWriteGap,
    CrossFileContract,
    CrossFileEffect,
    RepeatableEntry,
    RepeatedEffect,
    StateTransitionGap,
    UnstableScopeKey,
    UnstableToken,
}
pub const MECHANISM_ONTOLOGY: &[MechanismId] = &[
    MechanismId::CheckWriteGap,
    MechanismId::CrossFileContract,
    MechanismId::CrossFileEffect,
    MechanismId::RepeatableEntry,
    MechanismId::RepeatedEffect,
    MechanismId::StateTransitionGap,
    MechanismId::UnstableScopeKey,
    MechanismId::UnstableToken,
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionConfig {
    pub provider: String,
    pub model: String,
    pub model_revision: String,
    pub reasoning_effort: String,
    pub prompt_template_version: String,
    pub tool_policy_version: String,
    pub runner_version: String,
    pub inference_settings: BTreeMap<String, String>,
    pub budget: DeclaredBudget,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredBudget {
    pub wall_clock_seconds: Option<u64>,
    pub session_limit: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrialManifest {
    pub schema: String,
    pub trial_id: String,
    pub unit_id: String,
    pub arm: Arm,
    pub replicate: u32,
    pub input_tree_hash: ContentHash,
    pub packet_hashes: BTreeSet<ContentHash>,
    pub expected_packet_ids: BTreeSet<String>,
    pub source_inventory: Vec<SourceInventoryEntry>,
    pub source_bundle_hash: ContentHash,
    pub protocol_version: String,
    pub mechanism_ontology_version: String,
    pub paired_configuration_hash: ContentHash,
    pub limitations: BTreeSet<String>,
    pub execution: ExecutionConfig,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInventoryEntry {
    pub path: String,
    pub content_hash: ContentHash,
    pub line_count: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateFinding {
    pub local_id: String,
    pub locations: Vec<Location>,
    pub mechanism_tags: BTreeSet<MechanismId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObligationResult {
    pub packet_id: String,
    pub disposition: String,
    pub finding_local_ids: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateOutput {
    pub schema: String,
    pub trial_id: String,
    pub outcome: Outcome,
    pub findings: Vec<CandidateFinding>,
    pub obligation_results: Vec<ObligationResult>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleRoot {
    pub root_id: String,
    pub tree_hash: ContentHash,
    pub path: String,
    pub file_sha256: ContentHash,
    pub symbol: String,
    pub start_line: u32,
    pub end_line: u32,
    pub span_sha256: ContentHash,
    pub mechanism_tags: BTreeSet<MechanismId>,
    pub severity: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateOracle {
    pub schema: String,
    pub unit_id: String,
    pub input_tree_hash: ContentHash,
    pub source_bundle_hash: ContentHash,
    pub roots: Vec<OracleRoot>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Score {
    pub schema: String,
    pub trial_id: String,
    pub unit_id: String,
    pub arm: Arm,
    pub replicate: u32,
    pub manifest_hash: ContentHash,
    pub candidate_hash: ContentHash,
    pub oracle_hash: ContentHash,
    pub input_tree_hash: ContentHash,
    pub protocol_version: String,
    pub mechanism_ontology_version: String,
    pub paired_configuration_hash: ContentHash,
    pub root_count: u32,
    pub detected_root_count: u32,
    pub candidate_count: u32,
    pub matched_candidate_count: u32,
    pub unmatched_candidate_count: u32,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionOutcome {
    Structured,
    Abstained,
    ParseFailure,
    ProtocolInvalid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrialCollection {
    pub schema: String,
    pub trial_id: String,
    pub manifest_hash: ContentHash,
    pub candidate_hash: ContentHash,
    pub outcome: CollectionOutcome,
    pub protocol_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryTrial {
    pub trial_id: String,
    pub unit_id: String,
    pub arm: Arm,
    pub replicate: u32,
    pub manifest_hash: ContentHash,
    pub paired_configuration_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrialInventory {
    pub schema: String,
    pub trials: Vec<InventoryTrial>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunSummary {
    pub schema: String,
    pub prepared_trials: u32,
    pub valid_collections: u32,
    pub protocol_invalid_trials: u32,
    pub collection_binding_invalid_trials: u32,
    pub missing_trials: u32,
    pub eligible_pairs: u32,
    pub excluded_pairs: u32,
    pub exclusion_reason_counts: BTreeMap<String, u32>,
    pub b1_detected_roots: u32,
    pub g3_proxy_detected_roots: u32,
    pub total_roots_per_arm: u32,
    pub g3_minus_b1_detected_roots: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FindingMatch {
    pub finding_local_id: String,
    pub matched_root_ids: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DetailedMatchRecord {
    pub schema: String,
    pub trial_id: String,
    pub manifest_hash: ContentHash,
    pub candidate_hash: ContentHash,
    pub oracle_hash: ContentHash,
    pub matches: Vec<FindingMatch>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdjudicationDisposition {
    MatchesKnownRoot,
    ValidNovelDefect,
    Duplicate,
    FalsePositive,
    Insufficient,
}

/// Blinded expert decision. It binds a candidate-local identifier only; it
/// deliberately has no arm, oracle root, score, model, or provider field.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlindAdjudication {
    pub schema: String,
    pub item_id: String,
    pub disposition: AdjudicationDisposition,
    pub rationale: String,
}

pub fn canonical_hash<T: Serialize>(value: &T) -> Result<ContentHash> {
    Ok(ContentHash::sha256(&canonical_json(value)?))
}

fn text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096
}
fn tags(tags: &BTreeSet<MechanismId>) -> bool {
    !tags.is_empty() && tags.len() <= MAX_MECHANISM_TAGS
}
fn safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.starts_with('/')
        && !value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}
fn range(start: u32, end: u32) -> bool {
    start > 0 && start <= MAX_SOURCE_LINE && end >= start && end <= MAX_SOURCE_LINE
}
fn exact_git_hash(value: &ContentHash) -> bool {
    value.as_str().len() == 44
        && value.as_str().starts_with("git:")
        && value.as_str()[4..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn exact_sha256(value: &ContentHash) -> bool {
    value.as_str().len() == 71
        && value.as_str().starts_with("sha256:")
        && value.as_str()[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn paired_configuration_hash(
    input_tree_hash: &ContentHash,
    source_inventory: &[SourceInventoryEntry],
    source_bundle_hash: &ContentHash,
    execution: &ExecutionConfig,
) -> Result<ContentHash> {
    canonical_hash(&(
        PROTOCOL_VERSION,
        MECHANISM_ONTOLOGY_VERSION,
        input_tree_hash,
        source_inventory,
        source_bundle_hash,
        execution,
    ))
}

impl ExecutionConfig {
    pub fn validate(&self) -> Result<()> {
        if !text(&self.provider)
            || !text(&self.model)
            || !text(&self.model_revision)
            || !text(&self.reasoning_effort)
            || !text(&self.prompt_template_version)
            || !text(&self.tool_policy_version)
            || !text(&self.runner_version)
            || self.inference_settings.len() > 64
            || self
                .inference_settings
                .iter()
                .any(|(key, value)| !text(key) || !text(value))
            || self
                .budget
                .wall_clock_seconds
                .is_some_and(|value| value == 0)
            || self
                .budget
                .session_limit
                .as_deref()
                .is_some_and(|value| !text(value))
        {
            return Err(BenchmarkError::Validation(
                "invalid execution configuration",
            ));
        }
        Ok(())
    }
}

impl TrialManifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema != TRIAL_MANIFEST_SCHEMA
            || !text(&self.trial_id)
            || !text(&self.unit_id)
            || self.replicate == 0
            || !exact_git_hash(&self.input_tree_hash)
            || !exact_sha256(&self.source_bundle_hash)
            || !exact_sha256(&self.paired_configuration_hash)
            || self.packet_hashes.len() > 4096
            || self.packet_hashes.iter().any(|hash| !exact_sha256(hash))
            || self.expected_packet_ids.len() > 4096
            || self.expected_packet_ids.iter().any(|id| !text(id))
            || self.source_inventory.is_empty()
            || self.source_inventory.len() > 4096
            || self.protocol_version != PROTOCOL_VERSION
            || self.mechanism_ontology_version != MECHANISM_ONTOLOGY_VERSION
            || self.limitations.len() > 64
            || self.limitations.iter().any(|value| !text(value))
        {
            return Err(BenchmarkError::Validation("invalid trial manifest"));
        }
        self.execution.validate()?;
        let mut paths = BTreeSet::new();
        for source in &self.source_inventory {
            if !safe_relative_path(&source.path)
                || !paths.insert(&source.path)
                || !exact_sha256(&source.content_hash)
                || source.line_count == 0
                || source.line_count > MAX_SOURCE_LINE
            {
                return Err(BenchmarkError::Validation(
                    "invalid manifest source inventory",
                ));
            }
        }
        match self.arm {
            Arm::B1FreeForm if !self.expected_packet_ids.is_empty() => {
                return Err(BenchmarkError::Validation(
                    "B1 must declare no obligation packets",
                ));
            }
            Arm::G3Proxy | Arm::FullReviewGraphen if self.expected_packet_ids.is_empty() => {
                return Err(BenchmarkError::Validation(
                    "G3 proxy requires exact packet set",
                ));
            }
            _ => {}
        }
        if paired_configuration_hash(
            &self.input_tree_hash,
            &self.source_inventory,
            &self.source_bundle_hash,
            &self.execution,
        )? != self.paired_configuration_hash
        {
            return Err(BenchmarkError::Validation(
                "paired configuration hash mismatch",
            ));
        }
        Ok(())
    }
}

impl CandidateOutput {
    pub fn validate(&self) -> Result<()> {
        if self.schema != CANDIDATE_OUTPUT_SCHEMA
            || !text(&self.trial_id)
            || self.findings.len() > 1024
            || self.obligation_results.len() > 4096
        {
            return Err(BenchmarkError::Validation("invalid candidate output"));
        }
        let mut ids = BTreeSet::new();
        for finding in &self.findings {
            if !text(&finding.local_id)
                || !ids.insert(&finding.local_id)
                || finding.locations.is_empty()
                || finding.locations.len() > MAX_CANDIDATE_LOCATIONS
                || finding.locations.iter().any(|loc| {
                    !safe_relative_path(&loc.path) || !range(loc.start_line, loc.end_line)
                })
                || !tags(&finding.mechanism_tags)
                || finding
                    .severity
                    .as_deref()
                    .is_some_and(|value| !matches!(value, "critical" | "high" | "medium" | "low"))
                || finding
                    .rationale
                    .as_deref()
                    .is_some_and(|value| value.len() > 16_384)
            {
                return Err(BenchmarkError::Validation("invalid candidate finding"));
            }
        }
        let mut packets = BTreeSet::new();
        for result in &self.obligation_results {
            let issue_present = result.disposition == "issue_present";
            if !text(&result.packet_id)
                || !packets.insert(&result.packet_id)
                || !matches!(
                    result.disposition.as_str(),
                    "issue_present"
                        | "issue_absent"
                        | "inconclusive"
                        | "not_applicable"
                        | "abstained"
                )
                || result.finding_local_ids.iter().any(|id| !ids.contains(id))
                || issue_present == result.finding_local_ids.is_empty()
            {
                return Err(BenchmarkError::Validation("invalid obligation result"));
            }
        }
        if !matches!(self.outcome, Outcome::Structured)
            && (!self.findings.is_empty() || !self.obligation_results.is_empty())
        {
            return Err(BenchmarkError::Validation(
                "non-structured output cannot contain findings or obligations",
            ));
        }
        Ok(())
    }
}

pub fn validate_candidate_against_manifest(
    manifest: &TrialManifest,
    candidate: &CandidateOutput,
) -> Result<()> {
    manifest.validate()?;
    candidate.validate()?;
    if candidate.trial_id != manifest.trial_id {
        return Err(BenchmarkError::TrialMismatch);
    }
    if matches!(candidate.outcome, Outcome::Structured) {
        let actual = candidate
            .obligation_results
            .iter()
            .map(|result| result.packet_id.clone())
            .collect::<BTreeSet<_>>();
        match manifest.arm {
            Arm::B1FreeForm if !actual.is_empty() => {
                return Err(BenchmarkError::ProtocolInvalid(
                    "B1 returned obligation results",
                ));
            }
            Arm::G3Proxy | Arm::FullReviewGraphen if actual != manifest.expected_packet_ids => {
                return Err(BenchmarkError::ProtocolInvalid(
                    "G3 proxy packet set differs from manifest",
                ));
            }
            _ => {}
        }
        if matches!(manifest.arm, Arm::G3Proxy | Arm::FullReviewGraphen) {
            let linked = candidate
                .obligation_results
                .iter()
                .flat_map(|result| result.finding_local_ids.iter())
                .collect::<BTreeSet<_>>();
            if candidate
                .findings
                .iter()
                .any(|finding| !linked.contains(&finding.local_id))
            {
                return Err(BenchmarkError::ProtocolInvalid(
                    "G3 finding is not linked to an obligation",
                ));
            }
        }
    }
    for finding in &candidate.findings {
        for location in &finding.locations {
            let Some(source) = manifest
                .source_inventory
                .iter()
                .find(|source| source.path == location.path)
            else {
                return Err(BenchmarkError::ProtocolInvalid(
                    "candidate location outside inventory",
                ));
            };
            if location.end_line > source.line_count {
                return Err(BenchmarkError::ProtocolInvalid(
                    "candidate location exceeds source lines",
                ));
            }
        }
    }
    Ok(())
}

impl PrivateOracle {
    pub fn validate(&self) -> Result<()> {
        if self.schema != ORACLE_SCHEMA
            || !text(&self.unit_id)
            || !exact_git_hash(&self.input_tree_hash)
            || !exact_sha256(&self.source_bundle_hash)
            || self.roots.len() > 1024
        {
            return Err(BenchmarkError::Validation("invalid private oracle"));
        }
        let mut ids = BTreeSet::new();
        for root in &self.roots {
            if !text(&root.root_id)
                || !ids.insert(&root.root_id)
                || !safe_relative_path(&root.path)
                || !text(&root.symbol)
                || !exact_git_hash(&root.tree_hash)
                || !exact_sha256(&root.file_sha256)
                || !exact_sha256(&root.span_sha256)
                || !range(root.start_line, root.end_line)
                || !tags(&root.mechanism_tags)
                || !matches!(
                    root.severity.as_str(),
                    "critical" | "high" | "medium" | "low"
                )
            {
                return Err(BenchmarkError::Validation("invalid oracle root"));
            }
        }
        Ok(())
    }
}

pub fn validate_oracle_against_manifest(
    manifest: &TrialManifest,
    oracle: &PrivateOracle,
) -> Result<()> {
    manifest.validate()?;
    oracle.validate()?;
    if oracle.unit_id != manifest.unit_id {
        return Err(BenchmarkError::UnitMismatch);
    }
    if oracle.input_tree_hash != manifest.input_tree_hash
        || oracle.source_bundle_hash != manifest.source_bundle_hash
    {
        return Err(BenchmarkError::Validation(
            "oracle snapshot binding differs from manifest",
        ));
    }
    for root in &oracle.roots {
        let Some(source) = manifest
            .source_inventory
            .iter()
            .find(|source| source.path == root.path)
        else {
            return Err(BenchmarkError::Validation(
                "oracle root path outside source inventory",
            ));
        };
        if root.tree_hash != manifest.input_tree_hash
            || root.file_sha256 != source.content_hash
            || root.end_line > source.line_count
        {
            return Err(BenchmarkError::Validation(
                "oracle root differs from manifest source inventory",
            ));
        }
    }
    Ok(())
}

pub fn parse_execution_config(bytes: &[u8]) -> Result<ExecutionConfig> {
    let value: ExecutionConfig = serde_json::from_slice(bytes)?;
    value.validate()?;
    Ok(value)
}
pub fn parse_manifest(bytes: &[u8]) -> Result<TrialManifest> {
    let value: TrialManifest = serde_json::from_slice(bytes)?;
    value.validate()?;
    Ok(value)
}
pub fn parse_candidate(bytes: &[u8]) -> Result<CandidateOutput> {
    let value: CandidateOutput = serde_json::from_slice(bytes)?;
    value.validate()?;
    Ok(value)
}
pub fn parse_oracle(bytes: &[u8]) -> Result<PrivateOracle> {
    let value: PrivateOracle = serde_json::from_slice(bytes)?;
    value.validate()?;
    Ok(value)
}
pub fn parse_score(bytes: &[u8]) -> Result<Score> {
    let value: Score = serde_json::from_slice(bytes)?;
    value.validate()?;
    Ok(value)
}
pub fn parse_collection(bytes: &[u8]) -> Result<TrialCollection> {
    let value: TrialCollection = serde_json::from_slice(bytes)?;
    value.validate()?;
    Ok(value)
}
pub fn parse_inventory(bytes: &[u8]) -> Result<TrialInventory> {
    let value: TrialInventory = serde_json::from_slice(bytes)?;
    value.validate()?;
    Ok(value)
}

impl Score {
    pub fn validate(&self) -> Result<()> {
        let counts_add = self
            .matched_candidate_count
            .checked_add(self.unmatched_candidate_count);
        if self.schema != SCORE_SCHEMA
            || !text(&self.trial_id)
            || !text(&self.unit_id)
            || self.replicate == 0
            || !exact_sha256(&self.manifest_hash)
            || !exact_sha256(&self.candidate_hash)
            || !exact_sha256(&self.oracle_hash)
            || !exact_git_hash(&self.input_tree_hash)
            || !exact_sha256(&self.paired_configuration_hash)
            || self.protocol_version != PROTOCOL_VERSION
            || self.mechanism_ontology_version != MECHANISM_ONTOLOGY_VERSION
            || self.detected_root_count > self.root_count
            || counts_add != Some(self.candidate_count)
            || (!matches!(self.outcome, Outcome::Structured) && self.candidate_count != 0)
        {
            return Err(BenchmarkError::Validation("invalid score"));
        }
        Ok(())
    }
}

impl TrialCollection {
    pub fn validate(&self) -> Result<()> {
        let protocol_invalid = matches!(self.outcome, CollectionOutcome::ProtocolInvalid);
        if self.schema != COLLECTION_SCHEMA
            || !text(&self.trial_id)
            || !exact_sha256(&self.manifest_hash)
            || !exact_sha256(&self.candidate_hash)
            || protocol_invalid != self.protocol_error.is_some()
            || self
                .protocol_error
                .as_deref()
                .is_some_and(|value| !text(value))
        {
            return Err(BenchmarkError::Validation("invalid trial collection"));
        }
        Ok(())
    }
}

pub fn collect_trial(
    manifest: &TrialManifest,
    candidate: &CandidateOutput,
) -> Result<TrialCollection> {
    manifest.validate()?;
    candidate.validate()?;
    let protocol = validate_candidate_against_manifest(manifest, candidate);
    let (outcome, protocol_error) = match protocol {
        Ok(()) => (
            match candidate.outcome {
                Outcome::Structured => CollectionOutcome::Structured,
                Outcome::Abstained => CollectionOutcome::Abstained,
                Outcome::ParseFailure => CollectionOutcome::ParseFailure,
            },
            None,
        ),
        Err(error) => (CollectionOutcome::ProtocolInvalid, Some(error.to_string())),
    };
    let collection = TrialCollection {
        schema: COLLECTION_SCHEMA.to_owned(),
        trial_id: manifest.trial_id.clone(),
        manifest_hash: canonical_hash(manifest)?,
        candidate_hash: canonical_hash(candidate)?,
        outcome,
        protocol_error,
    };
    collection.validate()?;
    Ok(collection)
}

impl TrialInventory {
    pub fn from_manifests(manifests: &[TrialManifest]) -> Result<Self> {
        let mut trials = Vec::with_capacity(manifests.len());
        for manifest in manifests {
            manifest.validate()?;
            trials.push(InventoryTrial {
                trial_id: manifest.trial_id.clone(),
                unit_id: manifest.unit_id.clone(),
                arm: manifest.arm.clone(),
                replicate: manifest.replicate,
                manifest_hash: canonical_hash(manifest)?,
                paired_configuration_hash: manifest.paired_configuration_hash.clone(),
            });
        }
        let inventory = Self {
            schema: TRIAL_INVENTORY_SCHEMA.to_owned(),
            trials,
        };
        inventory.validate()?;
        Ok(inventory)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != TRIAL_INVENTORY_SCHEMA || self.trials.is_empty() {
            return Err(BenchmarkError::Validation("invalid trial inventory"));
        }
        let mut trial_ids = BTreeSet::new();
        let mut arms = BTreeMap::<(&str, u32), BTreeMap<&str, &InventoryTrial>>::new();
        for trial in &self.trials {
            if !text(&trial.trial_id)
                || !text(&trial.unit_id)
                || trial.replicate == 0
                || !exact_sha256(&trial.manifest_hash)
                || !exact_sha256(&trial.paired_configuration_hash)
                || !trial_ids.insert(&trial.trial_id)
            {
                return Err(BenchmarkError::Validation("invalid inventory trial"));
            }
            let arm = match trial.arm {
                Arm::B1FreeForm => "b1",
                Arm::G3Proxy => "g3",
                Arm::FullReviewGraphen => "full",
            };
            if arms
                .entry((&trial.unit_id, trial.replicate))
                .or_default()
                .insert(arm, trial)
                .is_some()
            {
                return Err(BenchmarkError::Validation("duplicate inventory arm"));
            }
        }
        for pair in arms.values() {
            let (Some(b1), Some(g3)) = (pair.get("b1"), pair.get("g3")) else {
                return Err(BenchmarkError::Validation("incomplete inventory pair"));
            };
            if b1.paired_configuration_hash != g3.paired_configuration_hash {
                return Err(BenchmarkError::Validation(
                    "inventory pair configuration mismatch",
                ));
            }
        }
        Ok(())
    }
}

fn overlaps(a: &Location, b: &OracleRoot) -> bool {
    a.path == b.path && a.start_line <= b.end_line && b.start_line <= a.end_line
}

pub fn detailed_match_record(
    manifest: &TrialManifest,
    candidate: &CandidateOutput,
    oracle: &PrivateOracle,
) -> Result<DetailedMatchRecord> {
    validate_candidate_against_manifest(manifest, candidate)?;
    validate_oracle_against_manifest(manifest, oracle)?;
    let matches = candidate
        .findings
        .iter()
        .map(|finding| FindingMatch {
            finding_local_id: finding.local_id.clone(),
            matched_root_ids: oracle
                .roots
                .iter()
                .filter(|root| {
                    finding
                        .mechanism_tags
                        .intersection(&root.mechanism_tags)
                        .next()
                        .is_some()
                        && finding
                            .locations
                            .iter()
                            .any(|location| overlaps(location, root))
                })
                .map(|root| root.root_id.clone())
                .collect(),
        })
        .collect();
    Ok(DetailedMatchRecord {
        schema: "reviewgraphen.benchmark.detailed_match.v1".to_owned(),
        trial_id: manifest.trial_id.clone(),
        manifest_hash: canonical_hash(manifest)?,
        candidate_hash: canonical_hash(candidate)?,
        oracle_hash: canonical_hash(oracle)?,
        matches,
    })
}

pub fn score(
    manifest: &TrialManifest,
    candidate: &CandidateOutput,
    oracle: &PrivateOracle,
) -> Result<Score> {
    let detail = detailed_match_record(manifest, candidate, oracle)?;
    let mut detected = BTreeSet::new();
    let mut matched = 0_u32;
    for finding_match in &detail.matches {
        if !finding_match.matched_root_ids.is_empty() {
            matched = matched
                .checked_add(1)
                .ok_or(BenchmarkError::Validation("matched candidate overflow"))?;
            detected.extend(finding_match.matched_root_ids.iter().cloned());
        }
    }
    let candidate_count = u32::try_from(candidate.findings.len())
        .map_err(|_| BenchmarkError::Validation("candidate count"))?;
    let root_count =
        u32::try_from(oracle.roots.len()).map_err(|_| BenchmarkError::Validation("root count"))?;
    let detected_root_count = u32::try_from(detected.len())
        .map_err(|_| BenchmarkError::Validation("detected root count"))?;
    let unmatched_candidate_count = candidate_count
        .checked_sub(matched)
        .ok_or(BenchmarkError::Validation("unmatched candidate underflow"))?;
    let score = Score {
        schema: SCORE_SCHEMA.to_owned(),
        trial_id: manifest.trial_id.clone(),
        unit_id: manifest.unit_id.clone(),
        arm: manifest.arm.clone(),
        replicate: manifest.replicate,
        manifest_hash: canonical_hash(manifest)?,
        candidate_hash: canonical_hash(candidate)?,
        oracle_hash: canonical_hash(oracle)?,
        input_tree_hash: manifest.input_tree_hash.clone(),
        protocol_version: manifest.protocol_version.clone(),
        mechanism_ontology_version: manifest.mechanism_ontology_version.clone(),
        paired_configuration_hash: manifest.paired_configuration_hash.clone(),
        root_count,
        detected_root_count,
        candidate_count,
        matched_candidate_count: matched,
        unmatched_candidate_count,
        outcome: candidate.outcome.clone(),
    };
    score.validate()?;
    Ok(score)
}

fn checked_increment(value: &mut u32, label: &'static str) -> Result<()> {
    *value = value
        .checked_add(1)
        .ok_or(BenchmarkError::Validation(label))?;
    Ok(())
}

fn add_reason(
    reasons: &mut BTreeMap<(String, u32), BTreeSet<String>>,
    trial: &InventoryTrial,
    reason: &str,
) {
    reasons
        .entry((trial.unit_id.clone(), trial.replicate))
        .or_default()
        .insert(reason.to_owned());
}

pub fn summarize_run(
    inventory: &TrialInventory,
    collections: &[TrialCollection],
    scores: &[Score],
) -> Result<RunSummary> {
    inventory.validate()?;
    let expected = inventory
        .trials
        .iter()
        .map(|trial| (trial.trial_id.as_str(), trial))
        .collect::<BTreeMap<_, _>>();
    let mut collection_map = BTreeMap::new();
    for collection in collections {
        collection.validate()?;
        if !expected.contains_key(collection.trial_id.as_str())
            || collection_map
                .insert(collection.trial_id.as_str(), collection)
                .is_some()
        {
            return Err(BenchmarkError::Validation(
                "unknown or duplicate trial collection",
            ));
        }
    }
    let mut score_map = BTreeMap::new();
    for score in scores {
        score.validate()?;
        if !expected.contains_key(score.trial_id.as_str())
            || score_map.insert(score.trial_id.as_str(), score).is_some()
        {
            return Err(BenchmarkError::Validation(
                "unknown or duplicate trial score",
            ));
        }
    }

    let mut summary = RunSummary {
        schema: RUN_SUMMARY_SCHEMA.to_owned(),
        prepared_trials: u32::try_from(inventory.trials.len())
            .map_err(|_| BenchmarkError::Validation("prepared trial count"))?,
        valid_collections: 0,
        protocol_invalid_trials: 0,
        collection_binding_invalid_trials: 0,
        missing_trials: 0,
        eligible_pairs: 0,
        excluded_pairs: 0,
        exclusion_reason_counts: BTreeMap::new(),
        b1_detected_roots: 0,
        g3_proxy_detected_roots: 0,
        total_roots_per_arm: 0,
        g3_minus_b1_detected_roots: 0,
    };
    let mut reasons = BTreeMap::<(String, u32), BTreeSet<String>>::new();
    let mut pairs = BTreeMap::<(String, u32), BTreeMap<&str, &Score>>::new();

    for trial in &inventory.trials {
        let Some(collection) = collection_map.get(trial.trial_id.as_str()) else {
            checked_increment(&mut summary.missing_trials, "missing trial count overflow")?;
            add_reason(&mut reasons, trial, "missing_collection");
            continue;
        };
        if collection.manifest_hash != trial.manifest_hash {
            checked_increment(
                &mut summary.collection_binding_invalid_trials,
                "collection binding invalid count overflow",
            )?;
            add_reason(&mut reasons, trial, "collection_manifest_mismatch");
            continue;
        }
        if matches!(collection.outcome, CollectionOutcome::ProtocolInvalid) {
            checked_increment(
                &mut summary.protocol_invalid_trials,
                "protocol invalid count overflow",
            )?;
            add_reason(&mut reasons, trial, "protocol_invalid");
            if score_map.contains_key(trial.trial_id.as_str()) {
                add_reason(&mut reasons, trial, "score_for_ineligible_trial");
            }
            continue;
        }
        checked_increment(
            &mut summary.valid_collections,
            "valid collection count overflow",
        )?;
        let Some(score) = score_map.get(trial.trial_id.as_str()) else {
            add_reason(&mut reasons, trial, "missing_score");
            continue;
        };
        let expected_outcome = match collection.outcome {
            CollectionOutcome::Structured => Outcome::Structured,
            CollectionOutcome::Abstained => Outcome::Abstained,
            CollectionOutcome::ParseFailure => Outcome::ParseFailure,
            CollectionOutcome::ProtocolInvalid => unreachable!("handled above"),
        };
        if score.unit_id != trial.unit_id
            || score.arm != trial.arm
            || score.replicate != trial.replicate
            || score.manifest_hash != trial.manifest_hash
            || score.candidate_hash != collection.candidate_hash
            || score.paired_configuration_hash != trial.paired_configuration_hash
            || score.outcome != expected_outcome
        {
            add_reason(&mut reasons, trial, "score_binding_mismatch");
            continue;
        }
        let arm = match trial.arm {
            Arm::B1FreeForm => "b1",
            Arm::G3Proxy => "g3",
            Arm::FullReviewGraphen => "full",
        };
        pairs
            .entry((trial.unit_id.clone(), trial.replicate))
            .or_default()
            .insert(arm, score);
    }

    let inventory_pairs = inventory
        .trials
        .iter()
        .map(|trial| (trial.unit_id.clone(), trial.replicate))
        .collect::<BTreeSet<_>>();
    for key in inventory_pairs {
        if !reasons.contains_key(&key) {
            let pair = pairs.get(&key);
            let compatible = pair
                .and_then(|pair| Some((pair.get("b1")?, pair.get("g3")?)))
                .is_some_and(|(b1, g3)| {
                    b1.root_count == g3.root_count
                        && b1.oracle_hash == g3.oracle_hash
                        && b1.input_tree_hash == g3.input_tree_hash
                        && b1.protocol_version == g3.protocol_version
                        && b1.mechanism_ontology_version == g3.mechanism_ontology_version
                        && b1.paired_configuration_hash == g3.paired_configuration_hash
                });
            if !compatible {
                reasons
                    .entry(key.clone())
                    .or_default()
                    .insert("incompatible_pair_scores".to_owned());
            }
        }
        if let Some(pair_reasons) = reasons.get(&key) {
            checked_increment(&mut summary.excluded_pairs, "excluded pair count overflow")?;
            for reason in pair_reasons {
                let count = summary
                    .exclusion_reason_counts
                    .entry(reason.clone())
                    .or_insert(0);
                checked_increment(count, "exclusion reason count overflow")?;
            }
            continue;
        }
        let pair = pairs
            .get(&key)
            .ok_or(BenchmarkError::Validation("eligible pair disappeared"))?;
        let b1 = pair
            .get("b1")
            .ok_or(BenchmarkError::Validation("eligible B1 score disappeared"))?;
        let g3 = pair
            .get("g3")
            .ok_or(BenchmarkError::Validation("eligible G3 score disappeared"))?;
        checked_increment(&mut summary.eligible_pairs, "eligible pair count overflow")?;
        summary.b1_detected_roots = summary
            .b1_detected_roots
            .checked_add(b1.detected_root_count)
            .ok_or(BenchmarkError::Validation("B1 detected roots overflow"))?;
        summary.g3_proxy_detected_roots = summary
            .g3_proxy_detected_roots
            .checked_add(g3.detected_root_count)
            .ok_or(BenchmarkError::Validation("G3 detected roots overflow"))?;
        summary.total_roots_per_arm = summary
            .total_roots_per_arm
            .checked_add(b1.root_count)
            .ok_or(BenchmarkError::Validation("root denominator overflow"))?;
    }
    summary.g3_minus_b1_detected_roots = i64::from(summary.g3_proxy_detected_roots)
        .checked_sub(i64::from(summary.b1_detected_roots))
        .ok_or(BenchmarkError::Validation("paired delta overflow"))?;
    let classified_trials = summary
        .valid_collections
        .checked_add(summary.protocol_invalid_trials)
        .and_then(|value| value.checked_add(summary.collection_binding_invalid_trials))
        .and_then(|value| value.checked_add(summary.missing_trials))
        .ok_or(BenchmarkError::Validation("trial status count overflow"))?;
    if classified_trials != summary.prepared_trials {
        return Err(BenchmarkError::Validation(
            "trial status counts do not partition prepared trials",
        ));
    }
    Ok(summary)
}

/// Candidate-only DTO for external adjudication. Candidate-local identifiers,
/// rationale, severity, arm, model, scores, and oracle data remain private.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlindFinding {
    pub locations: Vec<Location>,
    pub mechanism_tags: BTreeSet<MechanismId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlindAdjudicationItem {
    pub item_id: String,
    pub finding: BlindFinding,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateAdjudicationReconciliation {
    pub schema: String,
    pub item_id: String,
    pub trial_id: String,
    pub finding_local_id: String,
}

fn valid_adjudication_item_id(value: &str) -> bool {
    value.len() == 75
        && value.starts_with("adj:sha256:")
        && value[11..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_reconciliation(reconciliation: &[PrivateAdjudicationReconciliation]) -> Result<()> {
    if reconciliation.len() > 1024 {
        return Err(BenchmarkError::Validation(
            "adjudication reconciliation cap",
        ));
    }
    let mut item_ids = BTreeSet::new();
    let mut findings = BTreeSet::new();
    for mapping in reconciliation {
        if mapping.schema != "reviewgraphen.benchmark.adjudication_reconciliation.v1"
            || !valid_adjudication_item_id(&mapping.item_id)
            || !text(&mapping.trial_id)
            || !text(&mapping.finding_local_id)
            || !item_ids.insert(&mapping.item_id)
            || !findings.insert((&mapping.trial_id, &mapping.finding_local_id))
        {
            return Err(BenchmarkError::Validation(
                "invalid adjudication reconciliation",
            ));
        }
    }
    Ok(())
}

pub fn blind_adjudication_export(
    manifest: &TrialManifest,
    candidate: &CandidateOutput,
    oracle: &PrivateOracle,
) -> Result<(
    Vec<BlindAdjudicationItem>,
    Vec<PrivateAdjudicationReconciliation>,
)> {
    let detail = detailed_match_record(manifest, candidate, oracle)?;
    if detail.manifest_hash != canonical_hash(manifest)?
        || detail.candidate_hash != canonical_hash(candidate)?
        || detail.oracle_hash != canonical_hash(oracle)?
    {
        return Err(BenchmarkError::Validation(
            "detailed match hash binding mismatch",
        ));
    }
    let by_id = candidate
        .findings
        .iter()
        .map(|finding| (finding.local_id.as_str(), finding))
        .collect::<BTreeMap<_, _>>();
    let mut match_ids = BTreeSet::new();
    if detail.matches.len() != candidate.findings.len()
        || detail.matches.iter().any(|entry| {
            !by_id.contains_key(entry.finding_local_id.as_str())
                || !match_ids.insert(&entry.finding_local_id)
        })
    {
        return Err(BenchmarkError::Validation(
            "incomplete detailed match record",
        ));
    }
    let candidate_hash = canonical_hash(candidate)?;
    let mut public = Vec::new();
    let mut private = Vec::new();
    for entry in &detail.matches {
        if entry.matched_root_ids.len() == 1 {
            continue;
        }
        let finding = by_id
            .get(entry.finding_local_id.as_str())
            .ok_or(BenchmarkError::Validation("unknown detailed match finding"))?;
        let item_id = format!(
            "adj:{}",
            canonical_hash(&(candidate_hash.as_str(), finding.local_id.as_str()))?
        );
        public.push(BlindAdjudicationItem {
            item_id: item_id.clone(),
            finding: BlindFinding {
                locations: finding.locations.clone(),
                mechanism_tags: finding.mechanism_tags.clone(),
            },
        });
        private.push(PrivateAdjudicationReconciliation {
            schema: "reviewgraphen.benchmark.adjudication_reconciliation.v1".to_owned(),
            item_id,
            trial_id: candidate.trial_id.clone(),
            finding_local_id: finding.local_id.clone(),
        });
    }
    validate_reconciliation(&private)?;
    Ok((public, private))
}

/// Strictly admits adjudicator output against the candidate-only export. It
/// cannot change deterministic injected-root scoring or confer review authority.
pub fn import_blind_adjudication(
    reconciliation: &[PrivateAdjudicationReconciliation],
    bytes: &[u8],
) -> Result<(BlindAdjudication, PrivateAdjudicationReconciliation)> {
    validate_reconciliation(reconciliation)?;
    let value: BlindAdjudication = serde_json::from_slice(bytes)?;
    if value.schema != "reviewgraphen.benchmark.blind_adjudication.v1"
        || !valid_adjudication_item_id(&value.item_id)
        || !text(&value.rationale)
        || value.rationale.len() > 16_384
    {
        return Err(BenchmarkError::Validation("invalid blind adjudication"));
    }
    let mapping = reconciliation
        .iter()
        .find(|mapping| mapping.item_id == value.item_id)
        .ok_or(BenchmarkError::Validation(
            "unknown blind adjudication item",
        ))?;
    Ok((value, mapping.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(value: &str) -> ContentHash {
        ContentHash::parse(value).expect("content hash")
    }

    fn execution() -> ExecutionConfig {
        ExecutionConfig {
            provider: "codex".into(),
            model: "gpt-test".into(),
            model_revision: "unknown".into(),
            reasoning_effort: "high".into(),
            prompt_template_version: "m7-prompt.v2".into(),
            tool_policy_version: "m7-tools.v1".into(),
            runner_version: "reviewgraphen-benchmark.v1".into(),
            inference_settings: BTreeMap::from([("temperature".into(), "unknown".into())]),
            budget: DeclaredBudget {
                wall_clock_seconds: None,
                session_limit: Some("unknown".into()),
            },
        }
    }

    fn manifest(arm: Arm) -> TrialManifest {
        let input_tree_hash = hash("git:1234567890123456789012345678901234567890");
        let source_bundle_hash =
            hash("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let source_inventory = vec![SourceInventoryEntry {
            path: "src/a.rs".into(),
            content_hash: hash(
                "sha256:1234567890123456789012345678901234567890123456789012345678901234",
            ),
            line_count: 10,
        }];
        let execution = execution();
        let paired_configuration_hash = paired_configuration_hash(
            &input_tree_hash,
            &source_inventory,
            &source_bundle_hash,
            &execution,
        )
        .expect("paired configuration hash");
        TrialManifest {
            schema: TRIAL_MANIFEST_SCHEMA.into(),
            trial_id: match arm {
                Arm::B1FreeForm => "trial:b1".into(),
                Arm::G3Proxy => "trial:g3".into(),
                Arm::FullReviewGraphen => "trial:full".into(),
            },
            unit_id: "unit:a".into(),
            arm: arm.clone(),
            replicate: 1,
            input_tree_hash,
            packet_hashes: BTreeSet::new(),
            expected_packet_ids: matches!(arm, Arm::G3Proxy | Arm::FullReviewGraphen)
                .then(|| BTreeSet::from(["packet:test".into()]))
                .unwrap_or_default(),
            source_inventory,
            source_bundle_hash,
            protocol_version: PROTOCOL_VERSION.into(),
            mechanism_ontology_version: MECHANISM_ONTOLOGY_VERSION.into(),
            paired_configuration_hash,
            limitations: BTreeSet::new(),
            execution,
        }
    }

    fn oracle() -> PrivateOracle {
        PrivateOracle {
            schema: ORACLE_SCHEMA.into(),
            unit_id: "unit:a".into(),
            input_tree_hash: hash("git:1234567890123456789012345678901234567890"),
            source_bundle_hash: hash(
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            roots: vec![OracleRoot {
                root_id: "root:a".into(),
                tree_hash: hash("git:1234567890123456789012345678901234567890"),
                path: "src/a.rs".into(),
                file_sha256: hash(
                    "sha256:1234567890123456789012345678901234567890123456789012345678901234",
                ),
                symbol: "symbol:a".into(),
                start_line: 5,
                end_line: 7,
                span_sha256: hash(
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                ),
                mechanism_tags: BTreeSet::from([MechanismId::CheckWriteGap]),
                severity: "high".into(),
            }],
        }
    }

    fn candidate(arm: Arm) -> CandidateOutput {
        let finding = CandidateFinding {
            local_id: "f1".into(),
            locations: vec![Location {
                path: "src/a.rs".into(),
                start_line: 6,
                end_line: 6,
            }],
            mechanism_tags: BTreeSet::from([MechanismId::CheckWriteGap]),
            severity: Some("high".into()),
            rationale: Some("private rationale".into()),
        };
        CandidateOutput {
            schema: CANDIDATE_OUTPUT_SCHEMA.into(),
            trial_id: match arm {
                Arm::B1FreeForm => "trial:b1".into(),
                Arm::G3Proxy => "trial:g3".into(),
                Arm::FullReviewGraphen => "trial:full".into(),
            },
            outcome: Outcome::Structured,
            findings: vec![finding],
            obligation_results: matches!(arm, Arm::G3Proxy | Arm::FullReviewGraphen)
                .then(|| {
                    vec![ObligationResult {
                        packet_id: "packet:test".into(),
                        disposition: "issue_present".into(),
                        finding_local_ids: BTreeSet::from(["f1".into()]),
                    }]
                })
                .unwrap_or_default(),
        }
    }

    #[test]
    fn deterministic_matching_and_oracle_snapshot_binding() {
        let manifest = manifest(Arm::B1FreeForm);
        let candidate = candidate(Arm::B1FreeForm);
        let scored = score(&manifest, &candidate, &oracle()).expect("score");
        assert_eq!((scored.root_count, scored.detected_root_count), (1, 1));
        let mut wrong = oracle();
        wrong.input_tree_hash = hash("git:0000000000000000000000000000000000000000");
        wrong.roots.clear();
        assert!(score(&manifest, &candidate, &wrong).is_err());
    }

    #[test]
    fn candidate_cannot_claim_protocol_invalid_and_collection_owns_status() {
        assert!(parse_candidate(br#"{"schema":"reviewgraphen.benchmark.candidate_output.v1","trial_id":"trial:g3","outcome":"protocol_invalid","findings":[],"obligation_results":[]}"#).is_err());
        let mut incomplete = candidate(Arm::G3Proxy);
        incomplete.obligation_results.clear();
        let collected = collect_trial(&manifest(Arm::G3Proxy), &incomplete).expect("collection");
        assert!(matches!(
            collected.outcome,
            CollectionOutcome::ProtocolInvalid
        ));
        assert!(collected.protocol_error.is_some());
    }

    #[test]
    fn abstention_is_protocol_valid_and_scores_zero_against_denominator() {
        let candidate = CandidateOutput {
            schema: CANDIDATE_OUTPUT_SCHEMA.into(),
            trial_id: "trial:g3".into(),
            outcome: Outcome::Abstained,
            findings: Vec::new(),
            obligation_results: Vec::new(),
        };
        let manifest = manifest(Arm::G3Proxy);
        let collection = collect_trial(&manifest, &candidate).expect("collection");
        assert!(matches!(collection.outcome, CollectionOutcome::Abstained));
        let score = score(&manifest, &candidate, &oracle()).expect("score");
        assert_eq!(
            (
                score.root_count,
                score.detected_root_count,
                score.candidate_count
            ),
            (1, 0, 0)
        );
    }

    #[test]
    fn obligation_disposition_and_links_are_consistent() {
        let mut invalid = candidate(Arm::G3Proxy);
        invalid.obligation_results[0].disposition = "issue_absent".into();
        assert!(invalid.validate().is_err());
        let mut unlinked = candidate(Arm::G3Proxy);
        unlinked.obligation_results[0].finding_local_ids.clear();
        unlinked.obligation_results[0].disposition = "issue_absent".into();
        assert!(matches!(
            validate_candidate_against_manifest(&manifest(Arm::G3Proxy), &unlinked),
            Err(BenchmarkError::ProtocolInvalid(_))
        ));
    }

    #[test]
    fn inventory_summary_counts_missing_and_eligible_pairs() {
        let b1_manifest = manifest(Arm::B1FreeForm);
        let g3_manifest = manifest(Arm::G3Proxy);
        let inventory = TrialInventory::from_manifests(&[b1_manifest.clone(), g3_manifest.clone()])
            .expect("inventory");
        let b1_candidate = candidate(Arm::B1FreeForm);
        let b1_collection = collect_trial(&b1_manifest, &b1_candidate).expect("B1 collection");
        let b1_score = score(&b1_manifest, &b1_candidate, &oracle()).expect("B1 score");
        let incomplete = summarize_run(
            &inventory,
            std::slice::from_ref(&b1_collection),
            std::slice::from_ref(&b1_score),
        )
        .expect("incomplete summary");
        assert_eq!(
            (
                incomplete.prepared_trials,
                incomplete.missing_trials,
                incomplete.excluded_pairs
            ),
            (2, 1, 1)
        );
        assert_eq!(incomplete.exclusion_reason_counts["missing_collection"], 1);

        let g3_candidate = candidate(Arm::G3Proxy);
        let g3_collection = collect_trial(&g3_manifest, &g3_candidate).expect("G3 collection");
        let g3_score = score(&g3_manifest, &g3_candidate, &oracle()).expect("G3 score");
        let mut binding_invalid_collection = b1_collection.clone();
        binding_invalid_collection.manifest_hash =
            hash("sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc");
        let binding_invalid = summarize_run(
            &inventory,
            &[binding_invalid_collection, g3_collection.clone()],
            &[b1_score.clone(), g3_score.clone()],
        )
        .expect("binding-invalid summary");
        assert_eq!(
            (
                binding_invalid.prepared_trials,
                binding_invalid.valid_collections,
                binding_invalid.collection_binding_invalid_trials,
                binding_invalid.protocol_invalid_trials,
                binding_invalid.missing_trials,
            ),
            (2, 1, 1, 0, 0)
        );

        let complete = summarize_run(
            &inventory,
            &[b1_collection, g3_collection],
            &[b1_score, g3_score],
        )
        .expect("complete summary");
        assert_eq!((complete.eligible_pairs, complete.excluded_pairs), (1, 0));
    }

    #[test]
    fn blind_export_recomputes_matches_and_exports_only_unmatched_or_ambiguous() {
        let manifest = manifest(Arm::B1FreeForm);
        let mut candidate = candidate(Arm::B1FreeForm);
        candidate.findings.push(CandidateFinding {
            local_id: "f2".into(),
            locations: vec![Location {
                path: "src/a.rs".into(),
                start_line: 8,
                end_line: 8,
            }],
            mechanism_tags: BTreeSet::from([MechanismId::CheckWriteGap]),
            severity: Some("low".into()),
            rationale: Some("unmatched private rationale".into()),
        });
        candidate.findings.push(CandidateFinding {
            local_id: "f3".into(),
            locations: vec![Location {
                path: "src/a.rs".into(),
                start_line: 6,
                end_line: 9,
            }],
            mechanism_tags: BTreeSet::from([MechanismId::CheckWriteGap]),
            severity: Some("medium".into()),
            rationale: Some("ambiguous private rationale".into()),
        });
        let mut oracle = oracle();
        let mut second = oracle.roots[0].clone();
        second.root_id = "root:b".into();
        second.start_line = 9;
        second.end_line = 9;
        oracle.roots.push(second);

        let (public, private) = blind_adjudication_export(&manifest, &candidate, &oracle)
            .expect("verified blind export");
        assert_eq!((public.len(), private.len()), (2, 2));
        assert_eq!(
            private
                .iter()
                .map(|mapping| mapping.finding_local_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["f2", "f3"])
        );
        assert!(
            !private
                .iter()
                .any(|mapping| mapping.finding_local_id == "f1")
        );
        let rendered = serde_json::to_value(&public).expect("public JSON");
        for item in rendered.as_array().expect("blind items") {
            let finding = item["finding"].as_object().expect("blind finding object");
            assert_eq!(
                finding.keys().map(String::as_str).collect::<BTreeSet<_>>(),
                BTreeSet::from(["locations", "mechanism_tags"])
            );
        }
        let mut duplicate = private.clone();
        duplicate.push(private[0].clone());
        let payload = serde_json::json!({
            "schema":"reviewgraphen.benchmark.blind_adjudication.v1",
            "item_id":public[0].item_id,
            "disposition":"valid_novel_defect",
            "rationale":"reproduced"
        });
        assert!(
            import_blind_adjudication(&duplicate, &serde_json::to_vec(&payload).unwrap()).is_err()
        );
    }
}
