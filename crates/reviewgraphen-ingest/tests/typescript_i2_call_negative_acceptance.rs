//! Frozen negative I2 call rows for export aliases and named default exports.
//!
//! The fixture bytes and stage/reason literals below are contract oracles. They
//! deliberately do not reuse resolver output to derive an expectation.

use reviewgraphen_core::source_review::{
    admitted_source::{AdmittedSourceBundleV1, AdmittedSourceFileV1},
    basis::{
        SourceFileOutcome, SourceReviewBasisV1, SourceReviewFileV1, SourceReviewSyntaxV1,
        SourceSyntaxRole,
    },
    ids::{CanonicalFileKey, SnapshotBinding, SourceFileId, SourceHash},
    reasons::{CallReason, ResolutionOutcomeV1},
    registry::typescript_registry_binding,
};
use reviewgraphen_ingest::typescript::payload::{
    SyntaxRole, TypeScriptOutcomeV1, TypeScriptPayload, TypeScriptPayloadData,
    TypeScriptPayloadSourceViewV1, TypeScriptPrimaryReasonV1, TypeScriptReasonsV1,
    rebuild_payload_from_source,
};

const SNAPSHOT: &str = "i2-call-negative-acceptance@1";
const EXPECTED_STAGES: [&str; 9] = [
    "c.syntax",
    "c.caller",
    "c.import_form",
    "c.local_binding",
    "c.specifier",
    "c.candidates",
    "c.export_binding",
    "c.shadow",
    "c.writes",
];

struct CompleteTargetView {
    sources: AdmittedSourceBundleV1,
}

impl TypeScriptPayloadSourceViewV1 for CompleteTargetView {
    fn source_bundle(&self) -> &AdmittedSourceBundleV1 {
        &self.sources
    }

    fn target_outcome_for_path(&self, canonical_path: &str) -> Option<SourceFileOutcome> {
        self.sources.target_outcome_for_path(canonical_path)
    }

    fn target_inventory_complete(&self) -> bool {
        true
    }
}

fn file_id(path: &str) -> SourceFileId {
    SourceFileId::from_basis_file_key(
        CanonicalFileKey::from_basis_path(path).expect("literal fixture path"),
    )
}

fn complete_view(caller: &str, callee: &str) -> CompleteTargetView {
    let basis = SourceReviewBasisV1::new(
        typescript_registry_binding(),
        vec![
            SourceReviewFileV1 {
                path: "src/caller.ts".to_owned(),
                language: "typescript".to_owned(),
                outcome: SourceFileOutcome::Parsed,
            },
            SourceReviewFileV1 {
                path: "src/callee.ts".to_owned(),
                language: "typescript".to_owned(),
                outcome: SourceFileOutcome::Parsed,
            },
        ],
        vec![
            SourceReviewSyntaxV1 {
                file_path: "src/caller.ts".to_owned(),
                role: SourceSyntaxRole::Callable,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/caller.ts".to_owned(),
                role: SourceSyntaxRole::Call,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/caller.ts".to_owned(),
                role: SourceSyntaxRole::Binding,
            },
        ],
    )
    .expect("two parsed literal fixture files");
    let sources = AdmittedSourceBundleV1::new(
        &basis,
        vec![
            AdmittedSourceFileV1 {
                file_id: file_id("src/caller.ts"),
                bytes: caller.as_bytes().to_vec(),
                source_hash: SourceHash::from_source_bytes(caller.as_bytes()),
            },
            AdmittedSourceFileV1 {
                file_id: file_id("src/callee.ts"),
                bytes: callee.as_bytes().to_vec(),
                source_hash: SourceHash::from_source_bytes(callee.as_bytes()),
            },
        ],
    )
    .expect("basis-bound literal fixture bytes");
    CompleteTargetView { sources }
}

fn call_payload(view: &CompleteTargetView) -> TypeScriptPayload {
    let payloads = rebuild_payload_from_source(
        view,
        &typescript_registry_binding(),
        &SnapshotBinding::from_admitted_binding(SNAPSHOT),
        file_id("src/caller.ts"),
    )
    .expect("literal sources rebuild");
    let calls = payloads
        .into_iter()
        .filter(|payload| payload.role == SyntaxRole::Call)
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 1, "one literal imported call");
    calls.into_iter().next().expect("one call payload")
}

fn assert_written_binding_unresolved(call: TypeScriptPayload) {
    assert_eq!(
        call.outcome,
        TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Unresolved),
        "frozen Call outcome",
    );
    assert_eq!(
        call.reasons,
        TypeScriptReasonsV1::Call(reviewgraphen_core::source_review::reasons::ReasonSet::new(
            [CallReason::WrittenBinding]
        ),),
        "frozen written_binding reason",
    );
    assert_eq!(
        call.primary_reason,
        Some(TypeScriptPrimaryReasonV1::Call(CallReason::WrittenBinding)),
        "frozen written_binding primary reason",
    );
    let TypeScriptPayloadData::Call(data) = call.data else {
        panic!("Call payload retains CallDataV1");
    };
    assert_eq!(
        data.evaluated_stages
            .iter()
            .map(|stage| stage.wire_literal())
            .collect::<Vec<_>>(),
        EXPECTED_STAGES,
        "frozen stages stop at c.writes without c.resolution",
    );
    assert_eq!(
        data.callee_id, None,
        "unresolved call has no callee endpoint"
    );
    assert_eq!(
        data.resolution_kind, None,
        "unresolved call has no resolution endpoint"
    );
}

#[test]
fn named_export_alias_write_blocks_the_imported_call() {
    let caller =
        "import { exported } from './callee';\nexport function run(){return exported();}\n";
    let callee = "const local = () => 1;\nexport { local as exported };\nlocal = () => 2;\n";
    assert_written_binding_unresolved(call_payload(&complete_view(caller, callee)));
}

#[test]
fn named_default_export_write_blocks_the_default_imported_call() {
    let caller = "import exported from './callee';\nexport function run(){return exported();}\n";
    let callee = "export default function local(){return 1;}\nlocal = () => 2;\n";
    assert_written_binding_unresolved(call_payload(&complete_view(caller, callee)));
}
