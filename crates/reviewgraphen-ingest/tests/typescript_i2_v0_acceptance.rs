//! V0 acceptance for the source-native five-role TypeScript syntax catalogue.
//!
//! This file intentionally exercises only `collect_native_syntax`.  It neither
//! admits a basis nor asserts any cross-file resolution result.

use reviewgraphen_core::source_review::{
    ids::{CanonicalFileKey, SourceFileId, SourceRange},
    reasons::TypeScriptSyntaxKind,
};
use reviewgraphen_ingest::typescript::{
    native_syntax::{NativeSyntaxRecord, collect_native_syntax},
    payload::{PayloadImportKindV1, ScopeKindV1, VisibilityValueV1},
    syntax::{CallableOutcome, parse_typescript},
};

fn file_id(path: &str) -> SourceFileId {
    SourceFileId::from_basis_file_key(
        CanonicalFileKey::from_basis_path(path).expect("canonical fixture path"),
    )
}

fn unique_span(source: &str, node_source: &str) -> (u64, u64) {
    let mut matches = source.match_indices(node_source);
    let (start, _) = matches.next().expect("fixture node source is present");
    assert!(
        matches.next().is_none(),
        "fixture node source must occur exactly once: {node_source}"
    );
    (start as u64, (start + node_source.len()) as u64)
}

fn assert_range(range: &SourceRange, source: &str, node_source: &str) {
    let (start, end) = unique_span(source, node_source);
    assert_eq!(range.start(), start, "range start for {node_source}");
    assert_eq!(range.end(), end, "range end for {node_source}");
}

fn assert_registered_kind(kind: &TypeScriptSyntaxKind) {
    assert!(
        !kind.wire_literal().is_empty(),
        "a source-native record has a registered kind without asserting its opaque literal"
    );
}

fn contains_range(ranges: &[SourceRange], source: &str, node_source: &str) -> bool {
    let (start, end) = unique_span(source, node_source);
    ranges
        .iter()
        .any(|range| range.start() == start && range.end() == end)
}

#[test]
fn v0_collects_five_native_roles_with_source_ranges_and_preserves_i1_catalogue() {
    // F1-P2, F2-C1, and F3-R02; all expected spellings and byte ranges
    // below come from this immutable source literal, not a production result.
    let source = "import { remote as local } from \"./dep\";\n\
export const run: () => number = () => local();\n\
export { run as api };\n";
    let id = file_id("src/v0.ts");
    let records = collect_native_syntax(id.clone(), source.as_bytes());

    let callable = records
        .iter()
        .find(|record| {
            matches!(
                record,
                NativeSyntaxRecord::Callable {
                    binding_key: Some(key),
                    ..
                } if key == "run"
            )
        })
        .expect("one callable record for the direct const callable");
    match callable {
        NativeSyntaxRecord::Callable {
            kind,
            file_id,
            range,
            implementation_range,
            visibility,
            ..
        } => {
            assert_registered_kind(kind);
            assert_eq!(file_id, &id);
            // This is the complete variable_declarator, not its
            // initializer, parent lexical declaration, or export prefix.
            assert_range(range, source, "run: () => number = () => local()");
            assert_range(
                implementation_range
                    .as_ref()
                    .expect("direct callable has an implementation"),
                source,
                "() => local()",
            );
            assert_eq!(*visibility, VisibilityValueV1::Exported);
        }
        _ => unreachable!("the match above selects only callable records"),
    }

    let call = records
        .iter()
        .find(|record| {
            matches!(
                record,
                NativeSyntaxRecord::Call {
                    binding_key: Some(key),
                    ..
                } if key == "local"
            )
        })
        .expect("one direct call record for local()");
    match call {
        NativeSyntaxRecord::Call {
            kind,
            file_id,
            range,
            ..
        } => {
            assert_registered_kind(kind);
            assert_eq!(file_id, &id);
            assert_range(range, source, "local()");
        }
        _ => unreachable!("the match above selects only call records"),
    }

    let binding = records
        .iter()
        .find(|record| {
            matches!(
                record,
                NativeSyntaxRecord::Binding { local_name, .. } if local_name == "local"
            )
        })
        .expect("one named static-import binding for local");
    match binding {
        NativeSyntaxRecord::Binding {
            kind,
            file_id,
            range,
            import_kind,
            export_slot,
            specifier,
            specifier_range,
            ..
        } => {
            assert_registered_kind(kind);
            assert_eq!(file_id, &id);
            assert_range(range, source, "remote as local");
            assert_eq!(*import_kind, PayloadImportKindV1::Named);
            assert_eq!(
                export_slot
                    .as_ref()
                    .expect("named import observes its source export slot")
                    .as_str(),
                "remote"
            );
            assert_eq!(specifier, "./dep");
            assert_range(specifier_range, source, "\"./dep\"");
        }
        _ => unreachable!("the match above selects only binding records"),
    }

    let surface = records
        .iter()
        .find(|record| {
            matches!(
                record,
                NativeSyntaxRecord::Surface { export_slots, .. }
                    if export_slots.iter().any(|slot| slot == "api")
            )
        })
        .expect("one same-file export-list surface for api");
    match surface {
        NativeSyntaxRecord::Surface {
            kind,
            file_id,
            range,
            export_slots,
            local_names,
            target_specifier,
            ..
        } => {
            assert_registered_kind(kind);
            assert_eq!(file_id, &id);
            assert_range(range, source, "export { run as api };");
            assert_eq!(
                export_slots
                    .iter()
                    .map(|slot| slot.as_str())
                    .collect::<Vec<_>>(),
                vec!["api"]
            );
            assert_eq!(local_names, &vec!["run".to_owned()]);
            assert!(target_specifier.is_none());
        }
        _ => unreachable!("the match above selects only surface records"),
    }

    let scope = records
        .iter()
        .find(|record| matches!(record, NativeSyntaxRecord::Scope { .. }))
        .expect("one file-lexical scope record");
    match scope {
        NativeSyntaxRecord::Scope {
            kind,
            file_id,
            range,
            scope_kind,
            member_ranges,
            ..
        } => {
            assert_registered_kind(kind);
            assert_eq!(file_id, &id);
            assert_range(range, source, source);
            assert_eq!(*scope_kind, ScopeKindV1::FileLexical);
            assert_eq!(member_ranges.len(), 3);
            for member in [
                "import { remote as local } from \"./dep\";",
                "export const run: () => number = () => local();",
                "export { run as api };",
            ] {
                assert!(
                    contains_range(member_ranges, source, member),
                    "file scope retains member range: {member}"
                );
            }
        }
        _ => unreachable!("the match above selects only scope records"),
    }

    // I1 remains its own public catalogue. The same V0 fixture still has one
    // public direct-const callable and one containment entry.
    let legacy = parse_typescript("src/v0.ts", source.as_bytes());
    assert!(legacy.is_parsed());
    assert_eq!(legacy.public_callables().count(), 1);
    assert!(legacy.callables.iter().any(|callable| {
        callable.binding_key.as_deref() == Some("run")
            && callable.outcome == CallableOutcome::EligiblePublic
    }));
    assert_eq!(
        legacy
            .contains
            .iter()
            .map(|contains| contains.binding_key.as_str())
            .collect::<Vec<_>>(),
        vec!["run"]
    );
}

#[test]
fn v0_non_call_reference_does_not_create_a_call_record() {
    // F2-C10: `const ref=f` is a reference, not a call occurrence.
    let source = "export function f(){return 1}\nconst ref = f;\n";
    let records = collect_native_syntax(file_id("src/reference.ts"), source.as_bytes());

    assert!(
        records
            .iter()
            .all(|record| !matches!(record, NativeSyntaxRecord::Call { .. }))
    );
}

#[test]
fn v0_parse_failed_file_produces_no_native_records() {
    // The design's parse-failure rule: no callable/contains/call facts are
    // accepted; V0 likewise has no source-native record to project.
    let records = collect_native_syntax(file_id("src/broken.ts"), b"export function {");
    assert!(records.is_empty());
}
