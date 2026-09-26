//! R4 acceptance calls for the frozen I2 skeleton.
//!
//! Each assertion preserves its R3 oracle. Typed IDs and closed reason sets
//! replace only unchecked strings; no case, rejection, or set comparison is
//! weakened. Every test currently reaches a skeleton `todo!()`.

use reviewgraphen_core::source_review::admitted_source::{
    AdmittedSourceBundleError, AdmittedSourceBundleV1, AdmittedSourceFileV1,
};
use reviewgraphen_core::source_review::basis::{
    SourceFileOutcome, SourceReviewBasisV1, SourceReviewFileV1, SourceReviewSyntaxV1,
    SourceSyntaxRole,
};
use reviewgraphen_core::source_review::ids::{
    CallerId, CallsiteId, CanonicalFileKey, DeclarationId, SnapshotBinding, SourceFileId,
    SourceHash, SourceIdentity, SourceIdentityError, SourceRange, SourceWitnessKeyV1, SyntaxKeyV1,
};
use reviewgraphen_core::source_review::reasons::{
    CallReason, ReasonSet, RecordOutcomeV1, ResolutionKind, ResolutionOutcomeV1,
};
use reviewgraphen_core::source_review::registry::typescript_registry_binding;
use reviewgraphen_ingest::source_review::extraction_report::rebuild_canonical_extraction;
use reviewgraphen_ingest::typescript::calls::{
    CallerScope, RelativeCallInput, caller_binding_reasons, evaluate_local_calls,
    resolve_relative_call,
};
use reviewgraphen_ingest::typescript::changes::{
    ChangeWitnessInput, ChangeWitnessKind, change_witnesses,
};
use reviewgraphen_ingest::typescript::import_bindings::{
    CallableClassification, ImportBinding, collect_imports, collect_top_level_callables,
    resolve_export_binding,
};
use reviewgraphen_ingest::typescript::payload::{
    CallableOutcomeV1, ScopeKindV1, SyntaxRole, TypeScriptOutcomeV1, TypeScriptPayloadData,
    TypeScriptReasonsV1, encode_payload_draft, rebuild_payload_from_source,
};
use reviewgraphen_ingest::typescript::relative_paths::RelativeEntry;
use std::collections::BTreeSet;
use tree_sitter::{Node, Parser, Tree};

const I2_INGEST_PAYLOAD_SNAPSHOT: &str = "i2-v2-accept-ingest-payload-snapshot@1";

fn tree(source: &str) -> Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .expect("TypeScript grammar");
    parser.parse(source, None).expect("parse fixture")
}

fn file_id(path: &str) -> SourceFileId {
    SourceFileId::from_basis_file_key(
        CanonicalFileKey::from_basis_path(path).expect("canonical fixture path"),
    )
}

fn payload_fixture_snapshot_binding() -> SnapshotBinding {
    SnapshotBinding::from_admitted_binding(I2_INGEST_PAYLOAD_SNAPSHOT)
}

fn range(node: Node<'_>) -> SourceRange {
    SourceRange::new(node.start_byte() as u64, node.end_byte() as u64).expect("node range")
}

fn find_node<'tree>(node: Node<'tree>, source: &str, kind: &str, fragment: &str) -> Node<'tree> {
    find_node_opt(node, source, kind, fragment)
        .unwrap_or_else(|| panic!("fixture node {kind} containing {fragment}"))
}

fn find_node_opt<'tree>(
    node: Node<'tree>,
    source: &str,
    kind: &str,
    fragment: &str,
) -> Option<Node<'tree>> {
    if node.kind() == kind && source[node.byte_range()].contains(fragment) {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_node_opt(child, source, kind, fragment) {
            return Some(found);
        }
    }
    None
}

fn named_binding(root: Node<'_>, source: &str, file_id: SourceFileId) -> ImportBinding {
    collected_first_binding(root, source, file_id)
}

fn caller_scope(file_id: SourceFileId, declaration: Node<'_>) -> CallerScope {
    let declaration_range = range(declaration);
    let declaration = DeclarationId::from_source(file_id, declaration_range);
    CallerScope {
        caller_id: CallerId::from_declaration(declaration),
        caller_range: declaration_range,
    }
}

fn expected_reasons(wires: &[&str]) -> BTreeSet<CallReason> {
    wires
        .iter()
        .map(|wire| match *wire {
            "parse_failure" => CallReason::ParseFailure,
            "unsupported_syntax" => CallReason::UnsupportedSyntax,
            "unsupported_caller" => CallReason::UnsupportedCaller,
            "dynamic_dispatch" => CallReason::DynamicDispatch,
            "relative_target_unread" => CallReason::RelativeTargetUnread,
            "relative_target_ambiguous" => CallReason::RelativeTargetAmbiguous,
            "relative_target_excluded" => CallReason::RelativeTargetExcluded,
            "relative_target_missing" => CallReason::RelativeTargetMissing,
            "import_binding_ambiguous" => CallReason::ImportBindingAmbiguous,
            "type_only_binding" => CallReason::TypeOnlyBinding,
            "export_binding_unsupported" => CallReason::ExportBindingUnsupported,
            "import_resolution_unavailable" => CallReason::ImportResolutionUnavailable,
            "shadowed_binding" => CallReason::ShadowedBinding,
            "written_binding" => CallReason::WrittenBinding,
            _ => panic!("acceptance fixture must name its expected CallReason variant"),
        })
        .collect()
}

fn caller_reasons_for_fixture(
    source: &str,
) -> reviewgraphen_core::source_review::reasons::ReasonSet {
    let parsed = tree(source);
    let file_id = file_id("src/client.ts");
    let caller = find_node(parsed.root_node(), source, "function_declaration", "g");
    caller_binding_reasons(
        &caller_scope(file_id.clone(), caller),
        caller,
        source.as_bytes(),
        &named_binding(parsed.root_node(), source, file_id),
    )
}

fn admitted_call_basis() -> SourceReviewBasisV1 {
    SourceReviewBasisV1::new(
        typescript_registry_binding(),
        vec![SourceReviewFileV1 {
            path: "src/client.ts".to_owned(),
            language: "typescript".to_owned(),
            outcome: SourceFileOutcome::Parsed,
        }],
        vec![SourceReviewSyntaxV1 {
            file_path: "src/client.ts".to_owned(),
            role: SourceSyntaxRole::Call,
        }],
    )
    .expect("admitted parsed call fixture")
}

fn admitted_callable_scope_basis() -> SourceReviewBasisV1 {
    SourceReviewBasisV1::new(
        typescript_registry_binding(),
        vec![SourceReviewFileV1 {
            path: "src/main.ts".to_owned(),
            language: "typescript".to_owned(),
            outcome: SourceFileOutcome::Parsed,
        }],
        vec![
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Callable,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Surface,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Scope,
            },
        ],
    )
    .expect("admitted callable and scope fixture")
}

fn admitted_sources(basis: &SourceReviewBasisV1, files: &[(&str, &str)]) -> AdmittedSourceBundleV1 {
    AdmittedSourceBundleV1::new(
        basis,
        files
            .iter()
            .map(|(path, source)| AdmittedSourceFileV1 {
                file_id: file_id(path),
                bytes: source.as_bytes().to_vec(),
                source_hash: SourceHash::from_source_bytes(source.as_bytes()),
            })
            .collect(),
    )
    .expect("fixture bytes are admitted for each parsed basis file")
}

fn collected_first_binding(
    root: Node<'_>,
    source: &str,
    source_file_id: SourceFileId,
) -> ImportBinding {
    collect_imports(root, source.as_bytes(), source_file_id)
        .into_iter()
        .next()
        .expect("one static import binding")
}

fn ac6_candidate_entries() -> Vec<RelativeEntry> {
    vec![
        RelativeEntry::parse_failed("src/api.ts"),
        RelativeEntry::excluded("src/api/index.ts"),
    ]
}

#[ignore = "slice V3: A2"]
#[test]
fn k1_source_rebuilt_extraction_is_required_for_admission() {
    let basis = SourceReviewBasisV1::new(typescript_registry_binding(), Vec::new(), Vec::new())
        .expect("empty basis is structurally valid");
    let sources = admitted_sources(&basis, &[]);
    // The capability-producing A2/A3 calls live in runtime. The
    // remaining ingest seam reconstructs raw source material only.
    let _draft = rebuild_canonical_extraction(&basis, &sources)
        .expect("ingest rebuilds only raw extraction material for runtime A2/A3");
}

#[test]
fn k3_shadow_is_bound_to_the_enclosing_caller_not_the_whole_file() {
    let caller_source = "import {f} from './api'\n\
        export function shadowed(f:()=>number){return f()}\n\
        export function clean(){return f( )}\n";
    let callee_source = "export function f(){return 2}\n";
    let caller_tree = tree(caller_source);
    let callee_tree = tree(callee_source);
    let caller_file_id = file_id("src/client.ts");
    let callee_file_id = file_id("src/api.ts");
    let clean = find_node(
        caller_tree.root_node(),
        caller_source,
        "function_declaration",
        "clean",
    );
    let call = find_node(clean, caller_source, "call_expression", "f( )");
    let callee = find_node(
        callee_tree.root_node(),
        callee_source,
        "function_declaration",
        "function f(",
    );
    let scope = caller_scope(caller_file_id.clone(), clean);
    let binding = named_binding(
        caller_tree.root_node(),
        caller_source,
        caller_file_id.clone(),
    );
    let export_resolution = resolve_export_binding(
        caller_source.as_bytes(),
        callee_tree.root_node(),
        callee_source.as_bytes(),
        callee_file_id.clone(),
        &binding,
    );
    let result = resolve_relative_call(RelativeCallInput {
        caller: scope.clone(),
        callsite_key: CallsiteId::from_source(
            caller_file_id.clone(),
            range(call),
            Some(scope.caller_id),
        ),
        callsite_node: call,
        caller_source: caller_source.as_bytes(),
        caller_file_id: caller_file_id.clone(),
        binding: &binding,
        export_resolution,
        candidate_entries: vec![RelativeEntry::parsed("src/api.ts")],
        tree_complete: true,
        callee_root: callee_tree.root_node(),
        callee_source: callee_source.as_bytes(),
        callee_file_id: callee_file_id.clone(),
    });
    assert_eq!(
        result.edge.expect("clean caller edge").callee_id,
        DeclarationId::from_source(callee_file_id, range(callee)),
        "the clean caller's edge must resolve f by declaration identity"
    );
    assert!(result.unresolved.is_none(), "expected no shadowed_binding");
}

#[test]
fn k4_const_arrow_and_function_expression_keep_their_binding_as_caller_id() {
    let source = "export function f(){return 1}\n\
        const g=()=>f()\n\
        const h=function(){return f()}\n";
    let parsed = tree(source);
    let api_file_id = file_id("src/api.ts");
    let g_declarator = find_node(
        parsed.root_node(),
        source,
        "variable_declarator",
        "g=()=>f()",
    );
    let h_declarator = find_node(
        parsed.root_node(),
        source,
        "variable_declarator",
        "h=function(){return f()}",
    );
    let g = CallerId::from_declaration(DeclarationId::from_source(
        api_file_id.clone(),
        range(g_declarator),
    ));
    let h = CallerId::from_declaration(DeclarationId::from_source(
        api_file_id.clone(),
        range(h_declarator),
    ));
    let report = evaluate_local_calls(parsed.root_node(), source.as_bytes(), api_file_id);
    assert_eq!(
        report
            .edges
            .iter()
            .map(|edge| edge.caller_id.clone())
            .collect::<Vec<_>>(),
        vec![g, h],
        "R3 expects callers g and h"
    );
    assert!(report.unresolved.is_empty());
}

#[test]
fn k5_export_lists_and_overloads_never_promote_non_unique_runtime_callees() {
    let import_source = "import {f} from './api'\n";
    let import_tree = tree(import_source);
    let caller_file_id = file_id("src/client.ts");
    let binding = named_binding(import_tree.root_node(), import_source, caller_file_id);
    for source in [
        "const f=1\nexport {f}\n",
        "const f=()=>1\nexport {type f}\n",
        "export function f(x:string):string; export function f(x:any){return x}\n",
    ] {
        let parsed = tree(source);
        let rejected = resolve_export_binding(
            import_source.as_bytes(),
            parsed.root_node(),
            source.as_bytes(),
            file_id("src/api.ts"),
            &binding,
        )
        .expect_err("expected export_binding_unsupported");
        assert_eq!(
            rejected.all(),
            &expected_reasons(&["export_binding_unsupported"]),
            "{source}"
        );
    }
}

#[test]
fn k6_callee_writes_reject_the_relative_edge() {
    let caller_source = "import {f} from './api'\nexport function g(){return f()}\n";
    let callee_source = "export const f=()=>1\nf=()=>2\n";
    let caller_tree = tree(caller_source);
    let callee_tree = tree(callee_source);
    let caller_file_id = file_id("src/client.ts");
    let scope = caller_scope(
        caller_file_id.clone(),
        find_node(
            caller_tree.root_node(),
            caller_source,
            "function_declaration",
            "g",
        ),
    );
    let call = find_node(
        caller_tree.root_node(),
        caller_source,
        "call_expression",
        "f()",
    );
    let binding = named_binding(
        caller_tree.root_node(),
        caller_source,
        caller_file_id.clone(),
    );
    let callee_file_id = file_id("src/api.ts");
    let export_resolution = resolve_export_binding(
        caller_source.as_bytes(),
        callee_tree.root_node(),
        callee_source.as_bytes(),
        callee_file_id.clone(),
        &binding,
    );
    let result = resolve_relative_call(RelativeCallInput {
        caller: scope.clone(),
        callsite_key: CallsiteId::from_source(
            caller_file_id.clone(),
            range(call),
            Some(scope.caller_id),
        ),
        callsite_node: call,
        caller_source: caller_source.as_bytes(),
        caller_file_id: caller_file_id.clone(),
        binding: &binding,
        export_resolution,
        candidate_entries: vec![RelativeEntry::parsed("src/api.ts")],
        tree_complete: true,
        callee_root: callee_tree.root_node(),
        callee_source: callee_source.as_bytes(),
        callee_file_id,
    });
    let unresolved = result.unresolved.expect("written binding must block edge");
    assert_eq!(
        unresolved.reasons.all(),
        &expected_reasons(&["written_binding"])
    );
    assert_eq!(
        unresolved.reasons.primary(),
        Some(CallReason::WrittenBinding)
    );
    assert!(result.edge.is_none());
}

#[test]
fn k6_ast_write_shadow_and_eval_cases_do_not_use_substrings() {
    assert_eq!(
        caller_reasons_for_fixture(
            "import {f} from './api'\nexport function g(){f++;return f()}\n"
        )
        .all(),
        &expected_reasons(&["written_binding"])
    );
    assert_eq!(
        caller_reasons_for_fixture(
            "import {f} from './api'\nexport function g(){({f}=value);return f()}\n"
        )
        .all(),
        &expected_reasons(&["written_binding"])
    );
    assert_eq!(
        caller_reasons_for_fixture(
            "import {f} from './api'\nexport function g(){try{return f()}catch(f){return f()}}\n"
        )
        .all(),
        &expected_reasons(&["shadowed_binding"])
    );
    assert!(
        caller_reasons_for_fixture(
            "// eval is a comment\nimport {f} from './api'\nexport function g(){return f()}\n"
        )
        .is_empty()
    );
}

#[test]
fn k11_module_level_unresolved_call_has_no_fabricated_caller_id() {
    let source = "f()\n";
    let parsed = tree(source);
    let report = evaluate_local_calls(
        parsed.root_node(),
        source.as_bytes(),
        file_id("src/client.ts"),
    );
    let unresolved = report
        .unresolved
        .first()
        .expect("module-level unresolved call");
    assert_eq!(unresolved.caller_id, None);
    assert_eq!(
        unresolved.reasons.primary(),
        Some(CallReason::UnsupportedCaller)
    );
}

#[ignore = "slice V5: A4"]
#[test]
fn k12_change_witnesses_are_declaration_id_and_range_based() {
    let base = "export function f(){return 1}\n";
    let target = "export function f(){return 2}\n";
    let base_tree = tree(base);
    let target_tree = tree(target);
    let api_file_id = file_id("src/api.ts");
    let base_declaration = find_node(
        base_tree.root_node(),
        base,
        "function_declaration",
        "function f(",
    );
    let target_declaration = find_node(
        target_tree.root_node(),
        target,
        "function_declaration",
        "function f(",
    );
    let declaration = DeclarationId::from_source(api_file_id.clone(), range(base_declaration));
    let witnesses = change_witnesses(ChangeWitnessInput {
        base_identity: SourceIdentity::from_admitted_source(
            api_file_id.clone(),
            SourceHash::from_source_bytes(base.as_bytes()),
        ),
        base_root: base_tree.root_node(),
        base_source: base.as_bytes(),
        base_declaration_id: declaration.clone(),
        target_identity: SourceIdentity::from_admitted_source(
            api_file_id,
            SourceHash::from_source_bytes(target.as_bytes()),
        ),
        target_root: target_tree.root_node(),
        target_source: target.as_bytes(),
        target_declaration_id: DeclarationId::from_source(
            file_id("src/api.ts"),
            range(target_declaration),
        ),
    })
    .expect("body-only declaration change is a witness");
    assert_eq!(witnesses.len(), 1);
    assert_eq!(witnesses[0].compared_range, range(base_declaration));
}

#[test]
fn k14_node_and_relative_callee_use_the_same_parenthesized_callable_predicate() {
    let callee_source = "export const f=(function(){return 2})\n";
    let caller_source = "import {f} from './api'\nexport function g(){return f()}\n";
    let callee_tree = tree(callee_source);
    let caller_tree = tree(caller_source);
    let callee_file_id = file_id("src/api.ts");
    let node_callee = collect_top_level_callables(
        callee_tree.root_node(),
        callee_source.as_bytes(),
        callee_file_id.clone(),
    )
    .into_iter()
    .find_map(|item| match item {
        CallableClassification::Callable(id) => Some(id),
        CallableClassification::Rejected(_) => None,
    });
    assert!(
        node_callee.is_some(),
        "the design strips parentheses around a direct callable"
    );
    let caller_file_id = file_id("src/client.ts");
    let caller = find_node(
        caller_tree.root_node(),
        caller_source,
        "function_declaration",
        "g",
    );
    let call = find_node(caller, caller_source, "call_expression", "f()");
    let scope = caller_scope(caller_file_id.clone(), caller);
    let binding = named_binding(
        caller_tree.root_node(),
        caller_source,
        caller_file_id.clone(),
    );
    let export_resolution = resolve_export_binding(
        caller_source.as_bytes(),
        callee_tree.root_node(),
        callee_source.as_bytes(),
        callee_file_id.clone(),
        &binding,
    );
    let relative = resolve_relative_call(RelativeCallInput {
        caller: scope.clone(),
        callsite_key: CallsiteId::from_source(
            caller_file_id.clone(),
            range(call),
            Some(scope.caller_id),
        ),
        callsite_node: call,
        caller_source: caller_source.as_bytes(),
        caller_file_id: caller_file_id.clone(),
        binding: &binding,
        export_resolution,
        candidate_entries: vec![RelativeEntry::parsed("src/api.ts")],
        tree_complete: true,
        callee_root: callee_tree.root_node(),
        callee_source: callee_source.as_bytes(),
        callee_file_id,
    });
    assert_eq!(
        relative.edge.expect("relative callable edge").callee_id,
        node_callee.expect("Node predicate must admit f"),
        "Node and relative resolution use the same parenthesized callable predicate"
    );
}

#[ignore = "slice V3: A2"]
#[test]
fn ac1_source_reconstruction_rejects_deleted_added_and_changed_catalog_rows() {
    // The A2/A3 comparison lives in runtime,
    // where the only context origin is A0. This target retains the independent
    // raw source fixture needed by that runtime acceptance.
    let client_source = "export function f(){return 1}\nexport function g(){return f()}\n";
    let basis = SourceReviewBasisV1::new(
        typescript_registry_binding(),
        vec![SourceReviewFileV1 {
            path: "src/client.ts".to_owned(),
            language: "typescript".to_owned(),
            outcome: SourceFileOutcome::Parsed,
        }],
        vec![SourceReviewSyntaxV1 {
            file_path: "src/client.ts".to_owned(),
            role: SourceSyntaxRole::Call,
        }],
    )
    .expect("one parsed call file is structurally valid");
    let sources = admitted_sources(&basis, &[("src/client.ts", client_source)]);
    let _draft = rebuild_canonical_extraction(&basis, &sources)
        .expect("runtime A2/A3 will compare missing, extra, range, and typed-data variants");
}

#[test]
fn ac2_payload_reconstruction_never_accepts_a_submitted_draft_as_input() {
    // Runtime A1 owns the unresolved literal oracle;
    // this raw helper rebuilds the file's payload catalogue without receiving
    // the earlier self-comparison draft (`data={callee:f}`, empty reasons,
    // SyntacticUnique) or any submitted payload field.
    let source = "export function g(){return f()}\n";
    let basis = admitted_call_basis();
    let sources = admitted_sources(&basis, &[("src/client.ts", source)]);
    let snapshot = payload_fixture_snapshot_binding();
    let payloads = rebuild_payload_from_source(
        &sources,
        &typescript_registry_binding(),
        &snapshot,
        file_id("src/client.ts"),
    )
    .expect("runtime A1 will compare this source-only rebuild with its submission");
    let call_start = source
        .rfind("f()")
        .expect("AC2 fixture has exactly its unresolved call") as u64;
    let call = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Call)
        .expect("V0b call observation rebuilds into one call payload");
    assert_eq!(
        call.range,
        SourceRange::new(call_start, call_start + 3).expect("call range")
    );
    assert_eq!(
        call.outcome,
        TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Unresolved)
    );
    assert_eq!(
        call.reasons,
        TypeScriptReasonsV1::Call(ReasonSet::new([CallReason::UnresolvedName]))
    );
    assert_eq!(
        call.primary_reason,
        Some(
            reviewgraphen_ingest::typescript::payload::TypeScriptPrimaryReasonV1::Call(
                CallReason::UnresolvedName
            )
        )
    );
    let TypeScriptPayloadData::Call(data) = &call.data else {
        panic!("AC2 call keeps CallDataV1")
    };
    assert!(
        data.caller_id.is_some(),
        "the enclosing g callable is source-derived"
    );
    assert_eq!(data.callee_id, None);
    assert_eq!(data.resolution_kind, None);
    assert_eq!(data.binding_key, None, "bare f has no import binding");
}

#[test]
fn v2_rebuilds_callable_and_scope_payloads_from_source_literals() {
    // The byte ranges are calculated
    // from the immutable fixture's TypeScript nodes, never from rebuild output.
    let source = "export function value(){return 2;}\n";
    let parsed = tree(source);
    let callable_range = range(find_node(
        parsed.root_node(),
        source,
        "function_declaration",
        "value",
    ));
    let scope_range = range(parsed.root_node());
    let basis = admitted_callable_scope_basis();
    let sources = admitted_sources(&basis, &[("src/main.ts", source)]);
    let snapshot = payload_fixture_snapshot_binding();
    let payloads = rebuild_payload_from_source(
        &sources,
        &typescript_registry_binding(),
        &snapshot,
        file_id("src/main.ts"),
    )
    .expect("source-only rebuild produces the fixture payload catalogue");

    let callable = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Callable)
        .expect("fixture has its exported callable payload");
    assert_eq!(callable.range, callable_range);
    assert_eq!(
        callable.outcome,
        TypeScriptOutcomeV1::Callable(CallableOutcomeV1::EligiblePublic)
    );
    assert!(matches!(
        &callable.reasons,
        TypeScriptReasonsV1::Callable(reasons) if reasons.is_empty()
    ));
    assert_eq!(callable.primary_reason, None);
    let TypeScriptPayloadData::Callable(callable_data) = &callable.data else {
        panic!("callable role keeps CallableDataV1")
    };
    assert_eq!(callable_data.declaration_range, callable_range);
    assert_eq!(
        callable_data.visibility_value,
        reviewgraphen_ingest::typescript::payload::VisibilityValueV1::Exported
    );

    let scope = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Scope)
        .expect("fixture has its file-lexical scope payload");
    assert_eq!(scope.range, scope_range);
    assert_eq!(
        scope.outcome,
        TypeScriptOutcomeV1::Scope(RecordOutcomeV1::Recorded)
    );
    assert!(matches!(
        &scope.reasons,
        TypeScriptReasonsV1::Scope(reasons) if reasons.is_empty()
    ));
    assert_eq!(scope.primary_reason, None);
    let TypeScriptPayloadData::Scope(scope_data) = &scope.data else {
        panic!("scope role keeps ScopeDataV1")
    };
    assert_eq!(scope_data.scope_kind, ScopeKindV1::FileLexical);
    assert!(
        !scope_data.member_keys.is_empty(),
        "the file scope retains source-derived member keys"
    );
}

#[test]
fn v2fix_encoder_keeps_structured_data_and_nonempty_obstruction_keys() {
    // Data stays a JSON object and the
    // encoder copies every field without filling a fixed obstruction value.
    let source = "export function value(){return 2;}\n";
    let basis = admitted_callable_scope_basis();
    let sources = admitted_sources(&basis, &[("src/main.ts", source)]);
    let snapshot = payload_fixture_snapshot_binding();
    let mut callable = rebuild_payload_from_source(
        &sources,
        &typescript_registry_binding(),
        &snapshot,
        file_id("src/main.ts"),
    )
    .expect("source-only rebuild produces the callable encoder fixture")
    .into_iter()
    .find(|payload| payload.role == SyntaxRole::Callable)
    .expect("fixture has its exported callable payload");
    let obstruction_key = SourceWitnessKeyV1::Syntax(SyntaxKeyV1::derive_from_source(
        &typescript_registry_binding(),
        &snapshot,
        &file_id("src/main.ts"),
        SourceSyntaxRole::Callable,
        &callable.kind,
        callable.range,
    ));
    let TypeScriptPayloadData::Callable(callable_data) = &mut callable.data else {
        panic!("callable payload keeps CallableDataV1")
    };
    callable_data.binding_obstruction_keys = vec![obstruction_key];
    let expected_obstruction_keys = serde_json::Value::Array(
        callable_data
            .binding_obstruction_keys
            .iter()
            .map(|key| match key {
                SourceWitnessKeyV1::Syntax(key) => {
                    serde_json::Value::String(key.wire_literal().to_owned())
                }
                SourceWitnessKeyV1::BasisEndpoint(key) => {
                    serde_json::Value::String(key.wire_literal().to_owned())
                }
            })
            .collect(),
    );
    assert!(
        !callable_data.binding_obstruction_keys.is_empty(),
        "the encoder input has an obstruction key that must not be replaced"
    );

    let draft = encode_payload_draft(&callable);
    assert!(
        draft.data.is_object(),
        "the design requires object payload data"
    );
    assert_eq!(
        draft.data["binding_obstruction_keys"], expected_obstruction_keys,
        "the encoder preserves the caller-supplied obstruction keys exactly"
    );
}

#[ignore = "slice V2-call: after V0"]
#[test]
fn ac3_relative_export_surface_cases_use_the_export_resolution_entry() {
    // F3-R03/R09/R10/R12: names alone never form an exact
    // callee; the public resolver is the only way to obtain that proof.
    let named_import = "import {f} from './api'\nexport function run(){return f()}\n";
    let named_tree = tree(named_import);
    let named_binding = collected_first_binding(
        named_tree.root_node(),
        named_import,
        file_id("src/client.ts"),
    );
    for callee in [
        "function f(){return 1}\n",
        "const f=()=>1\nexport {type f}\n",
        "export function f(x:string):string; export function f(x:any){return x}\n",
    ] {
        let callee_tree = tree(callee);
        let reasons = resolve_export_binding(
            named_import.as_bytes(),
            callee_tree.root_node(),
            callee.as_bytes(),
            file_id("src/api.ts"),
            &named_binding,
        )
        .expect_err("non-public, type-only, and overload surfaces are not exact callees");
        assert_eq!(
            reasons.all(),
            &expected_reasons(&["export_binding_unsupported"]),
            "{callee}"
        );
    }

    for (import_source, callee_source, expected_declaration) in [
        (
            "import {renamed as local} from './api'\nexport function run(){return local()}\n",
            "function f(){return 1}\nexport {f as renamed}\n",
            "function f(){return 1}",
        ),
        (
            "import local from './api'\nexport function run(){return local()}\n",
            "export default function f(){return 1}\n",
            "function f(){return 1}",
        ),
    ] {
        let import_tree = tree(import_source);
        let callee_tree = tree(callee_source);
        let binding = collected_first_binding(
            import_tree.root_node(),
            import_source,
            file_id("src/client.ts"),
        );
        let expected_start = callee_source
            .find(expected_declaration)
            .expect("fixture has the expected full declaration")
            as u64;
        let expected_range = SourceRange::new(
            expected_start,
            expected_start + expected_declaration.len() as u64,
        )
        .expect("full declaration range");
        let resolved = resolve_export_binding(
            import_source.as_bytes(),
            callee_tree.root_node(),
            callee_source.as_bytes(),
            file_id("src/api.ts"),
            &binding,
        )
        .expect("a same-file runtime export resolves through its export slot");
        assert_eq!(
            resolved.callee_id(),
            &DeclarationId::from_source(file_id("src/api.ts"), expected_range)
        );
    }

    let namespace_source = "import * as ns from './api'\nexport function run(){return ns.f()}\n";
    let namespace_tree = tree(namespace_source);
    let namespace = collected_first_binding(
        namespace_tree.root_node(),
        namespace_source,
        file_id("src/client.ts"),
    );
    let api = "export function f(){return 1}\n";
    let api_tree = tree(api);
    let export_resolution = resolve_export_binding(
        namespace_source.as_bytes(),
        api_tree.root_node(),
        api.as_bytes(),
        file_id("src/api.ts"),
        &namespace,
    );
    let namespace_caller = find_node(
        namespace_tree.root_node(),
        namespace_source,
        "function_declaration",
        "run",
    );
    let namespace_call = find_node(
        namespace_caller,
        namespace_source,
        "call_expression",
        "ns.f()",
    );
    let namespace_file_id = file_id("src/client.ts");
    let namespace_scope = caller_scope(namespace_file_id.clone(), namespace_caller);
    let namespace_result = resolve_relative_call(RelativeCallInput {
        caller: namespace_scope.clone(),
        callsite_key: CallsiteId::from_source(
            namespace_file_id.clone(),
            range(namespace_call),
            Some(namespace_scope.caller_id),
        ),
        callsite_node: namespace_call,
        caller_source: namespace_source.as_bytes(),
        caller_file_id: namespace_file_id,
        binding: &namespace,
        export_resolution,
        candidate_entries: vec![RelativeEntry::parsed("src/api.ts")],
        tree_complete: true,
        callee_root: api_tree.root_node(),
        callee_source: api.as_bytes(),
        callee_file_id: file_id("src/api.ts"),
    });
    assert!(
        namespace_result.edge.is_none(),
        "namespace dispatch has no exact edge"
    );
    assert_eq!(
        namespace_result
            .unresolved
            .expect("namespace dispatch is recorded")
            .reasons
            .all(),
        &expected_reasons(&["dynamic_dispatch", "import_resolution_unavailable"])
    );

    // F3-R12 is unambiguous in the frozen design table: two existing .ts/.tsx
    // candidates are ambiguous and produce no D edge. We assert that required
    // reason without importing the frozen CLI's conflicting primary literal.
    let caller_source = "import {f} from './api'\nexport function run(){return f()}\n";
    let caller_tree = tree(caller_source);
    let api_tree = tree(api);
    let binding = collected_first_binding(
        caller_tree.root_node(),
        caller_source,
        file_id("src/client.ts"),
    );
    let export_resolution = resolve_export_binding(
        caller_source.as_bytes(),
        api_tree.root_node(),
        api.as_bytes(),
        file_id("src/api.ts"),
        &binding,
    );
    let caller = find_node(
        caller_tree.root_node(),
        caller_source,
        "function_declaration",
        "run",
    );
    let call = find_node(caller, caller_source, "call_expression", "f()");
    let caller_file_id = file_id("src/client.ts");
    let scope = caller_scope(caller_file_id.clone(), caller);
    let coexist = resolve_relative_call(RelativeCallInput {
        caller: scope.clone(),
        callsite_key: CallsiteId::from_source(
            caller_file_id.clone(),
            range(call),
            Some(scope.caller_id),
        ),
        callsite_node: call,
        caller_source: caller_source.as_bytes(),
        caller_file_id,
        binding: &binding,
        export_resolution,
        candidate_entries: vec![
            RelativeEntry::parsed("src/api.ts"),
            RelativeEntry::unread("src/api.tsx"),
        ],
        tree_complete: true,
        callee_root: api_tree.root_node(),
        callee_source: api.as_bytes(),
        callee_file_id: file_id("src/api.ts"),
    });
    assert!(coexist.edge.is_none(), "F3-R12 has D=0");
    assert!(
        coexist
            .unresolved
            .expect("F3-R12 is recorded rather than dropped")
            .reasons
            .all()
            .contains(&CallReason::RelativeTargetAmbiguous),
        "F3-R12 keeps the TSX candidate as an ambiguity"
    );
}

#[test]
fn ac4_local_calls_record_shadow_write_dynamic_and_unsupported_callers() {
    // The design requires every occurrence to be an edge or an
    // unresolved record; no local name match may bypass these conditions.
    for (source, expected) in [
        (
            "function f(){}\nexport function g(f:()=>void){return f()}\n",
            CallReason::ShadowedBinding,
        ),
        (
            "function f(){}\nf=()=>{}\nexport function g(){return f()}\n",
            CallReason::WrittenBinding,
        ),
        (
            "function f(){}\nexport function g(){eval('f');return f()}\n",
            CallReason::UnsupportedSyntax,
        ),
        (
            "function f(){}\nexport function g(){return [1].map(()=>f())}\n",
            CallReason::UnsupportedCaller,
        ),
        (
            "function f(){}\nexport class C { m(){return f()} }\n",
            CallReason::UnsupportedCaller,
        ),
        (
            "export function g(){return obj.f()}\n",
            CallReason::DynamicDispatch,
        ),
    ] {
        let parsed = tree(source);
        let report = evaluate_local_calls(
            parsed.root_node(),
            source.as_bytes(),
            file_id("src/client.ts"),
        );
        assert!(report.edges.is_empty(), "{source}");
        assert!(
            report
                .unresolved
                .iter()
                .any(|call| call.reasons.primary() == Some(expected)),
            "{source}"
        );
    }
    for source in [
        "function f(){}\nfunction f(){}\nexport function g(){return f()}\n",
        "function f(){}\nexport function g(){with (obj) { return f() }}\n",
    ] {
        let parsed = tree(source);
        let report = evaluate_local_calls(
            parsed.root_node(),
            source.as_bytes(),
            file_id("src/client.ts"),
        );
        assert!(report.edges.is_empty(), "{source}");
        assert!(
            !report.unresolved.is_empty(),
            "duplicate declarations and with are recorded rather than dropped: {source}"
        );
    }
}

#[test]
fn ac5_relative_call_checks_caller_containment_and_import_alias_binding() {
    // The AST import extractor supplies the alias;
    // the relative entry cannot substitute an unbound name for it.
    let caller_source = "import {f as local} from './api'\nexport function run(){return local()}\n";
    let callee_source = "export function f(){return 1}\n";
    let caller_tree = tree(caller_source);
    let callee_tree = tree(callee_source);
    let caller_file_id = file_id("src/client.ts");
    let binding = collected_first_binding(
        caller_tree.root_node(),
        caller_source,
        caller_file_id.clone(),
    );
    let export_resolution = resolve_export_binding(
        caller_source.as_bytes(),
        callee_tree.root_node(),
        callee_source.as_bytes(),
        file_id("src/api.ts"),
        &binding,
    );
    let caller = find_node(
        caller_tree.root_node(),
        caller_source,
        "function_declaration",
        "run",
    );
    let call = find_node(caller, caller_source, "call_expression", "local()");
    let scope = caller_scope(caller_file_id.clone(), caller);
    let exact = resolve_relative_call(RelativeCallInput {
        caller: scope.clone(),
        callsite_key: CallsiteId::from_source(
            caller_file_id.clone(),
            range(call),
            Some(scope.caller_id),
        ),
        callsite_node: call,
        caller_source: caller_source.as_bytes(),
        caller_file_id: caller_file_id.clone(),
        binding: &binding,
        export_resolution,
        candidate_entries: vec![RelativeEntry::parsed("src/api.ts")],
        tree_complete: true,
        callee_root: callee_tree.root_node(),
        callee_source: callee_source.as_bytes(),
        callee_file_id: file_id("src/api.ts"),
    });
    let callee = find_node(
        callee_tree.root_node(),
        callee_source,
        "function_declaration",
        "function f(",
    );
    assert_eq!(
        exact.edge.expect("alias has one runtime export").callee_id,
        DeclarationId::from_source(file_id("src/api.ts"), range(callee))
    );

    // The same export proof must not authorize an arbitrary caller range that
    // does not contain the callsite.
    let unrelated_source =
        "import {f} from './api'\nexport function outside(){}\nexport function run(){return f()}\n";
    let unrelated_tree = tree(unrelated_source);
    let binding = collected_first_binding(
        unrelated_tree.root_node(),
        unrelated_source,
        file_id("src/client.ts"),
    );
    let export_resolution = resolve_export_binding(
        unrelated_source.as_bytes(),
        callee_tree.root_node(),
        callee_source.as_bytes(),
        file_id("src/api.ts"),
        &binding,
    );
    let outside = find_node(
        unrelated_tree.root_node(),
        unrelated_source,
        "function_declaration",
        "outside",
    );
    let run = find_node(
        unrelated_tree.root_node(),
        unrelated_source,
        "function_declaration",
        "run",
    );
    let call = find_node(run, unrelated_source, "call_expression", "f()");
    let caller_file_id = file_id("src/client.ts");
    let result = resolve_relative_call(RelativeCallInput {
        caller: caller_scope(caller_file_id.clone(), outside),
        callsite_key: CallsiteId::from_source(caller_file_id.clone(), range(call), None),
        callsite_node: call,
        caller_source: unrelated_source.as_bytes(),
        caller_file_id,
        binding: &binding,
        export_resolution,
        candidate_entries: vec![RelativeEntry::parsed("src/api.ts")],
        tree_complete: true,
        callee_root: callee_tree.root_node(),
        callee_source: callee_source.as_bytes(),
        callee_file_id: file_id("src/api.ts"),
    });
    assert!(
        result.edge.is_none(),
        "foreign caller scope cannot make an edge"
    );
    assert_eq!(
        result
            .unresolved
            .expect("out-of-scope caller is recorded")
            .reasons
            .primary(),
        Some(CallReason::UnsupportedCaller)
    );
}

#[test]
fn v2callfix_relative_exact_pair_has_the_frozen_relative_resolution_kind() {
    // This named import has one candidate file,
    // one runtime export slot, and one enclosing caller. The lookup selects the
    // call by syntax role; these assertions observe endpoint and kind fields.
    let caller_source =
        "import {dependency} from './dependency'\nexport function value(){return dependency()}\n";
    let callee_source = "export function dependency(){return 1}\n";
    let caller_tree = tree(caller_source);
    let callee_tree = tree(callee_source);
    let caller_file_id = file_id("src/main.ts");
    let binding = collected_first_binding(
        caller_tree.root_node(),
        caller_source,
        caller_file_id.clone(),
    );
    let export_resolution = resolve_export_binding(
        caller_source.as_bytes(),
        callee_tree.root_node(),
        callee_source.as_bytes(),
        file_id("src/dependency.ts"),
        &binding,
    );
    let caller = find_node(
        caller_tree.root_node(),
        caller_source,
        "function_declaration",
        "function value(",
    );
    let call = find_node(caller, caller_source, "call_expression", "dependency()");
    let scope = caller_scope(caller_file_id.clone(), caller);
    let result = resolve_relative_call(RelativeCallInput {
        caller: scope.clone(),
        callsite_key: CallsiteId::from_source(
            caller_file_id.clone(),
            range(call),
            Some(scope.caller_id),
        ),
        callsite_node: call,
        caller_source: caller_source.as_bytes(),
        caller_file_id,
        binding: &binding,
        export_resolution,
        candidate_entries: vec![RelativeEntry::parsed("src/dependency.ts")],
        tree_complete: true,
        callee_root: callee_tree.root_node(),
        callee_source: callee_source.as_bytes(),
        callee_file_id: file_id("src/dependency.ts"),
    });
    let edge = result.edge.expect("exact relative pair resolves");
    let callee = find_node(
        callee_tree.root_node(),
        callee_source,
        "function_declaration",
        "function dependency(",
    );
    assert_eq!(
        edge.callee_id,
        DeclarationId::from_source(file_id("src/dependency.ts"), range(callee))
    );
    assert_eq!(
        edge.resolution_kind,
        ResolutionKind::SyntacticUniqueRelativeImportV1
    );
}

#[test]
fn ac6_relative_candidate_records_all_reasons_and_uses_frozen_primary() {
    // The design requires the complete candidate scan before primary
    // selection and fixes parse_failure before the other reasons.
    let caller_source = "import {f} from './api'\nexport function run(){return f()}\n";
    let callee_source = "export function f(){return 1}\n";
    let caller_tree = tree(caller_source);
    let callee_tree = tree(callee_source);
    let caller_file_id = file_id("src/client.ts");
    let binding = collected_first_binding(
        caller_tree.root_node(),
        caller_source,
        caller_file_id.clone(),
    );
    let export_resolution = resolve_export_binding(
        caller_source.as_bytes(),
        callee_tree.root_node(),
        callee_source.as_bytes(),
        file_id("src/api.ts"),
        &binding,
    );
    let caller = find_node(
        caller_tree.root_node(),
        caller_source,
        "function_declaration",
        "run",
    );
    let call = find_node(caller, caller_source, "call_expression", "f()");
    let scope = caller_scope(caller_file_id.clone(), caller);
    let result = resolve_relative_call(RelativeCallInput {
        caller: scope.clone(),
        callsite_key: CallsiteId::from_source(
            caller_file_id.clone(),
            range(call),
            Some(scope.caller_id),
        ),
        callsite_node: call,
        caller_source: caller_source.as_bytes(),
        caller_file_id,
        binding: &binding,
        export_resolution,
        candidate_entries: ac6_candidate_entries(),
        tree_complete: true,
        callee_root: callee_tree.root_node(),
        callee_source: callee_source.as_bytes(),
        callee_file_id: file_id("src/api.ts"),
    });
    let unresolved = result.unresolved.expect("candidate rejection is recorded");
    assert_eq!(
        unresolved.reasons.all(),
        &expected_reasons(&[
            "parse_failure",
            "relative_target_unread",
            "relative_target_ambiguous",
            "relative_target_excluded",
        ])
    );
    assert_eq!(unresolved.reasons.primary(), Some(CallReason::ParseFailure));
    assert!(result.edge.is_none());
}

#[test]
fn ac7_const_callable_identity_uses_each_variable_declarator_range() {
    // A const callable's declaration ID is the corresponding
    // `variable_declarator` byte half-open range: binding, annotation, and
    // initializer are included; the sibling, common export/const prefix, and
    // statement terminator are excluded. K4 uses this same oracle for callers.
    let source = "export const a: () => number = () => b(), b = () => a()\n";
    let parsed = tree(source);
    let a_declarator = find_node(parsed.root_node(), source, "variable_declarator", "a:");
    let b_declarator = find_node(parsed.root_node(), source, "variable_declarator", "b =");
    let a_range = range(a_declarator);
    let b_range = range(b_declarator);
    let a_id = DeclarationId::from_source(file_id("src/api.ts"), a_range);
    let b_id = DeclarationId::from_source(file_id("src/api.ts"), b_range);
    let expected_node_ids = BTreeSet::from([a_id.clone(), b_id.clone()]);

    let node_ids =
        collect_top_level_callables(parsed.root_node(), source.as_bytes(), file_id("src/api.ts"))
            .into_iter()
            .filter_map(|candidate| match candidate {
                CallableClassification::Callable(id) => Some(id),
                CallableClassification::Rejected(_) => None,
            })
            .collect::<BTreeSet<_>>();
    assert_eq!(
        node_ids, expected_node_ids,
        "N=2 keeps the two declarator IDs distinct"
    );
    assert_eq!(
        node_ids.len(),
        2,
        "export const a=..., b=... is two callables"
    );

    let a_text = &source[a_declarator.byte_range()];
    let b_text = &source[b_declarator.byte_range()];
    assert!(a_text.contains("a: () => number = () => b()"));
    assert!(b_text.contains("b = () => a()"));
    assert!(!a_text.contains("b ="), "a excludes its sibling declarator");
    assert!(
        !b_text.contains("export const"),
        "b excludes the shared declaration prefix"
    );

    let report = evaluate_local_calls(parsed.root_node(), source.as_bytes(), file_id("src/api.ts"));
    let expected_edges = BTreeSet::from([
        (CallerId::from_declaration(a_id.clone()), b_id.clone()),
        (CallerId::from_declaration(b_id.clone()), a_id.clone()),
    ]);
    let observed_edges = report
        .edges
        .iter()
        .map(|edge| (edge.caller_id.clone(), edge.callee_id.clone()))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        observed_edges, expected_edges,
        "Node, caller, and callee use the same per-declarator DeclarationId"
    );
}

#[ignore = "slice V5: A4"]
#[test]
fn ac8_change_witnesses_cover_export_surface_changes_and_reject_foreign_identity() {
    // F3-C6: same-file export-list and export const
    // changes are D witnesses; a different source identity is a typed error.
    let base = "function f(){return 1}\nexport {f}\n";
    let target = "function f(){return 1}\n";
    let base_tree = tree(base);
    let target_tree = tree(target);
    let base_declaration = find_node(
        base_tree.root_node(),
        base,
        "function_declaration",
        "function f(",
    );
    let target_declaration = find_node(
        target_tree.root_node(),
        target,
        "function_declaration",
        "function f(",
    );
    let witnesses = change_witnesses(ChangeWitnessInput {
        base_identity: SourceIdentity::from_admitted_source(
            file_id("src/api.ts"),
            SourceHash::from_source_bytes(base.as_bytes()),
        ),
        base_root: base_tree.root_node(),
        base_source: base.as_bytes(),
        base_declaration_id: DeclarationId::from_source(
            file_id("src/api.ts"),
            range(base_declaration),
        ),
        target_identity: SourceIdentity::from_admitted_source(
            file_id("src/api.ts"),
            SourceHash::from_source_bytes(target.as_bytes()),
        ),
        target_root: target_tree.root_node(),
        target_source: target.as_bytes(),
        target_declaration_id: DeclarationId::from_source(
            file_id("src/api.ts"),
            range(target_declaration),
        ),
    })
    .expect("export-list removal is a same-file export witness");
    assert!(
        witnesses
            .iter()
            .any(|witness| witness.kind == ChangeWitnessKind::SameFileExport),
        "export-list witness is retained"
    );

    let const_base = "export const f=()=>1\n";
    let const_target = "const f=()=>1\n";
    let const_base_tree = tree(const_base);
    let const_target_tree = tree(const_target);
    let const_base_declaration = find_node(
        const_base_tree.root_node(),
        const_base,
        "lexical_declaration",
        "f=",
    );
    let const_target_declaration = find_node(
        const_target_tree.root_node(),
        const_target,
        "lexical_declaration",
        "f=",
    );
    let const_witnesses = change_witnesses(ChangeWitnessInput {
        base_identity: SourceIdentity::from_admitted_source(
            file_id("src/api.ts"),
            SourceHash::from_source_bytes(const_base.as_bytes()),
        ),
        base_root: const_base_tree.root_node(),
        base_source: const_base.as_bytes(),
        base_declaration_id: DeclarationId::from_source(
            file_id("src/api.ts"),
            range(const_base_declaration),
        ),
        target_identity: SourceIdentity::from_admitted_source(
            file_id("src/api.ts"),
            SourceHash::from_source_bytes(const_target.as_bytes()),
        ),
        target_root: const_target_tree.root_node(),
        target_source: const_target.as_bytes(),
        target_declaration_id: DeclarationId::from_source(
            file_id("src/api.ts"),
            range(const_target_declaration),
        ),
    })
    .expect("export const attachment/removal is a change witness");
    assert!(
        const_witnesses
            .iter()
            .any(|witness| witness.kind == ChangeWitnessKind::SameFileExport),
        "export const surface change is retained"
    );

    let foreign = change_witnesses(ChangeWitnessInput {
        base_identity: SourceIdentity::from_admitted_source(
            file_id("src/api.ts"),
            SourceHash::from_source_bytes(base.as_bytes()),
        ),
        base_root: base_tree.root_node(),
        base_source: base.as_bytes(),
        base_declaration_id: DeclarationId::from_source(
            file_id("src/api.ts"),
            range(base_declaration),
        ),
        target_identity: SourceIdentity::from_admitted_source(
            file_id("src/foreign.ts"),
            SourceHash::from_source_bytes(target.as_bytes()),
        ),
        target_root: target_tree.root_node(),
        target_source: target.as_bytes(),
        target_declaration_id: DeclarationId::from_source(
            file_id("src/foreign.ts"),
            range(target_declaration),
        ),
    });
    assert!(
        foreign.is_err(),
        "foreign source identity never becomes unchanged"
    );
}

#[ignore = "slice V3: A2"]
#[test]
fn ac11_invalid_basis_path_is_typed_and_catalog_differences_keep_keys() {
    // Malformed basis paths are typed failures, and
    // a source/catalog mismatch retains diagnostic keys rather than panicking.
    assert_eq!(
        CanonicalFileKey::from_basis_path("../outside.ts"),
        Err(SourceIdentityError::InvalidBasisPath)
    );
    let basis = admitted_call_basis();
    let sources = admitted_sources(
        &basis,
        &[("src/client.ts", "export function g(){return f()}\n")],
    );
    let _draft = rebuild_canonical_extraction(&basis, &sources)
        .expect("runtime A2 retains missing and extra canonical diagnostic keys");
}

#[test]
#[allow(clippy::type_complexity)]
fn ac14_source_bundle_rejects_omitted_foreign_and_hash_mismatched_inputs() {
    // SK5--SK7: all source-reconstruction APIs below receive this sealed
    // bundle type, so invalid source input must fail at its shared admission
    // boundary rather than be replaced by basis or submitted rows.
    let basis = admitted_call_basis();
    assert!(matches!(
        AdmittedSourceBundleV1::new(&basis, Vec::new()),
        Err(AdmittedSourceBundleError::MissingParsedFile { .. })
    ));
    assert!(matches!(
        AdmittedSourceBundleV1::new(
            &basis,
            vec![AdmittedSourceFileV1 {
                file_id: file_id("src/foreign.ts"),
                bytes: b"export function foreign(){}\n".to_vec(),
                source_hash: SourceHash::from_source_bytes(b"export function foreign(){}\n"),
            }],
        ),
        Err(AdmittedSourceBundleError::UnknownBasisFile { .. })
    ));
    let client_bytes = b"export function g(){return f()}\n";
    assert!(matches!(
        AdmittedSourceBundleV1::new(
            &basis,
            vec![AdmittedSourceFileV1 {
                file_id: file_id("src/client.ts"),
                bytes: client_bytes.to_vec(),
                source_hash: SourceHash::from_source_bytes(b"hash of other bytes"),
            }],
        ),
        Err(AdmittedSourceBundleError::HashMismatch { .. })
    ));

    let _rebuild: fn(
        &SourceReviewBasisV1,
        &AdmittedSourceBundleV1,
    ) -> Result<
        reviewgraphen_ingest::source_review::extraction_report::CanonicalExtractionDraft,
        reviewgraphen_ingest::source_review::extraction_report::ExtractionError,
    > = reviewgraphen_ingest::source_review::extraction_report::rebuild_canonical_extraction;
    let _payload: fn(
        &AdmittedSourceBundleV1,
        &reviewgraphen_core::source_review::registry::TypeScriptRegistryBinding,
        &SnapshotBinding,
        SourceFileId,
    ) -> Result<
        Vec<reviewgraphen_ingest::typescript::payload::TypeScriptPayload>,
        reviewgraphen_ingest::typescript::payload::PayloadError,
    > = rebuild_payload_from_source;
    let _export_resolution: fn(
        &[u8],
        Node<'_>,
        &[u8],
        SourceFileId,
        &ImportBinding,
    ) -> Result<
        reviewgraphen_ingest::typescript::import_bindings::ResolvedExportBinding,
        ReasonSet,
    > = resolve_export_binding;
}
