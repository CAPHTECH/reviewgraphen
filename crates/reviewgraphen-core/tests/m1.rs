use proptest::prelude::*;
use reviewgraphen_core::{
    ClaimDisposition, ClaimPolarity, ContentHash, Coverage, Decision, DecisionOutcome, DomainError,
    EventAdmissions, EventCommand, EventEnvelope, EventLog, Evidence, EvidenceBinding,
    EvidenceDetails, EvidenceRelation, Finding, FindingStatus, FindingTrace, IdRegistry,
    MvpRulePack, ObligationLifecycle, ProgramSpace, Projection, Provenance, ReviewAggregate,
    ReviewClaim, ReviewReport, SourceRef, StableId, TrustedHumanAdmission, Verification,
    VerificationOutcome, VersionTuple, canonical_json,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const FIXTURE: &[u8] = include_bytes!("../../../examples/double-submit-payment/program-space.json");
const OBLIGATION_SCHEMA: &[u8] =
    include_bytes!("../../../schemas/reviewgraphen.obligation.schema.json");
const OBLIGATION_EXAMPLE: &[u8] =
    include_bytes!("../../../schemas/reviewgraphen.obligation.example.json");
const LEGACY_OBLIGATION_SCHEMA: &[u8] =
    include_bytes!("../../../schemas/reviewgraphen.obligation.v1.schema.json");
const LEGACY_OBLIGATION_EXAMPLE: &[u8] =
    include_bytes!("../../../schemas/reviewgraphen.obligation.v1.example.json");
const REPORT_SCHEMA: &[u8] = include_bytes!("../../../schemas/reviewgraphen.report.schema.json");
const EMPTY_REPORT_FIXTURE: &[u8] = include_bytes!("fixtures/m1-empty-report.json");
const POPULATED_REPORT_FIXTURE: &[u8] = include_bytes!("fixtures/m1-populated-report.json");
const POPULATED_REPORT_HASH: &str = include_str!("fixtures/m1-populated-report.sha256");

fn id(value: &str) -> StableId {
    StableId::parse(value).expect("test identifier")
}

fn program() -> ProgramSpace {
    ProgramSpace::from_json_slice(FIXTURE).expect("reference fixture parses")
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ObligationSemanticTuple {
    target_kind: String,
    target_refs: Vec<String>,
    property_id: String,
    property_version: String,
    context_ids: BTreeSet<String>,
    required_capabilities: BTreeSet<String>,
    include_relation_kinds: BTreeSet<String>,
    max_relation_depth: u64,
    include_tests: bool,
    include_existing_evidence: bool,
}

fn required_string(value: &Value, pointer: &str) -> std::result::Result<String, String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("missing string at `{pointer}`"))
}

fn required_string_array(value: &Value, pointer: &str) -> std::result::Result<Vec<String>, String> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing array at `{pointer}`"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| format!("non-string member at `{pointer}`"))
        })
        .collect()
}

fn required_string_set(
    value: &Value,
    pointer: &str,
) -> std::result::Result<BTreeSet<String>, String> {
    let values = required_string_array(value, pointer)?;
    let unique = values.iter().cloned().collect::<BTreeSet<_>>();
    if unique.len() != values.len() {
        return Err(format!("duplicate member at `{pointer}`"));
    }
    Ok(unique)
}

fn required_nonnegative_integer(value: &Value, pointer: &str) -> std::result::Result<u64, String> {
    value
        .pointer(pointer)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing non-negative integer at `{pointer}`"))
}

fn required_bool(value: &Value, pointer: &str) -> std::result::Result<bool, String> {
    value
        .pointer(pointer)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("missing boolean at `{pointer}`"))
}

fn obligation_semantics(
    contract: &Value,
) -> std::result::Result<BTreeSet<ObligationSemanticTuple>, String> {
    let obligations = contract
        .get("obligations")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing obligations array".to_owned())?;
    let mut semantics = BTreeSet::new();
    for obligation in obligations {
        let target_kind = required_string(obligation, "/target/kind")?;
        let mut target_refs = required_string_array(obligation, "/target/refs")?;
        if target_kind != "path" {
            target_refs.sort();
        }
        let semantic = ObligationSemanticTuple {
            target_kind,
            target_refs,
            property_id: required_string(obligation, "/property/id")?,
            property_version: required_string(obligation, "/property/version")?,
            context_ids: required_string_set(obligation, "/context_requirement/context_ids")?,
            required_capabilities: required_string_set(
                obligation,
                "/context_requirement/required_capabilities",
            )?,
            include_relation_kinds: required_string_set(
                obligation,
                "/context_requirement/include_relation_kinds",
            )?,
            max_relation_depth: required_nonnegative_integer(
                obligation,
                "/context_requirement/max_relation_depth",
            )?,
            include_tests: required_bool(obligation, "/context_requirement/include_tests")?,
            include_existing_evidence: required_bool(
                obligation,
                "/context_requirement/include_existing_evidence",
            )?,
        };
        if !semantics.insert(semantic) {
            return Err("duplicate obligation semantic tuple".to_owned());
        }
    }
    Ok(semantics)
}

fn assert_legacy_v1_corresponds_to_resynthesis(
    legacy: &Value,
    retained_program: &ProgramSpace,
) -> std::result::Result<Value, String> {
    let legacy_snapshot = required_string(legacy, "/universe/snapshot_id")?;
    if legacy_snapshot != retained_program.snapshot_id().to_string() {
        return Err("legacy fixture snapshot does not match the retained ProgramSpace".to_owned());
    }
    let resynthesized = serde_json::to_value(
        MvpRulePack::synthesize(retained_program)
            .map_err(|error| format!("cannot re-synthesize retained ProgramSpace: {error}"))?,
    )
    .map_err(|error| format!("cannot serialize v2 re-synthesis: {error}"))?;
    let current_snapshot = required_string(&resynthesized, "/universe/snapshot_id")?;
    if current_snapshot != legacy_snapshot {
        return Err("v2 re-synthesis snapshot does not match the legacy fixture".to_owned());
    }
    let legacy_semantics = obligation_semantics(legacy)?;
    let current_semantics = obligation_semantics(&resynthesized)?;
    if legacy_semantics != current_semantics {
        return Err("legacy obligation semantics do not match v2 re-synthesis".to_owned());
    }
    Ok(resynthesized)
}

fn aggregate() -> ReviewAggregate {
    aggregate_for(program())
}

fn aggregate_for(program: ProgramSpace) -> ReviewAggregate {
    let bundle = MvpRulePack::synthesize(&program).expect("reference synthesis");
    let (universe, obligations) = bundle.into_parts();
    ReviewAggregate::new(program, universe, obligations).expect("validated aggregate")
}

fn log(run: &str) -> EventLog {
    EventLog::new(id(run), aggregate()).expect("valid run")
}

fn human() -> TrustedHumanAdmission {
    TrustedHumanAdmission::from_trusted_host("human:reviewer", "reviewer:fixture")
        .expect("trusted host admission")
}

fn evidence_with_targets(
    evidence_id: &str,
    targets: BTreeSet<StableId>,
    observed_program: &ProgramSpace,
) -> Evidence {
    let source = SourceRef::new(
        "tool",
        "fixture-verifier@1",
        Some("1".to_owned()),
        None,
        None,
    )
    .expect("source");
    let provenance = Provenance::accepted_deterministic(
        source,
        "fixture.verifier.v1",
        Some("1".to_owned()),
        Some(1.0),
    )
    .expect("provenance");
    Evidence::new(
        id(evidence_id),
        "static_fact",
        targets,
        EvidenceDetails::new(
            Some("fixture witness".to_owned()),
            Some(ContentHash::parse("sha256:abcdabcd").expect("hash")),
            BTreeMap::new(),
        ),
        provenance,
        observed_program.evidence_snapshot_admission(),
    )
    .expect("evidence")
}

fn evidence_with(evidence_id: &str, observed_program: &ProgramSpace) -> Evidence {
    evidence_with_targets(
        evidence_id,
        BTreeSet::from([id("function:checkout-submit")]),
        observed_program,
    )
}

fn node_obligation(log: &EventLog) -> StableId {
    log.aggregate()
        .obligations()
        .find(|item| item.target_kind() == "node")
        .expect("node obligation")
        .id()
        .clone()
}

fn two_obligations(log: &EventLog) -> BTreeSet<StableId> {
    log.aggregate()
        .obligations()
        .filter(|item| {
            item.target_kind() == "node"
                || (item.target_kind() == "relation"
                    && item.property_id() == "async.concurrent_reentry")
        })
        .map(|item| item.id().clone())
        .collect()
}

fn append_supported_claim(
    log: &mut EventLog,
    obligations: BTreeSet<StableId>,
    evidence_program: &ProgramSpace,
) -> (StableId, StableId, StableId) {
    let claim = ReviewClaim::propose_ai(
        id("claim:duplicate-charge"),
        id("execution:fixture"),
        obligations,
        ClaimPolarity::IssuePresent,
        "The bounded fixture reaches an external charge twice.",
        BTreeSet::from([id("state:checkout-loading")]),
        Some(1.0),
    )
    .expect("claim");
    let claim_id = claim.id().clone();
    log.append(EventCommand::claim_proposed(claim))
        .expect("claim event");
    let evidence = evidence_with("evidence:double-submit-witness", evidence_program);
    let evidence_id = evidence.id().clone();
    let admission = log
        .admit_evidence(&evidence_program.evidence_snapshot_admission(), &evidence)
        .expect("run-bound evidence admission");
    log.append(EventCommand::evidence_recorded(evidence, admission))
        .expect("evidence event");
    let binding = EvidenceBinding::new(
        id("binding:duplicate-charge"),
        claim_id.clone(),
        evidence_id.clone(),
        EvidenceRelation::Reproduces,
        BTreeMap::from([(
            "property_id".to_owned(),
            "async.concurrent_reentry".to_owned(),
        )]),
    )
    .expect("binding");
    let binding_admission = log
        .admit_evidence_binding(&binding)
        .expect("binding admission");
    log.append(EventCommand::evidence_bound(binding, binding_admission))
        .expect("binding event");
    let verification = Verification::new(
        id("verification:duplicate-charge"),
        claim_id.clone(),
        VerificationOutcome::Passed,
        "fixture-verifier@1",
        BTreeSet::from([evidence_id.clone()]),
    )
    .expect("verification");
    let verification_id = verification.id().clone();
    let verification_admission = log
        .admit_verification(verification.clone())
        .expect("verification admission");
    log.append(EventCommand::verification_recorded(
        verification,
        verification_admission,
    ))
    .expect("verification event");
    (claim_id, evidence_id, verification_id)
}

fn append_supported_claim_with_summary(
    log: &mut EventLog,
    summary: &str,
    binding_marker: Option<&str>,
    verifier: &str,
) -> (StableId, StableId, StableId) {
    let obligation = node_obligation(log);
    let claim = ReviewClaim::propose_ai(
        id("claim:duplicate-charge"),
        id("execution:fixture"),
        BTreeSet::from([obligation]),
        ClaimPolarity::IssuePresent,
        summary,
        BTreeSet::from([id("state:checkout-loading")]),
        Some(1.0),
    )
    .unwrap();
    let claim_id = claim.id().clone();
    log.append(EventCommand::claim_proposed(claim)).unwrap();
    let observed = program();
    let evidence = evidence_with("evidence:double-submit-witness", &observed);
    let evidence_id = evidence.id().clone();
    let evidence_admission = log
        .admit_evidence(&observed.evidence_snapshot_admission(), &evidence)
        .unwrap();
    log.append(EventCommand::evidence_recorded(
        evidence,
        evidence_admission,
    ))
    .unwrap();
    let mut binding_scope = BTreeMap::from([(
        "property_id".to_owned(),
        "async.concurrent_reentry".to_owned(),
    )]);
    if let Some(marker) = binding_marker {
        binding_scope.insert("marker".to_owned(), marker.to_owned());
    }
    let binding = EvidenceBinding::new(
        id("binding:duplicate-charge"),
        claim_id.clone(),
        evidence_id.clone(),
        EvidenceRelation::Reproduces,
        binding_scope,
    )
    .unwrap();
    let binding_admission = log.admit_evidence_binding(&binding).unwrap();
    log.append(EventCommand::evidence_bound(binding, binding_admission))
        .unwrap();
    let verification = Verification::new(
        id("verification:duplicate-charge"),
        claim_id.clone(),
        VerificationOutcome::Passed,
        verifier,
        BTreeSet::from([evidence_id.clone()]),
    )
    .unwrap();
    let verification_id = verification.id().clone();
    let verification_admission = log.admit_verification(verification.clone()).unwrap();
    log.append(EventCommand::verification_recorded(
        verification,
        verification_admission,
    ))
    .unwrap();
    (claim_id, evidence_id, verification_id)
}

fn accept(
    log: &mut EventLog,
    claim_id: StableId,
    evidence_id: StableId,
    verification_id: StableId,
) -> reviewgraphen_core::DecisionAdmission {
    let decision = Decision::human(
        id("decision:accept-duplicate-charge"),
        claim_id.clone(),
        DecisionOutcome::Accept,
        human(),
        "The evidence and verifier trace support this exact claim.",
        BTreeSet::from([claim_id, evidence_id, verification_id]),
    )
    .expect("decision");
    let admission = log
        .admit_decision(&human(), &decision)
        .expect("run-bound decision admission");
    log.append(EventCommand::decision_recorded(decision, admission.clone()))
        .expect("accepted decision event");
    admission
}

fn append_fully_connected_multi_claim(
    log: &mut EventLog,
) -> (
    StableId,
    StableId,
    StableId,
    reviewgraphen_core::DecisionAdmission,
) {
    let obligations = two_obligations(log);
    assert_eq!(
        obligations.len(),
        2,
        "fixture must expose node and reentry obligations"
    );
    let claim = ReviewClaim::propose_ai(
        id("claim:multi-connected"),
        id("execution:fixture"),
        obligations,
        ClaimPolarity::IssuePresent,
        "The UI re-entry path has a connected witness for each claimed obligation.",
        BTreeSet::from([id("context:ui-event")]),
        Some(0.8),
    )
    .expect("claim");
    let claim_id = claim.id().clone();
    log.append(EventCommand::claim_proposed(claim)).unwrap();
    let evidence = evidence_with_targets(
        "evidence:multi-connected",
        BTreeSet::from([
            id("function:checkout-submit"),
            id("relation:tap-handled-by-submit"),
        ]),
        log.aggregate().program(),
    );
    let evidence_id = evidence.id().clone();
    let evidence_admission = log
        .admit_evidence(
            &log.aggregate().program().evidence_snapshot_admission(),
            &evidence,
        )
        .unwrap();
    log.append(EventCommand::evidence_recorded(
        evidence,
        evidence_admission,
    ))
    .unwrap();
    let binding = EvidenceBinding::new(
        id("binding:multi-connected"),
        claim_id.clone(),
        evidence_id.clone(),
        EvidenceRelation::Supports,
        BTreeMap::from([(
            "property_id".to_owned(),
            "async.concurrent_reentry".to_owned(),
        )]),
    )
    .unwrap();
    let binding_admission = log.admit_evidence_binding(&binding).unwrap();
    log.append(EventCommand::evidence_bound(binding, binding_admission))
        .unwrap();
    let verification = Verification::new(
        id("verification:multi-connected"),
        claim_id.clone(),
        VerificationOutcome::Passed,
        "fixture-verifier@1",
        BTreeSet::from([evidence_id.clone()]),
    )
    .unwrap();
    let verification_id = verification.id().clone();
    let verification_admission = log.admit_verification(verification.clone()).unwrap();
    log.append(EventCommand::verification_recorded(
        verification,
        verification_admission,
    ))
    .unwrap();
    let decision = Decision::human(
        id("decision:multi-connected"),
        claim_id.clone(),
        DecisionOutcome::Accept,
        human(),
        "Each claimed obligation has this joined evidence and verifier trace.",
        BTreeSet::from([
            claim_id.clone(),
            evidence_id.clone(),
            verification_id.clone(),
        ]),
    )
    .unwrap();
    let admission = log.admit_decision(&human(), &decision).unwrap();
    log.append(EventCommand::decision_recorded(decision, admission.clone()))
        .unwrap();
    (claim_id, evidence_id, verification_id, admission)
}

fn populated_report_log() -> EventLog {
    let mut input: Value = serde_json::from_slice(FIXTURE).expect("fixture JSON");
    input["extraction"]["capabilities"]
        .as_object_mut()
        .expect("capabilities")
        .remove("test_mapping");
    let populated_program =
        ProgramSpace::from_json_slice(&serde_json::to_vec(&input).expect("JSON")).expect("program");
    let mut log = EventLog::new(
        id("run:populated-report"),
        aggregate_for(populated_program.clone()),
    )
    .expect("log");
    let claim = ReviewClaim::propose_ai(
        id("claim:populated-report"),
        id("execution:fixture"),
        BTreeSet::from([node_obligation(&log)]),
        ClaimPolarity::IssuePresent,
        "The changed submit function has a connected review witness.",
        BTreeSet::from([id("state:checkout-loading")]),
        Some(0.7),
    )
    .expect("claim");
    let claim_id = claim.id().clone();
    log.append(EventCommand::claim_proposed(claim))
        .expect("claim event");
    let evidence = evidence_with_targets(
        "evidence:populated-report",
        BTreeSet::from([id("function:checkout-submit")]),
        &populated_program,
    );
    let evidence_id = evidence.id().clone();
    let evidence_admission = log
        .admit_evidence(&populated_program.evidence_snapshot_admission(), &evidence)
        .expect("evidence admission");
    log.append(EventCommand::evidence_recorded(
        evidence,
        evidence_admission,
    ))
    .expect("evidence event");
    let binding = EvidenceBinding::new(
        id("binding:populated-report"),
        claim_id.clone(),
        evidence_id.clone(),
        EvidenceRelation::Supports,
        BTreeMap::from([(
            "property_id".to_owned(),
            "async.concurrent_reentry".to_owned(),
        )]),
    )
    .expect("binding");
    let binding_admission = log
        .admit_evidence_binding(&binding)
        .expect("binding admission");
    log.append(EventCommand::evidence_bound(binding, binding_admission))
        .expect("binding event");
    let verification = Verification::new(
        id("verification:populated-report"),
        claim_id.clone(),
        VerificationOutcome::Passed,
        "fixture-verifier@1",
        BTreeSet::from([evidence_id.clone()]),
    )
    .expect("verification");
    let verification_id = verification.id().clone();
    let verification_admission = log
        .admit_verification(verification.clone())
        .expect("verification admission");
    log.append(EventCommand::verification_recorded(
        verification,
        verification_admission,
    ))
    .expect("verification event");
    let decision = Decision::human(
        id("decision:populated-report"),
        claim_id.clone(),
        DecisionOutcome::Accept,
        human(),
        "A trusted human accepted this exact joined trace.",
        BTreeSet::from([
            claim_id.clone(),
            evidence_id.clone(),
            verification_id.clone(),
        ]),
    )
    .expect("decision");
    let decision_admission = log
        .admit_decision(&human(), &decision)
        .expect("decision admission");
    log.append(EventCommand::decision_recorded(
        decision,
        decision_admission,
    ))
    .expect("decision event");
    log.append(EventCommand::finding_recorded(Finding::new(
        id("finding:populated-report"),
        claim_id,
        FindingStatus::Accepted,
        FindingTrace::new(
            BTreeSet::from([evidence_id]),
            BTreeSet::from([verification_id]),
            Some(id("decision:populated-report")),
            BTreeSet::from([id("state:checkout-loading")]),
        ),
    )))
    .expect("finding event");
    log
}

#[test]
fn reference_obligation_semantics_are_an_independent_five_item_oracle() {
    let bundle = MvpRulePack::synthesize(&program()).expect("synthesis");
    let value: Value = serde_json::from_slice(bundle.contract().canonical().bytes()).expect("JSON");
    let obligations = value["obligations"].as_array().expect("obligations array");
    let semantics = obligations
        .iter()
        .map(|item| {
            (
                item["target"]["kind"].as_str().unwrap(),
                item["property"]["id"].as_str().unwrap(),
                item["target"]["refs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect::<Vec<_>>(),
                item["context_requirement"]["context_ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        semantics,
        vec![
            (
                "node",
                "async.concurrent_reentry",
                vec!["function:checkout-submit"],
                vec!["context:ui-event"]
            ),
            (
                "relation",
                "async.concurrent_reentry",
                vec!["relation:tap-handled-by-submit"],
                vec!["context:ui-event"]
            ),
            (
                "relation",
                "payment.idempotency_contract",
                vec!["relation:payment-calls-stripe"],
                vec!["context:payment"]
            ),
            (
                "path",
                "payment.at_most_once",
                vec![
                    "relation:tap-handled-by-submit",
                    "relation:submit-calls-payment",
                    "relation:payment-calls-stripe"
                ],
                vec!["context:ui-event", "context:payment", "context:test"]
            ),
            (
                "invariant",
                "payment.at_most_once",
                vec!["invariant:payment-at-most-once"],
                vec!["context:payment", "context:test", "context:ui-event"]
            ),
        ]
    );
}

#[test]
fn reviewed_obligation_example_is_the_complete_canonical_fixture_oracle() {
    let generated = MvpRulePack::synthesize(&program()).unwrap();
    let reviewed: Value = serde_json::from_slice(OBLIGATION_EXAMPLE).unwrap();
    assert_eq!(
        generated.contract().canonical().bytes(),
        canonical_json(&reviewed).unwrap().as_slice(),
        "the checked-in reviewed contract must cover every generated field, not only a hash"
    );
}

#[test]
fn obligation_contract_and_report_adapter_validate_checked_in_schemas() {
    let program = program();
    let bundle = MvpRulePack::synthesize(&program).expect("bundle");
    let obligation_schema: Value = serde_json::from_slice(OBLIGATION_SCHEMA).expect("schema");
    jsonschema::validator_for(&obligation_schema)
        .expect("obligation schema")
        .validate(&serde_json::to_value(&bundle).expect("bundle json"))
        .expect("generated obligation contract validates");
    let mut without_generator: Value = serde_json::to_value(&bundle).expect("bundle JSON");
    without_generator["obligations"][0]["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("generator_ids");
    assert!(
        jsonschema::validator_for(&obligation_schema)
            .unwrap()
            .validate(&without_generator)
            .is_err(),
        "v2 requires the generator provenance that v1 did not retain"
    );
    let mut without_exclusions: Value = serde_json::to_value(&bundle).expect("bundle JSON");
    without_exclusions["universe"]
        .as_object_mut()
        .unwrap()
        .remove("exclusions");
    assert!(
        jsonschema::validator_for(&obligation_schema)
            .unwrap()
            .validate(&without_exclusions)
            .is_err(),
        "v2 retains an explicit exclusion trace even when it is empty"
    );
    let legacy_schema: Value = serde_json::from_slice(LEGACY_OBLIGATION_SCHEMA).expect("schema");
    let legacy_fixture: Value =
        serde_json::from_slice(LEGACY_OBLIGATION_EXAMPLE).expect("legacy fixture");
    jsonschema::validator_for(&legacy_schema)
        .expect("legacy obligation schema")
        .validate(&legacy_fixture)
        .expect("preserved v1 fixture validates against the v1 contract");
    assert!(
        jsonschema::validator_for(&obligation_schema)
            .expect("current obligation schema")
            .validate(&legacy_fixture)
            .is_err(),
        "v1 output cannot be represented losslessly as the extended v2 contract"
    );
    let resynthesized = assert_legacy_v1_corresponds_to_resynthesis(&legacy_fixture, &program)
        .expect("legacy semantic tuple corresponds to v2 re-synthesis from the same ProgramSpace");
    assert_eq!(
        resynthesized["schema"],
        "reviewgraphen.review_obligations.v2"
    );
    assert!(
        legacy_fixture["obligations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| {
                item["provenance"].get("generator_ids").is_none()
                    && item["provenance"].get("origin_rule").is_none()
            }),
        "v1 retained neither generator nor origin-rule provenance"
    );
    assert!(
        legacy_fixture["universe"].get("exclusions").is_none(),
        "v1 retained no explicit exclusion trace"
    );
    assert!(
        resynthesized["obligations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["provenance"]["generator_ids"]
                .as_array()
                .is_some_and(|ids| !ids.is_empty())),
        "v2 generator trace is recovered by synthesis from ProgramSpace, not by JSON migration"
    );
    assert_eq!(
        resynthesized["universe"]["exclusions"],
        json!([]),
        "the retained reference ProgramSpace has no declared policy exclusions"
    );
    assert!(
        resynthesized["obligations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["provenance"].get("origin_rule").is_none()),
        "the retained complete ProgramSpace has no capability-gap origin trace to recover"
    );
    let mut unrelated_legacy = legacy_fixture.clone();
    unrelated_legacy["obligations"][0]["property"]["id"] = json!("unrelated.property");
    jsonschema::validator_for(&legacy_schema)
        .unwrap()
        .validate(&unrelated_legacy)
        .expect("altered property remains shape-valid v1");
    assert!(
        assert_legacy_v1_corresponds_to_resynthesis(&unrelated_legacy, &program).is_err(),
        "a shape-valid but unrelated v1 bundle must not pass semantic correspondence"
    );
    let mut changed_capabilities = legacy_fixture.clone();
    changed_capabilities["obligations"][0]["context_requirement"]["required_capabilities"] =
        json!(["direct_calls"]);
    jsonschema::validator_for(&legacy_schema)
        .unwrap()
        .validate(&changed_capabilities)
        .expect("altered capabilities remain shape-valid v1");
    assert!(
        assert_legacy_v1_corresponds_to_resynthesis(&changed_capabilities, &program).is_err(),
        "a capability-altered v1 bundle must not pass semantic correspondence"
    );
    let mut changed_depth = legacy_fixture.clone();
    changed_depth["obligations"][0]["context_requirement"]["max_relation_depth"] = json!(3);
    jsonschema::validator_for(&legacy_schema)
        .unwrap()
        .validate(&changed_depth)
        .expect("altered relation depth remains shape-valid v1");
    assert!(
        assert_legacy_v1_corresponds_to_resynthesis(&changed_depth, &program).is_err(),
        "a relation-depth-altered v1 bundle must not pass semantic correspondence"
    );
    let report = ReviewReport::from_aggregate(id("run:report"), &aggregate()).expect("report");
    let report_schema: Value = serde_json::from_slice(REPORT_SCHEMA).expect("schema");
    jsonschema::validator_for(&report_schema)
        .expect("report schema")
        .validate(report.body())
        .expect("generated report validates");
    assert_eq!(
        report.canonical().bytes(),
        canonical_json(report.body()).unwrap()
    );
    assert_eq!(
        report.canonical().hash().to_string(),
        include_str!("fixtures/m1-empty-report.sha256").trim()
    );
    let reviewed_report: Value = serde_json::from_slice(EMPTY_REPORT_FIXTURE).unwrap();
    assert_eq!(
        report.canonical().bytes(),
        canonical_json(&reviewed_report).unwrap().as_slice(),
        "the M1 report adapter is compared with a full reviewed report fixture"
    );
}

#[test]
fn populated_report_fixture_is_canonical_schema_valid_and_connected() {
    let log = populated_report_log();
    let report = ReviewReport::from_aggregate(log.run_id().clone(), log.aggregate()).unwrap();
    let report_schema: Value = serde_json::from_slice(REPORT_SCHEMA).expect("schema");
    jsonschema::validator_for(&report_schema)
        .expect("report schema")
        .validate(report.body())
        .expect("connected report validates");
    let reviewed: Value = serde_json::from_slice(POPULATED_REPORT_FIXTURE).unwrap();
    assert_eq!(
        report.canonical().bytes(),
        canonical_json(&reviewed).unwrap().as_slice(),
        "the populated report fixture is the full canonical report oracle"
    );
    assert_eq!(
        report.canonical().hash().to_string(),
        POPULATED_REPORT_HASH.trim()
    );
    for field in [
        "claims",
        "evidence_bindings",
        "verifications",
        "decisions",
        "findings",
        "obstructions",
    ] {
        assert!(
            !report.body()["result"][field]
                .as_array()
                .unwrap()
                .is_empty(),
            "{field} must be represented in the populated fixture"
        );
    }
    assert_eq!(
        report.body()["coverage"]["stages"]["fresh_verified"]["numerator"],
        1
    );
    let executions = report.body()["result"]["executions"].as_array().unwrap();
    assert!(!executions.is_empty());
    for claim in report.body()["result"]["claims"].as_array().unwrap() {
        let execution = executions
            .iter()
            .find(|execution| execution["id"] == claim["execution_id"])
            .expect("every claim execution reference resolves in the report");
        assert!(
            execution["claim_ids"]
                .as_array()
                .unwrap()
                .iter()
                .any(|claim_id| claim_id == &claim["id"]),
            "the matching execution records the claim it produced"
        );
        assert!(
            execution["obligation_ids"]
                .as_array()
                .is_some_and(|ids| !ids.is_empty()),
            "a report execution has an explicit, nonempty obligation scope"
        );
        assert_eq!(execution["status"], "abstained");
        assert_eq!(execution["reviewer"]["kind"], "unknown");
        assert_eq!(execution["reviewer"]["identity"], "unresolved");
        assert_eq!(execution["context_envelope_id"], "context:unresolved");
        assert!(
            execution["abstention_reason"].as_str().is_some_and(
                |reason| reason.contains("does not retain reviewer or context metadata")
            )
        );
        assert_ne!(execution["reviewer"]["identity"], "reviewgraphen-core:m1");
        assert_ne!(execution["reviewer"]["kind"], "ai");
        assert!(
            claim["limitations"]
                .as_array()
                .unwrap()
                .contains(&json!("execution_metadata_unavailable"))
        );
    }
    let execution_obstructions = report.body()["result"]["obstructions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "execution_metadata_unavailable")
        .collect::<Vec<_>>();
    assert_eq!(execution_obstructions.len(), executions.len());
    for execution in executions {
        let obstruction = execution_obstructions
            .iter()
            .find(|item| {
                execution["claim_ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|claim_id| item["blocks"].as_array().unwrap().contains(claim_id))
            })
            .expect("unresolved execution has a blocking report obstruction");
        for obligation_id in execution["obligation_ids"].as_array().unwrap() {
            assert!(
                obstruction["blocks"]
                    .as_array()
                    .unwrap()
                    .contains(obligation_id)
            );
        }
    }
}

#[test]
fn deterministic_reorder_and_fixed_fixture_bytes_are_stable() {
    let original = MvpRulePack::synthesize(&program()).expect("original");
    let mut input: Value = serde_json::from_slice(FIXTURE).expect("fixture JSON");
    for key in [
        "artifacts",
        "relations",
        "contexts",
        "invariants",
        "evidence",
    ] {
        input[key].as_array_mut().expect("array").reverse();
    }
    input["extraction"]["limitations"]
        .as_array_mut()
        .expect("limitations")
        .reverse();
    let reordered = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
    let reordered = MvpRulePack::synthesize(&reordered).expect("reordered");
    assert_eq!(
        original.contract().canonical().bytes(),
        reordered.contract().canonical().bytes()
    );
    assert_eq!(original.universe().id(), reordered.universe().id());
    assert_eq!(
        original.contract().canonical().hash().to_string(),
        "sha256:ddfec2ef04bef43ea2c69ad1c3d7307da22a3974cdaa0b6eafb28b92d706edce"
    );
    assert_eq!(original.contract().canonical().bytes().len(), 9753);
}

#[test]
fn completed_is_not_evidence_verified_fresh_or_human_accepted() {
    let mut log = log("run:completed");
    let obligation = node_obligation(&log);
    for next in [
        ObligationLifecycle::Planned,
        ObligationLifecycle::InProgress,
        ObligationLifecycle::Completed,
    ] {
        log.append(EventCommand::obligation_transition(
            obligation.clone(),
            next,
        ))
        .expect("legal transition");
    }
    let coverage = Coverage::from_aggregate(log.aggregate());
    assert_eq!(coverage.raw.raw.numerator, 1.0);
    assert_eq!(coverage.evidence_supported.raw.numerator, 0.0);
    assert_eq!(coverage.verified.raw.numerator, 0.0);
    assert_eq!(coverage.fresh.raw.numerator, 0.0);
    assert_eq!(coverage.human_accepted.raw.numerator, 0.0);
}

#[test]
fn stale_evidence_remains_auditable_but_cannot_be_presented_as_current() {
    let mut old_input: Value = serde_json::from_slice(FIXTURE).unwrap();
    old_input["snapshot"]["id"] = json!("snapshot:old-double-submit");
    for limitation in old_input["extraction"]["limitations"]
        .as_array_mut()
        .unwrap()
    {
        for source in limitation["source_ids"].as_array_mut().unwrap() {
            if source == "snapshot:double-submit-v1" {
                *source = json!("snapshot:old-double-submit");
            }
        }
    }
    let old_program =
        ProgramSpace::from_json_slice(&serde_json::to_vec(&old_input).unwrap()).unwrap();
    let mut log = log("run:stale");
    let obligation = node_obligation(&log);
    let claim = ReviewClaim::propose_ai(
        id("claim:stale-evidence"),
        id("execution:fixture"),
        BTreeSet::from([obligation]),
        ClaimPolarity::IssuePresent,
        "Old evidence must not sign off this snapshot.",
        BTreeSet::from([id("function:checkout-submit")]),
        Some(1.0),
    )
    .unwrap();
    log.append(EventCommand::claim_proposed(claim)).unwrap();
    let evidence = evidence_with("evidence:old-witness", &old_program);
    let stale_admission = log
        .admit_evidence(&old_program.evidence_snapshot_admission(), &evidence)
        .unwrap();
    let evidence_id = evidence.id().clone();
    log.append(EventCommand::evidence_recorded(evidence, stale_admission))
        .expect("historical evidence remains auditable");
    let binding = EvidenceBinding::new(
        id("binding:stale-evidence"),
        id("claim:stale-evidence"),
        evidence_id.clone(),
        EvidenceRelation::Supports,
        BTreeMap::from([(
            "property_id".to_owned(),
            "async.concurrent_reentry".to_owned(),
        )]),
    )
    .unwrap();
    let binding_admission = log.admit_evidence_binding(&binding).unwrap();
    log.append(EventCommand::evidence_bound(binding, binding_admission))
        .unwrap();
    let verification = Verification::new(
        id("verification:stale-evidence"),
        id("claim:stale-evidence"),
        VerificationOutcome::Passed,
        "fixture-verifier@1",
        BTreeSet::from([evidence_id]),
    )
    .unwrap();
    let verification_admission = log.admit_verification(verification.clone()).unwrap();
    log.append(EventCommand::verification_recorded(
        verification,
        verification_admission,
    ))
    .expect("stale verification remains auditable");
    let coverage = Coverage::from_aggregate(log.aggregate());
    assert_eq!(
        log.aggregate()
            .claims()
            .find(|claim| claim.id() == &id("claim:stale-evidence"))
            .unwrap()
            .disposition(),
        ClaimDisposition::Proposed,
        "historical support is auditable but cannot support the current claim"
    );
    assert_eq!(coverage.evidence_supported.raw.numerator, 0.0);
    assert_eq!(coverage.verified.raw.numerator, 1.0);
    assert_eq!(coverage.fresh.raw.numerator, 0.0);
}

#[test]
fn stale_deleted_targets_remain_auditable_but_never_ground_current_support() {
    let mut old_input: Value = serde_json::from_slice(FIXTURE).unwrap();
    old_input["snapshot"]["id"] = json!("snapshot:old-target-name");
    for limitation in old_input["extraction"]["limitations"]
        .as_array_mut()
        .unwrap()
    {
        for source in limitation["source_ids"].as_array_mut().unwrap() {
            if source == "snapshot:double-submit-v1" {
                *source = json!("snapshot:old-target-name");
            }
        }
    }
    let old_program =
        ProgramSpace::from_json_slice(&serde_json::to_vec(&old_input).unwrap()).unwrap();

    let mut current_input: Value = serde_json::from_slice(FIXTURE).unwrap();
    let old_target = "function:checkout-submit";
    let renamed_target = "function:checkout-submit-renamed";
    for artifact in current_input["artifacts"].as_array_mut().unwrap() {
        if artifact["id"] == old_target {
            artifact["id"] = json!(renamed_target);
        }
    }
    for relation in current_input["relations"].as_array_mut().unwrap() {
        if relation["source_id"] == old_target {
            relation["source_id"] = json!(renamed_target);
        }
        for target in relation["target_ids"].as_array_mut().unwrap() {
            if target == old_target {
                *target = json!(renamed_target);
            }
        }
    }
    for context in current_input["contexts"].as_array_mut().unwrap() {
        for member in context["member_ids"].as_array_mut().unwrap() {
            if member == old_target {
                *member = json!(renamed_target);
            }
        }
    }
    for evidence in current_input["evidence"].as_array_mut().unwrap() {
        for target in evidence["target_ids"].as_array_mut().unwrap() {
            if target == old_target {
                *target = json!(renamed_target);
            }
        }
    }
    let current_program =
        ProgramSpace::from_json_slice(&serde_json::to_vec(&current_input).unwrap()).unwrap();
    let mut log = EventLog::new(
        id("run:stale-deleted-target"),
        aggregate_for(current_program.clone()),
    )
    .unwrap();
    let claim = ReviewClaim::propose_ai(
        id("claim:stale-deleted-target"),
        id("execution:fixture"),
        BTreeSet::from([node_obligation(&log)]),
        ClaimPolarity::IssuePresent,
        "The historical target was renamed in the active snapshot.",
        BTreeSet::from([id("context:ui-event")]),
        None,
    )
    .unwrap();
    log.append(EventCommand::claim_proposed(claim)).unwrap();
    let old_evidence = evidence_with("evidence:stale-deleted-target", &old_program);
    let old_evidence_id = old_evidence.id().clone();
    let old_admission = log
        .admit_evidence(&old_program.evidence_snapshot_admission(), &old_evidence)
        .expect("old observed target remains recordable for audit");
    log.append(EventCommand::evidence_recorded(old_evidence, old_admission))
        .expect("historical evidence is retained even though the current target is gone");
    let binding = EvidenceBinding::new(
        id("binding:stale-deleted-target"),
        id("claim:stale-deleted-target"),
        old_evidence_id,
        EvidenceRelation::Supports,
        BTreeMap::from([(
            "property_id".to_owned(),
            "async.concurrent_reentry".to_owned(),
        )]),
    )
    .unwrap();
    assert!(matches!(
        log.admit_evidence_binding(&binding),
        Err(DomainError::Validation(_))
    ));
    let forged_current = evidence_with("evidence:current-missing-target", &current_program);
    assert!(matches!(
        log.admit_evidence(
            &current_program.evidence_snapshot_admission(),
            &forged_current
        ),
        Err(DomainError::DanglingReference { .. })
    ));
    let coverage = Coverage::from_aggregate(log.aggregate());
    assert_eq!(
        log.aggregate()
            .claims()
            .find(|claim| claim.id() == &id("claim:stale-deleted-target"))
            .unwrap()
            .disposition(),
        ClaimDisposition::Proposed
    );
    assert_eq!(coverage.evidence_supported.raw.numerator, 0.0);
    assert_eq!(coverage.verified.raw.numerator, 0.0);
    assert_eq!(coverage.fresh.raw.numerator, 0.0);
}

#[test]
fn accepted_multi_obligation_claim_requires_a_chain_for_each_obligation() {
    let mut log = log("run:multi");
    let obligations = two_obligations(&log);
    let (claim, evidence, verification) = append_supported_claim(&mut log, obligations, &program());
    let decision = Decision::human(
        id("decision:multi-accept"),
        claim.clone(),
        DecisionOutcome::Accept,
        human(),
        "One evidence record cannot accept every obligation.",
        BTreeSet::from([claim, evidence, verification]),
    )
    .unwrap();
    assert!(matches!(
        log.admit_decision(&human(), &decision),
        Err(DomainError::Validation(_))
    ));
    let coverage = Coverage::from_aggregate(log.aggregate());
    assert_eq!(coverage.verified.raw.numerator, 1.0);
    assert_eq!(coverage.human_accepted.raw.numerator, 0.0);
}

#[test]
fn finding_statuses_and_traces_cannot_conflate_claim_trust() {
    let mut log = log("run:findings");
    let obligation = node_obligation(&log);
    let (claim, evidence, verification) =
        append_supported_claim(&mut log, BTreeSet::from([obligation]), &program());
    let rejected = Finding::new(
        id("finding:bad-candidate"),
        claim,
        FindingStatus::VerifiedCandidate,
        FindingTrace::new(
            BTreeSet::from([evidence]),
            BTreeSet::from([verification]),
            None,
            BTreeSet::from([id("state:checkout-loading")]),
        ),
    );
    log.append(EventCommand::finding_recorded(rejected))
        .expect("supported verified candidate is valid");

    let mut rejected_log = EventLog::new(id("run:rejected-finding"), aggregate()).unwrap();
    let obligation = node_obligation(&rejected_log);
    let (claim, evidence, verification) =
        append_supported_claim(&mut rejected_log, BTreeSet::from([obligation]), &program());
    let rejection = Decision::human(
        id("decision:reject-duplicate-charge"),
        claim.clone(),
        DecisionOutcome::Reject,
        human(),
        "The reviewer rejects this claim.",
        BTreeSet::from([claim.clone()]),
    )
    .unwrap();
    let admission = rejected_log.admit_decision(&human(), &rejection).unwrap();
    rejected_log
        .append(EventCommand::decision_recorded(rejection, admission))
        .unwrap();
    let invalid_candidate = Finding::new(
        id("finding:rejected-as-verified"),
        claim,
        FindingStatus::VerifiedCandidate,
        FindingTrace::new(
            BTreeSet::from([evidence]),
            BTreeSet::from([verification]),
            None,
            BTreeSet::from([id("state:checkout-loading")]),
        ),
    );
    assert!(matches!(
        rejected_log.append(EventCommand::finding_recorded(invalid_candidate)),
        Err(DomainError::Validation(_))
    ));
}

#[test]
fn event_json_requires_known_payload_hash_genesis_and_exact_decision_admission() {
    let mut log = log("run:accepted");
    let obligation = node_obligation(&log);
    let (claim, evidence, verification) =
        append_supported_claim(&mut log, BTreeSet::from([obligation]), &program());
    let admission = accept(&mut log, claim, evidence, verification);
    let envelopes = log.envelopes().cloned().collect::<Vec<_>>();
    let admissions = EventAdmissions::new(
        log.events()
            .iter()
            .filter_map(|event| event.evidence_admission().cloned())
            .collect(),
        log.events()
            .iter()
            .filter_map(|event| event.decision_admission().cloned())
            .collect(),
    )
    .with_trace_admissions(
        log.events()
            .iter()
            .filter_map(|event| event.binding_admission().cloned())
            .collect(),
        log.events()
            .iter()
            .filter_map(|event| event.verification_admission().cloned())
            .collect(),
    );
    assert!(
        log.events()
            .iter()
            .any(|event| event.decision_admission() == Some(&admission))
    );
    let imported = envelopes
        .iter()
        .map(|envelope| EventEnvelope::from_json_slice(&serde_json::to_vec(envelope).unwrap()))
        .collect::<Result<Vec<_>, _>>()
        .expect("envelopes deserialize");
    let replayed =
        EventLog::replay_envelopes(id("run:accepted"), aggregate(), &imported, &admissions)
            .expect("admitted serialized prefix replays");
    assert_eq!(replayed.events().len(), imported.len());
    assert!(
        EventLog::replay_envelopes(
            id("run:accepted"),
            aggregate(),
            &imported,
            &EventAdmissions::default(),
        )
        .is_err()
    );
    let mut resumed =
        EventLog::replay_envelopes(id("run:accepted"), aggregate(), &imported[..2], &admissions)
            .expect("prefix replays");
    resumed
        .resume_envelopes(&imported[2..], &admissions)
        .expect("suffix resumes the prefix");
    assert_eq!(resumed.events().len(), imported.len());

    let mut unknown: Value = serde_json::to_value(&imported[0]).unwrap();
    unknown["payload"]["type"] = json!("future_payload");
    assert!(EventEnvelope::from_json_slice(&serde_json::to_vec(&unknown).unwrap()).is_err());
    let mut tampered: Value = serde_json::to_value(&imported[0]).unwrap();
    tampered["payload"]["data"]["next"] = json!("completed");
    assert!(EventEnvelope::from_json_slice(&serde_json::to_vec(&tampered).unwrap()).is_err());
    assert!(
        EventLog::replay_envelopes(
            id("run:other"),
            aggregate(),
            &imported,
            &EventAdmissions::default()
        )
        .is_err()
    );
    assert!(
        EventLog::replay_envelopes(
            id("run:accepted"),
            aggregate(),
            &[imported[0].clone(), imported[0].clone()],
            &EventAdmissions::default(),
        )
        .is_err()
    );
    assert!(EventLog::new(id("not-a-run:empty"), aggregate()).is_err());
}

#[test]
fn capability_gaps_remain_unknown_obligations_in_the_coverage_denominator() {
    let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
    input["extraction"]["capabilities"]
        .as_object_mut()
        .unwrap()
        .remove("direct_calls");
    let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
    let bundle = MvpRulePack::synthesize(&program).unwrap();
    assert!(bundle.universe().exclusions().is_empty());
    assert_eq!(bundle.universe().raw_denominator(), 9);
    assert_eq!(
        bundle
            .obligations()
            .iter()
            .filter(|obligation| obligation.version().rule() == "capability_gap.origin_rule@1")
            .count(),
        4,
        "concrete candidates do not hide origin-rule capability gaps"
    );
    assert!(bundle.obligations().iter().all(|obligation| {
        obligation.target_kind() != "subgraph"
            || obligation.version().rule() == "capability_gap.origin_rule@1"
    }));
    let contract: Value = serde_json::to_value(&bundle).unwrap();
    assert!(
        contract["obligations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["version"]["rule"] == "capability_gap.origin_rule@1"
                    && item["provenance"]["origin_rule"] == "relation.concurrent_reentry@1"
            }),
        "the origin rule is retained in the public gap provenance trace"
    );
    assert!(
        contract["obligations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| {
                item["applicability"]["status"] == "unknown"
                    || item["applicability"]["status"] == "applicable"
            })
    );
    assert!(
        contract["obligations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["applicability"]["reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|reason| reason.as_str().unwrap().starts_with("capability_missing:"))
            })
    );
}

#[test]
fn schema_valid_input_can_still_be_rejected_by_the_fact_authority_boundary() {
    let schema: Value = serde_json::from_slice(include_bytes!(
        "../../../schemas/reviewgraphen.input.schema.json"
    ))
    .unwrap();
    let serialized_program = serde_json::to_value(program()).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&serialized_program)
        .expect("ProgramSpace external serialization remains input-schema compatible");
    let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
    input["source"]["kind"] = json!("model");
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&input)
        .expect("model source is structurally schema-valid");
    assert!(ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).is_err());
    let mut invalid: Value = serde_json::from_slice(FIXTURE).unwrap();
    invalid["snapshot"]["unknown"] = json!(true);
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .validate(&invalid)
            .is_err()
    );
    assert!(ProgramSpace::from_json_slice(&serde_json::to_vec(&invalid).unwrap()).is_err());
    let mut nested_invalid: Value = serde_json::from_slice(FIXTURE).unwrap();
    nested_invalid["artifacts"][0]["provenance"]["unknown"] = json!(true);
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .validate(&nested_invalid)
            .is_err()
    );
    assert!(ProgramSpace::from_json_slice(&serde_json::to_vec(&nested_invalid).unwrap()).is_err());
}

#[test]
fn schema_and_domain_rejection_corpus_keeps_the_boundary_explicit() {
    let schema: Value = serde_json::from_slice(include_bytes!(
        "../../../schemas/reviewgraphen.input.schema.json"
    ))
    .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();

    let mut invalid_id: Value = serde_json::from_slice(FIXTURE).unwrap();
    invalid_id["artifacts"][0]["id"] = json!("function:-invalid");
    let mut invalid_enum: Value = serde_json::from_slice(FIXTURE).unwrap();
    invalid_enum["artifacts"][0]["kind"] = json!("not_a_kind");
    let mut invalid_min_items: Value = serde_json::from_slice(FIXTURE).unwrap();
    invalid_min_items["relations"][0]["target_ids"] = json!([]);
    for invalid in [&invalid_id, &invalid_enum, &invalid_min_items] {
        assert!(
            validator.validate(invalid).is_err(),
            "schema-invalid corpus item"
        );
        assert!(ProgramSpace::from_json_slice(&serde_json::to_vec(invalid).unwrap()).is_err());
    }

    let mut reviewed_fact: Value = serde_json::from_slice(FIXTURE).unwrap();
    reviewed_fact["artifacts"][0]["provenance"]["review_status"] = json!("human_reviewed");
    let mut parsed_over_total: Value = serde_json::from_slice(FIXTURE).unwrap();
    parsed_over_total["extraction"]["adapters"][0]["parsed"] = json!(4);
    parsed_over_total["extraction"]["adapters"][0]["total"] = json!(3);
    let mut duplicate_fact_id: Value = serde_json::from_slice(FIXTURE).unwrap();
    duplicate_fact_id["artifacts"][1]["id"] = duplicate_fact_id["artifacts"][0]["id"].clone();
    for domain_rejected in [&reviewed_fact, &parsed_over_total, &duplicate_fact_id] {
        validator
            .validate(domain_rejected)
            .expect("shape is deliberately schema-valid");
        assert!(
            ProgramSpace::from_json_slice(&serde_json::to_vec(domain_rejected).unwrap()).is_err()
        );
    }
}

#[test]
fn deterministic_id_registry_rejects_same_id_for_different_canonical_content() {
    let mut registry = IdRegistry::default();
    let collision = id("obligation:collision");
    registry
        .reserve(collision.clone(), &json!({ "value": 1 }))
        .unwrap();
    assert!(matches!(
        registry.reserve(collision, &json!({ "value": 2 })),
        Err(DomainError::IdCollision { .. })
    ));
}

#[test]
fn version_tuple_deserialization_revalidates_its_snapshot_namespace() {
    let invalid = json!({
        "profile": "code-review@1",
        "rule": "node.changed_public_symbol@1",
        "extractor_set": "sha256:7777777777777777",
        "snapshot": "function:not-a-snapshot",
    });
    assert!(serde_json::from_value::<VersionTuple>(invalid).is_err());
}

#[test]
fn evidence_admission_is_exactly_bound_to_run_snapshot_and_body() {
    let mut log = log("run:evidence-admission");
    let current = program();
    let evidence = evidence_with("evidence:exact-admission", &current);
    let wrong_run = EventLog::new(id("run:other"), aggregate())
        .unwrap()
        .admit_evidence(&current.evidence_snapshot_admission(), &evidence)
        .unwrap();
    assert!(matches!(
        log.append(EventCommand::evidence_recorded(evidence.clone(), wrong_run)),
        Err(DomainError::Validation(_))
    ));
    let exact = log
        .admit_evidence(&current.evidence_snapshot_admission(), &evidence)
        .unwrap();
    let different_body = Evidence::new(
        id("evidence:exact-admission"),
        "static_fact",
        BTreeSet::from([id("function:checkout-submit")]),
        EvidenceDetails::new(Some("altered witness".to_owned()), None, BTreeMap::new()),
        Provenance::accepted_deterministic(
            SourceRef::new("tool", "fixture-verifier@1", None, None, None).unwrap(),
            "fixture.verifier.v1",
            Some("1".to_owned()),
            Some(1.0),
        )
        .unwrap(),
        current.evidence_snapshot_admission(),
    )
    .unwrap();
    assert!(matches!(
        log.append(EventCommand::evidence_recorded(different_body, exact)),
        Err(DomainError::Validation(_))
    ));
    let exact = log
        .admit_evidence(&current.evidence_snapshot_admission(), &evidence)
        .unwrap();
    log.append(EventCommand::evidence_recorded(evidence, exact))
        .expect("exact current admission succeeds");
}

#[test]
fn every_authority_admission_is_bound_to_its_exact_stream_position() {
    let observed = program();
    let mut origin = log("run:position-admissions");
    let mut fork = log("run:position-admissions");
    let origin_obligation = node_obligation(&origin);
    let fork_obligation = fork
        .aggregate()
        .obligations()
        .find(|obligation| obligation.target_kind() == "relation")
        .unwrap()
        .id()
        .clone();
    origin
        .append(EventCommand::obligation_transition(
            origin_obligation.clone(),
            ObligationLifecycle::Planned,
        ))
        .unwrap();
    fork.append(EventCommand::obligation_transition(
        fork_obligation,
        ObligationLifecycle::Planned,
    ))
    .unwrap();
    let origin_claim = ReviewClaim::propose_ai(
        id("claim:position-admissions"),
        id("execution:fixture"),
        BTreeSet::from([origin_obligation]),
        ClaimPolarity::IssuePresent,
        "Same IDs must not make a forked stream authoritative.",
        BTreeSet::from([id("state:checkout-loading")]),
        None,
    )
    .unwrap();
    let fork_claim = ReviewClaim::propose_ai(
        id("claim:position-admissions"),
        id("execution:fixture"),
        BTreeSet::from([node_obligation(&fork)]),
        ClaimPolarity::IssuePresent,
        "Same IDs with a different claim referent must not reuse an admission.",
        BTreeSet::from([id("state:checkout-loading")]),
        None,
    )
    .unwrap();
    origin
        .append(EventCommand::claim_proposed(origin_claim))
        .unwrap();
    fork.append(EventCommand::claim_proposed(fork_claim))
        .unwrap();
    let evidence = evidence_with("evidence:position-admissions", &observed);
    let evidence_id = evidence.id().clone();
    let origin_evidence_admission = origin
        .admit_evidence(&observed.evidence_snapshot_admission(), &evidence)
        .unwrap();
    assert!(matches!(
        fork.append(EventCommand::evidence_recorded(
            evidence.clone(),
            origin_evidence_admission.clone(),
        )),
        Err(DomainError::Validation(_))
    ));
    origin
        .append(EventCommand::evidence_recorded(
            evidence.clone(),
            origin_evidence_admission,
        ))
        .unwrap();
    let fork_evidence_admission = fork
        .admit_evidence(&observed.evidence_snapshot_admission(), &evidence)
        .unwrap();
    fork.append(EventCommand::evidence_recorded(
        evidence,
        fork_evidence_admission,
    ))
    .unwrap();
    let binding = EvidenceBinding::new(
        id("binding:position-admissions"),
        id("claim:position-admissions"),
        evidence_id.clone(),
        EvidenceRelation::Supports,
        BTreeMap::from([(
            "property_id".to_owned(),
            "async.concurrent_reentry".to_owned(),
        )]),
    )
    .unwrap();
    let origin_binding_admission = origin.admit_evidence_binding(&binding).unwrap();
    assert!(matches!(
        fork.append(EventCommand::evidence_bound(
            binding.clone(),
            origin_binding_admission.clone(),
        )),
        Err(DomainError::Validation(_))
    ));
    origin
        .append(EventCommand::evidence_bound(
            binding.clone(),
            origin_binding_admission,
        ))
        .unwrap();
    let fork_binding_admission = fork.admit_evidence_binding(&binding).unwrap();
    fork.append(EventCommand::evidence_bound(
        binding,
        fork_binding_admission,
    ))
    .unwrap();
    let verification = Verification::new(
        id("verification:position-admissions"),
        id("claim:position-admissions"),
        VerificationOutcome::Passed,
        "fixture-verifier@1",
        BTreeSet::from([evidence_id.clone()]),
    )
    .unwrap();
    let origin_verification_admission = origin.admit_verification(verification.clone()).unwrap();
    assert!(matches!(
        fork.append(EventCommand::verification_recorded(
            verification.clone(),
            origin_verification_admission.clone(),
        )),
        Err(DomainError::Validation(_))
    ));
    origin
        .append(EventCommand::verification_recorded(
            verification.clone(),
            origin_verification_admission,
        ))
        .unwrap();
    let fork_verification_admission = fork.admit_verification(verification.clone()).unwrap();
    fork.append(EventCommand::verification_recorded(
        verification,
        fork_verification_admission,
    ))
    .unwrap();
    let decision = Decision::human(
        id("decision:position-admissions"),
        id("claim:position-admissions"),
        DecisionOutcome::Accept,
        human(),
        "The decision token must bind the same tail, not just the same closure.",
        BTreeSet::from([
            id("claim:position-admissions"),
            evidence_id,
            id("verification:position-admissions"),
        ]),
    )
    .unwrap();
    let origin_decision_admission = origin.admit_decision(&human(), &decision).unwrap();
    assert!(matches!(
        fork.append(EventCommand::decision_recorded(
            decision,
            origin_decision_admission,
        )),
        Err(DomainError::Validation(_))
    ));
}

#[test]
fn event_genesis_rejects_preaccepted_or_nonpristine_aggregates() {
    let mut active = log("run:genesis-source");
    append_fully_connected_multi_claim(&mut active);
    assert!(EventLog::new(id("run:genesis-reject"), active.aggregate().clone()).is_err());
    assert!(
        EventLog::replay_envelopes(
            id("run:genesis-reject"),
            active.aggregate().clone(),
            &[],
            &EventAdmissions::default(),
        )
        .is_err()
    );
}

#[test]
fn imported_event_payload_requires_exact_canonical_private_dto_form() {
    let mut log = log("run:canonical-event");
    let claim = ReviewClaim::propose_ai(
        id("claim:canonical-event"),
        id("execution:fixture"),
        BTreeSet::from([node_obligation(&log)]),
        ClaimPolarity::IssuePresent,
        "Canonical payload test.",
        BTreeSet::from([id("context:ui-event"), id("function:checkout-submit")]),
        Some(0.2),
    )
    .unwrap();
    log.append(EventCommand::claim_proposed(claim)).unwrap();
    let envelope = log.envelopes().next().unwrap();
    let mut reordered: Value = serde_json::to_value(envelope).unwrap();
    reordered["payload"]["data"]["source_ids"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert!(EventEnvelope::from_json_slice(&serde_json::to_vec(&reordered).unwrap()).is_err());

    let current = program();
    let evidence = evidence_with("evidence:canonical-event", &current);
    let admission = log
        .admit_evidence(&current.evidence_snapshot_admission(), &evidence)
        .unwrap();
    log.append(EventCommand::evidence_recorded(evidence, admission))
        .unwrap();
    let mut omitted: Value = serde_json::to_value(log.envelopes().last().unwrap()).unwrap();
    omitted["payload"]["data"]
        .as_object_mut()
        .unwrap()
        .remove("artifact_ref");
    assert!(EventEnvelope::from_json_slice(&serde_json::to_vec(&omitted).unwrap()).is_err());
    let mut unknown_nested: Value = serde_json::to_value(log.envelopes().last().unwrap()).unwrap();
    unknown_nested["payload"]["data"]["provenance"]["unexpected"] = json!(true);
    assert!(EventEnvelope::from_json_slice(&serde_json::to_vec(&unknown_nested).unwrap()).is_err());
}

#[test]
fn all_multi_obligation_chains_expand_each_coverage_axis() {
    let mut log = log("run:multi-positive");
    let (claim, evidence, verification, _) = append_fully_connected_multi_claim(&mut log);
    let coverage = Coverage::from_aggregate(log.aggregate());
    assert_eq!(coverage.evidence_supported.raw.numerator, 2.0);
    assert_eq!(coverage.verified.raw.numerator, 2.0);
    assert_eq!(coverage.fresh.raw.numerator, 2.0);
    assert_eq!(coverage.human_accepted.raw.numerator, 2.0);
    log.append(EventCommand::finding_recorded(Finding::new(
        id("finding:multi-positive"),
        claim,
        FindingStatus::Accepted,
        FindingTrace::new(
            BTreeSet::from([evidence]),
            BTreeSet::from([verification]),
            Some(id("decision:multi-connected")),
            BTreeSet::from([id("context:ui-event")]),
        ),
    )))
    .expect("accepted finding has every per-obligation chain");
}

#[test]
fn claim_sources_must_ground_every_claimed_obligation() {
    let mut log = log("run:claim-sources");
    let payment = log
        .aggregate()
        .obligations()
        .find(|item| item.property_id() == "payment.idempotency_contract")
        .unwrap()
        .id()
        .clone();
    let claim = ReviewClaim::propose_ai(
        id("claim:ungrounded-multi"),
        id("execution:fixture"),
        BTreeSet::from([node_obligation(&log), payment]),
        ClaimPolarity::IssuePresent,
        "One source cannot ground the payment relation.",
        BTreeSet::from([id("state:checkout-loading")]),
        None,
    )
    .unwrap();
    assert!(matches!(
        log.append(EventCommand::claim_proposed(claim)),
        Err(DomainError::Validation(_))
    ));
}

#[test]
fn accepted_finding_rejects_spliced_dangling_and_bare_verification_traces() {
    let mut log = log("run:finding-trace");
    let obligation = node_obligation(&log);
    let (claim, evidence, verification) =
        append_supported_claim(&mut log, BTreeSet::from([obligation]), &program());
    let decision_admission = accept(
        &mut log,
        claim.clone(),
        evidence.clone(),
        verification.clone(),
    );
    let valid = Finding::new(
        id("finding:accepted-trace"),
        claim.clone(),
        FindingStatus::Accepted,
        FindingTrace::new(
            BTreeSet::from([evidence.clone()]),
            BTreeSet::from([verification.clone()]),
            Some(id("decision:accept-duplicate-charge")),
            BTreeSet::from([id("state:checkout-loading")]),
        ),
    );
    log.append(EventCommand::finding_recorded(valid))
        .expect("complete joined finding trace is accepted");

    let spliced = Finding::new(
        id("finding:spliced-trace"),
        claim.clone(),
        FindingStatus::Accepted,
        FindingTrace::new(
            BTreeSet::from([id("evidence:not-in-chain")]),
            BTreeSet::from([verification]),
            Some(id("decision:accept-duplicate-charge")),
            BTreeSet::from([id("state:checkout-loading")]),
        ),
    );
    assert!(matches!(
        log.append(EventCommand::finding_recorded(spliced)),
        Err(DomainError::DanglingReference { .. }) | Err(DomainError::Validation(_))
    ));
    let dangling = Finding::new(
        id("finding:dangling-decision"),
        claim,
        FindingStatus::Accepted,
        FindingTrace::new(
            BTreeSet::from([evidence]),
            BTreeSet::new(),
            Some(id("decision:not-recorded")),
            BTreeSet::from([id("state:checkout-loading")]),
        ),
    );
    assert!(matches!(
        log.append(EventCommand::finding_recorded(dangling)),
        Err(DomainError::DanglingReference { .. }) | Err(DomainError::Validation(_))
    ));
    assert!(
        log.events()
            .iter()
            .any(|event| event.decision_admission() == Some(&decision_admission))
    );
}

#[test]
fn bare_passed_verification_cannot_increase_coverage() {
    let mut log = log("run:bare-verification");
    let obligation = node_obligation(&log);
    let claim = ReviewClaim::propose_ai(
        id("claim:bare-verification"),
        id("execution:fixture"),
        BTreeSet::from([obligation]),
        ClaimPolarity::IssuePresent,
        "No evidence binding exists.",
        BTreeSet::from([id("function:checkout-submit")]),
        None,
    )
    .unwrap();
    let claim_id = claim.id().clone();
    log.append(EventCommand::claim_proposed(claim)).unwrap();
    let evidence = evidence_with("evidence:bare-verification", log.aggregate().program());
    let evidence_id = evidence.id().clone();
    let admission = log
        .admit_evidence(
            &log.aggregate().program().evidence_snapshot_admission(),
            &evidence,
        )
        .unwrap();
    log.append(EventCommand::evidence_recorded(evidence, admission))
        .unwrap();
    let verification = Verification::new(
        id("verification:bare"),
        claim_id,
        VerificationOutcome::Passed,
        "fixture-verifier@1",
        BTreeSet::from([evidence_id]),
    )
    .unwrap();
    assert!(matches!(
        log.admit_verification(verification),
        Err(DomainError::Validation(_))
    ));
    assert_eq!(
        Coverage::from_aggregate(log.aggregate())
            .verified
            .raw
            .numerator,
        0.0
    );
}

#[test]
fn decision_admission_is_exactly_bound_to_run_actor_outcome_and_body() {
    let mut log = log("run:decision-admission");
    let obligation = node_obligation(&log);
    let (claim, evidence, verification) =
        append_supported_claim(&mut log, BTreeSet::from([obligation]), &program());
    let decision = Decision::human(
        id("decision:admission-exact"),
        claim.clone(),
        DecisionOutcome::Accept,
        human(),
        "A decision admission must bind this exact body.",
        BTreeSet::from([claim.clone(), evidence.clone(), verification.clone()]),
    )
    .unwrap();
    let mut other_log = EventLog::new(id("run:other"), aggregate()).unwrap();
    let other_obligation = node_obligation(&other_log);
    let (other_claim, other_evidence, other_verification) = append_supported_claim(
        &mut other_log,
        BTreeSet::from([other_obligation]),
        &program(),
    );
    let wrong_run_decision = Decision::human(
        id("decision:admission-exact"),
        other_claim.clone(),
        DecisionOutcome::Accept,
        human(),
        "A decision admission must bind this exact body.",
        BTreeSet::from([other_claim, other_evidence, other_verification]),
    )
    .unwrap();
    let wrong_run = other_log
        .admit_decision(&human(), &wrong_run_decision)
        .unwrap();
    assert!(matches!(
        log.append(EventCommand::decision_recorded(decision.clone(), wrong_run)),
        Err(DomainError::Validation(_))
    ));
    let exact = log.admit_decision(&human(), &decision).unwrap();
    let different_body = Decision::human(
        id("decision:admission-exact"),
        claim,
        DecisionOutcome::Accept,
        human(),
        "A materially different rationale changes the admitted body.",
        BTreeSet::from([id("claim:duplicate-charge"), evidence, verification]),
    )
    .unwrap();
    assert!(matches!(
        log.append(EventCommand::decision_recorded(different_body, exact)),
        Err(DomainError::Validation(_))
    ));
}

#[test]
fn decision_and_evidence_admissions_bind_the_exact_closure_and_genesis() {
    let mut original = log("run:closure-bound");
    let (claim, evidence, verification) = append_supported_claim_with_summary(
        &mut original,
        "The original claim body is part of the admitted closure.",
        None,
        "fixture-verifier@1",
    );
    let decision = Decision::human(
        id("decision:closure-bound"),
        claim.clone(),
        DecisionOutcome::Accept,
        human(),
        "Accept only this exact evidence and verification closure.",
        BTreeSet::from([claim.clone(), evidence.clone(), verification.clone()]),
    )
    .unwrap();
    let decision_admission = original.admit_decision(&human(), &decision).unwrap();

    let mut changed_claim = log("run:closure-bound");
    append_supported_claim_with_summary(
        &mut changed_claim,
        "A same-ID claim with a changed summary must not reuse admission.",
        None,
        "fixture-verifier@1",
    );
    assert!(matches!(
        changed_claim.append(EventCommand::decision_recorded(
            decision.clone(),
            decision_admission.clone(),
        )),
        Err(DomainError::Validation(_))
    ));

    let mut changed_binding = log("run:closure-bound");
    append_supported_claim_with_summary(
        &mut changed_binding,
        "The original claim body is part of the admitted closure.",
        Some("different-but-same-id-binding"),
        "fixture-verifier@1",
    );
    assert!(matches!(
        changed_binding.append(EventCommand::decision_recorded(
            decision.clone(),
            decision_admission.clone(),
        )),
        Err(DomainError::Validation(_))
    ));

    let mut changed_verification = log("run:closure-bound");
    append_supported_claim_with_summary(
        &mut changed_verification,
        "The original claim body is part of the admitted closure.",
        None,
        "fixture-verifier@2",
    );
    assert!(matches!(
        changed_verification.append(EventCommand::decision_recorded(
            decision.clone(),
            decision_admission.clone(),
        )),
        Err(DomainError::Validation(_))
    ));

    let observed = program();
    let evidence_body = evidence_with("evidence:genesis-bound", &observed);
    let evidence_admission = original
        .admit_evidence(&observed.evidence_snapshot_admission(), &evidence_body)
        .unwrap();
    let mut altered_input: Value = serde_json::from_slice(FIXTURE).unwrap();
    altered_input["repository"]["root"] = json!("/other/genesis");
    let altered_program =
        ProgramSpace::from_json_slice(&serde_json::to_vec(&altered_input).unwrap()).unwrap();
    let mut changed_genesis =
        EventLog::new(id("run:closure-bound"), aggregate_for(altered_program)).unwrap();
    assert_ne!(original.genesis_hash(), changed_genesis.genesis_hash());
    assert!(matches!(
        changed_genesis.append(EventCommand::evidence_recorded(
            evidence_body,
            evidence_admission,
        )),
        Err(DomainError::Validation(_))
    ));

    append_supported_claim_with_summary(
        &mut changed_genesis,
        "The original claim body is part of the admitted closure.",
        None,
        "fixture-verifier@1",
    );
    assert!(matches!(
        changed_genesis.append(EventCommand::decision_recorded(
            decision,
            decision_admission
        )),
        Err(DomainError::Validation(_))
    ));
}

#[test]
fn imported_binding_and_verification_require_retained_exact_admissions() {
    let mut log = log("run:trace-admissions");
    let obligation = node_obligation(&log);
    append_supported_claim(&mut log, BTreeSet::from([obligation]), &program());
    let envelopes = log.envelopes().cloned().collect::<Vec<_>>();
    let evidence = log
        .events()
        .iter()
        .filter_map(|event| event.evidence_admission().cloned())
        .collect::<Vec<_>>();
    let bindings = log
        .events()
        .iter()
        .filter_map(|event| event.binding_admission().cloned())
        .collect::<Vec<_>>();
    let verifications = log
        .events()
        .iter()
        .filter_map(|event| event.verification_admission().cloned())
        .collect::<Vec<_>>();
    let complete = EventAdmissions::new(evidence.clone(), Vec::new())
        .with_trace_admissions(bindings.clone(), verifications.clone());
    assert!(
        EventLog::replay_envelopes(
            id("run:trace-admissions"),
            aggregate(),
            &envelopes,
            &complete
        )
        .is_ok()
    );
    let without_binding = EventAdmissions::new(evidence.clone(), Vec::new())
        .with_trace_admissions(Vec::new(), verifications.clone());
    assert!(
        EventLog::replay_envelopes(
            id("run:trace-admissions"),
            aggregate(),
            &envelopes,
            &without_binding,
        )
        .is_err()
    );
    let without_verification =
        EventAdmissions::new(evidence, Vec::new()).with_trace_admissions(bindings, Vec::new());
    assert!(
        EventLog::replay_envelopes(
            id("run:trace-admissions"),
            aggregate(),
            &envelopes,
            &without_verification,
        )
        .is_err()
    );
}

#[test]
fn event_hash_chain_rejects_a_fork_splice_and_allows_a_split_resume() {
    let mut left = log("run:hash-chain");
    let left_id = node_obligation(&left);
    left.append(EventCommand::obligation_transition(
        left_id.clone(),
        ObligationLifecycle::Planned,
    ))
    .unwrap();
    left.append(EventCommand::obligation_transition(
        left_id,
        ObligationLifecycle::InProgress,
    ))
    .unwrap();
    let mut right = log("run:hash-chain");
    let right_id = right
        .aggregate()
        .obligations()
        .find(|obligation| obligation.target_kind() == "relation")
        .unwrap()
        .id()
        .clone();
    right
        .append(EventCommand::obligation_transition(
            right_id.clone(),
            ObligationLifecycle::Planned,
        ))
        .unwrap();
    right
        .append(EventCommand::obligation_transition(
            right_id,
            ObligationLifecycle::InProgress,
        ))
        .unwrap();
    let fork_splice = vec![
        left.envelopes().next().unwrap().clone(),
        right.envelopes().nth(1).unwrap().clone(),
    ];
    assert!(
        EventLog::replay_envelopes(
            id("run:hash-chain"),
            aggregate(),
            &fork_splice,
            &EventAdmissions::default(),
        )
        .is_err()
    );
    let valid = left.envelopes().cloned().collect::<Vec<_>>();
    let mut replayed = EventLog::replay_envelopes(
        id("run:hash-chain"),
        aggregate(),
        &valid[..1],
        &EventAdmissions::default(),
    )
    .unwrap();
    replayed
        .resume_envelopes(&valid[1..], &EventAdmissions::default())
        .unwrap();
    assert_eq!(replayed.tail_hash(), valid.last().unwrap().event_hash());
}

#[test]
fn finding_polarity_sources_and_per_obligation_grounding_are_enforced() {
    let mut absent = log("run:finding-polarity");
    let absent_claim = ReviewClaim::propose_ai(
        id("claim:issue-absent"),
        id("execution:fixture"),
        BTreeSet::from([node_obligation(&absent)]),
        ClaimPolarity::IssueAbsent,
        "This claim has the wrong polarity for an issue finding.",
        BTreeSet::from([id("state:checkout-loading")]),
        None,
    )
    .unwrap();
    let absent_id = absent_claim.id().clone();
    absent
        .append(EventCommand::claim_proposed(absent_claim))
        .unwrap();
    assert!(
        absent
            .append(EventCommand::finding_recorded(Finding::new(
                id("finding:issue-absent"),
                absent_id,
                FindingStatus::UnverifiedCandidate,
                FindingTrace::new(
                    BTreeSet::new(),
                    BTreeSet::new(),
                    None,
                    BTreeSet::from([id("state:checkout-loading")])
                ),
            )))
            .is_err()
    );

    let mut unrelated_source = log("run:finding-source-subset");
    let source_obligation = node_obligation(&unrelated_source);
    let (supported_claim, _, _) = append_supported_claim(
        &mut unrelated_source,
        BTreeSet::from([source_obligation]),
        &program(),
    );
    assert!(
        unrelated_source
            .append(EventCommand::finding_recorded(Finding::new(
                id("finding:not-a-claim-source"),
                supported_claim,
                FindingStatus::UnverifiedCandidate,
                FindingTrace::new(
                    BTreeSet::new(),
                    BTreeSet::new(),
                    None,
                    BTreeSet::from([id("function:checkout-submit")]),
                ),
            )))
            .is_err()
    );

    let mut multi = log("run:finding-grounding");
    let payment = multi
        .aggregate()
        .obligations()
        .find(|obligation| obligation.property_id() == "payment.idempotency_contract")
        .unwrap()
        .id()
        .clone();
    let claim = ReviewClaim::propose_ai(
        id("claim:finding-grounding"),
        id("execution:fixture"),
        BTreeSet::from([node_obligation(&multi), payment]),
        ClaimPolarity::IssuePresent,
        "Each obligation needs a finding source of its own.",
        BTreeSet::from([
            id("state:checkout-loading"),
            id("relation:payment-calls-stripe"),
        ]),
        None,
    )
    .unwrap();
    let claim_id = claim.id().clone();
    multi.append(EventCommand::claim_proposed(claim)).unwrap();
    assert!(
        multi
            .append(EventCommand::finding_recorded(Finding::new(
                id("finding:partial-source-grounding"),
                claim_id,
                FindingStatus::UnverifiedCandidate,
                FindingTrace::new(
                    BTreeSet::new(),
                    BTreeSet::new(),
                    None,
                    BTreeSet::from([id("state:checkout-loading")]),
                ),
            )))
            .is_err()
    );
}

#[test]
fn capability_missing_without_concrete_targets_retains_a_conservative_unknown_denominator() {
    let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
    input["extraction"]["capabilities"]
        .as_object_mut()
        .unwrap()
        .remove("direct_calls");
    input["relations"]
        .as_array_mut()
        .unwrap()
        .retain(|relation| relation["id"] != "relation:tap-handled-by-submit");
    input["contexts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .for_each(|context| {
            context["member_ids"]
                .as_array_mut()
                .unwrap()
                .retain(|member| member != "relation:tap-handled-by-submit");
        });
    input["relations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .for_each(|relation| {
            relation["target_ids"]
                .as_array_mut()
                .unwrap()
                .retain(|target| target != "relation:tap-handled-by-submit");
        });
    let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
    let bundle = MvpRulePack::synthesize(&program).unwrap();
    let fallback = bundle
        .obligations()
        .iter()
        .find(|obligation| {
            obligation.version().rule() == "capability_gap.origin_rule@1"
                && obligation
                    .applicability_reasons()
                    .contains("origin_rule:relation.concurrent_reentry@1")
        })
        .expect("missing factual relation has a conservative candidate");
    assert_eq!(fallback.target_kind(), "subgraph");
    assert_eq!(fallback.applicability_status(), "unknown");
    assert!(
        fallback
            .applicability_reasons()
            .contains("capability_missing:direct_calls")
    );
    assert!(fallback.source_ids().contains(program.repository_id()));
    assert!(fallback.source_ids().contains(program.snapshot_id()));
    assert!(bundle.universe().raw_denominator() > 0);
}

#[test]
fn invariant_scope_and_cancelled_lifecycle_are_retained_explicitly() {
    let bundle = MvpRulePack::synthesize(&program()).unwrap();
    let invariant = bundle
        .obligations()
        .iter()
        .find(|obligation| obligation.target_kind() == "invariant")
        .unwrap();
    assert!(
        invariant
            .source_ids()
            .contains(&id("invariant:payment-at-most-once"))
    );
    assert!(
        invariant
            .source_ids()
            .contains(&id("requirement:payment-at-most-once"))
    );
    assert!(
        invariant
            .generator_ids()
            .contains(&id("invariant:payment-at-most-once"))
    );
    assert!(
        invariant
            .generator_ids()
            .contains(&id("requirement:payment-at-most-once"))
    );

    let mut cancelled = log("run:cancelled-visited");
    let obligation = node_obligation(&cancelled);
    cancelled
        .append(EventCommand::obligation_transition(
            obligation,
            ObligationLifecycle::Cancelled,
        ))
        .unwrap();
    let report =
        ReviewReport::from_aggregate(cancelled.run_id().clone(), cancelled.aggregate()).unwrap();
    assert_eq!(
        report.body()["coverage"]["stages"]["visited"]["numerator"],
        0
    );
}

#[test]
fn program_space_serialization_preserves_null_accepted_attributes() {
    let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
    input["artifacts"][0]["attributes"]["intentional_null"] = Value::Null;
    let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
    let serialized = serde_json::to_value(program).unwrap();
    assert!(
        serialized["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|artifact| artifact["attributes"]
                .get("intentional_null")
                .is_some_and(Value::is_null))
    );
    assert!(serialized["repository"].get("root").is_none());
}

#[test]
fn extractor_version_changes_universe_but_not_obligation_identity() {
    let original = MvpRulePack::synthesize(&program()).unwrap();
    let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
    input["extraction"]["adapter_set_hash"] = json!("sha256:9999999999999999");
    let changed_program =
        ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
    let changed = MvpRulePack::synthesize(&changed_program).unwrap();
    assert_ne!(original.universe().id(), changed.universe().id());
    let original_ids = original
        .obligations()
        .iter()
        .map(|item| (item.semantic_key().to_owned(), item.id().clone()))
        .collect::<BTreeMap<_, _>>();
    let changed_ids = changed
        .obligations()
        .iter()
        .map(|item| (item.semantic_key().to_owned(), item.id().clone()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(original_ids, changed_ids);
}

#[test]
fn missing_and_unknown_capabilities_are_obstructions_not_exclusions() {
    let complete = MvpRulePack::synthesize(&program()).unwrap();
    let complete_gap_ids = complete
        .obligations()
        .iter()
        .filter(|item| item.version().rule() == "capability_gap.origin_rule@1")
        .map(|item| item.id().clone())
        .collect::<BTreeSet<_>>();
    let mut capability_universes = BTreeSet::new();
    for state in [None, Some("unknown")] {
        let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
        let capabilities = input["extraction"]["capabilities"].as_object_mut().unwrap();
        if let Some(state) = state {
            capabilities.insert("test_mapping".to_owned(), json!(state));
        } else {
            capabilities.remove("test_mapping");
        }
        let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
        let bundle = MvpRulePack::synthesize(&program).unwrap();
        assert_ne!(bundle.universe().id(), complete.universe().id());
        capability_universes.insert(bundle.universe().id().clone());
        let gap_ids = bundle
            .obligations()
            .iter()
            .filter(|item| item.version().rule() == "capability_gap.origin_rule@1")
            .map(|item| item.id().clone())
            .collect::<BTreeSet<_>>();
        assert!(!gap_ids.is_empty());
        assert_ne!(gap_ids, complete_gap_ids);
        assert_eq!(bundle.universe().raw_denominator(), 7);
        assert!(bundle.universe().exclusions().is_empty());
        assert!(
            bundle
                .obligations()
                .iter()
                .any(|item| item.applicability_status() == "unknown")
        );
        let (universe, obligations) = bundle.into_parts();
        let aggregate = ReviewAggregate::new(program, universe, obligations).unwrap();
        let report = ReviewReport::from_aggregate(id("run:capability"), &aggregate).unwrap();
        assert!(
            !report.body()["result"]["obstructions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            report.body()["coverage"]["limitations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "capability_missing")
        );
    }
    assert_eq!(
        capability_universes.len(),
        2,
        "missing and explicit unknown capability declarations are distinct denominator inputs"
    );
}

#[test]
fn explicit_policy_exclusions_retain_exact_floating_weight() {
    let original = MvpRulePack::synthesize(&program()).unwrap();
    let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
    input["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["id"] == "function:checkout-submit")
        .unwrap()["attributes"]["reviewgraphen_excluded"] = json!("policy:fixture-only");
    let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
    let bundle = MvpRulePack::synthesize(&program).unwrap();
    assert_eq!(bundle.universe().raw_denominator(), 4);
    assert_ne!(bundle.universe().id(), original.universe().id());
    assert_eq!(bundle.universe().exclusions().len(), 1);
    assert_eq!(bundle.universe().excluded_weight(), 3.0);
    let value = serde_json::to_value(&bundle).unwrap();
    assert!(value["universe"]["exclusions"][0]["excluded_weight"].is_f64());
}

#[test]
fn invariant_generation_merges_all_equivalent_path_dependencies() {
    let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
    let duplicate = input["relations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "relation:payment-calls-stripe")
        .unwrap()
        .clone();
    let mut duplicate = duplicate;
    duplicate["id"] = json!("relation:payment-calls-stripe-branch");
    input["relations"].as_array_mut().unwrap().push(duplicate);
    input["contexts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["id"] == "context:payment")
        .unwrap()["member_ids"]
        .as_array_mut()
        .unwrap()
        .push(json!("relation:payment-calls-stripe-branch"));
    input["relations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["id"] == "relation:test-covers-path")
        .unwrap()["target_ids"]
        .as_array_mut()
        .unwrap()
        .push(json!("relation:payment-calls-stripe-branch"));
    let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
    let bundle = MvpRulePack::synthesize(&program).unwrap();
    let invariants = bundle
        .obligations()
        .iter()
        .filter(|item| item.target_kind() == "invariant")
        .collect::<Vec<_>>();
    assert_eq!(invariants.len(), 1);
    assert_eq!(invariants[0].depends_on().len(), 2);
    assert_eq!(invariants[0].generator_ids().len(), 6);
}

#[test]
fn invariant_scope_contexts_gate_path_applicability() {
    let matching = MvpRulePack::synthesize(&program()).unwrap();
    let matching_invariant = matching
        .obligations()
        .iter()
        .find(|obligation| obligation.target_kind() == "invariant")
        .expect("matching fixture scope generates its invariant");
    assert!(
        matching_invariant
            .generator_ids()
            .contains(&id("context:ui-event"))
    );
    assert!(
        matching_invariant
            .generator_ids()
            .contains(&id("context:payment"))
    );

    let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
    let mut disjoint_context = input["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|context| context["id"] == "context:ui-event")
        .unwrap()
        .clone();
    disjoint_context["id"] = json!("context:disjoint-invariant-scope");
    disjoint_context["member_ids"] = json!(["event:buy-tap"]);
    input["contexts"]
        .as_array_mut()
        .unwrap()
        .push(disjoint_context);
    input["invariants"][0]["scope_ids"] = json!(["context:disjoint-invariant-scope"]);
    let disjoint = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
    let bundle = MvpRulePack::synthesize(&disjoint).unwrap();
    assert!(
        bundle
            .obligations()
            .iter()
            .any(|obligation| obligation.target_kind() == "path")
    );
    assert!(
        bundle
            .obligations()
            .iter()
            .all(|obligation| obligation.target_kind() != "invariant")
    );
}

#[test]
fn program_contract_preserves_repository_and_snapshot_optional_values() {
    let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
    input["repository"]["root"] = json!("/workspace/reviewgraphen-a");
    input["repository"]["uri"] = json!("https://example.invalid/a.git");
    input["snapshot"]["created_at"] = json!("2026-08-08T01:02:03Z");
    let parsed = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
    assert_eq!(parsed.repository_root(), Some("/workspace/reviewgraphen-a"));
    assert_eq!(
        parsed.repository_uri(),
        Some("https://example.invalid/a.git")
    );
    assert_eq!(parsed.snapshot_created_at(), Some("2026-08-08T01:02:03Z"));
    let serialized = serde_json::to_value(parsed).unwrap();
    assert_eq!(
        serialized["repository"]["root"],
        input["repository"]["root"]
    );
    assert_eq!(serialized["repository"]["uri"], input["repository"]["uri"]);
    assert_eq!(
        serialized["snapshot"]["created_at"],
        input["snapshot"]["created_at"]
    );
}

#[test]
fn report_visited_and_weighted_stage_values_follow_lifecycle_and_weights() {
    let mut log = log("run:report-coverage");
    let obligation = node_obligation(&log);
    log.append(EventCommand::obligation_transition(
        obligation.clone(),
        ObligationLifecycle::Planned,
    ))
    .unwrap();
    let report = ReviewReport::from_aggregate(log.run_id().clone(), log.aggregate()).unwrap();
    let stages = &report.body()["coverage"]["stages"];
    assert_eq!(stages["visited"]["numerator"], 0);
    assert_eq!(stages["completed"]["numerator"], 0);
    assert_eq!(stages["visited"]["weighted"], json!(0.0));
    log.append(EventCommand::obligation_transition(
        obligation.clone(),
        ObligationLifecycle::InProgress,
    ))
    .unwrap();
    let in_progress = ReviewReport::from_aggregate(log.run_id().clone(), log.aggregate()).unwrap();
    assert_eq!(
        in_progress.body()["coverage"]["stages"]["visited"]["weighted"],
        json!(3.0 / 24.5)
    );
    log.append(EventCommand::obligation_transition(
        obligation,
        ObligationLifecycle::Completed,
    ))
    .unwrap();
    let completed = ReviewReport::from_aggregate(log.run_id().clone(), log.aggregate()).unwrap();
    assert_eq!(
        completed.body()["coverage"]["stages"]["completed"]["weighted"],
        json!(3.0 / 24.5)
    );
}

#[test]
fn audience_projections_retain_sources_and_declare_meaningful_loss() {
    let aggregate = aggregate();
    for projection in [
        Projection::human(&aggregate).unwrap(),
        Projection::ai(&aggregate).unwrap(),
        Projection::audit(&aggregate).unwrap(),
    ] {
        assert!(!projection.source_ids().is_empty());
        assert!(!projection.information_loss().is_empty());
        let expected = canonical_json(&(
            projection.kind(),
            projection.source_ids(),
            projection.information_loss(),
            projection.payload(),
        ))
        .unwrap();
        assert_eq!(
            projection.projection_hash(),
            ContentHash::sha256(&expected).to_string()
        );
    }
}

proptest! {
    #[test]
    fn stable_ids_and_canonical_objects_are_order_independent(
        left in "[a-z]{1,12}",
        right in "[a-z]{1,12}"
    ) {
        let first = BTreeMap::from([
            ("a".to_owned(), json!(left)),
            ("b".to_owned(), json!(right)),
        ]);
        let second = BTreeMap::from([
            ("b".to_owned(), json!(right)),
            ("a".to_owned(), json!(left)),
        ]);
        prop_assert_eq!(canonical_json(&first).unwrap(), canonical_json(&second).unwrap());
        prop_assert_eq!(StableId::derived("obligation", &first).unwrap(), StableId::derived("obligation", &second).unwrap());
    }

    #[test]
    fn profile_rule_and_snapshot_tuple_components_do_not_alias(
        profile in "[a-z]{1,8}",
        rule in "[a-z]{1,8}",
        snapshot in "[a-z]{1,8}"
    ) {
        let base = BTreeMap::from([
            ("profile".to_owned(), json!(profile)),
            ("rule".to_owned(), json!(rule)),
            ("snapshot_semantic_id".to_owned(), json!(format!("snapshot:{snapshot}"))),
        ]);
        let mut changed_profile = base.clone();
        changed_profile.insert("profile".to_owned(), json!(format!("{profile}-other")));
        let mut changed_rule = base.clone();
        changed_rule.insert("rule".to_owned(), json!(format!("{rule}-other")));
        let mut changed_snapshot = base.clone();
        changed_snapshot.insert(
            "snapshot_semantic_id".to_owned(),
            json!(format!("snapshot:{snapshot}-other")),
        );
        let obligation_id = StableId::derived("obligation", &base).unwrap();
        prop_assert_ne!(&obligation_id, &StableId::derived("obligation", &changed_profile).unwrap());
        prop_assert_ne!(&obligation_id, &StableId::derived("obligation", &changed_rule).unwrap());
        prop_assert_ne!(&obligation_id, &StableId::derived("obligation", &changed_snapshot).unwrap());
    }

    #[test]
    fn nested_set_like_input_order_does_not_change_the_contract(
        reverse_artifacts in any::<bool>(),
        reverse_relations in any::<bool>(),
        reverse_context_members in any::<bool>(),
        reverse_evidence_targets in any::<bool>(),
    ) {
        let original = MvpRulePack::synthesize(&program()).unwrap();
        let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
        if reverse_artifacts {
            input["artifacts"].as_array_mut().unwrap().reverse();
        }
        if reverse_relations {
            input["relations"].as_array_mut().unwrap().reverse();
        }
        if reverse_context_members {
            for context in input["contexts"].as_array_mut().unwrap() {
                context["member_ids"].as_array_mut().unwrap().reverse();
            }
        }
        if reverse_evidence_targets {
            for evidence in input["evidence"].as_array_mut().unwrap() {
                evidence["target_ids"].as_array_mut().unwrap().reverse();
            }
        }
        let reordered = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
        let changed = MvpRulePack::synthesize(&reordered).unwrap();
        prop_assert_eq!(
            original.contract().canonical().bytes(),
            changed.contract().canonical().bytes(),
        );
    }
}
