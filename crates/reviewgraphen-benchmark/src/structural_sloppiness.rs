//! Experimental, non-authoritative structural-sloppiness report contract.
//!
//! This module classifies the exact, bounded consumer predicate documented in
//! the fixed benchmark contract. It neither infers a source defect nor grants
//! review authority.

use reviewgraphen_core::{
    Artifact, CapabilityDeclaration, CapabilityState, ContentHash, Location, ProgramSpace,
    Relation, StableId, canonical_json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const REPORT_SCHEMA: &str = "reviewgraphen.benchmark.structural_sloppiness_report.v1";
pub const ANALYZER_ID: &str = "reviewgraphen.benchmark.structural_sloppiness";
pub const ANALYZER_VERSION: &str = "1";
pub const CONTRACT_ID: &str = "changed_input_consumer_bridge_mismatch@1";
pub const PRODUCER_PROVENANCE: &str = "reviewgraphen.ingest.git.changed_structure.v1";
pub const FLAT_PROJECTION_SCHEMA: &str =
    "reviewgraphen.benchmark.structural_sloppiness_flat_projection.v1";

const CONTRACT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/structural-sloppiness-v1/CONTRACT.md"
));
const REPORT_INFORMATION_LOSS: &str = "report_projection_omits_unrelated_program_facts";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExerciseStatus {
    Exercised,
    NotExercised,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    NonPublicFunction,
    NotFreeFunction,
    ProducerProvenanceMismatch,
    ChangeProvenanceMismatch,
    ChangeShapeInvalid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzerDescriptor {
    pub id: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContractBinding {
    pub id: String,
    pub hash: ContentHash,
    pub producer_provenance: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputBinding {
    pub program_space_hash: ContentHash,
    pub snapshot_id: StableId,
    pub base_revision: String,
    pub target_revision: String,
    pub profile: String,
    pub rule_set_hash: ContentHash,
    pub extractor_set_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityBoundary {
    pub classification: String,
    pub accepted: bool,
    pub verified: bool,
    pub human_accepted: bool,
    pub sign_off: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityObservation {
    pub capability: String,
    pub state: CapabilityState,
    pub source_ids: Vec<StableId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeSummary {
    pub status: ExerciseStatus,
    pub eligible_count: u64,
    pub excluded_count: u64,
    pub excluded_by_reason: BTreeMap<ExclusionReason, u64>,
    pub capabilities: Vec<CapabilityObservation>,
    pub limitation_ids: Vec<StableId>,
    pub information_loss: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceLocation {
    pub path: String,
    pub start_line: Option<u64>,
    pub end_line: Option<u64>,
    pub start_column: Option<u64>,
    pub end_column: Option<u64>,
    pub symbol_id: Option<StableId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedGate {
    pub id: String,
    pub symbol_id: StableId,
    pub symbol_location: Option<SourceLocation>,
    pub change_id: StableId,
    pub changed_by_relation_id: StableId,
    pub producer_source_ids: Vec<StableId>,
    pub own_changed: bool,
    pub marked_container_ids: Vec<StableId>,
    pub matching_contains_relation_ids: Vec<StableId>,
    pub consumer_observed_changed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateClaim {
    pub id: String,
    pub kind: String,
    pub observation_ids: Vec<String>,
    pub source_ids: Vec<StableId>,
    pub statement: String,
    pub disposition: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisObstruction {
    pub id: String,
    pub kind: String,
    pub source_ids: Vec<StableId>,
    pub limitation_ids: Vec<StableId>,
    pub statement: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisReport {
    pub schema: String,
    pub analyzer: AnalyzerDescriptor,
    pub contract: ContractBinding,
    pub input: InputBinding,
    pub authority: AuthorityBoundary,
    pub scope: ScopeSummary,
    pub observed_facts: Vec<ObservedGate>,
    pub candidate_claims: Vec<CandidateClaim>,
    pub obstructions: Vec<AnalysisObstruction>,
}

#[derive(Debug, Error)]
pub enum StructuralSloppinessError {
    #[error("report does not match the supplied ProgramSpace: {0}")]
    ReportMismatch(&'static str),
    #[error("structural-sloppiness relation inventory count overflow")]
    CountOverflow,
    #[error("structural-sloppiness canonicalization failed: {0}")]
    Canonical(#[from] reviewgraphen_core::DomainError),
    #[error("structural-sloppiness JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StructuralSloppinessError>;

/// Analyze only accepted facts already admitted to `program`.
pub fn analyze(program: &ProgramSpace) -> Result<AnalysisReport> {
    let artifacts = program
        .artifacts()
        .iter()
        .map(|artifact| (artifact.id.clone(), artifact))
        .collect::<BTreeMap<_, _>>();
    let incoming_contains = incoming_contains(program);
    let mut exclusions = BTreeMap::new();
    let mut observed_facts = Vec::new();

    for relation in program
        .relations()
        .iter()
        .filter(|relation| relation.kind == "changed_by")
    {
        for change_id in &relation.target_ids {
            let Some(symbol) = artifacts.get(&relation.source_id).copied() else {
                // ProgramSpace admission prevents this. Keep the analysis total if a
                // future core variant admits relation-only sources.
                increment_exclusion(&mut exclusions, ExclusionReason::NotFreeFunction)?;
                continue;
            };
            let Some(change) = artifacts.get(change_id).copied() else {
                increment_exclusion(&mut exclusions, ExclusionReason::ChangeShapeInvalid)?;
                continue;
            };

            if relation.provenance.extraction_method() != PRODUCER_PROVENANCE {
                increment_exclusion(&mut exclusions, ExclusionReason::ProducerProvenanceMismatch)?;
                continue;
            }
            if symbol.kind != "function" {
                increment_exclusion(&mut exclusions, ExclusionReason::NotFreeFunction)?;
                continue;
            }
            if !attribute_bool(&symbol.attributes, "public") {
                increment_exclusion(&mut exclusions, ExclusionReason::NonPublicFunction)?;
                continue;
            }
            if change.provenance.extraction_method() != PRODUCER_PROVENANCE {
                increment_exclusion(&mut exclusions, ExclusionReason::ChangeProvenanceMismatch)?;
                continue;
            }
            if !valid_change_shape(change) {
                increment_exclusion(&mut exclusions, ExclusionReason::ChangeShapeInvalid)?;
                continue;
            }

            observed_facts.push(observe_gate(
                relation,
                symbol,
                change,
                incoming_contains.get(&symbol.id),
                &artifacts,
            )?);
        }
    }

    observed_facts.sort_by(|left, right| left.id.cmp(&right.id));
    let mut candidate_claims = observed_facts
        .iter()
        .filter(|observation| !observation.consumer_observed_changed)
        .map(candidate_for)
        .collect::<Result<Vec<_>>>()?;
    candidate_claims.sort_by(|left, right| left.id.cmp(&right.id));
    let excluded_count = exclusions.values().try_fold(0_u64, |total, count| {
        total
            .checked_add(*count)
            .ok_or(StructuralSloppinessError::CountOverflow)
    })?;
    let eligible_count = u64::try_from(observed_facts.len())
        .map_err(|_| StructuralSloppinessError::CountOverflow)?;

    Ok(AnalysisReport {
        schema: REPORT_SCHEMA.to_owned(),
        analyzer: AnalyzerDescriptor {
            id: ANALYZER_ID.to_owned(),
            version: ANALYZER_VERSION.to_owned(),
        },
        contract: ContractBinding {
            id: CONTRACT_ID.to_owned(),
            hash: ContentHash::sha256(CONTRACT_BYTES),
            producer_provenance: PRODUCER_PROVENANCE.to_owned(),
        },
        input: InputBinding {
            program_space_hash: ContentHash::sha256(&canonical_json(program)?),
            snapshot_id: program.snapshot_id().clone(),
            base_revision: program.base_revision().to_owned(),
            target_revision: program.target_revision().to_owned(),
            profile: program.profile_key(),
            rule_set_hash: program.rule_set_hash().clone(),
            extractor_set_hash: program.extractor_set_hash().clone(),
        },
        authority: AuthorityBoundary {
            classification: "non_authority".to_owned(),
            accepted: false,
            verified: false,
            human_accepted: false,
            sign_off: false,
        },
        scope: ScopeSummary {
            status: if eligible_count == 0 {
                ExerciseStatus::NotExercised
            } else {
                ExerciseStatus::Exercised
            },
            eligible_count,
            excluded_count,
            excluded_by_reason: exclusions,
            capabilities: capability_observations(program),
            limitation_ids: program
                .extraction()
                .limitations
                .iter()
                .map(|limitation| limitation.id.clone())
                .collect(),
            information_loss: vec![REPORT_INFORMATION_LOSS.to_owned()],
        },
        observed_facts,
        candidate_claims,
        obstructions: extraction_obstructions(program)?,
    })
}

/// Recompute the complete report and reject stale or tampered output.
pub fn validate_report(program: &ProgramSpace, report: &AnalysisReport) -> Result<()> {
    let expected = analyze(program)?;
    if report.input.program_space_hash != expected.input.program_space_hash {
        return Err(StructuralSloppinessError::ReportMismatch(
            "full canonical ProgramSpace hash differs",
        ));
    }
    if report != &expected {
        return Err(StructuralSloppinessError::ReportMismatch(
            "recomputed deterministic analysis differs",
        ));
    }
    Ok(())
}

/// Canonical bytes of the endpoint-erasing inventory ablation.
pub fn flat_projection_canonical_bytes(program: &ProgramSpace) -> Result<Vec<u8>> {
    let mut inventory = BTreeMap::<(String, Vec<u8>), (Value, u64)>::new();
    for relation in program.relations() {
        let provenance = serde_json::to_value(&relation.provenance)?;
        let canonical_provenance = canonical_json(&provenance)?;
        let entry = inventory
            .entry((relation.kind.clone(), canonical_provenance))
            .or_insert((provenance, 0));
        entry.1 = entry
            .1
            .checked_add(1)
            .ok_or(StructuralSloppinessError::CountOverflow)?;
    }
    let relation_inventory = inventory
        .into_iter()
        .map(|((kind, _), (provenance, count))| FlatRelationInventory {
            kind,
            provenance,
            count,
        })
        .collect();

    Ok(canonical_json(&FlatProjection {
        schema: FLAT_PROJECTION_SCHEMA,
        snapshot_id: program.snapshot_id(),
        base_revision: program.base_revision(),
        target_revision: program.target_revision(),
        profile: &program.profile_key(),
        rule_set_hash: program.rule_set_hash(),
        extractor_set_hash: program.extractor_set_hash(),
        artifacts: program.artifacts(),
        capabilities: &program.extraction().capabilities,
        limitations: &program.extraction().limitations,
        relation_inventory,
        information_loss: vec![
            "relation_record_endpoints_and_ids_erased".to_owned(),
            "not_a_static_analysis_competitor_or_highergraphen_library_proof".to_owned(),
        ],
    })?)
}

fn attribute_bool(attributes: &BTreeMap<String, Value>, key: &str) -> bool {
    attributes
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn valid_change_shape(change: &Artifact) -> bool {
    if change.id.kind() != "change" || change.kind != "custom" {
        return false;
    }
    let kind = change.attributes.get("change_kind").and_then(Value::as_str);
    let base_path = change.attributes.get("base_path").and_then(Value::as_str);
    let target_path = change.attributes.get("target_path").and_then(Value::as_str);
    match (kind, base_path, target_path) {
        (Some("added"), Some(""), Some(target)) => !target.is_empty(),
        (Some("deleted"), Some(base), Some("")) => !base.is_empty(),
        (Some("modified" | "type_changed"), Some(base), Some(target)) => {
            !base.is_empty() && base == target
        }
        (Some("renamed" | "copied"), Some(base), Some(target)) => {
            !base.is_empty() && !target.is_empty()
        }
        _ => false,
    }
}

fn incoming_contains(program: &ProgramSpace) -> BTreeMap<StableId, Vec<&Relation>> {
    let mut result = BTreeMap::<StableId, Vec<&Relation>>::new();
    for relation in program
        .relations()
        .iter()
        .filter(|relation| relation.kind == "contains")
    {
        for target_id in &relation.target_ids {
            result.entry(target_id.clone()).or_default().push(relation);
        }
    }
    result
}

fn observe_gate(
    changed_by: &Relation,
    symbol: &Artifact,
    change: &Artifact,
    incoming_contains: Option<&Vec<&Relation>>,
    artifacts: &BTreeMap<StableId, &Artifact>,
) -> Result<ObservedGate> {
    let mut marked_container_ids = BTreeSet::new();
    let mut matching_contains_relation_ids = BTreeSet::new();
    for relation in incoming_contains.into_iter().flatten() {
        if artifacts
            .get(&relation.source_id)
            .is_some_and(|container| attribute_bool(&container.attributes, "changed"))
        {
            marked_container_ids.insert(relation.source_id.clone());
            matching_contains_relation_ids.insert(relation.id.clone());
        }
    }
    let own_changed = attribute_bool(&symbol.attributes, "changed");
    let consumer_observed_changed = own_changed || !marked_container_ids.is_empty();
    Ok(ObservedGate {
        id: observation_id(changed_by, change)?,
        symbol_id: symbol.id.clone(),
        symbol_location: source_location(symbol.location.as_ref()),
        change_id: change.id.clone(),
        changed_by_relation_id: changed_by.id.clone(),
        producer_source_ids: vec![change.id.clone(), changed_by.id.clone()],
        own_changed,
        marked_container_ids: marked_container_ids.into_iter().collect(),
        matching_contains_relation_ids: matching_contains_relation_ids.into_iter().collect(),
        consumer_observed_changed,
    })
}

fn source_location(location: Option<&Location>) -> Option<SourceLocation> {
    location.map(|location| SourceLocation {
        path: location.path.clone(),
        start_line: location.start_line,
        end_line: location.end_line,
        start_column: location.start_column,
        end_column: location.end_column,
        symbol_id: location.symbol_id.clone(),
    })
}

fn candidate_for(observation: &ObservedGate) -> Result<CandidateClaim> {
    let source_ids = BTreeSet::from([
        observation.symbol_id.clone(),
        observation.change_id.clone(),
        observation.changed_by_relation_id.clone(),
    ]);
    Ok(CandidateClaim {
        id: candidate_id(observation)?,
        kind: "consumer_bridge_mismatch".to_owned(),
        observation_ids: vec![observation.id.clone()],
        source_ids: source_ids.into_iter().collect(),
        statement: "The fixed changed-input consumer predicate did not observe this eligible public function in the supplied ProgramSpace.".to_owned(),
        disposition: "candidate".to_owned(),
    })
}

fn capability_observations(program: &ProgramSpace) -> Vec<CapabilityObservation> {
    program
        .extraction()
        .capabilities
        .iter()
        .map(|(capability, declaration)| CapabilityObservation {
            capability: capability.clone(),
            state: declaration.state,
            source_ids: declaration.source_ids.iter().cloned().collect(),
        })
        .collect()
}

fn extraction_obstructions(program: &ProgramSpace) -> Result<Vec<AnalysisObstruction>> {
    let incomplete = program
        .extraction()
        .capabilities
        .iter()
        .filter(|(_, declaration)| declaration.state != CapabilityState::Complete)
        .collect::<Vec<_>>();
    if incomplete.is_empty() && program.extraction().limitations.is_empty() {
        return Ok(Vec::new());
    }

    let kind = if incomplete
        .iter()
        .any(|(_, declaration)| declaration.state == CapabilityState::Partial)
    {
        "partial_extraction"
    } else if incomplete
        .iter()
        .any(|(_, declaration)| declaration.state == CapabilityState::Missing)
    {
        "missing_extraction"
    } else if incomplete
        .iter()
        .any(|(_, declaration)| declaration.state == CapabilityState::Unknown)
    {
        "unknown_extraction"
    } else {
        "extraction_limitation"
    };
    let source_ids = incomplete
        .into_iter()
        .flat_map(|(_, declaration)| declaration.source_ids.iter().cloned())
        .chain(
            program
                .extraction()
                .limitations
                .iter()
                .flat_map(|limitation| limitation.source_ids.iter().cloned()),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let limitation_ids = program
        .extraction()
        .limitations
        .iter()
        .map(|limitation| limitation.id.clone())
        .collect();
    Ok(vec![AnalysisObstruction {
        id: obstruction_id(kind)?,
        kind: kind.to_owned(),
        source_ids,
        limitation_ids,
        statement: "Extraction completeness limits this benchmark observation; it does not suppress accepted-fact predicate results or grant authority.".to_owned(),
    }])
}

fn observation_id(changed_by: &Relation, change: &Artifact) -> Result<String> {
    derived_report_id(
        "observation",
        BTreeMap::from([
            ("contract".to_owned(), Value::String(CONTRACT_ID.to_owned())),
            (
                "changed_by_relation_id".to_owned(),
                Value::String(changed_by.id.to_string()),
            ),
            ("change_id".to_owned(), Value::String(change.id.to_string())),
        ]),
    )
}

fn candidate_id(observation: &ObservedGate) -> Result<String> {
    derived_report_id(
        "candidate",
        BTreeMap::from([
            (
                "kind".to_owned(),
                Value::String("consumer_bridge_mismatch".to_owned()),
            ),
            (
                "observation_id".to_owned(),
                Value::String(observation.id.clone()),
            ),
        ]),
    )
}

fn obstruction_id(kind: &str) -> Result<String> {
    derived_report_id(
        "obstruction",
        BTreeMap::from([
            ("contract".to_owned(), Value::String(CONTRACT_ID.to_owned())),
            ("kind".to_owned(), Value::String(kind.to_owned())),
        ]),
    )
}

fn derived_report_id(kind: &str, bindings: BTreeMap<String, Value>) -> Result<String> {
    Ok(StableId::derived(kind, &bindings)?.to_string())
}

fn increment_exclusion(
    exclusions: &mut BTreeMap<ExclusionReason, u64>,
    reason: ExclusionReason,
) -> Result<()> {
    let count = exclusions.entry(reason).or_insert(0);
    *count = count
        .checked_add(1)
        .ok_or(StructuralSloppinessError::CountOverflow)?;
    Ok(())
}

#[derive(Serialize)]
struct FlatProjection<'a> {
    schema: &'static str,
    snapshot_id: &'a StableId,
    base_revision: &'a str,
    target_revision: &'a str,
    profile: &'a str,
    rule_set_hash: &'a ContentHash,
    extractor_set_hash: &'a ContentHash,
    artifacts: &'a [Artifact],
    capabilities: &'a BTreeMap<String, CapabilityDeclaration>,
    limitations: &'a [reviewgraphen_core::Limitation],
    relation_inventory: Vec<FlatRelationInventory>,
    information_loss: Vec<String>,
}

#[derive(Serialize)]
struct FlatRelationInventory {
    kind: String,
    provenance: Value,
    count: u64,
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod identity_tests;

#[cfg(test)]
mod ordering_tests;

#[cfg(test)]
mod boundary_tests;
