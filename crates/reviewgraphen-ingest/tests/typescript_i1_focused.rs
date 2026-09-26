// Literals copied independently from the frozen design (F1/F2/F3/F4/F6).
use reviewgraphen_core::source_review::registry::{
    typescript_registry_binding, validate_typescript_binding, validate_typescript_tuple,
};
use reviewgraphen_ingest::typescript::{
    inventory::{FileOutcome, InventoryEntry, InventoryLimits, TreeEntryKind, inventory},
    local_calls::local_call_reasons,
    relative_paths::{RelativeEntry, resolve_relative, resolve_relative_in_tree},
    syntax::{CallableOutcome, parse_typescript},
};

fn entry(path: &str, source: &str) -> InventoryEntry {
    InventoryEntry::regular(path, source.as_bytes().to_vec())
}

#[test]
fn f4_t1_through_t7_and_f6_file_accounting_literals() {
    let records = inventory(
        vec![
            entry("src/f.ts", "export function f(){}"),
            entry("src/f.tsx", "export function f(){}"),
            entry("node_modules/f.ts", "export function f(){}"),
            entry("dist/f.ts", "export function f(){}"),
            entry("src/f.d.ts", "export function f(){}"),
            entry("src/f.test.ts", "export function f(){}"),
            entry("src/test_helpers.ts", "export function f(){}"),
            entry("src/f.TS", "export function f(){}"),
            entry("src/broken.ts", "export function {"),
            entry("src/giant.ts", &"x".repeat(80)),
            InventoryEntry::new("src/link.ts", TreeEntryKind::Symlink, None),
        ],
        InventoryLimits::new(20, 64, 180),
    );
    let expected = [
        ("src/f.ts", FileOutcome::Parsed, true),
        ("src/f.tsx", FileOutcome::NonTargetExtension, false),
        ("node_modules/f.ts", FileOutcome::ProfileExcluded, false),
        ("dist/f.ts", FileOutcome::ProfileExcluded, false),
        ("src/f.d.ts", FileOutcome::ProfileExcluded, false),
        ("src/f.test.ts", FileOutcome::ProfileExcluded, false),
        ("src/test_helpers.ts", FileOutcome::Parsed, true),
        ("src/f.TS", FileOutcome::NonTargetExtension, false),
        ("src/broken.ts", FileOutcome::ParseFailed, true),
        ("src/giant.ts", FileOutcome::UnreadBound, false),
        ("src/link.ts", FileOutcome::UnsupportedEntry, false),
    ];
    for (path, outcome, bytes_read) in expected {
        let record = records.iter().find(|record| record.path == path).unwrap();
        assert_eq!(record.outcome, outcome, "{path}");
        assert_eq!(record.bytes_read, bytes_read, "{path}");
        if !bytes_read || outcome == FileOutcome::ParseFailed {
            assert_eq!(record.latent_callable_count, None, "{path}");
        }
    }
    let parsed = records
        .iter()
        .find(|record| record.path == "src/f.ts")
        .unwrap();
    assert_eq!(parsed.language.as_deref(), Some("typescript"));
    assert!(parsed.source_hash.is_some());
}

#[test]
fn f6_9_tuple_and_language_tampering_are_rejected() {
    let binding = typescript_registry_binding();
    assert!(validate_typescript_tuple(&binding.tuple).is_ok());
    assert!(validate_typescript_binding(&binding).is_ok());
    let mut forged = binding.tuple;
    forged.language = "typescriptx".into();
    assert!(validate_typescript_tuple(&forged).is_err());
    let mut forged = typescript_registry_binding();
    forged.registry_hash = "sha256:tampered".into();
    assert!(validate_typescript_binding(&forged).is_err());
}

#[test]
fn f1_p1_through_p13_parse_public_and_contains_without_parse_failure_facts() {
    let cases = [
        ("export function f(){}", 1, CallableOutcome::EligiblePublic),
        ("export const f=()=>1", 1, CallableOutcome::EligiblePublic),
        (
            "export const f=function(){}",
            1,
            CallableOutcome::EligiblePublic,
        ),
        (
            "export default function f(){}",
            1,
            CallableOutcome::EligiblePublic,
        ),
        (
            "export default function(){}",
            1,
            CallableOutcome::EligiblePublic,
        ),
        ("export default ()=>1", 1, CallableOutcome::EligiblePublic),
        (
            "function f(){}; export {f as g,f as default}",
            1,
            CallableOutcome::EligiblePublic,
        ),
        (
            "function f(){}; export default f",
            1,
            CallableOutcome::EligiblePublic,
        ),
        ("function f(){}", 0, CallableOutcome::NonPublic),
        ("const f=()=>1", 0, CallableOutcome::NonPublic),
        ("export const n=1", 0, CallableOutcome::NotRuntimeCallable),
        (
            "type T={n:number}; export type {T}",
            0,
            CallableOutcome::NotRuntimeCallable,
        ),
        ("export class C { m(){} }", 0, CallableOutcome::OutOfScope),
        ("export let f=()=>1", 0, CallableOutcome::Unsupported),
        ("export const f=factory()", 0, CallableOutcome::Unsupported),
        (
            "export function f(x:string):string; export function f(x:any){return x}",
            0,
            CallableOutcome::Unsupported,
        ),
        (
            "export namespace N { export function f(){} }",
            0,
            CallableOutcome::OutOfScope,
        ),
        (
            "function outer(){function inner(){}}",
            0,
            CallableOutcome::OutOfScope,
        ),
        ("consume(()=>1)", 0, CallableOutcome::OutOfScope),
        (
            "export const f=(function(){})",
            0,
            CallableOutcome::Unsupported,
        ),
        (
            "function f(){}; module.exports=f",
            0,
            CallableOutcome::Unsupported,
        ),
        ("export class C { f=()=>1 }", 0, CallableOutcome::OutOfScope),
        (
            "export async function f(){}",
            1,
            CallableOutcome::EligiblePublic,
        ),
        (
            "export function* f(){yield 1}",
            1,
            CallableOutcome::EligiblePublic,
        ),
        (
            "export const f:()=>number=()=>1",
            1,
            CallableOutcome::EligiblePublic,
        ),
    ];
    for (source, public, outcome) in cases {
        let parsed = parse_typescript("src/public.ts", source.as_bytes());
        assert!(parsed.is_parsed(), "{source}");
        assert_eq!(parsed.public_callables().count(), public, "{source}");
        assert!(
            parsed.callables.iter().any(|item| item.outcome == outcome),
            "{source}"
        );
    }
    let failed = parse_typescript("src/broken.ts", b"export function {");
    assert!(!failed.is_parsed());
    assert!(failed.callables.is_empty());
    assert!(failed.contains.is_empty());
    assert_eq!(failed.latent_callable_count, None);
}

#[test]
fn f2_c7_through_c10_negative_local_reasons_are_not_edges() {
    for (source, reason) in [
        (
            "export function f(){} function g(f:()=>number){return f()}",
            "shadowed_binding",
        ),
        (
            "export function f(){} function g(){let f=()=>1;return f()}",
            "shadowed_binding",
        ),
        (
            "export function f(){} f=()=>3; function g(){return f()}",
            "written_binding",
        ),
        (
            "export function f(){} eval('f'); function g(){return f()}",
            "unsupported_syntax",
        ),
        (
            "export function f(){} function g(){return f?.()}",
            "dynamic_dispatch",
        ),
        (
            "export function f(){} function g(){return obj.f()}",
            "dynamic_dispatch",
        ),
        (
            "export function f(){} function g(){return new F()}",
            "dynamic_dispatch",
        ),
        (
            "export function f(){} function g(){return (f)()}",
            "unsupported_syntax",
        ),
        (
            "export function f(){} class C{g(){return f()}}",
            "unsupported_caller",
        ),
    ] {
        assert!(
            local_call_reasons(source).contains(&reason.into()),
            "{source}"
        );
    }
}

#[test]
fn f3_relative_candidate_and_reason_closure_is_git_tree_only() {
    // F3-R01/R04-R06/R08/R11-R20/R31/R32/R34-R36.path literals copied from the design.
    let normal = resolve_relative(
        "src/client.ts",
        "./api",
        vec![RelativeEntry::parsed("src/api.ts")],
    );
    assert_eq!(normal.target.as_deref(), Some("src/api.ts")); // R01, R35: no host lookup
    let js = resolve_relative(
        "src/client.ts",
        "./api.js",
        vec![RelativeEntry::parsed("src/api.ts")],
    );
    assert_eq!(
        js.candidates,
        vec!["src/api.js", "src/api.ts", "src/api.tsx"]
    );
    assert_eq!(js.target.as_deref(), Some("src/api.ts"));
    let index = resolve_relative(
        "src/ui/client.ts",
        "../api",
        vec![RelativeEntry::parsed("src/api/index.ts")],
    );
    assert_eq!(index.target.as_deref(), Some("src/api/index.ts"));
    let nested = resolve_relative(
        "src/ui/client.ts",
        "../api",
        vec![RelativeEntry::parsed("src/api.ts")],
    );
    assert_eq!(nested.target.as_deref(), Some("src/api.ts")); // R06
    assert_eq!(
        resolve_relative(
            "src/client.ts",
            "./api.ts",
            vec![RelativeEntry::parsed("src/api.ts")],
        )
        .target
        .as_deref(),
        Some("src/api.ts")
    ); // R08
    let file_index = resolve_relative(
        "src/client.ts",
        "./api",
        vec![
            RelativeEntry::parsed("src/api.ts"),
            RelativeEntry::parsed("src/api/index.ts"),
        ],
    );
    assert_eq!(file_index.reasons, vec!["relative_target_ambiguous"]); // R11
    let conflict = resolve_relative(
        "src/client.ts",
        "./api",
        vec![
            RelativeEntry::parsed("src/api.ts"),
            RelativeEntry::other("src/api.tsx"),
        ],
    );
    assert_eq!(conflict.target, None);
    assert_eq!(conflict.reasons, vec!["relative_target_ambiguous"]); // R12
    let explicit_js_collision = resolve_relative(
        "src/client.ts",
        "./api.js",
        vec![
            RelativeEntry::other("src/api.js"),
            RelativeEntry::parsed("src/api.ts"),
        ],
    );
    assert_eq!(
        explicit_js_collision.reasons,
        vec!["relative_target_ambiguous"]
    ); // R13
    let witness = resolve_relative(
        "src/client.ts",
        "./api",
        vec![RelativeEntry::other("src/api.js")],
    );
    assert_eq!(
        witness.reasons,
        vec!["relative_target_ambiguous", "relative_target_missing"]
    );
    assert_eq!(
        witness.primary_reason.as_deref(),
        Some("relative_target_ambiguous")
    );
    let index_witness = resolve_relative(
        "src/client.ts",
        "./api",
        vec![
            RelativeEntry::parsed("src/api.ts"),
            RelativeEntry::other("src/api/index.js"),
        ],
    );
    assert_eq!(index_witness.reasons, vec!["relative_target_ambiguous"]); // R14b
    assert_eq!(
        resolve_relative("src/client.ts", "./api", vec![]).reasons,
        vec!["relative_target_missing"]
    ); // R15
    assert_eq!(
        resolve_relative(
            "src/client.ts",
            "./api.test.ts",
            vec![RelativeEntry::excluded("src/api.test.ts")],
        )
        .reasons,
        vec!["relative_target_excluded"]
    ); // R16
    assert_eq!(
        resolve_relative(
            "src/client.ts",
            "./api",
            vec![RelativeEntry::unread("src/api.ts")],
        )
        .reasons,
        vec!["relative_target_unread"]
    ); // R17/R19a/R31
    let parse_failed = resolve_relative(
        "src/client.ts",
        "./api",
        vec![RelativeEntry::parse_failed("src/api.ts")],
    );
    assert_eq!(
        parse_failed.reasons,
        vec!["parse_failure", "relative_target_unread"]
    ); // R18
    let excluded_index = resolve_relative(
        "src/client.ts",
        "./examples",
        vec![
            RelativeEntry::parsed("src/examples.ts"),
            RelativeEntry::excluded("src/examples/index.ts"),
        ],
    );
    assert_eq!(
        excluded_index.reasons,
        vec!["relative_target_ambiguous", "relative_target_excluded"]
    ); // R32a
    let unread_index = resolve_relative(
        "src/client.ts",
        "./api",
        vec![
            RelativeEntry::parsed("src/api.ts"),
            RelativeEntry::unread("src/api/index.ts"),
        ],
    );
    assert_eq!(
        unread_index.reasons,
        vec!["relative_target_unread", "relative_target_ambiguous"]
    ); // R32b
    for specifier in ["../../outside", "./api.mts", "./api?raw"] {
        assert_eq!(
            resolve_relative("src/client.ts", specifier, vec![]).reasons,
            vec!["relative_specifier_unsupported"],
            "{specifier}"
        );
    }
    assert_eq!(
        resolve_relative_in_tree(
            "src/client.ts",
            "./api",
            vec![RelativeEntry::parsed("src/api.ts")],
            false,
        )
        .reasons,
        vec!["relative_target_unread"]
    ); // R36
}
