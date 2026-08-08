use crate::{
    CanonicalJson, Coverage, DomainError, InformationLoss, Result, ReviewAggregate, StableId,
    canonical_hash,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Audience-specific result projection kinds supported by M1.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionKind {
    /// Concise human review summary.
    Human,
    /// Stable IDs and next-state information for an automated reviewer.
    Ai,
    /// Trace-oriented record for audit/replay investigation.
    Audit,
}

/// A non-canonical view that retains sources and declares meaningful loss.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Projection {
    kind: ProjectionKind,
    source_ids: BTreeSet<StableId>,
    information_loss: Vec<InformationLoss>,
    payload: BTreeMap<String, Value>,
    projection_hash: String,
}

/// Alias documenting a human-facing result projection.
pub type HumanProjection = Projection;
/// Alias documenting an AI-facing result projection.
pub type AuditProjection = Projection;

/// Schema-compatible deterministic `reviewgraphen.review.report.v1` adapter.
///
/// [`Projection`] remains an internal audience view with declared loss; this
/// type is the only public report-shaped output in M1. For each execution
/// reference retained by a claim, it emits an explicitly abstained and
/// unresolved schema placeholder plus an obstruction; it does not claim that
/// M1 observed a reviewer or context envelope. It never invents gluing or
/// stale-record semantics and reports those arrays as empty rather than
/// implying they occurred.
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewReport {
    body: Value,
    canonical: CanonicalJson,
}

impl Serialize for ReviewReport {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.body.serialize(serializer)
    }
}

impl ReviewReport {
    /// Adapts one validated M1 aggregate into the checked-in report contract.
    pub fn from_aggregate(run_id: StableId, aggregate: &ReviewAggregate) -> Result<Self> {
        if run_id.kind() != "run" {
            return Err(DomainError::Validation(
                "report generation requires a run ID".to_owned(),
            ));
        }
        aggregate.validate()?;
        let report_id = StableId::derived(
            "report",
            &BTreeMap::from([
                ("run".to_owned(), Value::String(run_id.to_string())),
                (
                    "snapshot".to_owned(),
                    Value::String(aggregate.program().snapshot_id().to_string()),
                ),
                (
                    "universe".to_owned(),
                    Value::String(aggregate.universe().id().to_string()),
                ),
            ]),
        )?;
        let coverage = Coverage::from_aggregate(aggregate);
        let human = Projection::human(aggregate)?;
        let ai = Projection::ai(aggregate)?;
        let audit = Projection::audit(aggregate)?;
        let ci_loss = vec![loss(
            "m1_no_gluing",
            "M1 does not perform cross-context gluing; CI status remains partial.",
            false,
            None,
            BTreeSet::from([aggregate.universe().id().clone()]),
        )];
        let ci = Projection::new(
            ProjectionKind::Ai,
            BTreeSet::from([aggregate.universe().id().clone()]),
            ci_loss,
            BTreeMap::from([("status".to_owned(), json!("partial"))]),
        )?;
        let claims = aggregate
            .claims()
            .map(|claim| {
                json!({
                    "id": claim.id(),
                    "execution_id": claim.execution_id(),
                    "obligation_ids": claim.obligation_ids(),
                    "polarity": claim.polarity(),
                    "disposition": claim.disposition(),
                    "summary": claim.summary(),
                    "candidate_confidence": claim.candidate_confidence(),
                    "source_ids": claim.source_ids(),
                    "limitations": ["execution_metadata_unavailable"],
                    "review_status": claim.review_status(),
                })
            })
            .collect::<Vec<_>>();
        let unresolved_executions = unresolved_executions(aggregate);
        let executions = report_executions(&unresolved_executions);
        let bindings = aggregate
            .bindings()
            .map(|binding| {
                json!({
                    "id": binding.id(),
                    "claim_id": binding.claim_id(),
                    "evidence_id": binding.evidence_id(),
                    "relation": binding.relation(),
                    "scope": binding.scope(),
                })
            })
            .collect::<Vec<_>>();
        let verifications = aggregate
            .verifications()
            .map(|verification| {
                json!({
                    "id": verification.id(),
                    "claim_id": verification.claim_id(),
                    "result": verification.outcome(),
                    "verifier_id": verification.verifier_id(),
                    "evidence_ids": verification.evidence_ids(),
                    "limitations": [],
                    "freshness": verification.freshness(),
                })
            })
            .collect::<Vec<_>>();
        let decisions = aggregate
            .decisions()
            .map(|decision| {
                json!({
                    "id": decision.id(),
                    "target_id": decision.target_claim_id(),
                    "outcome": decision.outcome(),
                    "authority": decision.authority(),
                    "rationale": decision.rationale(),
                    "source_ids": decision.source_ids(),
                })
            })
            .collect::<Vec<_>>();
        let findings = aggregate
            .findings()
            .map(|finding| {
                let title = aggregate
                    .claims()
                    .find(|claim| claim.id() == finding.claim_id())
                    .map_or("M1 review finding", |claim| claim.summary());
                json!({
                    "id": finding.id(),
                    "claim_id": finding.claim_id(),
                    "severity": "medium",
                    "title": title,
                    "status": finding.status(),
                    "evidence_ids": finding.evidence_ids(),
                    "verification_ids": finding.verification_ids(),
                    "decision_id": finding.decision_id(),
                    "location_refs": finding.source_ids(),
                })
            })
            .collect::<Vec<_>>();
        let freshness = aggregate
            .verifications()
            .fold((0_u64, 0_u64, 0_u64), |counts, item| {
                match item.freshness() {
                    crate::Freshness::Fresh => (counts.0 + 1, counts.1, counts.2),
                    crate::Freshness::Stale => (counts.0, counts.1 + 1, counts.2),
                    crate::Freshness::Unknown => (counts.0, counts.1, counts.2 + 1),
                }
            });
        let mut obstructions = aggregate
            .obligations()
            .filter(|obligation| obligation.applicability_status() == "unknown")
            .flat_map(|obligation| {
                obligation
                    .applicability_reasons()
                    .iter()
                    .filter_map(move |reason| {
                        capability_gap_kind(reason).map(|kind| (obligation, reason, kind))
                    })
            })
            .map(|(obligation, reason, kind)| {
                Ok(json!({
                    "id": StableId::derived(
                        "obstruction",
                        &BTreeMap::from([
                            ("obligation".to_owned(), Value::String(obligation.id().to_string())),
                            ("reason".to_owned(), Value::String(reason.clone())),
                            ("universe".to_owned(), Value::String(aggregate.universe().id().to_string())),
                        ]),
                    )?,
                    "kind": kind,
                    "severity": "medium",
                    "title": capability_gap_obstruction_title(kind),
                    "source_ids": obligation.source_ids(),
                    "required_resolution": [reason.clone()],
                    "blocks": [obligation.id().to_string()],
                    "review_status": "unreviewed",
                }))
            })
            .collect::<Result<Vec<_>>>()?;
        obstructions.extend(
            unresolved_executions
                .iter()
                .map(|execution| execution.obstruction(aggregate.universe().id()))
                .collect::<Result<Vec<_>>>()?,
        );
        obstructions.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
        // Every capability-gap reason on an "unknown" obligation is already
        // conveyed above as its own typed obstruction, and the qualifying
        // limitation it points back to (when one exists) is a real
        // `Extraction.limitations` entry included below. Re-emitting a
        // second limitation keyed by the same `qualification_id` would
        // duplicate that real record's ID with fabricated generic
        // kind/description/severity, silently overwriting its actual
        // classification (for example turning a `partial` capability's real
        // `projection_loss` limitation into a fake `capability_missing`
        // one). So this loop never invents a limitation from an obligation;
        // it only lists the limitations the ProgramSpace actually declared.
        let mut report_limitations = aggregate
            .program()
            .extraction()
            .limitations
            .iter()
            .map(|limitation| {
                json!({
                    "id": limitation.id,
                    "kind": limitation.kind,
                    "description": limitation.description,
                    "severity": limitation.severity,
                    "source_ids": limitation.source_ids,
                })
            })
            .collect::<Vec<_>>();
        report_limitations.extend(
            unresolved_executions
                .iter()
                .map(UnresolvedExecution::limitation)
                .collect::<Result<Vec<_>>>()?,
        );
        report_limitations.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
        let body = json!({
            "schema": "reviewgraphen.review.report.v1",
            "report_type": "review",
            "report_version": 1,
            "metadata": {
                "report_id": report_id,
                "run_id": run_id,
                "generated_at": "1970-01-01T00:00:00Z",
                "tool_versions": { "reviewgraphen-core": "0.1.0-m1" },
                "profile_id": aggregate.universe().profile_id(),
                "rule_set_hash": aggregate.universe().rule_set_hash(),
                "policy_version": aggregate.universe().policy_version(),
            },
            "scenario": {
                "repository_id": aggregate.program().repository_id(),
                "snapshot_id": aggregate.program().snapshot_id(),
                "base_revision": aggregate.program().base_revision(),
                "target_revision": aggregate.program().target_revision(),
                "program_space_ref": format!("program-space:{}", aggregate.program().snapshot_id()),
                "review_space_ref": format!("review-space:{}", aggregate.universe().id()),
                "evidence_space_ref": format!("evidence-space:{}", aggregate.program().snapshot_id()),
                "universe_id": aggregate.universe().id(),
                "extraction_limitation_ids": aggregate.universe().limitation_ids(),
            },
            "result": {
                "status": "partial",
                "obligation_ids": aggregate.universe().obligation_ids(),
                "executions": executions,
                "claims": claims,
                "evidence_bindings": bindings,
                "verifications": verifications,
                "decisions": decisions,
                "findings": findings,
                "obstructions": obstructions,
                "completion_candidates": [],
                "gluing_results": [],
                "stale_records": [],
            },
            "coverage": {
                "universe_id": aggregate.universe().id(),
                "extraction": aggregate.program().extraction(),
                "stages": {
                    "generated": report_ratio(coverage.denominator, coverage.denominator, 1.0),
                    "visited": report_ratio(
                        aggregate
                            .obligations()
                            .filter(|item| {
                                matches!(
                                    item.lifecycle(),
                                    crate::ObligationLifecycle::InProgress
                                        | crate::ObligationLifecycle::Completed
                                )
                            })
                            .count(),
                        coverage.denominator,
                        lifecycle_weighted_ratio(aggregate),
                    ),
                    "completed": measure_value(&coverage.raw),
                    "evidence_supported": measure_value(&coverage.evidence_supported),
                    "verified": measure_value(&coverage.verified),
                    "fresh_verified": measure_value(&coverage.fresh),
                },
                "limitations": report_limitations,
                "freshness": { "fresh": freshness.0, "stale": freshness.1, "unknown": freshness.2 },
            },
            "projection": {
                "human_review": projection_value(&human),
                "ai_view": projection_value(&ai),
                "audit_trace": projection_value(&audit),
                "ci_gate": projection_value(&ci),
            },
        });
        let canonical = CanonicalJson::from_serializable(&body)?;
        Ok(Self { body, canonical })
    }

    /// Immutable canonical report bytes and hash.
    #[must_use]
    pub fn canonical(&self) -> &CanonicalJson {
        &self.canonical
    }

    /// Read-only JSON body for schema validation or transport.
    #[must_use]
    pub fn body(&self) -> &Value {
        &self.body
    }
}

const EXECUTION_METADATA_UNAVAILABLE: &str = "execution_metadata_unavailable";
const UNRESOLVED_CONTEXT_ENVELOPE_ID: &str = "context:unresolved";
const UNRESOLVED_EXECUTION_REASON: &str =
    "M1 retains the claim execution reference but does not retain reviewer or context metadata.";

#[derive(Clone, Debug)]
struct UnresolvedExecution {
    id: StableId,
    obligation_ids: BTreeSet<StableId>,
    claim_ids: BTreeSet<StableId>,
}

impl UnresolvedExecution {
    fn limitation(&self) -> Result<Value> {
        let id = StableId::derived(
            "limitation",
            &BTreeMap::from([
                ("execution".to_owned(), Value::String(self.id.to_string())),
                (
                    "kind".to_owned(),
                    Value::String(EXECUTION_METADATA_UNAVAILABLE.to_owned()),
                ),
            ]),
        )?;
        Ok(json!({
            "id": id,
            "kind": "projection_loss",
            "description": UNRESOLVED_EXECUTION_REASON,
            "severity": "info",
            "source_ids": self.claim_ids,
        }))
    }

    fn obstruction(&self, universe_id: &StableId) -> Result<Value> {
        let id = StableId::derived(
            "obstruction",
            &BTreeMap::from([
                ("execution".to_owned(), Value::String(self.id.to_string())),
                (
                    "kind".to_owned(),
                    Value::String(EXECUTION_METADATA_UNAVAILABLE.to_owned()),
                ),
                (
                    "universe".to_owned(),
                    Value::String(universe_id.to_string()),
                ),
            ]),
        )?;
        let blocks = self
            .claim_ids
            .iter()
            .chain(self.obligation_ids.iter())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        Ok(json!({
            "id": id,
            "kind": EXECUTION_METADATA_UNAVAILABLE,
            "severity": "info",
            "title": "Execution metadata is unavailable in M1",
            "source_ids": self.claim_ids,
            "required_resolution": [
                "Record reviewer and context-envelope metadata in an execution-capable milestone."
            ],
            "blocks": blocks,
            "review_status": "unreviewed",
        }))
    }
}

fn unresolved_executions(aggregate: &ReviewAggregate) -> Vec<UnresolvedExecution> {
    let mut executions = BTreeMap::<StableId, (BTreeSet<StableId>, BTreeSet<StableId>)>::new();
    for claim in aggregate.claims() {
        let entry = executions.entry(claim.execution_id().clone()).or_default();
        entry.0.extend(claim.obligation_ids().iter().cloned());
        entry.1.insert(claim.id().clone());
    }
    executions
        .into_iter()
        .map(|(id, (obligation_ids, claim_ids))| UnresolvedExecution {
            id,
            obligation_ids,
            claim_ids,
        })
        .collect()
}

fn report_executions(executions: &[UnresolvedExecution]) -> Vec<Value> {
    executions
        .iter()
        .map(|execution| {
            json!({
                "id": execution.id,
                "obligation_ids": execution.obligation_ids,
                "reviewer": {
                    "kind": "unknown",
                    "identity": "unresolved",
                },
                "context_envelope_id": UNRESOLVED_CONTEXT_ENVELOPE_ID,
                "status": "abstained",
                "claim_ids": execution.claim_ids,
                "abstention_reason": UNRESOLVED_EXECUTION_REASON,
            })
        })
        .collect()
}

/// Classifies a `synthesize::capability_gap_reason` tag into its typed
/// obstruction `kind`, or `None` for a reason this is not one of (for
/// example `origin_rule:*`). Keeps the four capability-gap reason kinds
/// (`partial`/`missing`/`unknown`/undeclared) distinguishable at the
/// obstruction level instead of collapsing them into one kind.
fn capability_gap_kind(reason: &str) -> Option<&'static str> {
    match reason.split_once(':')?.0 {
        "capability_partial" => Some("capability_partial"),
        "capability_missing" => Some("capability_missing"),
        "capability_unknown" => Some("capability_unknown"),
        "capability_undeclared" => Some("capability_undeclared"),
        _ => None,
    }
}

fn capability_gap_obstruction_title(kind: &str) -> &'static str {
    match kind {
        "capability_partial" => "Required extraction capability is only partially available",
        "capability_unknown" => "Required extraction capability completeness is unknown",
        "capability_undeclared" => "Required extraction capability was never declared",
        _ => "Required extraction capability is unavailable",
    }
}

fn report_ratio(numerator: usize, denominator: usize, weighted: f64) -> Value {
    json!({ "numerator": numerator, "denominator": denominator, "weighted": weighted })
}

fn measure_value(value: &crate::CoverageMeasure) -> Value {
    report_ratio(
        value.raw.numerator as usize,
        value.raw.denominator as usize,
        value.weighted.percentage,
    )
}

fn lifecycle_weighted_ratio(aggregate: &ReviewAggregate) -> f64 {
    let total = aggregate
        .obligations()
        .map(|item| item.weight())
        .sum::<f64>();
    let visited = aggregate
        .obligations()
        .filter(|item| {
            matches!(
                item.lifecycle(),
                crate::ObligationLifecycle::InProgress | crate::ObligationLifecycle::Completed
            )
        })
        .map(|item| item.weight())
        .sum::<f64>();
    if total == 0.0 { 0.0 } else { visited / total }
}

fn projection_value(projection: &Projection) -> Value {
    let mut value = json!({
        "source_ids": projection.source_ids(),
        "information_loss": projection.information_loss(),
        "payload": projection.payload(),
    });
    remove_report_nulls(&mut value);
    value
}

fn remove_report_nulls(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(remove_report_nulls),
        Value::Object(entries) => {
            entries.retain(|_, item| !item.is_null());
            entries.values_mut().for_each(remove_report_nulls);
        }
        _ => {}
    }
}

impl Projection {
    /// Creates a validated projection. Every projection declares at least one
    /// information-loss record; a report is never canonical state.
    pub fn new(
        kind: ProjectionKind,
        source_ids: BTreeSet<StableId>,
        information_loss: Vec<InformationLoss>,
        payload: BTreeMap<String, Value>,
    ) -> Result<Self> {
        if source_ids.is_empty() || information_loss.is_empty() {
            return Err(DomainError::Validation(
                "projection requires source IDs and an explicit information-loss declaration"
                    .to_owned(),
            ));
        }
        let projection_hash = canonical_hash(&(kind, &source_ids, &information_loss, &payload))?;
        Ok(Self {
            kind,
            source_ids,
            information_loss,
            payload,
            projection_hash,
        })
    }

    /// Intended audience.
    #[must_use]
    pub fn kind(&self) -> ProjectionKind {
        self.kind
    }

    /// Canonical records used by this projection.
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }

    /// Explicit meaningful information omitted or collapsed by this view.
    #[must_use]
    pub fn information_loss(&self) -> &[InformationLoss] {
        &self.information_loss
    }

    /// Audience-specific JSON-safe data.
    #[must_use]
    pub fn payload(&self) -> &BTreeMap<String, Value> {
        &self.payload
    }

    /// Hash recomputed from the immutable projection body at construction.
    #[must_use]
    pub fn projection_hash(&self) -> &str {
        &self.projection_hash
    }

    /// Builds the concise M1 human view.
    pub fn human(aggregate: &ReviewAggregate) -> Result<HumanProjection> {
        let mut source_ids = BTreeSet::from([aggregate.universe().id().clone()]);
        source_ids.extend(aggregate.claims().map(|claim| claim.id().clone()));
        let payload = BTreeMap::from([
            ("universe_id".to_owned(), json!(aggregate.universe().id())),
            (
                "obligation_count".to_owned(),
                json!(aggregate.universe().raw_denominator()),
            ),
            ("claim_count".to_owned(), json!(aggregate.claims().count())),
        ]);
        Self::new(
            ProjectionKind::Human,
            source_ids,
            vec![loss(
                "omitted_structural_detail",
                "Human view omits detailed program structure, event history, and raw artifacts.",
                true,
                Some("audit projection"),
                BTreeSet::new(),
            )],
            payload,
        )
    }

    /// Builds the stable-ID M1 AI view.
    pub fn ai(aggregate: &ReviewAggregate) -> Result<Projection> {
        let mut source_ids = aggregate.known_ids();
        source_ids.insert(aggregate.universe().id().clone());
        let payload = BTreeMap::from([
            (
                "obligation_ids".to_owned(),
                json!(aggregate.universe().obligation_ids()),
            ),
            (
                "claim_ids".to_owned(),
                json!(
                    aggregate
                        .claims()
                        .map(|claim| claim.id())
                        .collect::<Vec<_>>()
                ),
            ),
        ]);
        Self::new(
            ProjectionKind::Ai,
            source_ids,
            vec![loss(
                "raw_artifacts_by_reference",
                "Raw model and tool artifacts are deliberately absent from M1 canonical state.",
                false,
                None,
                BTreeSet::new(),
            )],
            payload,
        )
    }

    /// Builds the trace-oriented M1 audit view.
    pub fn audit(aggregate: &ReviewAggregate) -> Result<AuditProjection> {
        let mut source_ids = aggregate.known_ids();
        source_ids.insert(aggregate.universe().id().clone());
        let payload = BTreeMap::from([
            (
                "snapshot_id".to_owned(),
                json!(aggregate.program().snapshot_id()),
            ),
            ("universe_id".to_owned(), json!(aggregate.universe().id())),
            (
                "verification_ids".to_owned(),
                json!(
                    aggregate
                        .verifications()
                        .map(|verification| verification.id())
                        .collect::<Vec<_>>()
                ),
            ),
        ]);
        Self::new(
            ProjectionKind::Audit,
            source_ids,
            vec![loss(
                "source_code_not_embedded",
                "Audit stores source IDs and hashes rather than source bytes.",
                true,
                Some("fixed ProgramSpace snapshot"),
                BTreeSet::from([aggregate.program().snapshot_id().clone()]),
            )],
            payload,
        )
    }
}

fn loss(
    kind: &str,
    reason: &str,
    recoverable: bool,
    recoverable_via: Option<&str>,
    source_ids: BTreeSet<StableId>,
) -> InformationLoss {
    InformationLoss {
        kind: kind.to_owned(),
        reason: reason.to_owned(),
        recoverable,
        recoverable_via: recoverable_via.map(str::to_owned),
        source_ids,
    }
}
