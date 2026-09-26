//! Core exposes the public registry tuple, call reason and resolution APIs.
//! Only literals determined by the frozen design and supplied registry data appear.

use std::collections::BTreeSet;

use reviewgraphen_core::source_review::reasons::{CallReason, ReasonSet, ResolutionKind};
use reviewgraphen_core::source_review::registry::{
    TYPESCRIPT_ARM_ID, TYPESCRIPT_EXTRACTOR_SET_HASH, TYPESCRIPT_REGISTRY_ID,
    typescript_registry_binding, validate_typescript_binding, validate_typescript_tuple,
};
use reviewgraphen_core::typescript::rules::{CoverageLayer, PropertyId, RuleId, TargetKind};

const CALL_ORDER: [(&str, CallReason); 16] = [
    ("parse_failure", CallReason::ParseFailure),
    ("unsupported_syntax", CallReason::UnsupportedSyntax),
    ("unsupported_caller", CallReason::UnsupportedCaller),
    ("dynamic_dispatch", CallReason::DynamicDispatch),
    (
        "relative_specifier_unsupported",
        CallReason::RelativeSpecifierUnsupported,
    ),
    ("relative_target_unread", CallReason::RelativeTargetUnread),
    (
        "relative_target_ambiguous",
        CallReason::RelativeTargetAmbiguous,
    ),
    (
        "relative_target_excluded",
        CallReason::RelativeTargetExcluded,
    ),
    ("relative_target_missing", CallReason::RelativeTargetMissing),
    (
        "import_binding_ambiguous",
        CallReason::ImportBindingAmbiguous,
    ),
    ("type_only_binding", CallReason::TypeOnlyBinding),
    (
        "export_binding_unsupported",
        CallReason::ExportBindingUnsupported,
    ),
    (
        "import_resolution_unavailable",
        CallReason::ImportResolutionUnavailable,
    ),
    ("shadowed_binding", CallReason::ShadowedBinding),
    ("written_binding", CallReason::WrittenBinding),
    ("unresolved_name", CallReason::UnresolvedName),
];

const D_ONLY_CALL_REASONS: [(&str, CallReason); 3] = [
    ("unchanged_callee", CallReason::UnchangedCallee),
    ("non_public_callee", CallReason::NonPublicCallee),
    ("excluded_endpoint", CallReason::ExcludedEndpoint),
];

fn generated_foreign_wires(registered: &[String], other_vocabulary: &[String]) -> Vec<String> {
    let mut foreign = Vec::with_capacity(registered.len() * 4 + other_vocabulary.len() + 1);
    for wire in registered {
        foreign.push(format!("{wire}x"));
        foreign.push(wire.to_uppercase());
        foreign.push(format!(" {wire}"));
        foreign.push(format!("{wire} "));
    }
    foreign.extend(other_vocabulary.iter().cloned());
    foreign.push(String::new());
    foreign
}

fn assert_generated_foreign_wires_rejected(
    registered: &[String],
    other_vocabulary: &[String],
    rejects: impl Fn(&str) -> bool,
) {
    assert_eq!(
        registered.len(),
        registered.iter().collect::<BTreeSet<_>>().len(),
        "registered wires must be one-to-one with their variants"
    );
    let foreign = generated_foreign_wires(registered, other_vocabulary);
    assert_eq!(
        foreign.len(),
        registered.len() * 4 + other_vocabulary.len() + 1,
        "foreign sample count must grow with the registered vocabulary"
    );
    for wire in foreign {
        assert!(rejects(&wire), "foreign wire must be rejected: {wire:?}");
    }
}

fn all_call_reasons_are_classified(reason: CallReason) -> bool {
    match reason {
        CallReason::ParseFailure
        | CallReason::UnsupportedSyntax
        | CallReason::UnsupportedCaller
        | CallReason::DynamicDispatch
        | CallReason::RelativeSpecifierUnsupported
        | CallReason::RelativeTargetUnread
        | CallReason::RelativeTargetAmbiguous
        | CallReason::RelativeTargetExcluded
        | CallReason::RelativeTargetMissing
        | CallReason::ImportBindingAmbiguous
        | CallReason::TypeOnlyBinding
        | CallReason::ExportBindingUnsupported
        | CallReason::ImportResolutionUnavailable
        | CallReason::ShadowedBinding
        | CallReason::WrittenBinding
        | CallReason::UnresolvedName => true,
        CallReason::UnchangedCallee
        | CallReason::NonPublicCallee
        | CallReason::ExcludedEndpoint => false,
    }
}

fn resolution_kinds() -> [ResolutionKind; 2] {
    [
        ResolutionKind::SyntacticUnique,
        ResolutionKind::SyntacticUniqueRelativeImportV1,
    ]
}

fn all_resolution_kinds_are_classified(value: ResolutionKind) {
    match value {
        ResolutionKind::SyntacticUnique | ResolutionKind::SyntacticUniqueRelativeImportV1 => {}
    }
}

fn coverage_layers() -> [CoverageLayer; 3] {
    [
        CoverageLayer::SingleLayer,
        CoverageLayer::TwoLayer,
        CoverageLayer::SnapshotGap,
    ]
}

fn all_coverage_layers_are_classified(value: CoverageLayer) {
    match value {
        CoverageLayer::SingleLayer | CoverageLayer::TwoLayer | CoverageLayer::SnapshotGap => {}
    }
}

fn property_ids() -> [PropertyId; 3] {
    [
        PropertyId::CalleeContractReview,
        PropertyId::PublicFunctionContractReview,
        PropertyId::CapabilityGap,
    ]
}

fn all_property_ids_are_classified(value: PropertyId) {
    match value {
        PropertyId::CalleeContractReview
        | PropertyId::PublicFunctionContractReview
        | PropertyId::CapabilityGap => {}
    }
}

fn rule_ids() -> [RuleId; 3] {
    [
        RuleId::ChangedPublicCallee,
        RuleId::PublicFunctionContract,
        RuleId::CapabilityGapOrigin,
    ]
}

fn all_rule_ids_are_classified(value: RuleId) {
    match value {
        RuleId::ChangedPublicCallee
        | RuleId::PublicFunctionContract
        | RuleId::CapabilityGapOrigin => {}
    }
}

fn target_kinds() -> [TargetKind; 3] {
    [TargetKind::Node, TargetKind::Relation, TargetKind::Subgraph]
}

fn all_target_kinds_are_classified(value: TargetKind) {
    match value {
        TargetKind::Node | TargetKind::Relation | TargetKind::Subgraph => {}
    }
}

fn coverage_layer_wires() -> Vec<String> {
    coverage_layers()
        .into_iter()
        .map(|value| value.wire_literal().to_owned())
        .collect()
}

fn property_id_wires() -> Vec<String> {
    property_ids()
        .into_iter()
        .map(|value| value.wire_literal().to_owned())
        .collect()
}

fn rule_id_wires() -> Vec<String> {
    rule_ids()
        .into_iter()
        .map(|value| value.wire_literal().to_owned())
        .collect()
}

fn target_kind_wires() -> Vec<String> {
    target_kinds()
        .into_iter()
        .map(|value| value.wire_literal().to_owned())
        .collect()
}

macro_rules! assert_rejected_wire_is_preserved {
    ($type:ty, [$($rejected_wire:expr),+ $(,)?]) => {
        $(
            let rejected_wire = $rejected_wire;
            let error = <$type>::parse_wire(rejected_wire)
                .expect_err("independently foreign wire must be rejected");
            assert_eq!(
                error.rejected_wire.as_bytes(),
                rejected_wire.as_bytes(),
                "rejected wire payload must preserve the original bytes"
            );
        )+
    };
}

fn resolution_kind_wires() -> Vec<String> {
    resolution_kinds()
        .into_iter()
        .map(|value| value.wire_literal().to_owned())
        .collect()
}

#[test]
fn s0_registry_identity_and_seven_field_tuple() {
    assert_eq!(
        TYPESCRIPT_REGISTRY_ID,
        "reviewgraphen.source_review_registry.r2"
    );
    assert_eq!(TYPESCRIPT_ARM_ID, "typescript.production.v1.source@1");
    let binding = typescript_registry_binding();
    assert_eq!(binding.registry_id, TYPESCRIPT_REGISTRY_ID);
    assert_eq!(binding.arm_id, TYPESCRIPT_ARM_ID);
    let tuple = &binding.tuple;
    assert_eq!(tuple.profile_id, "typescript.production.v1");
    assert_eq!(tuple.profile_version, "1");
    assert_eq!(tuple.language, "typescript");
    assert_eq!(
        tuple.producer_id,
        "reviewgraphen.ingest.typescript_tree_sitter@1"
    );
    assert_eq!(tuple.projection_id, "typescript.obligation_report@1");
    assert_eq!(tuple.extractor_set_hash, TYPESCRIPT_EXTRACTOR_SET_HASH);
    assert!(validate_typescript_tuple(tuple).is_ok());
    assert!(validate_typescript_binding(&binding).is_ok());
    let mut foreign = binding.clone();
    foreign.tuple.projection_id = "typescript.other@1".into();
    assert!(validate_typescript_binding(&foreign).is_err());
    let mut wrong_hash = binding;
    wrong_hash.registry_hash = "sha256:foreign".into();
    assert!(validate_typescript_binding(&wrong_hash).is_err());
}

#[test]
fn s0_extractor_hash_of_supplied_definition() {
    // Independent H(extractor_definition), not a copied hash fixture literal.
    assert_eq!(
        TYPESCRIPT_EXTRACTOR_SET_HASH,
        "sha256:f5722ef19c0a2a4a5b6ff583f95aa8fdc9d0cf4f3cbc0c5631c041770842b39f"
    );
}

#[test]
#[allow(clippy::needless_range_loop)]
fn s0_call_reason_wire_and_full_primary_order() {
    for &(wire, reason) in &CALL_ORDER {
        assert_eq!(reason.wire_literal(), wire);
        assert_eq!(CallReason::parse_wire(wire).unwrap(), reason);
    }
    for earlier in 0..CALL_ORDER.len() {
        for later in earlier + 1..CALL_ORDER.len() {
            let a = CALL_ORDER[earlier].1;
            let b = CALL_ORDER[later].1;
            assert!(a.precedence() < b.precedence());
            let set = ReasonSet::new([b, a]);
            assert_eq!(set.primary(), Some(a));
            assert_eq!(set.all().len(), 2);
        }
    }
    assert_eq!(ReasonSet::new([]).primary(), None);
    for foreign in ["not_a_reason", "unchanged_callee", "", "parse_failure@2"] {
        assert!(CallReason::parse_wire(foreign).is_err());
    }
}

#[test]
fn s0_only_two_resolution_kind_wires() {
    let values = [
        (ResolutionKind::SyntacticUnique, "syntactic_unique"),
        (
            ResolutionKind::SyntacticUniqueRelativeImportV1,
            "syntactic_unique_relative_import@1",
        ),
    ];
    for (kind, wire) in values {
        assert_eq!(kind.wire_literal(), wire);
        assert_eq!(ResolutionKind::parse_wire(wire).unwrap(), kind);
    }
    for wire in ["compiler_resolved", "relative", "", "syntactic_unique@2"] {
        assert!(ResolutionKind::parse_wire(wire).is_err());
    }
}

#[test]
fn s0_call_reason_closed_call_and_d_domains() {
    assert_eq!(CALL_ORDER.len(), 16);
    assert_eq!(D_ONLY_CALL_REASONS.len(), 3);
    for &(wire, reason) in &CALL_ORDER {
        assert!(all_call_reasons_are_classified(reason));
        assert_eq!(reason.wire_literal(), wire);
        assert_eq!(CallReason::parse_wire(wire).unwrap(), reason);
    }
    for &(wire, reason) in &D_ONLY_CALL_REASONS {
        assert!(!all_call_reasons_are_classified(reason));
        assert_eq!(reason.wire_literal(), wire);
        assert!(CallReason::parse_wire(wire).is_err());
    }
    let registered = CALL_ORDER
        .iter()
        .map(|(wire, _)| (*wire).to_owned())
        .collect::<Vec<_>>();
    assert_generated_foreign_wires_rejected(&registered, &coverage_layer_wires(), |wire| {
        CallReason::parse_wire(wire).is_err()
    });
}

#[test]
fn s0_resolution_kind_closed_vocabulary() {
    assert_eq!(resolution_kinds().len(), 2);
    for value in resolution_kinds() {
        all_resolution_kinds_are_classified(value);
        let wire = value.wire_literal().to_owned();
        let parsed = ResolutionKind::parse_wire(&wire).unwrap();
        assert_eq!(parsed.wire_literal(), wire.as_str());
    }
    let registered = resolution_kind_wires();
    assert_generated_foreign_wires_rejected(&registered, &target_kind_wires(), |wire| {
        ResolutionKind::parse_wire(wire).is_err()
    });
}

#[test]
fn s0_coverage_layer_closed_vocabulary() {
    assert_eq!(coverage_layers().len(), 3);
    for value in coverage_layers() {
        all_coverage_layers_are_classified(value);
        let wire = value.wire_literal().to_owned();
        let parsed = CoverageLayer::parse_wire(&wire).unwrap();
        assert_eq!(parsed.wire_literal(), wire.as_str());
    }
    let registered = coverage_layer_wires();
    assert_generated_foreign_wires_rejected(&registered, &property_id_wires(), |wire| {
        CoverageLayer::parse_wire(wire).is_err()
    });
}

#[test]
fn s0_property_id_closed_vocabulary() {
    assert_eq!(property_ids().len(), 3);
    for value in property_ids() {
        all_property_ids_are_classified(value);
        let wire = value.wire_literal().to_owned();
        let parsed = PropertyId::parse_wire(&wire).unwrap();
        assert_eq!(parsed.wire_literal(), wire.as_str());
    }
    let registered = property_id_wires();
    assert_generated_foreign_wires_rejected(&registered, &rule_id_wires(), |wire| {
        PropertyId::parse_wire(wire).is_err()
    });
}

#[test]
fn s0_rule_id_closed_vocabulary() {
    assert_eq!(rule_ids().len(), 3);
    for value in rule_ids() {
        all_rule_ids_are_classified(value);
        let wire = value.wire_literal().to_owned();
        let parsed = RuleId::parse_wire(&wire).unwrap();
        assert_eq!(parsed.wire_literal(), wire.as_str());
    }
    let registered = rule_id_wires();
    assert_generated_foreign_wires_rejected(&registered, &target_kind_wires(), |wire| {
        RuleId::parse_wire(wire).is_err()
    });
}

#[test]
fn s0_target_kind_closed_vocabulary() {
    assert_eq!(target_kinds().len(), 3);
    for value in target_kinds() {
        all_target_kinds_are_classified(value);
        let wire = value.wire_literal().to_owned();
        let parsed = TargetKind::parse_wire(&wire).unwrap();
        assert_eq!(parsed.wire_literal(), wire.as_str());
    }
    let registered = target_kind_wires();
    assert_generated_foreign_wires_rejected(&registered, &resolution_kind_wires(), |wire| {
        TargetKind::parse_wire(wire).is_err()
    });
}

#[test]
fn s0_typescript_rule_wires_match_independent_registry_literals() {
    for (variant, literal) in [
        (
            RuleId::ChangedPublicCallee,
            "relation.changed_public_callee@2",
        ),
        (
            RuleId::PublicFunctionContract,
            "node.public_function_contract@2",
        ),
        (RuleId::CapabilityGapOrigin, "capability_gap.origin_rule@1"),
    ] {
        assert_eq!(variant.wire_literal(), literal);
        assert_eq!(RuleId::parse_wire(literal).unwrap(), variant);
    }

    for (variant, literal) in [
        (
            PropertyId::CalleeContractReview,
            "typescript.callee_contract_review@1",
        ),
        (
            PropertyId::PublicFunctionContractReview,
            "typescript.public_function_contract_review@1",
        ),
        (PropertyId::CapabilityGap, "reviewgraphen.capability_gap"),
    ] {
        assert_eq!(variant.wire_literal(), literal);
        assert_eq!(PropertyId::parse_wire(literal).unwrap(), variant);
    }

    for (variant, literal) in [
        (TargetKind::Relation, "relation"),
        (TargetKind::Node, "node"),
        (TargetKind::Subgraph, "subgraph"),
    ] {
        assert_eq!(variant.wire_literal(), literal);
        assert_eq!(TargetKind::parse_wire(literal).unwrap(), variant);
    }

    for (variant, literal) in [
        (CoverageLayer::TwoLayer, "two_layer"),
        (CoverageLayer::SingleLayer, "single_layer"),
        (CoverageLayer::SnapshotGap, "snapshot_gap"),
    ] {
        assert_eq!(variant.wire_literal(), literal);
        assert_eq!(CoverageLayer::parse_wire(literal).unwrap(), variant);
    }
}

#[test]
fn s0_typescript_rule_wire_rejections_preserve_raw_input_bytes() {
    assert_rejected_wire_is_preserved!(
        RuleId,
        [
            " relation.changed_public_callee@2",
            "relation.changed_public_callee@2 ",
            "RELATION.CHANGED_PUBLIC_CALLEE@2",
            "relation.changed_public_callee@2x",
            "typescript.callee_contract_review@1",
            "",
        ]
    );
    assert_rejected_wire_is_preserved!(
        PropertyId,
        [
            " typescript.callee_contract_review@1",
            "typescript.callee_contract_review@1 ",
            "TYPESCRIPT.CALLEE_CONTRACT_REVIEW@1",
            "typescript.callee_contract_review@1x",
            "relation.changed_public_callee@2",
            "",
        ]
    );
    assert_rejected_wire_is_preserved!(
        TargetKind,
        [
            " relation",
            "relation ",
            "RELATION",
            "relationx",
            "two_layer",
            "",
        ]
    );
    assert_rejected_wire_is_preserved!(
        CoverageLayer,
        [
            " two_layer",
            "two_layer ",
            "TWO_LAYER",
            "two_layerx",
            "relation",
            "",
        ]
    );
}
