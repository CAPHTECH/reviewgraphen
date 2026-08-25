use reviewgraphen_core::{
    ArtifactRegistered, ArtifactSensitivity, ArtifactSource, CanonicalJson, ContentHash,
    DomainError, EventLog, MvpRulePack, PlanBudget, ProgramSpace, ReviewAggregate, Severity,
    SnapshotSourceRecordEntry, SnapshotSourcesRecorded, StableId, plan,
    plan_resolved_target_obligations, prepare_subject_windows_v2,
};
use serde_json::{Value, json};

const D_RULE: &str = "relation.changed_public_callee@1";
const D_PROPERTY: &str = "rust.callee_contract_review@1";
type ContractMutation<'a> = Box<dyn Fn(&mut Value) + 'a>;

fn fixture() -> Value {
    let mut value: Value = serde_json::from_slice(include_bytes!(
        "../../../examples/double-submit-payment/program-space.json"
    ))
    .expect("reference ProgramSpace JSON");
    value["profile"]["id"] = json!("rust.production.v1");
    value["profile"]["version"] = json!("1");

    let artifacts = value["artifacts"].as_array_mut().expect("artifacts");
    artifacts
        .iter_mut()
        .find(|artifact| artifact["id"] == "file:payment-repository")
        .expect("callee file")["attributes"]["changed"] = json!(true);
    artifacts
        .iter_mut()
        .find(|artifact| artifact["id"] == "function:payment-charge")
        .expect("callee")["attributes"]["public"] = json!(true);
    artifacts
        .iter_mut()
        .find(|artifact| artifact["id"] == "function:checkout-submit")
        .expect("caller")["attributes"]["changed"] = json!(true);

    let relations = value["relations"].as_array_mut().expect("relations");
    relations
        .iter_mut()
        .find(|relation| relation["id"] == "relation:submit-calls-payment")
        .expect("call relation")["attributes"]["resolution"] = json!("syntactic_unique");
    let mut contains = relations[0].clone();
    contains["id"] = json!("relation:payment-file-contains-charge");
    contains["kind"] = json!("contains");
    contains["source_id"] = json!("file:payment-repository");
    contains["target_ids"] = json!(["function:payment-charge"]);
    contains["attributes"] = json!({});
    relations.push(contains);

    value["extraction"]["capabilities"]["containment"] = json!({
        "state": "complete",
        "source_ids": ["relation:payment-file-contains-charge"]
    });
    value["extraction"]["capabilities"]["changed_structure"] = json!({
        "state": "complete",
        "source_ids": ["file:payment-repository"]
    });
    value["extraction"]["capabilities"]["direct_calls"]["state"] = json!("partial");
    value["extraction"]["limitations"]
        .as_array_mut()
        .expect("limitations")
        .push(json!({
            "id": "limitation:direct-calls",
            "kind": "projection_loss",
            "description": "Only syntactically unique local calls are enumerated.",
            "severity": "medium",
            "source_ids": ["relation:submit-calls-payment"],
            "related_capabilities": ["direct_calls"]
        }));
    value
}

fn program(value: Value) -> ProgramSpace {
    ProgramSpace::from_json_slice(&serde_json::to_vec(&value).expect("JSON bytes"))
        .expect("valid D ProgramSpace")
}

fn d_bundle(value: Value) -> (ProgramSpace, reviewgraphen_core::ObligationBundle) {
    let program = program(value);
    let bundle = MvpRulePack::synthesize_changed_public_callee(&program).expect("D synthesis");
    (program, bundle)
}

fn source_ready_d_aggregate() -> (ReviewAggregate, StableId, StableId, StableId, Vec<u8>) {
    let source_bytes = b"x\n".repeat(512);
    let source_hash = ContentHash::sha256(&source_bytes);
    let mut value = fixture();
    for artifact in value["artifacts"].as_array_mut().unwrap() {
        if artifact["kind"] == "file" {
            artifact["content_hash"] = json!(source_hash.to_string());
        }
    }
    let (program, bundle) = d_bundle(value);
    let run_id = StableId::parse("run:d-source-registration").unwrap();
    let mut registrations = Vec::new();
    let mut entries = Vec::new();
    for artifact in program
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
    {
        let source = ArtifactSource::SnapshotIngest {
            run_id: run_id.clone(),
            snapshot_id: program.snapshot_id().clone(),
            adapter_id: "d-source-registration@1".to_owned(),
        };
        let media_type = "text/rust";
        let registration_id = StableId::derived(
            "registration",
            &std::collections::BTreeMap::from([
                ("run_id".to_owned(), Value::String(run_id.to_string())),
                (
                    "cas_hash".to_owned(),
                    Value::String(source_hash.to_string()),
                ),
                (
                    "media_type".to_owned(),
                    Value::String(media_type.to_owned()),
                ),
                (
                    "sensitivity".to_owned(),
                    Value::String("workspace_source".to_owned()),
                ),
                ("source".to_owned(), serde_json::to_value(&source).unwrap()),
            ]),
        )
        .unwrap();
        if registrations.is_empty() {
            registrations.push(
                ArtifactRegistered::new(
                    run_id.clone(),
                    registration_id.clone(),
                    source_hash.clone(),
                    media_type,
                    source_bytes.len() as u64,
                    ArtifactSensitivity::WorkspaceSource,
                    source,
                )
                .unwrap(),
            );
        }
        entries.push(
            SnapshotSourceRecordEntry::new(
                artifact.id.clone(),
                artifact.location.as_ref().unwrap().path.clone(),
                source_hash.clone(),
                registration_id,
                source_hash.clone(),
                513,
            )
            .unwrap(),
        );
    }
    entries.sort_by(|left, right| left.path().cmp(right.path()));
    let aggregate = ReviewAggregate::read_only_from_d_two_layer_bundle(program.clone(), &bundle)
        .unwrap()
        .with_read_only_d_snapshot_sources(
            run_id,
            registrations,
            SnapshotSourcesRecorded::new(program.snapshot_id().clone(), entries).unwrap(),
        )
        .unwrap();
    let obligation_id = bundle
        .obligations()
        .iter()
        .find(|obligation| obligation.version().rule() == D_RULE)
        .unwrap()
        .id()
        .clone();
    (
        aggregate,
        obligation_id,
        StableId::parse("function:checkout-submit").unwrap(),
        StableId::parse("function:payment-charge").unwrap(),
        source_bytes,
    )
}

fn substantive(bundle: &reviewgraphen_core::ObligationBundle) -> &reviewgraphen_core::Obligation {
    bundle
        .obligations()
        .iter()
        .find(|obligation| obligation.version().rule() == D_RULE)
        .expect("one D obligation")
}

#[test]
fn coverage_extension_keeps_event_runtime_carriers_compact() {
    // EventLog owns two aggregates. Keeping the D-only coverage body inline
    // added 392 bytes to each aggregate (and each contract carrier), which
    // overflowed the default test-thread stack in deep V5 recovery paths.
    assert!(
        size_of::<reviewgraphen_core::UniverseDescriptor>() <= 256,
        "coverage details must remain indirectly stored"
    );
    assert!(
        size_of::<reviewgraphen_core::ReviewAggregate>() <= 1_840,
        "aggregate growth must not inflate every event recovery frame"
    );
    assert!(
        size_of::<reviewgraphen_core::ObligationBundle>() <= 672,
        "the internal and contract coverage carriers must both stay compact"
    );
    assert!(
        size_of::<reviewgraphen_core::EventLog>() <= 4_616,
        "an event log must stay near its pre-extension stack footprint"
    );
}

#[test]
fn accepted_unique_changed_public_callee_has_the_exact_relation_contract() {
    let bundle =
        MvpRulePack::synthesize_changed_public_callee(&program(fixture())).expect("D synthesis");
    let obligation = substantive(&bundle);
    assert_eq!(obligation.property_id(), D_PROPERTY);
    assert_eq!(obligation.target_kind(), "relation");
    assert_eq!(obligation.target_refs().len(), 1);
    assert_eq!(
        obligation.target_refs()[0].to_string(),
        "relation:submit-calls-payment"
    );
    assert_eq!(obligation.weight(), 4.0);
    assert!(obligation.depends_on().is_empty());
    assert_eq!(
        obligation.required_capabilities(),
        &std::collections::BTreeSet::from([
            "ast".to_owned(),
            "changed_structure".to_owned(),
            "containment".to_owned(),
        ])
    );
    assert_eq!(
        obligation.accepted_evidence_modes(),
        &std::collections::BTreeSet::from(["source_inspection".to_owned(), "test".to_owned()])
    );
    assert_eq!(obligation.applicability_status(), "applicable");
    let contract: Value = serde_json::to_value(bundle.contract()).expect("D contract");
    let context_requirement = contract["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == obligation.id().to_string())
        .unwrap()
        .get("context_requirement")
        .unwrap();
    assert_eq!(
        context_requirement["target_support_capabilities"],
        json!(["ast", "changed_structure", "containment"])
    );
    assert_eq!(
        context_requirement["enumeration_capabilities"],
        json!(["direct_calls"])
    );
    assert!(context_requirement.get("required_capabilities").is_none());
    for id in [
        "relation:submit-calls-payment",
        "function:checkout-submit",
        "function:payment-charge",
        "file:payment-repository",
        "relation:payment-file-contains-charge",
    ] {
        assert!(
            obligation
                .generator_ids()
                .iter()
                .any(|value| value.to_string() == id)
        );
    }
    assert!(bundle.obligations().iter().any(|obligation| {
        obligation.version().rule() == "capability_gap.origin_rule@1"
            && obligation
                .applicability_reasons()
                .contains("capability_partial:direct_calls")
    }));
}

#[test]
fn changed_caller_or_callee_attribute_does_not_substitute_for_changed_containment() {
    let mut caller_only = fixture();
    caller_only["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|artifact| artifact["id"] == "file:payment-repository")
        .unwrap()["attributes"]["changed"] = json!(false);
    assert_eq!(
        caller_only["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|artifact| artifact["id"] == "function:checkout-submit")
            .unwrap()["attributes"]["changed"],
        json!(true),
        "the negative control must keep an actually changed caller"
    );
    assert!(
        MvpRulePack::synthesize_changed_public_callee(&program(caller_only))
            .unwrap()
            .obligations()
            .iter()
            .all(|obligation| obligation.version().rule() != D_RULE)
    );

    let mut own_attribute = fixture();
    own_attribute["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|artifact| artifact["id"] == "file:payment-repository")
        .unwrap()["attributes"]["changed"] = json!(false);
    own_attribute["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|artifact| artifact["id"] == "function:payment-charge")
        .unwrap()["attributes"]["changed"] = json!(true);
    assert!(
        MvpRulePack::synthesize_changed_public_callee(&program(own_attribute))
            .unwrap()
            .obligations()
            .iter()
            .all(|obligation| obligation.version().rule() != D_RULE)
    );
}

#[test]
fn only_calls_relations_may_trigger_the_d_rule() {
    let mut value = fixture();
    let relation = value["relations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|relation| relation["id"] == "relation:submit-calls-payment")
        .unwrap();
    relation["kind"] = json!("contains");
    let bundle = MvpRulePack::synthesize_changed_public_callee(&program(value)).unwrap();
    assert!(
        bundle
            .obligations()
            .iter()
            .all(|obligation| obligation.version().rule() != D_RULE)
    );
}

#[test]
fn trigger_negatives_do_not_create_a_substantive_obligation() {
    for (name, mutate) in [
        (
            "private",
            Box::new(|value: &mut Value| {
                value["artifacts"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|artifact| artifact["id"] == "function:payment-charge")
                    .unwrap()["attributes"]["public"] = json!(false)
            }) as Box<dyn Fn(&mut Value)>,
        ),
        (
            "method",
            Box::new(|value: &mut Value| {
                value["artifacts"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|artifact| artifact["id"] == "function:payment-charge")
                    .unwrap()["kind"] = json!("method")
            }),
        ),
        (
            "missing resolution",
            Box::new(|value: &mut Value| {
                value["relations"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|relation| relation["id"] == "relation:submit-calls-payment")
                    .unwrap()["attributes"]
                    .as_object_mut()
                    .unwrap()
                    .remove("resolution");
            }),
        ),
        (
            "non-unique resolution",
            Box::new(|value: &mut Value| {
                value["relations"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|relation| relation["id"] == "relation:submit-calls-payment")
                    .unwrap()["attributes"]["resolution"] = json!("method_dispatch")
            }),
        ),
    ] {
        let mut value = fixture();
        mutate(&mut value);
        assert!(
            MvpRulePack::synthesize_changed_public_callee(&program(value))
                .unwrap()
                .obligations()
                .iter()
                .all(|obligation| obligation.version().rule() != D_RULE),
            "{name}"
        );
    }
}

#[test]
fn multiple_accepted_targets_is_a_typed_synthesis_obstruction() {
    let mut value = fixture();
    value["relations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|relation| relation["id"] == "relation:submit-calls-payment")
        .unwrap()["target_ids"] = json!(["function:payment-charge", "function:stripe-charge"]);
    let error = MvpRulePack::synthesize_changed_public_callee(&program(value)).unwrap_err();
    assert!(matches!(
        error,
        DomainError::Incomplete {
            operation: "D calls relation accepted callee target arity",
            limit: 1,
            observed: 2
        }
    ));
}

#[test]
fn each_distinct_accepted_relation_is_a_distinct_obligation() {
    let mut value = fixture();
    let duplicate_edge = value["relations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|relation| relation["id"] == "relation:submit-calls-payment")
        .unwrap()
        .clone();
    let mut duplicate_edge = duplicate_edge;
    duplicate_edge["id"] = json!("relation:submit-calls-payment-again");
    value["relations"]
        .as_array_mut()
        .unwrap()
        .push(duplicate_edge);
    let bundle = MvpRulePack::synthesize(&program(value)).unwrap();
    let targets = bundle
        .obligations()
        .iter()
        .filter(|obligation| obligation.version().rule() == D_RULE)
        .map(|obligation| obligation.target_refs()[0].to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        targets,
        std::collections::BTreeSet::from([
            "relation:submit-calls-payment".to_owned(),
            "relation:submit-calls-payment-again".to_owned(),
        ])
    );
}

#[test]
fn invalid_relation_inputs_and_detached_endpoint_references_cannot_reach_synthesis() {
    let mut zero_target = fixture();
    zero_target["relations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|relation| relation["id"] == "relation:submit-calls-payment")
        .unwrap()["target_ids"] = json!([]);
    assert!(ProgramSpace::from_json_slice(&serde_json::to_vec(&zero_target).unwrap()).is_err());

    let mut duplicate_relation = fixture();
    let existing = duplicate_relation["relations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|relation| relation["id"] == "relation:submit-calls-payment")
        .unwrap()
        .clone();
    duplicate_relation["relations"]
        .as_array_mut()
        .unwrap()
        .push(existing);
    assert!(
        ProgramSpace::from_json_slice(&serde_json::to_vec(&duplicate_relation).unwrap()).is_err()
    );

    let program = program(fixture());
    let mut detached = program
        .relation(&reviewgraphen_core::StableId::parse("relation:submit-calls-payment").unwrap())
        .unwrap()
        .clone();
    detached.target_ids.clear();
    assert!(matches!(
        MvpRulePack::validate_changed_public_callee_endpoints(&program, &detached),
        Err(DomainError::Incomplete {
            operation: "D calls relation accepted callee target arity",
            observed: 0,
            ..
        })
    ));
    let relation_id = reviewgraphen_core::StableId::parse("relation:submit-calls-payment").unwrap();
    let missing_callee = reviewgraphen_core::StableId::parse("function:not-accepted").unwrap();
    detached.target_ids.insert(missing_callee.clone());
    assert!(matches!(
        MvpRulePack::validate_changed_public_callee_endpoints(&program, &detached),
        Err(DomainError::DanglingReference {
            owner: "D calls relation",
            owner_id,
            reference,
        }) if owner_id == relation_id && reference == missing_callee
    ));
    detached.target_ids.clear();
    detached
        .target_ids
        .insert(reviewgraphen_core::StableId::parse("function:payment-charge").unwrap());
    detached.source_id = reviewgraphen_core::StableId::parse("function:not-accepted").unwrap();
    assert!(matches!(
        MvpRulePack::validate_changed_public_callee_endpoints(&program, &detached),
        Err(DomainError::DanglingReference { .. })
    ));
}

#[test]
fn production_profile_exclusion_is_visible_outside_the_denominator() {
    let mut value = fixture();
    value["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|artifact| artifact["id"] == "function:payment-charge")
        .unwrap()["location"]["path"] = json!("tests/payment.rs");
    let bundle = MvpRulePack::synthesize_changed_public_callee(&program(value)).unwrap();
    assert!(
        bundle
            .obligations()
            .iter()
            .all(|obligation| obligation.version().rule() != D_RULE)
    );
    assert_eq!(bundle.universe().exclusions().len(), 1);
    let exclusion = &bundle.universe().exclusions()[0];
    assert_eq!(exclusion.reason, "profile.exclude.test@1");
    assert_eq!(exclusion.excluded_weight, 4.0);
}

#[test]
fn target_support_gap_makes_only_the_target_unknown_while_direct_calls_is_enumeration_only() {
    let mut support_partial = fixture();
    support_partial["extraction"]["capabilities"]["ast"]["state"] = json!("partial");
    support_partial["extraction"]["limitations"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": "limitation:ast",
            "kind": "projection_loss",
            "description": "One admitted Rust source did not parse.",
            "severity": "medium",
            "source_ids": ["file:payment-repository"],
            "related_capabilities": ["ast"]
        }));
    let support_bundle =
        MvpRulePack::synthesize_changed_public_callee(&program(support_partial)).unwrap();
    let target = substantive(&support_bundle);
    assert_eq!(target.applicability_status(), "unknown");
    assert!(
        target
            .applicability_reasons()
            .contains("capability_partial:ast")
    );

    let direct_bundle = MvpRulePack::synthesize_changed_public_callee(&program(fixture())).unwrap();
    let direct_partial = substantive(&direct_bundle);
    assert_eq!(direct_partial.applicability_status(), "applicable");
}

#[test]
fn every_target_support_capability_state_is_unknown_with_its_exact_limitation_trace() {
    for capability in ["ast", "containment", "changed_structure"] {
        for state in ["partial", "missing", "unknown"] {
            let mut value = fixture();
            value["extraction"]["capabilities"][capability]["state"] = json!(state);
            let limitation_id = format!("limitation:{capability}-{state}");
            value["extraction"]["limitations"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "id": limitation_id,
                    "kind": if state == "missing" { "capability_missing" } else { "projection_loss" },
                    "description": format!("{capability} is {state}."),
                    "severity": "medium",
                    "source_ids": ["file:payment-repository"],
                    "related_capabilities": [capability]
                }));
            let bundle = MvpRulePack::synthesize_changed_public_callee(&program(value)).unwrap();
            let obligation = substantive(&bundle);
            assert_eq!(obligation.applicability_status(), "unknown");
            assert!(
                obligation
                    .applicability_reasons()
                    .contains(&format!("capability_{state}:{capability}"))
            );
            assert!(
                obligation
                    .qualification_ids()
                    .iter()
                    .any(|id| id.to_string() == limitation_id)
            );
        }
    }
}

#[test]
fn d_contract_schema_rejects_capability_split_mutations_and_accepts_the_bundle() {
    let bundle = MvpRulePack::synthesize_changed_public_callee(&program(fixture())).unwrap();
    let schema: Value = serde_json::from_slice(include_bytes!(
        "../../../schemas/reviewgraphen.obligation.schema.json"
    ))
    .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let contract = serde_json::to_value(bundle.contract()).unwrap();
    assert!(validator.is_valid(&contract), "D contract must validate");
    let obligation = contract["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .position(|obligation| obligation["version"]["rule"] == D_RULE)
        .unwrap();
    let mutations: Vec<ContractMutation<'_>> = vec![
        Box::new(|contract: &mut Value| {
            contract["obligations"][obligation]["context_requirement"]
                .as_object_mut()
                .unwrap()
                .remove("target_support_capabilities");
        }),
        Box::new(|contract: &mut Value| {
            contract["obligations"][obligation]["context_requirement"]["target_support_capabilities"] =
                json!(["ast", "ast"]);
        }),
        Box::new(|contract: &mut Value| {
            contract["obligations"][obligation]["context_requirement"]["target_support_capabilities"] =
                json!(["direct_calls"]);
        }),
        Box::new(|contract: &mut Value| {
            contract["obligations"][obligation]["context_requirement"]["required_capabilities"] =
                json!(["ast"]);
        }),
        Box::new(|contract: &mut Value| {
            contract["obligations"][obligation]["context_requirement"]["enumeration_capabilities"] =
                Value::Null;
        }),
        Box::new(|contract: &mut Value| {
            contract["obligations"][obligation]["context_requirement"]["unknown"] = json!(true);
        }),
    ];
    for mutate in mutations {
        let mut mutated = contract.clone();
        mutate(&mut mutated);
        assert!(!validator.is_valid(&mutated));
    }
}

#[test]
fn coverage_contract_keeps_candidate_space_gaps_out_of_resolved_target_denominator() {
    let bundle = MvpRulePack::synthesize_changed_public_callee(&program(fixture())).unwrap();
    let contract = serde_json::to_value(bundle.contract()).unwrap();
    let coverage = &contract["universe"]["d_two_layer_coverage"];
    assert_eq!(coverage["rule"], D_RULE);
    assert_eq!(
        coverage["resolved_target_obligation_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        coverage["candidate_space_gap_obligation_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        contract["universe"]["obligation_ids"],
        coverage["resolved_target_obligation_ids"]
    );
    assert_eq!(coverage["call_graph_complete"], json!(false));
    assert_eq!(coverage["global_call_coverage_claim"], json!("prohibited"));
}

#[test]
fn resolved_target_planner_excludes_gap_but_retains_coverage_trace() {
    let (program, bundle) = d_bundle(fixture());
    let resolved_target_ids = bundle.universe().resolved_target_obligation_ids().clone();
    let gap_ids = bundle
        .universe()
        .candidate_space_gap_obligation_ids()
        .expect("D two-layer coverage")
        .clone();
    let planned =
        plan_resolved_target_obligations(&program, &bundle, PlanBudget::new(1, 1).unwrap())
            .expect("resolved target plan");
    let planned_ids = planned
        .waves()
        .iter()
        .flat_map(|wave| wave.obligation_ids().iter().cloned())
        .chain(planned.deferred().keys().cloned())
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(planned_ids, resolved_target_ids);
    assert!(planned_ids.is_disjoint(&gap_ids));
    assert!(gap_ids.iter().all(|id| {
        bundle
            .obligations()
            .iter()
            .any(|obligation| obligation.id() == id)
    }));
}

#[test]
fn resolved_target_planner_is_deterministic_under_program_vector_reordering() {
    let (first_program, first) = d_bundle(fixture());
    let mut reordered = fixture();
    reordered["artifacts"].as_array_mut().unwrap().reverse();
    reordered["relations"].as_array_mut().unwrap().reverse();
    reordered["contexts"].as_array_mut().unwrap().reverse();
    let (second_program, second) = d_bundle(reordered);
    let budget = PlanBudget::new(1, 1).unwrap();
    let first_plan = plan_resolved_target_obligations(&first_program, &first, budget).unwrap();
    let second_plan = plan_resolved_target_obligations(&second_program, &second, budget).unwrap();

    assert_eq!(first_plan.id(), second_plan.id());
    assert_eq!(
        first_plan.canonical_bytes().unwrap(),
        second_plan.canonical_bytes().unwrap()
    );
}

#[test]
fn resolved_target_planner_defers_overflow_with_its_id_and_weight() {
    let mut value = fixture();
    let duplicate = value["relations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|relation| relation["id"] == "relation:submit-calls-payment")
        .unwrap()
        .clone();
    let mut duplicate = duplicate;
    duplicate["id"] = json!("relation:submit-calls-payment-overflow");
    value["relations"].as_array_mut().unwrap().push(duplicate);
    let (program, bundle) = d_bundle(value);
    let resolved_target_ids = bundle.universe().resolved_target_obligation_ids().clone();
    let planned =
        plan_resolved_target_obligations(&program, &bundle, PlanBudget::new(1, 1).unwrap())
            .expect("resolved target plan");

    assert_eq!(resolved_target_ids.len(), 2);
    assert_eq!(planned.deferred().len(), 1);
    let deferred_id = planned.deferred().keys().next().unwrap();
    assert!(resolved_target_ids.contains(deferred_id));
    assert_eq!(
        bundle
            .obligations()
            .iter()
            .find(|obligation| obligation.id() == deferred_id)
            .expect("deferred resolved target")
            .weight(),
        4.0
    );
    assert!(planned.risk_breakdown().contains_key(deferred_id));
    assert_eq!(
        planned.risk_breakdown()[deferred_id].impact(),
        Severity::Critical,
        "the deferred D weight remains the fixed 4.0 priority input"
    );
}

#[test]
fn resolved_target_planner_does_not_change_legacy_plan_canonical_bytes() {
    let legacy_program = ProgramSpace::from_json_slice(include_bytes!(
        "../../../examples/double-submit-payment/program-space.json"
    ))
    .expect("legacy ProgramSpace");
    let legacy_bundle = MvpRulePack::synthesize(&legacy_program).expect("legacy synthesis");
    let (legacy_universe, legacy_obligations) = legacy_bundle.into_parts();
    let legacy = ReviewAggregate::new(legacy_program, legacy_universe, legacy_obligations)
        .expect("legacy aggregate");
    let budget = PlanBudget::new(16, 2).unwrap();
    let before = plan(&legacy, budget).unwrap().canonical_bytes().unwrap();

    let (d_program, d_bundle) = d_bundle(fixture());
    let _ = plan_resolved_target_obligations(&d_program, &d_bundle, budget).unwrap();

    assert_eq!(
        plan(&legacy, budget).unwrap().canonical_bytes().unwrap(),
        before
    );
}

#[test]
fn read_only_d_aggregate_retains_gap_trace_and_resolved_target_distinction() {
    let (program, bundle) = d_bundle(fixture());
    let resolved_target_ids = bundle.universe().resolved_target_obligation_ids().clone();
    let gap_ids = bundle
        .universe()
        .candidate_space_gap_obligation_ids()
        .expect("D two-layer coverage")
        .clone();
    let aggregate = ReviewAggregate::read_only_from_d_two_layer_bundle(program, &bundle)
        .expect("read-only D aggregate");
    let aggregate_ids = aggregate
        .obligations()
        .map(|obligation| obligation.id().clone())
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(
        aggregate.universe().resolved_target_obligation_ids(),
        &resolved_target_ids
    );
    assert_eq!(
        aggregate.universe().candidate_space_gap_obligation_ids(),
        Some(&gap_ids)
    );
    assert!(resolved_target_ids.is_disjoint(&gap_ids));
    assert_eq!(
        aggregate_ids,
        resolved_target_ids.union(&gap_ids).cloned().collect()
    );
}

#[test]
fn read_only_d_aggregate_is_deterministic_without_changing_legacy_new_bytes() {
    let (first_program, first_bundle) = d_bundle(fixture());
    let mut reordered = fixture();
    reordered["artifacts"].as_array_mut().unwrap().reverse();
    reordered["relations"].as_array_mut().unwrap().reverse();
    reordered["contexts"].as_array_mut().unwrap().reverse();
    let (second_program, second_bundle) = d_bundle(reordered);
    let first =
        ReviewAggregate::read_only_from_d_two_layer_bundle(first_program, &first_bundle).unwrap();
    let second =
        ReviewAggregate::read_only_from_d_two_layer_bundle(second_program, &second_bundle).unwrap();
    assert_eq!(
        CanonicalJson::from_serializable(&first).unwrap(),
        CanonicalJson::from_serializable(&second).unwrap()
    );

    let legacy_program = ProgramSpace::from_json_slice(include_bytes!(
        "../../../examples/double-submit-payment/program-space.json"
    ))
    .unwrap();
    let legacy_bundle = MvpRulePack::synthesize(&legacy_program).unwrap();
    let (legacy_universe, legacy_obligations) = legacy_bundle.into_parts();
    let legacy = ReviewAggregate::new(legacy_program, legacy_universe, legacy_obligations).unwrap();
    let before = CanonicalJson::from_serializable(&legacy).unwrap();
    let (d_program, d_bundle) = d_bundle(fixture());
    let _ = ReviewAggregate::read_only_from_d_two_layer_bundle(d_program, &d_bundle).unwrap();
    assert_eq!(CanonicalJson::from_serializable(&legacy).unwrap(), before);
}

#[test]
fn gap_removed_d_bundle_parts_are_rejected_by_the_legacy_aggregate_boundary() {
    let (program, bundle) = d_bundle(fixture());
    let gap_ids = bundle
        .universe()
        .candidate_space_gap_obligation_ids()
        .expect("D two-layer coverage")
        .clone();
    let (universe, mut obligations) = bundle.into_parts();
    obligations.retain(|obligation| !gap_ids.contains(obligation.id()));

    assert!(matches!(
        ReviewAggregate::new(program, universe, obligations),
        Err(DomainError::Validation(message))
            if message == "D two-layer denominator must retain disjoint resolved and candidate-space sets"
    ));
}

#[test]
fn read_only_d_source_registration_closes_sources_and_drives_c3_projection() {
    let (aggregate, obligation_id, caller_id, callee_id, source_bytes) = source_ready_d_aggregate();
    assert!(
        aggregate
            .universe()
            .candidate_space_gap_obligation_ids()
            .is_some_and(|gap_ids| !gap_ids.is_empty())
    );

    let mut session = prepare_subject_windows_v2(&aggregate, obligation_id, caller_id, callee_id)
        .expect("C3 subject-window session");
    let mut submitted = 0_usize;
    while let Some(request) = session.next_source_request().unwrap() {
        session.submit_source(&request, &source_bytes).unwrap();
        submitted += 1;
    }
    let built = session.finish().expect("C3 subject-window projection");
    assert!(submitted > 0);
    assert!(!built.candidate_source_ids().is_empty());
}

#[test]
fn read_only_d_source_registration_is_deterministic_and_cannot_open_an_event_log() {
    let (first, obligation_id, caller_id, callee_id, first_bytes) = source_ready_d_aggregate();
    let (second, _, _, _, second_bytes) = source_ready_d_aggregate();
    let mut first_session = prepare_subject_windows_v2(
        &first,
        obligation_id.clone(),
        caller_id.clone(),
        callee_id.clone(),
    )
    .unwrap();
    let mut second_session =
        prepare_subject_windows_v2(&second, obligation_id, caller_id, callee_id).unwrap();
    while let Some(request) = first_session.next_source_request().unwrap() {
        first_session.submit_source(&request, &first_bytes).unwrap();
    }
    while let Some(request) = second_session.next_source_request().unwrap() {
        second_session
            .submit_source(&request, &second_bytes)
            .unwrap();
    }
    let first_projection = first_session.finish().unwrap();
    let second_projection = second_session.finish().unwrap();
    assert_eq!(first_projection.id(), second_projection.id());
    assert_eq!(
        first_projection.projection_hash(),
        second_projection.projection_hash()
    );

    assert!(matches!(
        EventLog::new(StableId::parse("run:authority-attempt").unwrap(), first),
        Err(DomainError::Validation(message))
            if message == "universe denominator IDs do not match obligation records"
    ));
}

#[test]
fn read_only_d_source_registration_does_not_change_legacy_event_log_genesis_bytes() {
    let legacy_program = ProgramSpace::from_json_slice(include_bytes!(
        "../../../examples/double-submit-payment/program-space.json"
    ))
    .unwrap();
    let legacy_bundle = MvpRulePack::synthesize(&legacy_program).unwrap();
    let (legacy_universe, legacy_obligations) = legacy_bundle.into_parts();
    let legacy = ReviewAggregate::new(legacy_program, legacy_universe, legacy_obligations).unwrap();
    let before = EventLog::new(StableId::parse("run:legacy-event-bytes").unwrap(), legacy)
        .unwrap()
        .run_genesis_snapshot()
        .unwrap()
        .canonical_bytes()
        .unwrap();

    let _ = source_ready_d_aggregate();

    let legacy_program = ProgramSpace::from_json_slice(include_bytes!(
        "../../../examples/double-submit-payment/program-space.json"
    ))
    .unwrap();
    let legacy_bundle = MvpRulePack::synthesize(&legacy_program).unwrap();
    let (legacy_universe, legacy_obligations) = legacy_bundle.into_parts();
    let legacy = ReviewAggregate::new(legacy_program, legacy_universe, legacy_obligations).unwrap();
    let after = EventLog::new(StableId::parse("run:legacy-event-bytes").unwrap(), legacy)
        .unwrap()
        .run_genesis_snapshot()
        .unwrap()
        .canonical_bytes()
        .unwrap();
    assert_eq!(before, after);
}

#[test]
fn equivalent_program_vector_orders_have_identical_canonical_contracts() {
    let first = MvpRulePack::synthesize_changed_public_callee(&program(fixture())).unwrap();
    let mut reordered = fixture();
    reordered["artifacts"].as_array_mut().unwrap().reverse();
    reordered["relations"].as_array_mut().unwrap().reverse();
    reordered["contexts"].as_array_mut().unwrap().reverse();
    let second = MvpRulePack::synthesize_changed_public_callee(&program(reordered)).unwrap();
    assert_eq!(first.universe().id(), second.universe().id());
    assert_eq!(first.contract().canonical(), second.contract().canonical());
    assert_eq!(
        CanonicalJson::from_serializable(&first).unwrap(),
        CanonicalJson::from_serializable(&second).unwrap()
    );
}
