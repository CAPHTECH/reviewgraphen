//! Source-bound projection for `reviewgraphen.review.report.v2`.
//!
//! This crate deliberately consumes only a lock-bound journal, the v3 derived
//! index, and verified CAS objects. It does not treat a report as state.

mod bounds;

pub use bounds::{
    BoundsError, LogicalCharge, OwnershipError, ReportAccounting, ReportCounts, ReportLimits,
    ownership_charge,
};

use reviewgraphen_core::{DecodedPayload, EventContractVersion, ObligationLifecycle, StableId};
use reviewgraphen_store::{
    CasReader, DerivedIndex, EventJournal, IndexSnapshot, JournalIdentity, StoreError, StoreRoot,
};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{MapAccess, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    io::Write,
};
use thiserror::Error;

const SCHEMA: &str = "reviewgraphen.review.report.v2";
const INDEX_VERSION: &str = "reviewgraphen.index_projection.v3";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportRequest {
    pub report_id: StableId,
    pub repository_id: StableId,
    pub program_space_ref: StableId,
    pub plan_id: StableId,
    pub selected_obligation_ids: BTreeSet<StableId>,
    pub tool_versions: BTreeMap<String, String>,
    pub pre_review_obstructions: Vec<PreReviewObstruction>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PreReviewObstruction {
    pub message: String,
    pub source_ids: BTreeSet<StableId>,
    pub blocks: BTreeSet<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedReport {
    pub canonical_bytes: Vec<u8>,
    pub accounting: ReportAccounting,
}

#[derive(Debug, Error)]
pub enum ReportError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Index(#[from] reviewgraphen_store::IndexError),
    #[error(transparent)]
    Journal(#[from] reviewgraphen_store::JournalError),
    #[error(transparent)]
    Core(#[from] reviewgraphen_core::DomainError),
    #[error("report source closure is invalid: {0}")]
    Source(&'static str),
    #[error("report-v2 metadata {field} must be a full lowercase sha256 digest: {hash}")]
    UnsupportedMetadataHash { field: &'static str, hash: String },
    #[error("report JSON encoding failed")]
    Json,
    #[error("report construction {operation} exceeded bound {limit} (observed {observed})")]
    Incomplete {
        operation: &'static str,
        limit: u64,
        observed: u64,
    },
}

impl From<BoundsError> for ReportError {
    fn from(value: BoundsError) -> Self {
        match value {
            BoundsError::Incomplete {
                operation,
                limit,
                observed,
            } => Self::Incomplete {
                operation,
                limit,
                observed,
            },
        }
    }
}

/// Generates a canonical v2 report from one admitted root and identity.
/// The retained `JournalReader` prevents an append between journal/index/CAS
/// observation; `snapshot_current` independently compares its v3 image with
/// the same locked canonical event stream.
pub fn generate_v2(
    root: &StoreRoot,
    identity: JournalIdentity,
    request: &ReportRequest,
) -> Result<GeneratedReport, ReportError> {
    generate_v2_with_limits(root, identity, request, ReportLimits::default())
}

/// Generates a report with explicit inclusive construction limits.  This is
/// primarily useful to prove exact and limit-plus-one refusal at the real
/// source-bound construction seam.
pub fn generate_v2_with_limits(
    root: &StoreRoot,
    identity: JournalIdentity,
    request: &ReportRequest,
    limits: ReportLimits,
) -> Result<GeneratedReport, ReportError> {
    limits.preflight(
        ReportCounts {
            obstructions: u64::try_from(request.pre_review_obstructions.len()).unwrap_or(u64::MAX),
            ..ReportCounts::default()
        },
        0,
        0,
        0,
    )?;
    if request.selected_obligation_ids.is_empty()
        || request.tool_versions.is_empty()
        || request
            .tool_versions
            .iter()
            .any(|(key, value)| key.is_empty() || value.is_empty())
    {
        return Err(ReportError::Source(
            "selected obligations and nonempty tool versions are required",
        ));
    }
    validate_pre_review_obstructions(&request.pre_review_obstructions)?;
    let journal = EventJournal::open(root, identity)?;
    let reader = journal.reader()?;
    let index = DerivedIndex::open(root)?;
    let snapshot = index.snapshot_current(&journal)?;
    if snapshot.marker.confirmed_offset != reader.confirmed_offset()
        || snapshot.marker.tail_hash != *reader.tail_hash()
        || snapshot.marker.event_count != reader.events().len() as u64
        || snapshot.marker.event_contract_version != EventContractVersion::V2.schema()
    {
        return Err(ReportError::Source("journal/index confirmed-tail mismatch"));
    }
    // The locked journal was admitted only from canonical JSONL, so its
    // confirmed offset is exactly sum(event canonical bytes + one LF).
    let journal_bytes = reader.confirmed_offset();
    // Object key order does not affect encoded length. Count directly through
    // serde's writer API so measuring I never materializes a JSON tree/Vec.
    let index_bytes = json_encoded_len(&snapshot, "index_bytes", limits.working_bytes)?;
    build_from_snapshot(
        root,
        &reader,
        &snapshot,
        request,
        limits,
        journal_bytes,
        index_bytes,
    )
}

fn build_from_snapshot(
    root: &StoreRoot,
    reader: &reviewgraphen_store::JournalReader,
    snapshot: &IndexSnapshot,
    request: &ReportRequest,
    limits: ReportLimits,
    journal_bytes: u64,
    index_bytes: u64,
) -> Result<GeneratedReport, ReportError> {
    // Refuse an already-oversized retained journal/index pair before reading
    // any CAS object or deriving report metadata.
    limits.preflight(ReportCounts::default(), journal_bytes, index_bytes, 0)?;
    let (genesis_hash, genesis_size) = reader
        .events()
        .iter()
        .find_map(
            |event| match event.decode_for_streaming_projection().ok()?.payload() {
                DecodedPayload::RunGenesisManifest(manifest) => Some((
                    manifest.genesis_artifact().cas_hash().clone(),
                    manifest.genesis_artifact().size(),
                )),
                _ => None,
            },
        )
        .ok_or(ReportError::Source("missing run genesis manifest"))?;
    let cas = CasReader::open_existing(root)?;
    let universe = snapshot
        .universe
        .as_ref()
        .ok_or(ReportError::Source("missing universe"))?;
    let expected_program_space_ref =
        StableId::parse(format!("program-space:{}", universe.snapshot_id))?;
    if request.program_space_ref != expected_program_space_ref {
        return Err(ReportError::Source("repository/program-space closure"));
    }
    validate_metadata_hash("rule_set_hash", &universe.rule_set_hash)?;
    validate_metadata_hash("extractor_set_hash", &universe.extractor_set_hash)?;
    let plan = snapshot
        .review_plans
        .iter()
        .find(|p| p.plan_id == request.plan_id)
        .ok_or(ReportError::Source("missing requested plan"))?;
    if plan.universe_id != universe.universe_id || plan.snapshot_id != universe.snapshot_id {
        return Err(ReportError::Source("plan/universe closure"));
    }
    let plan_ids = plan_obligation_ids(&plan.waves_canonical_json)?;
    if !request.selected_obligation_ids.is_subset(&plan_ids) {
        return Err(ReportError::Source("selected obligation outside plan"));
    }
    drop(plan_ids);
    let mut preliminary_executions = 0_u64;
    let mut preliminary_claims = 0_u64;
    let mut preliminary_obstructions = 0_u64;
    for execution in &snapshot.executions {
        if execution.plan_id != request.plan_id {
            continue;
        }
        let scope = canonical_single_string(&execution.obligation_ids_canonical_json)?;
        if !request
            .selected_obligation_ids
            .iter()
            .any(|selected| selected.as_str() == scope)
        {
            continue;
        }
        preliminary_executions =
            preliminary_executions
                .checked_add(1)
                .ok_or(ReportError::Incomplete {
                    operation: "executions",
                    limit: limits.executions,
                    observed: u64::MAX,
                })?;
        preliminary_claims = preliminary_claims
            .checked_add(canonical_string_list_count(
                &execution.parsed_claim_ids_canonical_json,
            )?)
            .ok_or(ReportError::Incomplete {
                operation: "claims",
                limit: limits.claims,
                observed: u64::MAX,
            })?;
        if execution.outcome_kind != "structured" {
            preliminary_obstructions =
                preliminary_obstructions
                    .checked_add(1)
                    .ok_or(ReportError::Incomplete {
                        operation: "obstructions",
                        limit: limits.obstructions,
                        observed: u64::MAX,
                    })?;
        }
    }
    let preliminary_obstructions = if preliminary_executions == 0 {
        u64::try_from(request.pre_review_obstructions.len()).unwrap_or(u64::MAX)
    } else {
        preliminary_obstructions
    };
    limits.preflight(
        ReportCounts {
            registrations: preliminary_executions,
            executions: preliminary_executions,
            claims: preliminary_claims,
            obstructions: preliminary_obstructions,
            views: 1,
            information_loss_records: 1,
        },
        journal_bytes,
        index_bytes,
        0,
    )?;
    let denominator: BTreeSet<_> = snapshot
        .obligations
        .iter()
        .map(|o| o.obligation_id.clone())
        .collect();
    if denominator.is_empty() || !request.selected_obligation_ids.is_subset(&denominator) {
        return Err(ReportError::Source("universe denominator closure"));
    }

    let mut execution_ids = BTreeSet::new();
    let mut claim_ids = BTreeSet::new();
    let mut registration_ids = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut structured = BTreeSet::new();
    for execution in &snapshot.executions {
        if execution.plan_id != request.plan_id {
            continue;
        }
        let scope = ids(&execution.obligation_ids_canonical_json)?;
        if !scope.is_subset(&request.selected_obligation_ids) {
            continue;
        }
        if scope.len() != 1 || !execution_ids.insert(execution.execution_id.clone()) {
            return Err(ReportError::Source("non-canonical execution scope"));
        }
        visited.extend(scope.iter().cloned());
        if execution.outcome_kind == "structured" {
            structured.extend(scope);
        } else if reviewer_obstruction_kind(&execution.outcome_kind).is_none() {
            return Err(ReportError::Source("unknown reviewer outcome kind"));
        }
        claim_ids.extend(ids(&execution.parsed_claim_ids_canonical_json)?);
        registration_ids.insert(execution.raw_registration_id.clone());
    }
    let execution_count = u64::try_from(execution_ids.len()).unwrap_or(u64::MAX);
    limits.preflight(
        ReportCounts {
            executions: execution_count,
            ..ReportCounts::default()
        },
        journal_bytes,
        index_bytes,
        0,
    )?;
    let claim_count = snapshot
        .claims
        .iter()
        .filter(|c| claim_ids.contains(&c.claim_id))
        .count();
    if claim_count != claim_ids.len()
        || snapshot
            .claims
            .iter()
            .filter(|claim| claim_ids.contains(&claim.claim_id))
            .any(|c| !execution_ids.contains(&c.execution_id))
    {
        return Err(ReportError::Source("execution claim union"));
    }
    if preliminary_claims != u64::try_from(claim_ids.len()).unwrap_or(u64::MAX) {
        return Err(ReportError::Source("duplicate parsed claim reference"));
    }
    limits.preflight(
        ReportCounts {
            executions: execution_count,
            claims: u64::try_from(claim_count).unwrap_or(u64::MAX),
            ..ReportCounts::default()
        },
        journal_bytes,
        index_bytes,
        0,
    )?;
    for execution in snapshot
        .executions
        .iter()
        .filter(|execution| execution_ids.contains(&execution.execution_id))
    {
        let start = snapshot
            .claims
            .partition_point(|claim| claim.event_sequence < execution.event_sequence);
        let end = snapshot
            .claims
            .partition_point(|claim| claim.event_sequence <= execution.event_sequence);
        let event_claims = &snapshot.claims[start..end];
        if !canonical_claim_ids_match(
            &execution.parsed_claim_ids_canonical_json,
            &execution.execution_id,
            event_claims,
        )? || (execution.outcome_kind == "structured") == event_claims.is_empty()
        {
            return Err(ReportError::Source("outcome/claim pairing"));
        }
    }

    let registration_count = snapshot
        .artifact_registrations
        .iter()
        .filter(|r| registration_ids.contains(&r.registration_id))
        .count();
    if registration_count != registration_ids.len() {
        return Err(ReportError::Source("raw registration set"));
    }
    limits.preflight(
        ReportCounts {
            registrations: u64::try_from(registration_count).unwrap_or(u64::MAX),
            executions: execution_count,
            claims: u64::try_from(claim_count).unwrap_or(u64::MAX),
            ..ReportCounts::default()
        },
        journal_bytes,
        index_bytes,
        0,
    )?;
    for registration in snapshot
        .artifact_registrations
        .iter()
        .filter(|registration| registration_ids.contains(&registration.registration_id))
    {
        let source = reviewer_source(registration)?;
        let source_execution = StableId::parse(source.execution_id)?;
        // snapshot_current has already replay-compared the registration/hash/
        // execution/reviewer tuple against the locked canonical journal. This
        // gate checks the exact report-selected source set without imposing
        // an event-adjacency rule on crash recovery.
        if registration.sensitivity != "sensitive"
            || registration.run_id != snapshot.marker.run_id
            || source.run_id != registration.run_id.as_str()
            || !execution_ids.contains(&source_execution)
        {
            return Err(ReportError::Source("raw registration closure"));
        }
    }

    let completed: BTreeSet<_> = reader
        .events()
        .iter()
        .filter_map(|event| {
            let decoded = event.decode_for_streaming_projection().ok()?;
            match decoded.payload() {
                DecodedPayload::ObligationTransition {
                    obligation_id,
                    next: ObligationLifecycle::Completed,
                } if structured.contains(obligation_id) => Some(obligation_id.clone()),
                _ => None,
            }
        })
        .collect();
    let known_sources = KnownSources {
        snapshot,
        request,
        universe,
        denominator: &denominator,
        execution_ids: &execution_ids,
        claim_ids: &claim_ids,
        registration_ids: &registration_ids,
    };
    for obstruction in &request.pre_review_obstructions {
        if !obstruction
            .source_ids
            .iter()
            .all(|source| known_sources.contains(source.as_str()))
        {
            return Err(ReportError::Source(
                "unresolved pre-review obstruction source",
            ));
        }
    }
    let status = status(
        &request.selected_obligation_ids,
        execution_count,
        u64::try_from(registration_count).unwrap_or(u64::MAX),
        &visited,
        &completed,
        &request.pre_review_obstructions,
    )?;
    let counts = ReportCounts {
        registrations: u64::try_from(registration_count).unwrap_or(u64::MAX),
        executions: execution_count,
        claims: u64::try_from(claim_count).unwrap_or(u64::MAX),
        obstructions: match status {
            Status::UnsupportedInput => {
                u64::try_from(request.pre_review_obstructions.len()).unwrap_or(u64::MAX)
            }
            _ => u64::try_from(
                snapshot
                    .executions
                    .iter()
                    .filter(|execution| {
                        execution_ids.contains(&execution.execution_id)
                            && execution.outcome_kind != "structured"
                    })
                    .count(),
            )
            .unwrap_or(u64::MAX),
        },
        views: 1,
        information_loss_records: 1,
    };
    let reserved_report_bytes = report_shape_charge(
        ReportShapeSources {
            snapshot,
            request,
            universe,
            denominator: &denominator,
            visited: &visited,
            completed: &completed,
            execution_ids: &execution_ids,
            claim_ids: &claim_ids,
            registration_ids: &registration_ids,
            status,
        },
        limits.working_bytes,
    )?;
    limits.preflight(counts, journal_bytes, index_bytes, reserved_report_bytes)?;

    // The genesis CAS object is closure evidence, not report ownership. Read
    // and drop it in a tight scope only after the complete report preflight.
    {
        let genesis_capacity =
            usize::try_from(genesis_size).map_err(|_| ReportError::Source("genesis size"))?;
        let mut genesis_bytes = Vec::new();
        genesis_bytes
            .try_reserve_exact(genesis_capacity)
            .map_err(|_| ReportError::Incomplete {
                operation: "genesis_cas_bytes",
                limit: limits.working_bytes,
                observed: genesis_size,
            })?;
        cas.read_into(
            &reviewgraphen_store::CasHash::parse(genesis_hash.to_string())?,
            Some(genesis_size),
            &mut genesis_bytes,
        )?;
        let genesis = reviewgraphen_core::RunGenesisSnapshot::from_canonical_bytes(&genesis_bytes)?;
        if genesis.program_space().repository_id() != &request.repository_id
            || genesis.program_space().snapshot_id() != &universe.snapshot_id
        {
            return Err(ReportError::Source("repository/program-space closure"));
        }
    }

    // Raw reviewer artifacts are not allocated or read until all row and
    // working-set limits have accepted the borrow-only report shape.
    for registration in snapshot
        .artifact_registrations
        .iter()
        .filter(|row| registration_ids.contains(&row.registration_id))
    {
        let mut raw = Vec::with_capacity(
            usize::try_from(registration.size).map_err(|_| ReportError::Source("raw size"))?,
        );
        let hash = reviewgraphen_store::CasHash::parse(registration.cas_hash.to_string())?;
        cas.read_into(&hash, Some(registration.size), &mut raw)?;
    }

    let projected_obstructions = obstructions(
        status,
        &snapshot.executions,
        &execution_ids,
        &request.pre_review_obstructions,
    )?;

    let report = Report {
        schema: SCHEMA,
        report_type: "review",
        report_version: 2,
        metadata: Metadata {
            report_id: request.report_id.to_string(),
            run_id: snapshot.marker.run_id.to_string(),
            profile_id: universe.profile_id.clone(),
            rule_set_hash: universe.rule_set_hash.to_string(),
            extractor_set_hash: universe.extractor_set_hash.to_string(),
            policy_version: universe.policy_version.clone(),
            event_contract_version: EventContractVersion::V2.schema(),
            index_projection_version: INDEX_VERSION,
            genesis_hash: snapshot.marker.genesis_hash.to_string(),
            confirmed_offset: snapshot.marker.confirmed_offset,
            confirmed_tail_hash: snapshot.marker.tail_hash.to_string(),
            confirmed_event_count: snapshot.marker.event_count,
            tool_versions: request.tool_versions.clone(),
        },
        scenario: Scenario {
            repository_id: request.repository_id.to_string(),
            snapshot_id: universe.snapshot_id.to_string(),
            program_space_ref: request.program_space_ref.to_string(),
            universe_id: universe.universe_id.to_string(),
            plan_id: request.plan_id.to_string(),
            selected_obligation_ids: strings(&request.selected_obligation_ids),
            artifact_registration_ids: strings(&registration_ids),
        },
        result: ResultBody {
            status,
            artifact_registrations: snapshot
                .artifact_registrations
                .iter()
                .filter(|row| registration_ids.contains(&row.registration_id))
                .map(registration)
                .collect::<Result<_, _>>()?,
            executions: snapshot
                .executions
                .iter()
                .filter(|row| execution_ids.contains(&row.execution_id))
                .map(execution)
                .collect::<Result<_, _>>()?,
            claims: snapshot
                .claims
                .iter()
                .filter(|row| claim_ids.contains(&row.claim_id))
                .map(claim)
                .collect::<Result<_, _>>()?,
            obstructions: projected_obstructions,
            authority_records: Authority::default(),
        },
        coverage: Coverage {
            universe_id: universe.universe_id.to_string(),
            denominator_obligation_ids: strings(&denominator),
            visited_obligation_ids: strings(&visited),
            completed_obligation_ids: strings(&completed),
            selected: request.selected_obligation_ids.len() as u64,
            visited: visited.len() as u64,
            completed: completed.len() as u64,
            verified: 0,
            accepted: 0,
        },
        projection: Projection {
            views: vec![view(
                status,
                &execution_ids,
                &claim_ids,
                &snapshot.executions,
                &request.report_id,
            )],
        },
    };
    validate_local_report(
        &report,
        registration_ids.len(),
        execution_ids.len(),
        claim_ids.len(),
    )?;
    // Derived selection sets are projection scratch, not serialization
    // scratch. The owned report now contains their exact projections.
    drop(structured);
    drop(denominator);
    drop(visited);
    drop(completed);
    drop(execution_ids);
    drop(claim_ids);
    drop(registration_ids);
    let realized_report_bytes = ownership_charge(&report)
        .map_err(|error| ownership_report_error(error, limits.working_bytes))?;
    if realized_report_bytes != reserved_report_bytes {
        return Err(ReportError::Source(
            "borrowed report shape ownership mismatch",
        ));
    }
    let largest_record_bytes = largest_record_bytes(&report)?;
    let output_bytes = json_encoded_len(&report, "canonical_report_bytes", limits.canonical_bytes)?;
    limits.check_serialization(
        journal_bytes,
        index_bytes,
        realized_report_bytes,
        largest_record_bytes,
        output_bytes,
    )?;
    let output_capacity = usize::try_from(output_bytes).map_err(|_| ReportError::Incomplete {
        operation: "canonical_report_bytes",
        limit: limits.canonical_bytes,
        observed: output_bytes,
    })?;
    let mut output_buffer = Vec::new();
    output_buffer
        .try_reserve_exact(output_capacity)
        .map_err(|_| ReportError::Incomplete {
            operation: "canonical_report_bytes",
            limit: limits.canonical_bytes,
            observed: output_bytes,
        })?;
    let mut bounded_output = BoundedOutput {
        bytes: output_buffer,
        limit: output_capacity,
    };
    serde_json::to_writer(&mut bounded_output, &report).map_err(|error| {
        if error.is_io() {
            ReportError::Incomplete {
                operation: "canonical_report_bytes",
                limit: limits.canonical_bytes,
                observed: output_bytes.saturating_add(1),
            }
        } else {
            ReportError::Json
        }
    })?;
    let canonical_bytes = bounded_output.bytes;
    if canonical_bytes.len() != output_capacity {
        return Err(ReportError::Source(
            "report counting/serialization mismatch",
        ));
    }
    Ok(GeneratedReport {
        canonical_bytes,
        accounting: ReportAccounting {
            journal_bytes,
            index_bytes,
            reserved_report_bytes,
            realized_report_bytes,
            largest_record_bytes,
            canonical_report_bytes: output_bytes,
        },
    })
}

fn largest_record_bytes(report: &Report<'_>) -> Result<u64, ReportError> {
    let mut largest = 0_u64;
    macro_rules! measure {
        ($values:expr) => {
            for value in $values {
                largest = largest.max(json_encoded_len(value, "largest_record_bytes", u64::MAX)?);
            }
        };
    }
    measure!(&report.result.artifact_registrations);
    measure!(&report.result.executions);
    measure!(&report.result.claims);
    measure!(&report.result.obstructions);
    measure!(&report.projection.views);
    Ok(largest)
}

#[derive(Default)]
struct JsonByteCounter {
    bytes: u64,
}

struct BoundedOutput {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedOutput {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let next = self
            .bytes
            .len()
            .checked_add(buffer.len())
            .ok_or_else(|| std::io::Error::other("report output length overflow"))?;
        if next > self.limit {
            return Err(std::io::Error::other("report output exceeded preflight"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Write for JsonByteCounter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(u64::try_from(buffer.len()).unwrap_or(u64::MAX))
            .ok_or_else(|| std::io::Error::other("JSON byte count overflow"))?;
        Ok(buffer.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn json_encoded_len<T: Serialize>(
    value: &T,
    operation: &'static str,
    limit: u64,
) -> Result<u64, ReportError> {
    let mut counter = JsonByteCounter::default();
    serde_json::to_writer(&mut counter, value).map_err(|error| {
        if error.is_io() {
            ReportError::Incomplete {
                operation,
                limit,
                observed: u64::MAX,
            }
        } else {
            ReportError::Json
        }
    })?;
    Ok(counter.bytes)
}

#[derive(Deserialize)]
struct Wave {
    obligation_ids: Vec<StableId>,
    wave_index: u32,
}
fn plan_obligation_ids(input: &str) -> Result<BTreeSet<StableId>, ReportError> {
    let waves: Vec<Wave> = serde_json::from_str(input).map_err(|_| ReportError::Json)?;
    let _ = waves.iter().map(|w| w.wave_index).sum::<u32>();
    Ok(waves.into_iter().flat_map(|w| w.obligation_ids).collect())
}
fn ids(input: &str) -> Result<BTreeSet<StableId>, ReportError> {
    let values: Vec<StableId> = serde_json::from_str(input).map_err(|_| ReportError::Json)?;
    let set: BTreeSet<StableId> = values.iter().cloned().collect();
    if set.len() != values.len() {
        return Err(ReportError::Source("duplicate canonical ids"));
    }
    Ok(set)
}
fn canonical_single_string(input: &str) -> Result<&str, ReportError> {
    let [value]: [&str; 1] = serde_json::from_str(input).map_err(|_| {
        ReportError::Source("D2 execution scope must contain exactly one canonical obligation")
    })?;
    Ok(value)
}

fn canonical_string_list_count(input: &str) -> Result<u64, ReportError> {
    struct CountStrings;
    impl<'de> Visitor<'de> for CountStrings {
        type Value = u64;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a JSON string array")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut input: A) -> Result<Self::Value, A::Error> {
            let mut count = 0_u64;
            while input.next_element::<&'de str>()?.is_some() {
                count = count
                    .checked_add(1)
                    .ok_or_else(|| serde::de::Error::custom("string list count overflow"))?;
            }
            Ok(count)
        }
    }
    let mut deserializer = serde_json::Deserializer::from_str(input);
    let count = deserializer
        .deserialize_seq(CountStrings)
        .map_err(|_| ReportError::Json)?;
    deserializer.end().map_err(|_| ReportError::Json)?;
    Ok(count)
}
fn canonical_claim_ids_match(
    input: &str,
    execution_id: &StableId,
    claims: &[reviewgraphen_store::IndexClaim],
) -> Result<bool, ReportError> {
    struct MatchClaims<'a> {
        execution_id: &'a StableId,
        claims: &'a [reviewgraphen_store::IndexClaim],
    }
    impl<'de> Visitor<'de> for MatchClaims<'_> {
        type Value = bool;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a canonical claim ID array")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut input: A) -> Result<Self::Value, A::Error> {
            let mut index = 0_usize;
            while let Some(claim_id) = input.next_element::<&'de str>()? {
                let Some(claim) = self.claims.get(index) else {
                    return Ok(false);
                };
                if claim.claim_id.as_str() != claim_id || &claim.execution_id != self.execution_id {
                    return Ok(false);
                }
                index += 1;
            }
            Ok(index == self.claims.len())
        }
    }
    let mut deserializer = serde_json::Deserializer::from_str(input);
    let matches = deserializer
        .deserialize_seq(MatchClaims {
            execution_id,
            claims,
        })
        .map_err(|_| ReportError::Json)?;
    deserializer.end().map_err(|_| ReportError::Json)?;
    Ok(matches)
}
fn strings(values: &BTreeSet<StableId>) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

fn validate_pre_review_obstructions(values: &[PreReviewObstruction]) -> Result<(), ReportError> {
    for value in values {
        if value.message.is_empty() || value.source_ids.is_empty() || value.blocks.is_empty() {
            return Err(ReportError::Source(
                "pre-review obstruction fields must be nonempty",
            ));
        }
    }
    if values
        .windows(2)
        .any(|pair| compare_pre_obstructions(&pair[0], &pair[1]) != Ordering::Less)
    {
        return Err(ReportError::Source(
            "pre-review obstructions must be canonical and duplicate-free",
        ));
    }
    Ok(())
}

fn compare_pre_obstructions(left: &PreReviewObstruction, right: &PreReviewObstruction) -> Ordering {
    compare_json_string_lists(
        left.blocks.iter().map(StableId::as_str),
        right.blocks.iter().map(StableId::as_str),
    )
    .then_with(|| {
        compare_json_strings(
            "pre_review_unsupported_input",
            "pre_review_unsupported_input",
        )
    })
    .then_with(|| compare_json_strings(&left.message, &right.message))
    .then_with(|| {
        compare_json_string_lists(
            left.source_ids.iter().map(StableId::as_str),
            right.source_ids.iter().map(StableId::as_str),
        )
    })
}

fn compare_json_string_lists<'a>(
    mut left: impl Iterator<Item = &'a str>,
    mut right: impl Iterator<Item = &'a str>,
) -> Ordering {
    loop {
        match (left.next(), right.next()) {
            (Some(left), Some(right)) => {
                let ordering = compare_json_strings(left, right);
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (None, None) => return Ordering::Equal,
            // In JSON the next byte for a longer list is ',' (0x2c), while
            // the shorter list closes with ']' (0x5d).
            (None, Some(_)) => return Ordering::Greater,
            (Some(_), None) => return Ordering::Less,
        }
    }
}

fn compare_json_strings(left: &str, right: &str) -> Ordering {
    JsonStringBytes::new(left).cmp(JsonStringBytes::new(right))
}

struct JsonStringBytes<'a> {
    source: std::str::Bytes<'a>,
    pending: [u8; 6],
    pending_index: usize,
    pending_len: usize,
    opening: bool,
    closed: bool,
}
impl<'a> JsonStringBytes<'a> {
    fn new(value: &'a str) -> Self {
        Self {
            source: value.bytes(),
            pending: [0; 6],
            pending_index: 0,
            pending_len: 0,
            opening: true,
            closed: false,
        }
    }
    fn set_pending(&mut self, bytes: &[u8]) {
        self.pending[..bytes.len()].copy_from_slice(bytes);
        self.pending_index = 0;
        self.pending_len = bytes.len();
    }
}
impl Iterator for JsonStringBytes<'_> {
    type Item = u8;
    fn next(&mut self) -> Option<Self::Item> {
        if self.opening {
            self.opening = false;
            return Some(b'"');
        }
        if self.pending_index < self.pending_len {
            let value = self.pending[self.pending_index];
            self.pending_index += 1;
            return Some(value);
        }
        if let Some(value) = self.source.next() {
            match value {
                b'"' => self.set_pending(br#"\""#),
                b'\\' => self.set_pending(br#"\\"#),
                0x08 => self.set_pending(br"\b"),
                b'\t' => self.set_pending(br"\t"),
                b'\n' => self.set_pending(br"\n"),
                0x0c => self.set_pending(br"\f"),
                b'\r' => self.set_pending(br"\r"),
                0x00..=0x1f => {
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    self.set_pending(&[
                        b'\\',
                        b'u',
                        b'0',
                        b'0',
                        HEX[usize::from(value >> 4)],
                        HEX[usize::from(value & 0x0f)],
                    ]);
                }
                _ => return Some(value),
            }
            return self.next();
        }
        if !self.closed {
            self.closed = true;
            return Some(b'"');
        }
        None
    }
}

fn validate_metadata_hash(
    field: &'static str,
    hash: &reviewgraphen_core::ContentHash,
) -> Result<(), ReportError> {
    let value = hash.to_string();
    let valid = value.starts_with("sha256:")
        && value.len() == "sha256:".len() + 64
        && value["sha256:".len()..].bytes().all(|byte| {
            byte.is_ascii_digit() || (byte.is_ascii_lowercase() && byte.is_ascii_hexdigit())
        });
    if valid {
        Ok(())
    } else {
        Err(ReportError::UnsupportedMetadataHash { field, hash: value })
    }
}

#[derive(Clone, Copy, Deserialize)]
struct BorrowedReviewerSource<'a> {
    #[serde(borrow)]
    execution_id: &'a str,
    #[serde(borrow)]
    kind: &'a str,
    #[serde(borrow)]
    reviewer_id: &'a str,
    #[serde(borrow)]
    run_id: &'a str,
}
struct KnownSources<'a> {
    snapshot: &'a IndexSnapshot,
    request: &'a ReportRequest,
    universe: &'a reviewgraphen_store::IndexUniverse,
    denominator: &'a BTreeSet<StableId>,
    execution_ids: &'a BTreeSet<StableId>,
    claim_ids: &'a BTreeSet<StableId>,
    registration_ids: &'a BTreeSet<StableId>,
}
impl KnownSources<'_> {
    fn contains(&self, value: &str) -> bool {
        [
            &self.request.report_id,
            &self.request.repository_id,
            &self.request.program_space_ref,
            &self.snapshot.marker.run_id,
            &self.universe.snapshot_id,
            &self.universe.universe_id,
            &self.request.plan_id,
        ]
        .into_iter()
        .any(|source| source.as_str() == value)
            || self
                .denominator
                .iter()
                .any(|source| source.as_str() == value)
            || self
                .execution_ids
                .iter()
                .any(|source| source.as_str() == value)
            || self.claim_ids.iter().any(|source| source.as_str() == value)
            || self
                .registration_ids
                .iter()
                .any(|source| source.as_str() == value)
            || self
                .snapshot
                .events
                .iter()
                .any(|source| source.event_id.as_str() == value)
            || self
                .snapshot
                .program_objects
                .iter()
                .any(|source| source.object_id.as_str() == value)
            || self
                .snapshot
                .program_relations
                .iter()
                .any(|source| source.relation_id.as_str() == value)
            || self.snapshot.snapshot_sources.iter().any(|source| {
                source.artifact_id.as_str() == value || source.registration_id.as_str() == value
            })
            || self
                .snapshot
                .context_envelopes
                .iter()
                .any(|source| source.envelope_id.as_str() == value)
    }
}
fn reviewer_source(
    row: &reviewgraphen_store::IndexArtifactRegistration,
) -> Result<BorrowedReviewerSource<'_>, ReportError> {
    let source: BorrowedReviewerSource<'_> =
        serde_json::from_str(&row.source_id).map_err(|_| ReportError::Json)?;
    if source.kind != "reviewer_execution" || row.source_kind != source.kind {
        return Err(ReportError::Source("missing typed registration source"));
    }
    Ok(source)
}

fn status(
    selected: &BTreeSet<StableId>,
    execution_count: u64,
    registration_count: u64,
    visited: &BTreeSet<StableId>,
    completed: &BTreeSet<StableId>,
    pre: &[PreReviewObstruction],
) -> Result<Status, ReportError> {
    if execution_count == 0 {
        if registration_count == 0
            && visited.is_empty()
            && completed.is_empty()
            && !pre.is_empty()
            && pre.iter().all(|o| !o.source_ids.is_empty())
            && pre
                .iter()
                .flat_map(|o| o.blocks.iter())
                .all(|blocked| selected.contains(blocked))
            && selected.iter().all(|selected| {
                pre.iter()
                    .any(|obstruction| obstruction.blocks.contains(selected))
            })
        {
            return Ok(Status::UnsupportedInput);
        }
        return Err(ReportError::Source("unsupported-input exact condition"));
    }
    if !pre.is_empty() {
        return Err(ReportError::Source("pre-review obstruction with attempts"));
    }
    Ok(if completed == selected {
        Status::Completed
    } else {
        Status::Partial
    })
}

#[derive(Serialize)]
struct Report<'a> {
    coverage: Coverage,
    metadata: Metadata,
    projection: Projection,
    report_type: &'a str,
    report_version: u8,
    result: ResultBody,
    scenario: Scenario,
    schema: &'a str,
}
#[derive(Serialize)]
struct Metadata {
    confirmed_event_count: u64,
    confirmed_offset: u64,
    confirmed_tail_hash: String,
    event_contract_version: &'static str,
    extractor_set_hash: String,
    genesis_hash: String,
    index_projection_version: &'static str,
    policy_version: String,
    profile_id: String,
    report_id: String,
    rule_set_hash: String,
    run_id: String,
    tool_versions: BTreeMap<String, String>,
}
#[derive(Serialize)]
struct Scenario {
    artifact_registration_ids: Vec<String>,
    plan_id: String,
    program_space_ref: String,
    repository_id: String,
    selected_obligation_ids: Vec<String>,
    snapshot_id: String,
    universe_id: String,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Completed,
    Partial,
    UnsupportedInput,
}
#[derive(Serialize)]
struct ResultBody {
    artifact_registrations: Vec<Registration>,
    authority_records: Authority,
    claims: Vec<Claim>,
    executions: Vec<Execution>,
    obstructions: Vec<Obstruction>,
    status: Status,
}
#[derive(Serialize, Default)]
struct Authority {
    authority_reconciled: bool,
    decision_ids: Vec<String>,
    evidence_ids: Vec<String>,
    finding_ids: Vec<String>,
    verification_ids: Vec<String>,
}
#[derive(Serialize)]
struct Registration {
    cas_hash: String,
    event_id: String,
    event_sequence: u64,
    media_type: String,
    registration_id: String,
    run_id: String,
    sensitivity: &'static str,
    size: u64,
    source: RegistrationSource,
}
#[derive(Serialize)]
struct RegistrationSource {
    execution_id: String,
    kind: &'static str,
    reviewer_id: String,
    run_id: String,
}
#[derive(Serialize)]
struct Execution {
    attempt: u32,
    body_hash: String,
    envelope_id: String,
    event_id: String,
    event_sequence: u64,
    id: String,
    identity_body_hash: String,
    inference_settings: BTreeMap<String, String>,
    model: Option<String>,
    model_revision: Option<String>,
    obligation_ids: Vec<String>,
    outcome: Outcome,
    parsed_claim_ids: Vec<String>,
    plan_id: String,
    prompt_template_version: String,
    provider: Option<String>,
    raw_artifact_hash: String,
    raw_artifact_registration_id: String,
    reviewer_id: String,
    reviewer_kind: String,
    snapshot_id: String,
    system_prompt_version: String,
    tool_calls: Vec<()>,
    tool_policy_version: String,
    wave_id: String,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Outcome {
    Structured,
    Abstained { reason: String, detail: String },
    Malformed { reason: String, diagnostic: String },
    ProviderFailure { retryable: bool, diagnostic: String },
}
impl Serialize for Outcome {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        match self {
            Self::Structured => {
                let mut fields = serializer.serialize_struct("Outcome", 1)?;
                fields.serialize_field("kind", "structured")?;
                fields.end()
            }
            Self::Abstained { reason, detail } => {
                let mut fields = serializer.serialize_struct("Outcome", 3)?;
                fields.serialize_field("detail", detail)?;
                fields.serialize_field("kind", "abstained")?;
                fields.serialize_field("reason", reason)?;
                fields.end()
            }
            Self::Malformed { reason, diagnostic } => {
                let mut fields = serializer.serialize_struct("Outcome", 3)?;
                fields.serialize_field("diagnostic", diagnostic)?;
                fields.serialize_field("kind", "malformed")?;
                fields.serialize_field("reason", reason)?;
                fields.end()
            }
            Self::ProviderFailure {
                retryable,
                diagnostic,
            } => {
                let mut fields = serializer.serialize_struct("Outcome", 3)?;
                fields.serialize_field("diagnostic", diagnostic)?;
                fields.serialize_field("kind", "provider_failure")?;
                fields.serialize_field("retryable", retryable)?;
                fields.end()
            }
        }
    }
}
#[derive(Serialize)]
struct Claim {
    assumptions: Vec<String>,
    author_kind: String,
    body_hash: String,
    candidate_confidence: Option<f64>,
    disposition: String,
    event_id: String,
    event_sequence: u64,
    execution_id: String,
    id: String,
    identity_body_hash: String,
    obligation_ids: Vec<String>,
    polarity: String,
    property_id: String,
    requested_evidence: Vec<String>,
    review_status: String,
    source_ids: Vec<String>,
    summary: String,
    target_refs: Vec<String>,
}
#[derive(Serialize)]
struct Obstruction {
    blocks: Vec<String>,
    kind: String,
    message: String,
    source_ids: Vec<String>,
}
#[derive(Serialize)]
struct Coverage {
    accepted: u8,
    completed: u64,
    completed_obligation_ids: Vec<String>,
    denominator_obligation_ids: Vec<String>,
    selected: u64,
    universe_id: String,
    verified: u8,
    visited: u64,
    visited_obligation_ids: Vec<String>,
}
#[derive(Serialize)]
struct Projection {
    views: Vec<View>,
}
#[derive(Serialize)]
struct View {
    information_loss: Vec<Loss>,
    kind: &'static str,
    payload: Payload,
    source_ids: Vec<String>,
}
#[derive(Serialize)]
struct Loss {
    affected_properties: Vec<&'static str>,
    kind: &'static str,
    meaningful: bool,
    reason: &'static str,
    recoverable: bool,
    recovery_ref: String,
    source_ids: Vec<String>,
}
#[derive(Serialize)]
struct Payload {
    claim_ids: Vec<String>,
    execution_ids: Vec<String>,
    obstruction_kinds: Vec<String>,
    status: Status,
}

/// Borrow-only source view used for the normative `Rr` preflight.  It keeps
/// index strings and identifiers borrowed and serializes row projections one
/// at a time, so measuring ownership does not first construct the owned
/// `Report`, its record vectors, or its cloned strings.
struct ReportShapeSources<'a> {
    snapshot: &'a IndexSnapshot,
    request: &'a ReportRequest,
    universe: &'a reviewgraphen_store::IndexUniverse,
    denominator: &'a BTreeSet<StableId>,
    visited: &'a BTreeSet<StableId>,
    completed: &'a BTreeSet<StableId>,
    execution_ids: &'a BTreeSet<StableId>,
    claim_ids: &'a BTreeSet<StableId>,
    registration_ids: &'a BTreeSet<StableId>,
    status: Status,
}

fn report_shape_charge(
    sources: ReportShapeSources<'_>,
    working_limit: u64,
) -> Result<u64, ReportError> {
    let view_sources = ViewSourceIds {
        execution_ids: sources.execution_ids,
        report_id: &sources.request.report_id,
    };
    let shape = BorrowedReportShape {
        schema: SCHEMA,
        report_type: "review",
        report_version: 2,
        metadata: BorrowedMetadata {
            report_id: &sources.request.report_id,
            run_id: &sources.snapshot.marker.run_id,
            profile_id: &sources.universe.profile_id,
            rule_set_hash: &sources.universe.rule_set_hash,
            extractor_set_hash: &sources.universe.extractor_set_hash,
            policy_version: &sources.universe.policy_version,
            event_contract_version: EventContractVersion::V2.schema(),
            index_projection_version: INDEX_VERSION,
            genesis_hash: &sources.snapshot.marker.genesis_hash,
            confirmed_offset: sources.snapshot.marker.confirmed_offset,
            confirmed_tail_hash: &sources.snapshot.marker.tail_hash,
            confirmed_event_count: sources.snapshot.marker.event_count,
            tool_versions: &sources.request.tool_versions,
        },
        scenario: BorrowedScenario {
            repository_id: &sources.request.repository_id,
            snapshot_id: &sources.universe.snapshot_id,
            program_space_ref: &sources.request.program_space_ref,
            universe_id: &sources.universe.universe_id,
            plan_id: &sources.request.plan_id,
            selected_obligation_ids: &sources.request.selected_obligation_ids,
            artifact_registration_ids: sources.registration_ids,
        },
        result: BorrowedResult {
            status: sources.status,
            artifact_registrations: RegistrationRows {
                rows: &sources.snapshot.artifact_registrations,
                registration_ids: sources.registration_ids,
            },
            executions: ExecutionRows {
                rows: &sources.snapshot.executions,
                execution_ids: sources.execution_ids,
            },
            claims: ClaimRows {
                rows: &sources.snapshot.claims,
                claim_ids: sources.claim_ids,
            },
            obstructions: ObstructionRows {
                status: sources.status,
                executions: &sources.snapshot.executions,
                execution_ids: sources.execution_ids,
                pre: &sources.request.pre_review_obstructions,
            },
            authority_records: BorrowedAuthority::default(),
        },
        coverage: BorrowedCoverage {
            universe_id: &sources.universe.universe_id,
            denominator_obligation_ids: sources.denominator,
            visited_obligation_ids: sources.visited,
            completed_obligation_ids: sources.completed,
            selected: u64::try_from(sources.request.selected_obligation_ids.len())
                .unwrap_or(u64::MAX),
            visited: u64::try_from(sources.visited.len()).unwrap_or(u64::MAX),
            completed: u64::try_from(sources.completed.len()).unwrap_or(u64::MAX),
            verified: 0,
            accepted: 0,
        },
        projection: BorrowedProjection {
            views: OneView(BorrowedView {
                kind: "machine",
                source_ids: view_sources,
                information_loss: OneLoss(BorrowedLoss {
                    kind: "authority_coverage_omitted",
                    reason: "This authority-free D2 projection does not infer evidence, verification, freshness, or human acceptance.",
                    source_ids: view_sources,
                    affected_properties: ["review.authority_coverage"],
                    meaningful: true,
                    recoverable: true,
                    recovery_ref: &sources.request.report_id,
                }),
                payload: BorrowedPayload {
                    status: sources.status,
                    execution_ids: sources.execution_ids,
                    claim_ids: sources.claim_ids,
                    obstruction_kinds: ObstructionKinds {
                        executions: &sources.snapshot.executions,
                        execution_ids: sources.execution_ids,
                    },
                },
            }),
        },
    };
    ownership_charge(&shape).map_err(|error| ownership_report_error(error, working_limit))
}

fn ownership_report_error(error: OwnershipError, working_limit: u64) -> ReportError {
    match error {
        OwnershipError::Overflow => ReportError::Incomplete {
            operation: "report_ownership_bytes",
            limit: working_limit,
            observed: u64::MAX,
        },
        OwnershipError::NonStringMapKey | OwnershipError::Message(_) => ReportError::Json,
    }
}

#[derive(Serialize)]
struct BorrowedReportShape<'a> {
    schema: &'a str,
    report_type: &'a str,
    report_version: u8,
    metadata: BorrowedMetadata<'a>,
    scenario: BorrowedScenario<'a>,
    result: BorrowedResult<'a>,
    coverage: BorrowedCoverage<'a>,
    projection: BorrowedProjection<'a>,
}

#[derive(Serialize)]
struct BorrowedMetadata<'a> {
    report_id: &'a StableId,
    run_id: &'a StableId,
    profile_id: &'a str,
    rule_set_hash: &'a reviewgraphen_core::ContentHash,
    extractor_set_hash: &'a reviewgraphen_core::ContentHash,
    policy_version: &'a str,
    event_contract_version: &'static str,
    index_projection_version: &'static str,
    genesis_hash: &'a reviewgraphen_core::ContentHash,
    confirmed_offset: u64,
    confirmed_tail_hash: &'a reviewgraphen_core::ContentHash,
    confirmed_event_count: u64,
    tool_versions: &'a BTreeMap<String, String>,
}

#[derive(Serialize)]
struct BorrowedScenario<'a> {
    repository_id: &'a StableId,
    snapshot_id: &'a StableId,
    program_space_ref: &'a StableId,
    universe_id: &'a StableId,
    plan_id: &'a StableId,
    selected_obligation_ids: &'a BTreeSet<StableId>,
    artifact_registration_ids: &'a BTreeSet<StableId>,
}

#[derive(Serialize)]
struct BorrowedResult<'a> {
    status: Status,
    artifact_registrations: RegistrationRows<'a>,
    executions: ExecutionRows<'a>,
    claims: ClaimRows<'a>,
    obstructions: ObstructionRows<'a>,
    authority_records: BorrowedAuthority<'a>,
}

#[derive(Default, Serialize)]
struct BorrowedAuthority<'a> {
    evidence_ids: [&'a str; 0],
    verification_ids: [&'a str; 0],
    decision_ids: [&'a str; 0],
    finding_ids: [&'a str; 0],
    authority_reconciled: bool,
}

#[derive(Serialize)]
struct BorrowedCoverage<'a> {
    universe_id: &'a StableId,
    denominator_obligation_ids: &'a BTreeSet<StableId>,
    visited_obligation_ids: &'a BTreeSet<StableId>,
    completed_obligation_ids: &'a BTreeSet<StableId>,
    selected: u64,
    visited: u64,
    completed: u64,
    verified: u8,
    accepted: u8,
}

#[derive(Serialize)]
struct BorrowedProjection<'a> {
    views: OneView<'a>,
}

struct OneView<'a>(BorrowedView<'a>);
impl Serialize for OneView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(1))?;
        sequence.serialize_element(&self.0)?;
        sequence.end()
    }
}

#[derive(Serialize)]
struct BorrowedView<'a> {
    kind: &'static str,
    source_ids: ViewSourceIds<'a>,
    information_loss: OneLoss<'a>,
    payload: BorrowedPayload<'a>,
}

#[derive(Clone, Copy)]
struct ViewSourceIds<'a> {
    execution_ids: &'a BTreeSet<StableId>,
    report_id: &'a StableId,
}
impl Serialize for ViewSourceIds<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let size = self.execution_ids.len().max(1);
        let mut sequence = serializer.serialize_seq(Some(size))?;
        if self.execution_ids.is_empty() {
            sequence.serialize_element(self.report_id)?;
        } else {
            for execution_id in self.execution_ids {
                sequence.serialize_element(execution_id)?;
            }
        }
        sequence.end()
    }
}

struct OneLoss<'a>(BorrowedLoss<'a>);
impl Serialize for OneLoss<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(1))?;
        sequence.serialize_element(&self.0)?;
        sequence.end()
    }
}

#[derive(Serialize)]
struct BorrowedLoss<'a> {
    kind: &'static str,
    reason: &'static str,
    source_ids: ViewSourceIds<'a>,
    affected_properties: [&'static str; 1],
    meaningful: bool,
    recoverable: bool,
    recovery_ref: &'a StableId,
}

#[derive(Serialize)]
struct BorrowedPayload<'a> {
    status: Status,
    execution_ids: &'a BTreeSet<StableId>,
    claim_ids: &'a BTreeSet<StableId>,
    obstruction_kinds: ObstructionKinds<'a>,
}

struct RegistrationRows<'a> {
    rows: &'a [reviewgraphen_store::IndexArtifactRegistration],
    registration_ids: &'a BTreeSet<StableId>,
}
impl Serialize for RegistrationRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.registration_ids.len()))?;
        for row in self
            .rows
            .iter()
            .filter(|row| self.registration_ids.contains(&row.registration_id))
        {
            let source: BorrowedReviewerSource<'_> = serde_json::from_str(&row.source_id)
                .map_err(<S::Error as serde::ser::Error>::custom)?;
            sequence.serialize_element(&BorrowedRegistration::new(row, source))?;
        }
        sequence.end()
    }
}

#[derive(Serialize)]
struct BorrowedRegistration<'a> {
    #[serde(rename = "event_sequence")]
    event_sequence: u64,
    #[serde(rename = "event_id")]
    event_id: &'a StableId,
    #[serde(rename = "run_id")]
    run_id: &'a StableId,
    #[serde(rename = "registration_id")]
    registration_id: &'a StableId,
    #[serde(rename = "cas_hash")]
    cas_hash: &'a reviewgraphen_core::ContentHash,
    #[serde(rename = "media_type")]
    media_type: &'a str,
    size: u64,
    sensitivity: &'static str,
    source: BorrowedRegistrationSource<'a>,
}

impl<'a> BorrowedRegistration<'a> {
    fn new(
        row: &'a reviewgraphen_store::IndexArtifactRegistration,
        source: BorrowedReviewerSource<'a>,
    ) -> Self {
        Self {
            event_sequence: row.event_sequence,
            event_id: &row.event_id,
            run_id: &row.run_id,
            registration_id: &row.registration_id,
            cas_hash: &row.cas_hash,
            media_type: &row.media_type,
            size: row.size,
            sensitivity: "sensitive",
            source: BorrowedRegistrationSource {
                kind: "reviewer_execution",
                run_id: source.run_id,
                execution_id: source.execution_id,
                reviewer_id: source.reviewer_id,
            },
        }
    }
}

#[derive(Serialize)]
struct BorrowedRegistrationSource<'a> {
    kind: &'static str,
    run_id: &'a str,
    execution_id: &'a str,
    reviewer_id: &'a str,
}

struct ExecutionRows<'a> {
    rows: &'a [reviewgraphen_store::IndexExecution],
    execution_ids: &'a BTreeSet<StableId>,
}
impl Serialize for ExecutionRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.execution_ids.len()))?;
        for row in self
            .rows
            .iter()
            .filter(|row| self.execution_ids.contains(&row.execution_id))
        {
            sequence.serialize_element(&BorrowedExecution(row))?;
        }
        sequence.end()
    }
}

struct BorrowedExecution<'a>(&'a reviewgraphen_store::IndexExecution);
impl Serialize for BorrowedExecution<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let row = self.0;
        let outcome: BorrowedOutcome<'_> = serde_json::from_str(&row.outcome_canonical_json)
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        BorrowedExecutionFields {
            event_sequence: row.event_sequence,
            event_id: &row.event_id,
            id: &row.execution_id,
            plan_id: &row.plan_id,
            wave_id: &row.wave_id,
            obligation_ids: CanonicalStringList(&row.obligation_ids_canonical_json),
            envelope_id: &row.envelope_id,
            snapshot_id: &row.snapshot_id,
            reviewer_kind: &row.reviewer_kind,
            reviewer_id: &row.reviewer_id,
            provider: row.provider.as_deref(),
            model: row.model.as_deref(),
            model_revision: row.model_revision.as_deref(),
            system_prompt_version: &row.system_prompt_version,
            prompt_template_version: &row.prompt_template_version,
            inference_settings: CanonicalStringMap(&row.inference_settings_canonical_json),
            tool_policy_version: &row.tool_policy_version,
            tool_calls: [&(); 0],
            attempt: row.attempt,
            raw_artifact_registration_id: &row.raw_registration_id,
            raw_artifact_hash: &row.raw_hash,
            parsed_claim_ids: CanonicalStringList(&row.parsed_claim_ids_canonical_json),
            outcome,
            identity_body_hash: &row.identity_body_hash,
            body_hash: &row.body_hash,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize)]
struct BorrowedExecutionFields<'a> {
    event_sequence: u64,
    event_id: &'a StableId,
    id: &'a StableId,
    plan_id: &'a StableId,
    wave_id: &'a StableId,
    obligation_ids: CanonicalStringList<'a>,
    envelope_id: &'a StableId,
    snapshot_id: &'a StableId,
    reviewer_kind: &'a str,
    reviewer_id: &'a str,
    provider: Option<&'a str>,
    model: Option<&'a str>,
    model_revision: Option<&'a str>,
    system_prompt_version: &'a str,
    prompt_template_version: &'a str,
    inference_settings: CanonicalStringMap<'a>,
    tool_policy_version: &'a str,
    tool_calls: [&'a (); 0],
    attempt: u32,
    raw_artifact_registration_id: &'a StableId,
    raw_artifact_hash: &'a reviewgraphen_core::ContentHash,
    parsed_claim_ids: CanonicalStringList<'a>,
    outcome: BorrowedOutcome<'a>,
    identity_body_hash: &'a reviewgraphen_core::ContentHash,
    body_hash: &'a reviewgraphen_core::ContentHash,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BorrowedOutcome<'a> {
    Structured,
    Abstained {
        #[serde(borrow)]
        reason: &'a str,
        #[serde(borrow)]
        detail: &'a str,
    },
    Malformed {
        #[serde(borrow)]
        reason: &'a str,
        #[serde(borrow)]
        diagnostic: &'a str,
    },
    ProviderFailure {
        retryable: bool,
        #[serde(borrow)]
        diagnostic: &'a str,
    },
}

struct ClaimRows<'a> {
    rows: &'a [reviewgraphen_store::IndexClaim],
    claim_ids: &'a BTreeSet<StableId>,
}
impl Serialize for ClaimRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.claim_ids.len()))?;
        for row in self
            .rows
            .iter()
            .filter(|row| self.claim_ids.contains(&row.claim_id))
        {
            sequence.serialize_element(&BorrowedClaim(row))?;
        }
        sequence.end()
    }
}

struct BorrowedClaim<'a>(&'a reviewgraphen_store::IndexClaim);
impl Serialize for BorrowedClaim<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let row = self.0;
        let candidate_confidence: Option<f64> =
            serde_json::from_str(&row.candidate_confidence_canonical_json)
                .map_err(<S::Error as serde::ser::Error>::custom)?;
        BorrowedClaimFields {
            event_sequence: row.event_sequence,
            event_id: &row.event_id,
            id: &row.claim_id,
            execution_id: &row.execution_id,
            obligation_ids: CanonicalStringList(&row.obligation_ids_canonical_json),
            property_id: &row.property_id,
            target_refs: CanonicalStringList(&row.target_refs_canonical_json),
            polarity: &row.polarity,
            disposition: &row.disposition,
            summary: &row.summary,
            source_ids: CanonicalStringList(&row.source_ids_canonical_json),
            assumptions: CanonicalStringList(&row.assumptions_canonical_json),
            requested_evidence: CanonicalStringList(&row.requested_evidence_canonical_json),
            candidate_confidence,
            author_kind: &row.author_kind,
            review_status: &row.review_status,
            identity_body_hash: &row.identity_body_hash,
            body_hash: &row.body_hash,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize)]
struct BorrowedClaimFields<'a> {
    event_sequence: u64,
    event_id: &'a StableId,
    id: &'a StableId,
    execution_id: &'a StableId,
    obligation_ids: CanonicalStringList<'a>,
    property_id: &'a str,
    target_refs: CanonicalStringList<'a>,
    polarity: &'a str,
    disposition: &'a str,
    summary: &'a str,
    source_ids: CanonicalStringList<'a>,
    assumptions: CanonicalStringList<'a>,
    requested_evidence: CanonicalStringList<'a>,
    candidate_confidence: Option<f64>,
    author_kind: &'a str,
    review_status: &'a str,
    identity_body_hash: &'a reviewgraphen_core::ContentHash,
    body_hash: &'a reviewgraphen_core::ContentHash,
}

#[derive(Clone, Copy)]
struct CanonicalStringList<'a>(&'a str);
impl Serialize for CanonicalStringList<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut output = serializer.serialize_seq(None)?;
        let mut input = serde_json::Deserializer::from_str(self.0);
        input
            .deserialize_seq(ForwardStringList(&mut output))
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        input
            .end()
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        output.end()
    }
}

struct ForwardStringList<'a, S>(&'a mut S);
impl<'de, S: SerializeSeq> Visitor<'de> for ForwardStringList<'_, S> {
    type Value = ();
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON string array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut input: A) -> Result<Self::Value, A::Error> {
        while let Some(value) = input.next_element::<&'de str>()? {
            self.0
                .serialize_element(value)
                .map_err(|error| serde::de::Error::custom(error.to_string()))?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct CanonicalStringMap<'a>(&'a str);
impl Serialize for CanonicalStringMap<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut output = serializer.serialize_map(None)?;
        let mut input = serde_json::Deserializer::from_str(self.0);
        input
            .deserialize_map(ForwardStringMap(&mut output))
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        input
            .end()
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        output.end()
    }
}

struct ForwardStringMap<'a, S>(&'a mut S);
impl<'de, S: SerializeMap> Visitor<'de> for ForwardStringMap<'_, S> {
    type Value = ();
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON string map")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut input: A) -> Result<Self::Value, A::Error> {
        while let Some((key, value)) = input.next_entry::<&'de str, &'de str>()? {
            self.0
                .serialize_entry(key, value)
                .map_err(|error| serde::de::Error::custom(error.to_string()))?;
        }
        Ok(())
    }
}

struct ObstructionRows<'a> {
    status: Status,
    executions: &'a [reviewgraphen_store::IndexExecution],
    execution_ids: &'a BTreeSet<StableId>,
    pre: &'a [PreReviewObstruction],
}
impl Serialize for ObstructionRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let count = match self.status {
            Status::UnsupportedInput => self.pre.len(),
            _ => self
                .executions
                .iter()
                .filter(|row| {
                    self.execution_ids.contains(&row.execution_id)
                        && row.outcome_kind != "structured"
                })
                .count(),
        };
        let mut sequence = serializer.serialize_seq(Some(count))?;
        match self.status {
            Status::UnsupportedInput => {
                for obstruction in self.pre {
                    sequence.serialize_element(&BorrowedPreObstruction(obstruction))?;
                }
            }
            _ => {
                for row in self.executions.iter().filter(|row| {
                    self.execution_ids.contains(&row.execution_id)
                        && row.outcome_kind != "structured"
                }) {
                    let kind = reviewer_obstruction_kind(&row.outcome_kind).ok_or_else(|| {
                        <S::Error as serde::ser::Error>::custom("unknown reviewer outcome kind")
                    })?;
                    sequence.serialize_element(&BorrowedReviewerObstruction {
                        kind,
                        message: "The selected obligation remains in progress after this reviewer outcome.",
                        source_ids: [&row.execution_id],
                        blocks: CanonicalStringList(&row.obligation_ids_canonical_json),
                    })?;
                }
            }
        }
        sequence.end()
    }
}

struct BorrowedPreObstruction<'a>(&'a PreReviewObstruction);
impl Serialize for BorrowedPreObstruction<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Fields<'a> {
            kind: &'static str,
            message: &'a str,
            source_ids: &'a BTreeSet<StableId>,
            blocks: &'a BTreeSet<StableId>,
        }
        Fields {
            kind: "pre_review_unsupported_input",
            message: &self.0.message,
            source_ids: &self.0.source_ids,
            blocks: &self.0.blocks,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize)]
struct BorrowedReviewerObstruction<'a> {
    kind: &'static str,
    message: &'static str,
    source_ids: [&'a StableId; 1],
    blocks: CanonicalStringList<'a>,
}

struct ObstructionKinds<'a> {
    executions: &'a [reviewgraphen_store::IndexExecution],
    execution_ids: &'a BTreeSet<StableId>,
}
impl Serialize for ObstructionKinds<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let known = [
            ("abstained", "reviewer_abstained"),
            ("malformed", "reviewer_malformed"),
            ("provider_failure", "reviewer_provider_failure"),
        ];
        let count = known
            .iter()
            .filter(|(outcome, _)| {
                self.executions.iter().any(|row| {
                    self.execution_ids.contains(&row.execution_id) && row.outcome_kind == *outcome
                })
            })
            .count();
        let mut sequence = serializer.serialize_seq(Some(count))?;
        for (outcome, obstruction) in known {
            if self.executions.iter().any(|row| {
                self.execution_ids.contains(&row.execution_id) && row.outcome_kind == outcome
            }) {
                sequence.serialize_element(obstruction)?;
            }
        }
        sequence.end()
    }
}

fn reviewer_obstruction_kind(outcome_kind: &str) -> Option<&'static str> {
    match outcome_kind {
        "abstained" => Some("reviewer_abstained"),
        "malformed" => Some("reviewer_malformed"),
        "provider_failure" => Some("reviewer_provider_failure"),
        _ => None,
    }
}

fn registration(
    row: &reviewgraphen_store::IndexArtifactRegistration,
) -> Result<Registration, ReportError> {
    let source = reviewer_source(row)?;
    Ok(Registration {
        event_sequence: row.event_sequence,
        event_id: row.event_id.to_string(),
        run_id: row.run_id.to_string(),
        registration_id: row.registration_id.to_string(),
        cas_hash: row.cas_hash.to_string(),
        media_type: row.media_type.clone(),
        size: row.size,
        sensitivity: "sensitive",
        source: RegistrationSource {
            kind: "reviewer_execution",
            run_id: source.run_id.to_owned(),
            execution_id: source.execution_id.to_owned(),
            reviewer_id: source.reviewer_id.to_owned(),
        },
    })
}
fn execution(row: &reviewgraphen_store::IndexExecution) -> Result<Execution, ReportError> {
    Ok(Execution {
        event_sequence: row.event_sequence,
        event_id: row.event_id.to_string(),
        id: row.execution_id.to_string(),
        plan_id: row.plan_id.to_string(),
        wave_id: row.wave_id.to_string(),
        obligation_ids: strings(&ids(&row.obligation_ids_canonical_json)?),
        envelope_id: row.envelope_id.to_string(),
        snapshot_id: row.snapshot_id.to_string(),
        reviewer_kind: row.reviewer_kind.clone(),
        reviewer_id: row.reviewer_id.clone(),
        provider: row.provider.clone(),
        model: row.model.clone(),
        model_revision: row.model_revision.clone(),
        system_prompt_version: row.system_prompt_version.clone(),
        prompt_template_version: row.prompt_template_version.clone(),
        inference_settings: serde_json::from_str(&row.inference_settings_canonical_json)
            .map_err(|_| ReportError::Json)?,
        tool_policy_version: row.tool_policy_version.clone(),
        tool_calls: Vec::new(),
        attempt: row.attempt,
        raw_artifact_registration_id: row.raw_registration_id.to_string(),
        raw_artifact_hash: row.raw_hash.to_string(),
        parsed_claim_ids: strings(&ids(&row.parsed_claim_ids_canonical_json)?),
        outcome: serde_json::from_str(&row.outcome_canonical_json)
            .map_err(|_| ReportError::Json)?,
        identity_body_hash: row.identity_body_hash.to_string(),
        body_hash: row.body_hash.to_string(),
    })
}
fn claim(row: &reviewgraphen_store::IndexClaim) -> Result<Claim, ReportError> {
    Ok(Claim {
        event_sequence: row.event_sequence,
        event_id: row.event_id.to_string(),
        id: row.claim_id.to_string(),
        execution_id: row.execution_id.to_string(),
        obligation_ids: strings(&ids(&row.obligation_ids_canonical_json)?),
        property_id: row.property_id.clone(),
        target_refs: strings(&ids(&row.target_refs_canonical_json)?),
        polarity: row.polarity.clone(),
        disposition: row.disposition.clone(),
        summary: row.summary.clone(),
        source_ids: strings(&ids(&row.source_ids_canonical_json)?),
        assumptions: serde_json::from_str(&row.assumptions_canonical_json)
            .map_err(|_| ReportError::Json)?,
        requested_evidence: serde_json::from_str(&row.requested_evidence_canonical_json)
            .map_err(|_| ReportError::Json)?,
        candidate_confidence: serde_json::from_str(&row.candidate_confidence_canonical_json)
            .map_err(|_| ReportError::Json)?,
        author_kind: row.author_kind.clone(),
        review_status: row.review_status.clone(),
        identity_body_hash: row.identity_body_hash.to_string(),
        body_hash: row.body_hash.to_string(),
    })
}
fn obstructions(
    status: Status,
    executions: &[reviewgraphen_store::IndexExecution],
    execution_ids: &BTreeSet<StableId>,
    pre: &[PreReviewObstruction],
) -> Result<Vec<Obstruction>, ReportError> {
    let mut values: Vec<Obstruction> = match status {
        Status::UnsupportedInput => pre
            .iter()
            .map(|o| Obstruction {
                kind: "pre_review_unsupported_input".into(),
                message: o.message.clone(),
                source_ids: strings(&o.source_ids),
                blocks: strings(&o.blocks),
            })
            .collect(),
        _ => executions
            .iter()
            .filter(|e| execution_ids.contains(&e.execution_id) && e.outcome_kind != "structured")
            .map(|e| Obstruction {
                kind: format!("reviewer_{}", e.outcome_kind),
                message: "The selected obligation remains in progress after this reviewer outcome."
                    .into(),
                source_ids: vec![e.execution_id.to_string()],
                blocks: strings(&ids(&e.obligation_ids_canonical_json).expect("validated index")),
            })
            .collect(),
    };
    if !matches!(status, Status::UnsupportedInput) {
        values.sort_by(compare_obstructions);
    }
    if values
        .windows(2)
        .any(|pair| compare_obstructions(&pair[0], &pair[1]) != Ordering::Less)
    {
        return Err(ReportError::Source(
            "obstructions are not canonical and duplicate-free",
        ));
    }
    Ok(values)
}
fn compare_obstructions(left: &Obstruction, right: &Obstruction) -> Ordering {
    compare_json_string_lists(
        left.blocks.iter().map(String::as_str),
        right.blocks.iter().map(String::as_str),
    )
    .then_with(|| compare_json_strings(&left.kind, &right.kind))
    .then_with(|| compare_json_strings(&left.message, &right.message))
    .then_with(|| {
        compare_json_string_lists(
            left.source_ids.iter().map(String::as_str),
            right.source_ids.iter().map(String::as_str),
        )
    })
}
fn view(
    status: Status,
    execution_ids: &BTreeSet<StableId>,
    claim_ids: &BTreeSet<StableId>,
    executions: &[reviewgraphen_store::IndexExecution],
    report_id: &StableId,
) -> View {
    let source_ids = if execution_ids.is_empty() {
        vec![report_id.to_string()]
    } else {
        strings(execution_ids)
    };
    let kinds = executions
        .iter()
        .filter(|e| execution_ids.contains(&e.execution_id) && e.outcome_kind != "structured")
        .map(|e| format!("reviewer_{}", e.outcome_kind))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    View {
        kind: "machine",
        source_ids: source_ids.clone(),
        information_loss: vec![Loss {
            kind: "authority_coverage_omitted",
            reason: "This authority-free D2 projection does not infer evidence, verification, freshness, or human acceptance.",
            source_ids,
            affected_properties: vec!["review.authority_coverage"],
            meaningful: true,
            recoverable: true,
            recovery_ref: report_id.to_string(),
        }],
        payload: Payload {
            status,
            execution_ids: strings(execution_ids),
            claim_ids: strings(claim_ids),
            obstruction_kinds: kinds,
        },
    }
}

fn validate_local_report(
    report: &Report<'_>,
    expected_registrations: usize,
    expected_executions: usize,
    expected_claims: usize,
) -> Result<(), ReportError> {
    if report.result.authority_records.authority_reconciled
        || !report.result.authority_records.evidence_ids.is_empty()
        || !report.result.authority_records.verification_ids.is_empty()
        || !report.result.authority_records.decision_ids.is_empty()
        || !report.result.authority_records.finding_ids.is_empty()
        || report.coverage.verified != 0
        || report.coverage.accepted != 0
    {
        return Err(ReportError::Source("D2 report authority must remain empty"));
    }
    if report.coverage.selected
        != u64::try_from(report.scenario.selected_obligation_ids.len()).unwrap_or(u64::MAX)
        || report.coverage.visited
            != u64::try_from(report.coverage.visited_obligation_ids.len()).unwrap_or(u64::MAX)
        || report.coverage.completed
            != u64::try_from(report.coverage.completed_obligation_ids.len()).unwrap_or(u64::MAX)
        || report.coverage.universe_id != report.scenario.universe_id
    {
        return Err(ReportError::Source("coverage count/source mismatch"));
    }
    if report.result.artifact_registrations.len() != expected_registrations
        || report.scenario.artifact_registration_ids.len() != expected_registrations
        || report.result.executions.len() != expected_executions
        || report.result.claims.len() != expected_claims
    {
        return Err(ReportError::Source("report source set size mismatch"));
    }
    if report.result.artifact_registrations.windows(2).any(|pair| {
        (pair[0].event_sequence, pair[0].registration_id.as_str())
            >= (pair[1].event_sequence, pair[1].registration_id.as_str())
    }) || report.result.executions.windows(2).any(|pair| {
        (pair[0].event_sequence, pair[0].id.as_str())
            >= (pair[1].event_sequence, pair[1].id.as_str())
    }) || report.result.claims.windows(2).any(|pair| {
        (pair[0].event_sequence, pair[0].id.as_str())
            >= (pair[1].event_sequence, pair[1].id.as_str())
    }) {
        return Err(ReportError::Source("report record order mismatch"));
    }
    for obstruction in &report.result.obstructions {
        if obstruction.message.is_empty()
            || obstruction.source_ids.is_empty()
            || obstruction.blocks.is_empty()
            || !obstruction.blocks.iter().all(|value| {
                report
                    .scenario
                    .selected_obligation_ids
                    .binary_search(value)
                    .is_ok()
            })
        {
            return Err(ReportError::Source("obstruction source/block mismatch"));
        }
    }
    if report.projection.views.is_empty() {
        return Err(ReportError::Source("projection view is required"));
    }
    for view in &report.projection.views {
        if view.source_ids.is_empty()
            || view.information_loss.is_empty()
            || view.payload.status != report.result.status
        {
            return Err(ReportError::Source("projection source/status mismatch"));
        }
        for loss in &view.information_loss {
            if !loss.meaningful
                || !loss.recoverable
                || loss.source_ids.is_empty()
                || loss.affected_properties.is_empty()
                || loss.recovery_ref != report.metadata.report_id
            {
                return Err(ReportError::Source("projection loss source mismatch"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{PreReviewObstruction, ReportRequest, generate_v2};
    use reviewgraphen_core::{
        ContentHash, EventCommand, EventLog, MvpRulePack, PlanBudget, ProgramSpace,
        ReviewAggregate, StableId, plan,
    };
    use reviewgraphen_store::{
        CasHash, CasStore, DerivedIndex, EventJournal, JournalGenesis, JournalIdentity,
        StoreLimits, StoreRoot,
    };
    use serde_json::Value;
    use std::{
        collections::{BTreeMap, BTreeSet},
        io::Cursor,
    };

    fn schema() -> Value {
        serde_json::from_str(include_str!(
            "../../../schemas/reviewgraphen.report.v2.schema.json"
        ))
        .unwrap()
    }

    #[test]
    fn checked_in_v2_example_validates_against_the_contract() {
        let example: Value = serde_json::from_str(include_str!(
            "../../../schemas/reviewgraphen.report.v2.example.json"
        ))
        .unwrap();
        let validator = jsonschema::validator_for(&schema()).unwrap();
        assert!(validator.is_valid(&example));
    }

    #[test]
    fn source_bound_unsupported_input_is_deterministic_and_schema_valid() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let mut input: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        input["profile"]["rule_set_hash"] = Value::String(format!("sha256:{}", "3".repeat(64)));
        input["extraction"]["adapter_set_hash"] =
            Value::String(format!("sha256:{}", "7".repeat(64)));
        let program: ProgramSpace = serde_json::from_value(input).unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let run_id = StableId::parse("run:report-unsupported").unwrap();
        let mut log = EventLog::new(
            run_id.clone(),
            ReviewAggregate::new(program, universe, obligations).unwrap(),
        )
        .unwrap();
        let genesis = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let cas = CasStore::open(&root).unwrap();
        let genesis_hash = CasHash::parse(ContentHash::sha256(&genesis).to_string()).unwrap();
        cas.put(
            &genesis_hash,
            Some(genesis.len() as u64),
            Cursor::new(&genesis),
        )
        .unwrap();
        let identity = JournalIdentity::new(run_id.clone(), JournalGenesis::V2(genesis)).unwrap();
        let journal = EventJournal::initialize_v2(
            &root,
            identity.clone(),
            log.envelopes().next().unwrap().clone(),
        )
        .unwrap();
        let plan = plan(log.aggregate(), PlanBudget::new(16, 16).unwrap()).unwrap();
        let selected = plan.waves()[0].obligation_ids()[0].clone();
        log.append(EventCommand::review_plan_recorded(plan.clone()))
            .unwrap();
        journal
            .writer()
            .unwrap()
            .append(log.envelopes().nth(1).unwrap().clone())
            .unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        index.rebuild(&journal, &cas).unwrap();

        let request = ReportRequest {
            report_id: StableId::parse("report:unsupported-source-bound").unwrap(),
            repository_id: StableId::parse("repository:double-submit-payment").unwrap(),
            program_space_ref: StableId::parse("program-space:snapshot:double-submit-v1").unwrap(),
            plan_id: plan.id().clone(),
            selected_obligation_ids: BTreeSet::from([selected.clone()]),
            tool_versions: BTreeMap::from([(
                "reviewgraphen.fake-reviewer".to_owned(),
                "1".to_owned(),
            )]),
            pre_review_obstructions: vec![PreReviewObstruction {
                message: "The fixture intentionally has no reviewer attempt.".to_owned(),
                source_ids: BTreeSet::from([selected.clone()]),
                blocks: BTreeSet::from([selected]),
            }],
        };
        let first = generate_v2(&root, identity.clone(), &request).unwrap();
        let second = generate_v2(&root, identity, &request).unwrap();
        assert_eq!(first.canonical_bytes, second.canonical_bytes);
        let report: Value = serde_json::from_slice(&first.canonical_bytes).unwrap();
        assert_eq!(report["result"]["status"], "unsupported_input");
        assert!(
            jsonschema::validator_for(&schema())
                .unwrap()
                .is_valid(&report)
        );
    }

    #[test]
    fn abbreviated_metadata_hash_is_a_typed_rejection() {
        let hash = ContentHash::parse("sha256:3333333333333333").unwrap();
        assert!(matches!(
            super::validate_metadata_hash("rule_set_hash", &hash),
            Err(super::ReportError::UnsupportedMetadataHash {
                field: "rule_set_hash",
                ..
            })
        ));
    }
}
