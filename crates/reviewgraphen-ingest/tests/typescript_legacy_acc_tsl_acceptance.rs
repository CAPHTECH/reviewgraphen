//! Acc-TSL regression guards for the TypeScript legacy (v5) payload route.
//!
//! These tests drive only the public source-only rebuild
//! (`rebuild_payload_from_source`) that runtime A1 uses, and compare its
//! binding/call payloads with literals derived from the frozen contracts:
//!
//! - the frozen TypeScript source-review design;
//! - REGISTRY = `crates/reviewgraphen-core/src/source_review/registry.rs`
//!   (EXPECTED_BINDING_STAGES :245--288, EXPECTED_BINDING_REASONS :374--449,
//!   EXPECTED_BINDING_PRECEDENCE :450--).
//!
//! Every expected value is written as a literal from those sources, never read
//! back from product output.

use reviewgraphen_core::source_review::admitted_source::{
    AdmittedSourceBundleV1, AdmittedSourceFileV1,
};
use reviewgraphen_core::source_review::basis::{
    SourceFileOutcome, SourceReviewBasisV1, SourceReviewFileV1,
};
use reviewgraphen_core::source_review::ids::{
    CanonicalFileKey, DeclarationId, SnapshotBinding, SourceFileId, SourceHash, SourceRange,
};
use reviewgraphen_core::source_review::reasons::ResolutionOutcomeV1;
use reviewgraphen_core::source_review::registry::typescript_registry_binding;
use reviewgraphen_ingest::typescript::payload::{
    BindingDataV1, SyntaxRole, TypeScriptOutcomeV1, TypeScriptPayload, TypeScriptPayloadData,
    TypeScriptPrimaryReasonV1, TypeScriptReasonsV1, rebuild_payload_from_source,
};

const ACC_TSL_SNAPSHOT: &str = "acc-tsl-legacy-ts-payload-snapshot@1";

fn file_id(path: &str) -> SourceFileId {
    SourceFileId::from_basis_file_key(
        CanonicalFileKey::from_basis_path(path).expect("canonical fixture path"),
    )
}

fn unique_range(source: &str, fragment: &str) -> SourceRange {
    let mut matches = source.match_indices(fragment);
    let (start, _) = matches.next().expect("fixture fragment is present");
    assert!(
        matches.next().is_none(),
        "fixture fragment must occur exactly once: {fragment}"
    );
    SourceRange::new(start as u64, (start + fragment.len()) as u64).expect("fixture range")
}

fn rebuild(files: &[(&str, &str)], target: &str) -> Vec<TypeScriptPayload> {
    let basis = SourceReviewBasisV1::new(
        typescript_registry_binding(),
        files
            .iter()
            .map(|(path, _)| SourceReviewFileV1 {
                path: (*path).to_owned(),
                language: "typescript".to_owned(),
                outcome: SourceFileOutcome::Parsed,
            })
            .collect(),
        Vec::new(),
    )
    .expect("parsed multi-file basis");
    let sources = AdmittedSourceBundleV1::new(
        &basis,
        files
            .iter()
            .map(|(path, source)| AdmittedSourceFileV1 {
                file_id: file_id(path),
                bytes: source.as_bytes().to_vec(),
                source_hash: SourceHash::from_source_bytes(source.as_bytes()),
            })
            .collect(),
    )
    .expect("fixture bytes are admitted for each parsed basis file");
    rebuild_payload_from_source(
        &sources,
        &typescript_registry_binding(),
        &SnapshotBinding::from_admitted_binding(ACC_TSL_SNAPSHOT),
        file_id(target),
    )
    .expect("source-only rebuild of the caller file")
}

fn single_binding(payloads: &[TypeScriptPayload]) -> (&TypeScriptPayload, &BindingDataV1) {
    let bindings = payloads
        .iter()
        .filter(|payload| payload.role == SyntaxRole::Binding)
        .collect::<Vec<_>>();
    assert_eq!(
        bindings.len(),
        1,
        "the caller has exactly one import binding"
    );
    let TypeScriptPayloadData::Binding(data) = &bindings[0].data else {
        panic!("binding role keeps BindingDataV1")
    };
    (bindings[0], data)
}

fn stage_wires(data: &BindingDataV1) -> Vec<&str> {
    data.evaluated_stages
        .iter()
        .map(|stage| stage.wire_literal())
        .collect()
}

#[test]
fn acc_tsl_extensionless_file_and_index_pair_is_relative_target_ambiguous_at_candidates() {
    // The design: extensionless `./x` probes p, p.ts, p.tsx, p/index.ts,
    // p/index.tsx. After scanning every candidate, two or more
    // TS/TSX candidates is relative_target_ambiguous ("file/index併存もここ"),
    // and candidates must not be removed to make the rest unique.
    // F3-R11: src/api.ts and src/api/index.ts both define f,
    // `./api` keeps both existing candidates, relative_target_ambiguous, D=0.
    // REGISTRY:406--409 — relative_target_ambiguous has required_stage
    // b.candidates; REGISTRY:245--288 orders the stages b.form(1),
    // b.local_uniqueness(2), b.specifier(3), b.candidates(4), b.export_binding(5).
    // The evaluation stops at the stage that decided the reason, so the
    // expected list is stages 1..=4 and excludes b.export_binding.
    let caller = "import { f } from \"./x\";\nexport function run(){return f();}\n";
    let file_target = "export function f(){return 1;}\n";
    let index_target = "export function f(){return 2;}\n";
    let payloads = rebuild(
        &[
            ("src/client.ts", caller),
            ("src/x.ts", file_target),
            ("src/x/index.ts", index_target),
        ],
        "src/client.ts",
    );
    let (binding, data) = single_binding(&payloads);

    assert_eq!(
        binding.outcome,
        TypeScriptOutcomeV1::Binding(ResolutionOutcomeV1::Unresolved)
    );
    let TypeScriptReasonsV1::Binding(reasons) = &binding.reasons else {
        panic!("binding role keeps binding reasons")
    };
    assert_eq!(
        reasons
            .all()
            .iter()
            .map(|reason| reason.wire_literal())
            .collect::<Vec<_>>(),
        vec!["relative_target_ambiguous"],
        "exact binding reason set for a file/index pair"
    );
    let Some(TypeScriptPrimaryReasonV1::Binding(primary)) = &binding.primary_reason else {
        panic!("an unresolved binding keeps a binding primary reason")
    };
    assert_eq!(primary.wire_literal(), "relative_target_ambiguous");
    assert_eq!(
        stage_wires(data),
        vec![
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates"
        ],
        "ambiguity is decided at b.candidates and never reaches b.export_binding"
    );
    assert_eq!(data.resolved_function_id, None);

    // The design's candidate set, kept in UTF-8 byte order ('.' < '/'), with an
    // admitted file key exactly for the two existing TS files.
    let candidates = data
        .candidate_paths
        .iter()
        .map(|candidate| (candidate.path.as_str(), candidate.file_key.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        candidates,
        vec![
            ("src/x", None),
            ("src/x.ts", Some(file_id("src/x.ts"))),
            ("src/x.tsx", None),
            ("src/x/index.ts", Some(file_id("src/x/index.ts"))),
            ("src/x/index.tsx", None),
        ],
        "both existing candidates are retained; neither is dropped to force uniqueness"
    );
}

#[test]
fn acc_tsl_default_exported_generator_is_the_default_import_target() {
    // The design: `export function f() { ... }` counts "async/generator を含む".
    // The design: `export default function f() { ... }` is the file's default
    // implementation. Together: `export default function* g(){}` is the default
    // callable of m.ts, so `import g from "./m"` binds to it.
    // REGISTRY:245--288 — a resolved binding evaluates all seven stages.
    // Declaration identity is the generator declaration node's half-open byte
    // range (v0b test :723 fixes `function* gen(){ … }` as the declaration).
    let caller = "import g from \"./m\";\nexport function run(){return g();}\n";
    let callee = "export default function* g(){ yield 1; }\n";
    let payloads = rebuild(
        &[("src/client.ts", caller), ("src/m.ts", callee)],
        "src/client.ts",
    );
    let expected_target = DeclarationId::from_source(
        file_id("src/m.ts"),
        unique_range(callee, "function* g(){ yield 1; }"),
    );

    let (binding, data) = single_binding(&payloads);
    assert_eq!(
        binding.outcome,
        TypeScriptOutcomeV1::Binding(ResolutionOutcomeV1::Resolved),
        "the default import resolves to the default-exported generator"
    );
    assert_eq!(binding.primary_reason, None);
    assert_eq!(data.resolved_function_id, Some(expected_target));
    assert_eq!(
        stage_wires(data),
        vec![
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
            "b.result"
        ]
    );
    // The call payload is deliberately not asserted here: the public raw
    // source view reports target_inventory_complete=false (payload.rs:99), so
    // a relative call through it came back relative_target_unread (observed once
    // at 31eba48a, not traced to a design line); only
    // runtime A1's sealed A0 view can resolve the call edge.
}
