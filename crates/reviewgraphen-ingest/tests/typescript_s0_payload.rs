//! S0 public payload vocabulary checks; descriptor hashes are derived in
//! derive_s0.py. Ingest owns the public descriptor and concrete payload enums.

use reviewgraphen_ingest::typescript::payload::{
    CallableOutcomeV1, PayloadDescriptorId, PayloadImportKindV1, ScopeKindV1, SyntaxRole,
    VisibilityValueV1,
};

#[test]
#[ignore = "S0"]
fn s0_exactly_five_payload_descriptor_ids() {
    for role in ["binding", "call", "callable", "scope", "surface"] {
        let wire = format!("reviewgraphen.typescript_syntax.{role}@1");
        assert_eq!(
            PayloadDescriptorId::parse_wire(&wire)
                .unwrap()
                .wire_literal(),
            wire
        );
    }
    for wire in [
        "reviewgraphen.typescript_syntax.jsx@1",
        "reviewgraphen.typescript_syntax.binding@2",
        "",
        "freeform",
    ] {
        assert!(PayloadDescriptorId::parse_wire(wire).is_err());
    }
}

#[test]
#[ignore = "S0"]
fn s0_concrete_payload_enum_variants_are_closed() {
    fn role_name(role: SyntaxRole) -> &'static str {
        match role {
            SyntaxRole::Callable => "callable",
            SyntaxRole::Call => "call",
            SyntaxRole::Binding => "binding",
            SyntaxRole::Surface => "surface",
            SyntaxRole::Scope => "scope",
        }
    }
    assert_eq!(
        [
            SyntaxRole::Callable,
            SyntaxRole::Call,
            SyntaxRole::Binding,
            SyntaxRole::Surface,
            SyntaxRole::Scope
        ]
        .map(role_name),
        ["callable", "call", "binding", "surface", "scope"]
    );
    fn visibility_name(value: VisibilityValueV1) -> &'static str {
        match value {
            VisibilityValueV1::Exported => "exported",
            VisibilityValueV1::NonExported => "non_exported",
            VisibilityValueV1::Unknown => "unknown",
        }
    }
    assert_eq!(
        [
            VisibilityValueV1::Exported,
            VisibilityValueV1::NonExported,
            VisibilityValueV1::Unknown
        ]
        .map(visibility_name),
        ["exported", "non_exported", "unknown"]
    );
    fn import_name(value: PayloadImportKindV1) -> &'static str {
        match value {
            PayloadImportKindV1::Named => "named",
            PayloadImportKindV1::Default => "default",
            PayloadImportKindV1::TypeOnly => "type_only",
            PayloadImportKindV1::Namespace => "namespace",
        }
    }
    assert_eq!(
        [
            PayloadImportKindV1::Named,
            PayloadImportKindV1::Default,
            PayloadImportKindV1::TypeOnly,
            PayloadImportKindV1::Namespace
        ]
        .map(import_name),
        ["named", "default", "type_only", "namespace"]
    );
    fn eligibility_name(value: CallableOutcomeV1) -> &'static str {
        match value {
            CallableOutcomeV1::EligiblePublic => "eligible_public",
            CallableOutcomeV1::NonPublic => "non_public",
            CallableOutcomeV1::NotRuntimeCallable => "not_runtime_callable",
            CallableOutcomeV1::OutOfScope => "out_of_scope",
            CallableOutcomeV1::Unsupported => "unsupported",
        }
    }
    assert_eq!(
        [
            CallableOutcomeV1::EligiblePublic,
            CallableOutcomeV1::NonPublic,
            CallableOutcomeV1::NotRuntimeCallable,
            CallableOutcomeV1::OutOfScope,
            CallableOutcomeV1::Unsupported
        ]
        .map(eligibility_name),
        [
            "eligible_public",
            "non_public",
            "not_runtime_callable",
            "out_of_scope",
            "unsupported"
        ]
    );
    assert!(matches!(ScopeKindV1::FileLexical, ScopeKindV1::FileLexical));
}
