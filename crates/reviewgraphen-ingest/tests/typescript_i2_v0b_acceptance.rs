//! V0b acceptance for native TypeScript syntax forms omitted by V0.
//!
//! These assertions use only one-file source observations. They intentionally
//! do not assert opaque registered `kind` literals or record-vector order.

use reviewgraphen_core::source_review::ids::{CanonicalFileKey, SourceFileId, SourceRange};
use reviewgraphen_ingest::typescript::{
    native_syntax::{NativeSyntaxRecord, collect_native_syntax},
    payload::{PayloadImportKindV1, ScopeKindV1, VisibilityValueV1},
    syntax::parse_typescript,
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

fn assert_catalog_counts(records: &[NativeSyntaxRecord], expected: [usize; 5]) {
    let actual = records.iter().fold([0_usize; 5], |mut counts, record| {
        match record {
            NativeSyntaxRecord::Callable { .. } => counts[0] += 1,
            NativeSyntaxRecord::Call { .. } => counts[1] += 1,
            NativeSyntaxRecord::Binding { .. } => counts[2] += 1,
            NativeSyntaxRecord::Surface { .. } => counts[3] += 1,
            NativeSyntaxRecord::Scope { .. } => counts[4] += 1,
        }
        counts
    });
    assert_eq!(
        actual, expected,
        "callable/call/binding/surface/scope counts"
    );
    assert_eq!(
        records.len(),
        expected.iter().sum::<usize>(),
        "total record count"
    );
}

fn assert_all_file_ids(records: &[NativeSyntaxRecord], expected: &SourceFileId) {
    for record in records {
        let actual = match record {
            NativeSyntaxRecord::Callable { file_id, .. }
            | NativeSyntaxRecord::Call { file_id, .. }
            | NativeSyntaxRecord::Binding { file_id, .. }
            | NativeSyntaxRecord::Surface { file_id, .. }
            | NativeSyntaxRecord::Scope { file_id, .. } => file_id,
        };
        assert_eq!(
            actual, expected,
            "each record retains the fixture file identity"
        );
    }
}

fn assert_callable(
    records: &[NativeSyntaxRecord],
    source: &str,
    expected_binding_key: Option<&str>,
    declaration_source: &str,
    implementation_source: &str,
    expected_visibility: VisibilityValueV1,
) {
    let matches = records
        .iter()
        .filter(|record| {
            matches!(
                record,
                NativeSyntaxRecord::Callable { binding_key, .. }
                    if binding_key.as_deref() == expected_binding_key
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "one callable record for binding {expected_binding_key:?}"
    );

    match matches[0] {
        NativeSyntaxRecord::Callable {
            range,
            binding_key,
            implementation_range,
            visibility,
            ..
        } => {
            assert_eq!(binding_key.as_deref(), expected_binding_key);
            assert_range(range, source, declaration_source);
            assert_range(
                implementation_range
                    .as_ref()
                    .expect("fixture callable has an implementation"),
                source,
                implementation_source,
            );
            assert_eq!(*visibility, expected_visibility);
        }
        _ => unreachable!("the filter selects only callable records"),
    }
}

fn assert_callable_by_range(
    records: &[NativeSyntaxRecord],
    source: &str,
    declaration_source: &str,
    implementation_source: &str,
    expected_visibility: VisibilityValueV1,
) {
    let (start, end) = unique_span(source, declaration_source);
    let matches = records
        .iter()
        .filter(|record| {
            matches!(
                record,
                NativeSyntaxRecord::Callable { range, .. }
                    if range.start() == start && range.end() == end
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "one callable record for {declaration_source}"
    );

    match matches[0] {
        NativeSyntaxRecord::Callable {
            range,
            implementation_range,
            visibility,
            ..
        } => {
            assert_range(range, source, declaration_source);
            assert_range(
                implementation_range
                    .as_ref()
                    .expect("fixture callable has an implementation"),
                source,
                implementation_source,
            );
            assert_eq!(*visibility, expected_visibility);
        }
        _ => unreachable!("the filter selects only callable records"),
    }
}

fn assert_no_callable_named(records: &[NativeSyntaxRecord], name: &str) {
    assert!(
        records.iter().all(|record| {
            !matches!(
                record,
                NativeSyntaxRecord::Callable {
                    binding_key: Some(binding_key),
                    ..
                } if binding_key == name
            )
        }),
        "no top-level callable record is created for excluded {name}"
    );
}

#[allow(clippy::too_many_arguments)]
fn assert_binding(
    records: &[NativeSyntaxRecord],
    source: &str,
    local_name: &str,
    expected_kind: PayloadImportKindV1,
    expected_slot: Option<&str>,
    binding_source: &str,
    expected_specifier: &str,
    specifier_source: &str,
) {
    let matches = records
        .iter()
        .filter(|record| {
            matches!(
                record,
                NativeSyntaxRecord::Binding { local_name: actual, .. } if actual == local_name
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "one binding record for {local_name}");

    match matches[0] {
        NativeSyntaxRecord::Binding {
            range,
            local_name: actual_name,
            import_kind,
            export_slot,
            specifier,
            specifier_range,
            ..
        } => {
            assert_eq!(actual_name, local_name);
            assert_eq!(*import_kind, expected_kind);
            assert_eq!(export_slot.as_deref(), expected_slot);
            assert_eq!(specifier, expected_specifier);
            assert_range(range, source, binding_source);
            assert_range(specifier_range, source, specifier_source);
        }
        _ => unreachable!("the filter selects only binding records"),
    }
}

fn assert_surface(
    records: &[NativeSyntaxRecord],
    source: &str,
    surface_source: &str,
    expected_slots: &[&str],
    expected_local_names: &[&str],
    expected_target: Option<&str>,
) {
    let matches = records
        .iter()
        .filter(|record| {
            matches!(
                record,
                NativeSyntaxRecord::Surface { export_slots, .. }
                    if export_slots.iter().map(String::as_str).eq(expected_slots.iter().copied())
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "one surface record for {expected_slots:?}"
    );

    match matches[0] {
        NativeSyntaxRecord::Surface {
            range,
            export_slots,
            local_names,
            target_specifier,
            ..
        } => {
            assert_range(range, source, surface_source);
            assert!(
                export_slots
                    .iter()
                    .map(String::as_str)
                    .eq(expected_slots.iter().copied())
            );
            assert!(
                local_names
                    .iter()
                    .map(String::as_str)
                    .eq(expected_local_names.iter().copied())
            );
            assert_eq!(target_specifier.as_deref(), expected_target);
        }
        _ => unreachable!("the filter selects only surface records"),
    }
}

fn assert_no_surface_slot(records: &[NativeSyntaxRecord], slot: &str) {
    assert!(
        records.iter().all(|record| {
            !matches!(
                record,
                NativeSyntaxRecord::Surface { export_slots, .. }
                    if export_slots.iter().any(|actual| actual == slot)
            )
        }),
        "no export surface slot is created for excluded {slot}"
    );
}

fn assert_callable_not_exported(records: &[NativeSyntaxRecord], binding_key: &str) {
    let matches = records
        .iter()
        .filter(|record| {
            matches!(
                record,
                NativeSyntaxRecord::Callable { binding_key: Some(actual), .. }
                    if actual == binding_key
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "one callable record for {binding_key}");
    match matches[0] {
        NativeSyntaxRecord::Callable { visibility, .. } => assert_ne!(
            *visibility,
            VisibilityValueV1::Exported,
            "a type-only export does not make {binding_key} runtime-exported"
        ),
        _ => unreachable!("the filter selects only callable records"),
    }
}

fn assert_named_default_fixture(records: &[NativeSyntaxRecord], source: &str, id: &SourceFileId) {
    assert_catalog_counts(records, [1, 0, 0, 1, 1]);
    assert_all_file_ids(records, id);
    assert_callable(
        records,
        source,
        Some("f"),
        "function f(){}",
        "{}",
        VisibilityValueV1::Exported,
    );
    assert_surface(
        records,
        source,
        source.trim_end(),
        &["default"],
        &["f"],
        None,
    );
    assert_no_surface_slot(records, "f");
    assert_file_scope(records, source, &[source.trim_end()]);
}

fn assert_type_only_import_fixture(records: &[NativeSyntaxRecord], source: &str, local_name: &str) {
    assert_catalog_counts(records, [0, 0, 1, 0, 1]);
    assert_binding(
        records,
        source,
        local_name,
        PayloadImportKindV1::TypeOnly,
        Some(local_name),
        local_name,
        "./x",
        "\"./x\"",
    );
    assert_file_scope(records, source, &[source.trim_end()]);
}

fn assert_direct_noncallable_surface(records: &[NativeSyntaxRecord], source: &str) {
    for (surface_source, slot) in [
        ("export class C {}", "C"),
        ("export var x = 1;", "x"),
        ("export enum E { A }", "E"),
        ("export interface I {}", "I"),
    ] {
        assert_surface(records, source, surface_source, &[slot], &[slot], None);
    }
}

fn assert_namespace_export_fixture(
    records: &[NativeSyntaxRecord],
    source: &str,
    namespace: &str,
    grammar_has_export_statement: bool,
) {
    if grammar_has_export_statement {
        assert_catalog_counts(records, [0, 0, 0, 1, 1]);
        assert_surface(
            records,
            source,
            source.trim_end(),
            &[namespace],
            &[namespace],
            None,
        );
        assert_file_scope(records, source, &[source.trim_end()]);
    } else {
        // The pinned grammar splits this into an `export` statement and a
        // namespace statement, not an export_statement.  A source-native
        // surface must follow that parse tree rather than TypeScript intent.
        assert_catalog_counts(records, [0, 0, 0, 0, 1]);
    }
}

fn contains_range(ranges: &[SourceRange], source: &str, node_source: &str) -> bool {
    let (start, end) = unique_span(source, node_source);
    ranges
        .iter()
        .any(|range| range.start() == start && range.end() == end)
}

fn assert_file_scope(records: &[NativeSyntaxRecord], source: &str, members: &[&str]) {
    let scopes = records
        .iter()
        .filter(|record| matches!(record, NativeSyntaxRecord::Scope { .. }))
        .collect::<Vec<_>>();
    assert_eq!(scopes.len(), 1, "one file-lexical scope record");

    match scopes[0] {
        NativeSyntaxRecord::Scope {
            range,
            scope_kind,
            member_ranges,
            ..
        } => {
            assert_range(range, source, source);
            assert_eq!(*scope_kind, ScopeKindV1::FileLexical);
            assert_eq!(member_ranges.len(), members.len(), "scope member count");
            for member in members {
                assert!(
                    contains_range(member_ranges, source, member),
                    "file scope retains member range: {member}"
                );
            }
        }
        _ => unreachable!("the filter selects only scope records"),
    }
}

#[test]
fn v0b_top_level_catalogue_excludes_nested_callables_and_namespace_members() {
    // Nested forms are outside the top-level
    // free-function denominator; direct exports still have surface records.
    let source = "export function outer(){ const inner = () => 1; function g(){} }\n\
export namespace N { export const f = () => 1 }\n";
    let id = file_id("src/v0b-nested.ts");
    let records = collect_native_syntax(id.clone(), source.as_bytes());

    assert_catalog_counts(&records, [1, 0, 0, 2, 1]);
    assert_all_file_ids(&records, &id);
    assert_callable(
        &records,
        source,
        Some("outer"),
        "function outer(){ const inner = () => 1; function g(){} }",
        "{ const inner = () => 1; function g(){} }",
        VisibilityValueV1::Exported,
    );
    for excluded in ["inner", "g", "f"] {
        assert_no_callable_named(&records, excluded);
    }
    assert_surface(
        &records,
        source,
        "export function outer(){ const inner = () => 1; function g(){} }",
        &["outer"],
        &["outer"],
        None,
    );
    assert_surface(
        &records,
        source,
        "export namespace N { export const f = () => 1 }",
        &["N"],
        &["N"],
        None,
    );
    assert_file_scope(
        &records,
        source,
        &[
            "export function outer(){ const inner = () => 1; function g(){} }",
            "export namespace N { export const f = () => 1 }",
        ],
    );
}

#[test]
fn v0b_same_file_export_list_and_default_name_make_callable_exported() {
    // Both same-file forms are visibility witnesses.
    let source = "function f() {}\nexport { f };\nexport default f;\n";
    let id = file_id("src/v0b-export-list.ts");
    let records = collect_native_syntax(id.clone(), source.as_bytes());

    assert_catalog_counts(&records, [1, 0, 0, 2, 1]);
    assert_all_file_ids(&records, &id);
    assert_callable(
        &records,
        source,
        Some("f"),
        "function f() {}",
        "{}",
        VisibilityValueV1::Exported,
    );
    assert_surface(&records, source, "export { f };", &["f"], &["f"], None);
    assert_surface(
        &records,
        source,
        "export default f;",
        &["default"],
        &["f"],
        None,
    );
    assert_file_scope(
        &records,
        source,
        &["function f() {}", "export { f };", "export default f;"],
    );
}

#[test]
fn v0b_anonymous_default_function_and_arrow_have_default_implementation_ranges() {
    // The callable range is the default implementation
    // node itself; the source has no local spelling to synthesize as a binding key.
    let function_source = "export default function() {}\n";
    let function_id = file_id("src/v0b-anonymous-function.ts");
    let function_records = collect_native_syntax(function_id.clone(), function_source.as_bytes());

    assert_catalog_counts(&function_records, [1, 0, 0, 1, 1]);
    assert_all_file_ids(&function_records, &function_id);
    assert_callable_by_range(
        &function_records,
        function_source,
        "function() {}",
        "{}",
        VisibilityValueV1::Exported,
    );
    assert_surface(
        &function_records,
        function_source,
        "export default function() {}",
        &["default"],
        &[],
        None,
    );
    assert_file_scope(
        &function_records,
        function_source,
        &["export default function() {}"],
    );

    let arrow_source = "export default () => 1;\n";
    let arrow_id = file_id("src/v0b-anonymous-arrow.ts");
    let arrow_records = collect_native_syntax(arrow_id.clone(), arrow_source.as_bytes());

    assert_catalog_counts(&arrow_records, [1, 0, 0, 1, 1]);
    assert_all_file_ids(&arrow_records, &arrow_id);
    assert_callable_by_range(
        &arrow_records,
        arrow_source,
        "() => 1",
        "() => 1",
        VisibilityValueV1::Exported,
    );
    assert_surface(
        &arrow_records,
        arrow_source,
        "export default () => 1;",
        &["default"],
        &[],
        None,
    );
    assert_file_scope(&arrow_records, arrow_source, &["export default () => 1;"]);
}

#[test]
fn v0b_type_only_import_forms_are_distinct_bindings() {
    // Both syntactic forms are type-only bindings.
    let clause_source = "import type { A } from \"./x\";\n";
    let clause_id = file_id("src/v0b-import-type-clause.ts");
    let clause_records = collect_native_syntax(clause_id.clone(), clause_source.as_bytes());

    assert_catalog_counts(&clause_records, [0, 0, 1, 0, 1]);
    assert_all_file_ids(&clause_records, &clause_id);
    assert_binding(
        &clause_records,
        clause_source,
        "A",
        PayloadImportKindV1::TypeOnly,
        Some("A"),
        "A",
        "./x",
        "\"./x\"",
    );
    assert_file_scope(
        &clause_records,
        clause_source,
        &["import type { A } from \"./x\";"],
    );

    let specifier_source = "import { type A } from \"./x\";\n";
    let specifier_id = file_id("src/v0b-import-type-specifier.ts");
    let specifier_records =
        collect_native_syntax(specifier_id.clone(), specifier_source.as_bytes());

    assert_catalog_counts(&specifier_records, [0, 0, 1, 0, 1]);
    assert_all_file_ids(&specifier_records, &specifier_id);
    assert_binding(
        &specifier_records,
        specifier_source,
        "A",
        PayloadImportKindV1::TypeOnly,
        Some("A"),
        "type A",
        "./x",
        "\"./x\"",
    );
    assert_file_scope(
        &specifier_records,
        specifier_source,
        &["import { type A } from \"./x\";"],
    );
}

#[test]
fn v0b_default_and_namespace_imports_keep_their_binding_forms() {
    // A default import observes default; a namespace
    // import has no single export slot and must remain distinct.
    let source = "import dflt from \"./default\";\nimport * as ns from \"./namespace\";\n";
    let id = file_id("src/v0b-import-forms.ts");
    let records = collect_native_syntax(id.clone(), source.as_bytes());

    assert_catalog_counts(&records, [0, 0, 2, 0, 1]);
    assert_all_file_ids(&records, &id);
    assert_binding(
        &records,
        source,
        "dflt",
        PayloadImportKindV1::Default,
        Some("default"),
        "dflt",
        "./default",
        "\"./default\"",
    );
    assert_binding(
        &records,
        source,
        "ns",
        PayloadImportKindV1::Namespace,
        None,
        "* as ns",
        "./namespace",
        "\"./namespace\"",
    );
    assert_file_scope(
        &records,
        source,
        &[
            "import dflt from \"./default\";",
            "import * as ns from \"./namespace\";",
        ],
    );
}

#[test]
fn v0b_direct_exports_have_complete_surface_slots_and_local_names() {
    // The design keeps export slots, local names, and
    // re-export target separate rather than treating surface as a callable.
    let source = "export const run = () => 1;\n\
export function f() {}\n\
export default () => 2;\n\
export * as ns from \"./x\";\n";
    let id = file_id("src/v0b-direct-surfaces.ts");
    let records = collect_native_syntax(id.clone(), source.as_bytes());

    assert_catalog_counts(&records, [3, 0, 0, 4, 1]);
    assert_all_file_ids(&records, &id);
    assert_callable(
        &records,
        source,
        Some("run"),
        "run = () => 1",
        "() => 1",
        VisibilityValueV1::Exported,
    );
    assert_callable(
        &records,
        source,
        Some("f"),
        "function f() {}",
        "{}",
        VisibilityValueV1::Exported,
    );
    assert_callable_by_range(
        &records,
        source,
        "() => 2",
        "() => 2",
        VisibilityValueV1::Exported,
    );
    assert_surface(
        &records,
        source,
        "export const run = () => 1;",
        &["run"],
        &["run"],
        None,
    );
    assert_surface(
        &records,
        source,
        "export function f() {}",
        &["f"],
        &["f"],
        None,
    );
    assert_surface(
        &records,
        source,
        "export default () => 2;",
        &["default"],
        &[],
        None,
    );
    assert_surface(
        &records,
        source,
        "export * as ns from \"./x\";",
        &["ns"],
        &[],
        Some("./x"),
    );
    assert_file_scope(
        &records,
        source,
        &[
            "export const run = () => 1;",
            "export function f() {}",
            "export default () => 2;",
            "export * as ns from \"./x\";",
        ],
    );
}

#[test]
fn v0b_function_and_generator_callables_have_bodies_and_exclusions_leave_no_surplus() {
    // The design fixes declaration/body ranges.
    let source = "export function f(){ return 1; }\n\
export function* gen(){ yield 1; }\n\
function hidden(){ return 0; }\n\
let later = () => 2;\n\
var old = function() {};\n\
const value = 1;\n";
    let id = file_id("src/v0b-functions.ts");
    let records = collect_native_syntax(id.clone(), source.as_bytes());

    assert_catalog_counts(&records, [3, 0, 0, 2, 1]);
    assert_all_file_ids(&records, &id);
    assert_callable(
        &records,
        source,
        Some("f"),
        "function f(){ return 1; }",
        "{ return 1; }",
        VisibilityValueV1::Exported,
    );
    assert_callable(
        &records,
        source,
        Some("gen"),
        "function* gen(){ yield 1; }",
        "{ yield 1; }",
        VisibilityValueV1::Exported,
    );
    assert_callable(
        &records,
        source,
        Some("hidden"),
        "function hidden(){ return 0; }",
        "{ return 0; }",
        VisibilityValueV1::NonExported,
    );
    for excluded in ["later", "old", "value"] {
        assert_no_callable_named(&records, excluded);
    }
    assert_surface(
        &records,
        source,
        "export function f(){ return 1; }",
        &["f"],
        &["f"],
        None,
    );
    assert_surface(
        &records,
        source,
        "export function* gen(){ yield 1; }",
        &["gen"],
        &["gen"],
        None,
    );
    assert_file_scope(
        &records,
        source,
        &[
            "export function f(){ return 1; }",
            "export function* gen(){ yield 1; }",
            "function hidden(){ return 0; }",
            "let later = () => 2;",
            "var old = function() {};",
            "const value = 1;",
        ],
    );
}

#[test]
fn v0b_export_list_name_binds_only_the_top_level_declaration() {
    // A same-file export list binds the single top-level
    // callable; a nested declaration with the same name is not a callable record.
    let source = "function f() { function f() {} }\nexport { f };\n";
    let id = file_id("src/v0b-nested-export-list.ts");
    let records = collect_native_syntax(id.clone(), source.as_bytes());

    assert_catalog_counts(&records, [1, 0, 0, 1, 1]);
    assert_all_file_ids(&records, &id);
    assert_callable(
        &records,
        source,
        Some("f"),
        "function f() { function f() {} }",
        "{ function f() {} }",
        VisibilityValueV1::Exported,
    );
    assert_surface(&records, source, "export { f };", &["f"], &["f"], None);
    assert_file_scope(
        &records,
        source,
        &["function f() { function f() {} }", "export { f };"],
    );
}

#[test]
fn v0b_mixed_type_and_value_named_import_keeps_each_binding_form() {
    // In one import clause, the specifier marked type is
    // type_only and the unmarked specifier stays named.
    let source = "import { type A, B } from \"./x\";\n";
    let id = file_id("src/v0b-mixed-import.ts");
    let records = collect_native_syntax(id.clone(), source.as_bytes());

    assert_catalog_counts(&records, [0, 0, 2, 0, 1]);
    assert_all_file_ids(&records, &id);
    assert_binding(
        &records,
        source,
        "A",
        PayloadImportKindV1::TypeOnly,
        Some("A"),
        "type A",
        "./x",
        "\"./x\"",
    );
    assert_binding(
        &records,
        source,
        "B",
        PayloadImportKindV1::Named,
        Some("B"),
        "B",
        "./x",
        "\"./x\"",
    );
    assert_file_scope(&records, source, &["import { type A, B } from \"./x\";"]);
}

#[test]
fn v0bfix_default_export_prefix_variations_keep_only_the_default_slot() {
    // Whitespace and comments are trivia, not an
    // alternate named-export spelling for a named default declaration.
    let fixtures = [
        ("one-space", "export default function f(){}\n"),
        ("two-spaces", "export default  function f(){}\n"),
        ("newline", "export default\nfunction f(){}\n"),
        ("tab", "export default\tfunction f(){}\n"),
        (
            "block-comment",
            "export default /* note */ function f(){}\n",
        ),
        ("line-comment", "export default // note\nfunction f(){}\n"),
    ];

    for (label, source) in fixtures {
        let id = file_id(&format!("src/v0bfix-default-{label}.ts"));
        let records = collect_native_syntax(id.clone(), source.as_bytes());

        assert_named_default_fixture(&records, source, &id);
    }
}

#[test]
fn v0bfix_anonymous_default_generator_is_a_callable_record() {
    // Generator spelling is a callable default
    // implementation, even though it has no local name or named export slot.
    let source = "export default function*(){}\n";
    let id = file_id("src/v0bfix-anonymous-default-generator.ts");
    let records = collect_native_syntax(id.clone(), source.as_bytes());

    assert_catalog_counts(&records, [1, 0, 0, 1, 1]);
    assert_all_file_ids(&records, &id);
    assert_callable_by_range(
        &records,
        source,
        "function*(){}",
        "{}",
        VisibilityValueV1::Exported,
    );
    assert_surface(
        &records,
        source,
        "export default function*(){}",
        &["default"],
        &[],
        None,
    );
    assert_file_scope(&records, source, &["export default function*(){}"]);
}

#[test]
fn v0bfix_import_type_prefix_variations_keep_type_and_value_bindings_distinct() {
    // `type` is a keyword only in the syntactic positions
    // shown below; a local/default name or imported slot spelled type is a value.
    let value_default = "import type from \"./x\";\n";
    let value_default_records = collect_native_syntax(
        file_id("src/v0bfix-import-local-type.ts"),
        value_default.as_bytes(),
    );
    assert_catalog_counts(&value_default_records, [0, 0, 1, 0, 1]);
    assert_binding(
        &value_default_records,
        value_default,
        "type",
        PayloadImportKindV1::Default,
        Some("default"),
        "type",
        "./x",
        "\"./x\"",
    );

    let value_slot = "import { type as t } from \"./x\";\n";
    let value_slot_records = collect_native_syntax(
        file_id("src/v0bfix-import-slot-type.ts"),
        value_slot.as_bytes(),
    );
    assert_catalog_counts(&value_slot_records, [0, 0, 1, 0, 1]);
    assert_binding(
        &value_slot_records,
        value_slot,
        "t",
        PayloadImportKindV1::Named,
        Some("type"),
        "type as t",
        "./x",
        "\"./x\"",
    );

    let type_only_fixtures = [
        ("one-space", "A", "import type { A } from \"./x\";\n"),
        ("two-spaces", "B", "import  type { B } from \"./x\";\n"),
        ("newline", "C", "import\ntype { C } from \"./x\";\n"),
        ("tab", "D", "import\ttype { D } from \"./x\";\n"),
        (
            "block-comment",
            "E",
            "import /* note */ type { E } from \"./x\";\n",
        ),
        (
            "line-comment",
            "F",
            "import // note\ntype { F } from \"./x\";\n",
        ),
    ];
    for (label, local_name, source) in type_only_fixtures {
        let records = collect_native_syntax(
            file_id(&format!("src/v0bfix-import-type-{label}.ts")),
            source.as_bytes(),
        );
        assert_type_only_import_fixture(&records, source, local_name);
    }
}

#[test]
fn v0bfix_type_only_export_prefix_variations_do_not_export_a_callable() {
    // Type-only surfaces are not runtime callable exports; this
    // checks both export-list spellings and the same trivia variations.
    let fixtures = [
        ("type-specifier", "function f(){}\nexport { type f };\n"),
        ("one-space", "function f(){}\nexport type { f };\n"),
        ("two-spaces", "function f(){}\nexport  type { f };\n"),
        ("tab", "function f(){}\nexport\ttype { f };\n"),
        (
            "block-comment",
            "function f(){}\nexport /* note */ type { f };\n",
        ),
    ];
    for (label, source) in fixtures {
        let records = collect_native_syntax(
            file_id(&format!("src/v0bfix-type-export-{label}.ts")),
            source.as_bytes(),
        );
        assert_callable_not_exported(&records, "f");
        assert_file_scope(
            &records,
            source,
            &[
                "function f(){}",
                &source["function f(){}\n".len()..source.len() - 1],
            ],
        );
    }
}

#[test]
fn v0bfix_export_type_line_break_forms_are_parse_failed() {
    // The type-only rule only applies once the source has a parse tree. These two
    // negative fixtures distinguish trivia that TypeScript grammar admits from
    // a line break that terminates the export declaration before `type`.
    for negative_source in [
        "function f(){}\nexport\ntype { f };\n",
        "function f(){}\nexport // note\ntype { f };\n",
    ] {
        assert!(
            !parse_typescript(
                "src/v0bfix-invalid-export-type.ts",
                negative_source.as_bytes()
            )
            .is_parsed(),
            "the negative export-type spelling has a parse error"
        );
    }
}

#[test]
fn v0bfix_direct_noncallable_exports_have_surface_slots() {
    // Surfaces retain direct export slots even when
    // the declaration itself is not a top-level free-function callable.
    let source = "export class C {}\n\
export var x = 1;\n\
export enum E { A }\n\
export interface I {}\n";
    let id = file_id("src/v0bfix-direct-noncallable-exports.ts");
    let records = collect_native_syntax(id.clone(), source.as_bytes());

    assert_catalog_counts(&records, [0, 0, 0, 4, 1]);
    assert_all_file_ids(&records, &id);
    assert_direct_noncallable_surface(&records, source);
    assert_file_scope(
        &records,
        source,
        &[
            "export class C {}",
            "export var x = 1;",
            "export enum E { A }",
            "export interface I {}",
        ],
    );
}

#[test]
fn v0bfix_star_reexports_distinguish_an_alias_slot_from_no_slot() {
    // A star re-export has an unresolved target surface;
    // only the namespace-alias spelling carries the alias export slot.
    let source = "export * from \"./x\";\nexport * as ns from \"./x\";\n";
    let id = file_id("src/v0bfix-star-reexports.ts");
    let records = collect_native_syntax(id.clone(), source.as_bytes());

    assert_catalog_counts(&records, [0, 0, 0, 2, 1]);
    assert_all_file_ids(&records, &id);
    assert_surface(
        &records,
        source,
        "export * from \"./x\";",
        &[],
        &[],
        Some("./x"),
    );
    assert_surface(
        &records,
        source,
        "export * as ns from \"./x\";",
        &["ns"],
        &[],
        Some("./x"),
    );
    assert_file_scope(
        &records,
        source,
        &["export * from \"./x\";", "export * as ns from \"./x\";"],
    );
}

#[test]
fn v0bfix_namespace_export_prefix_variations_keep_the_namespace_surface() {
    // Namespace content is not a free-function callable.
    // Its direct surface is observable only where the pinned grammar creates
    // an export_statement; grammar-split trivia has scope alone.
    let fixtures = [
        ("one-space", "N1", "export namespace N1 {}\n", true),
        ("two-spaces", "N2", "export  namespace N2 {}\n", true),
        ("newline", "N3", "export\nnamespace N3 {}\n", false),
        ("tab", "N4", "export\tnamespace N4 {}\n", true),
        (
            "block-comment",
            "N5",
            "export /* note */ namespace N5 {}\n",
            true,
        ),
        (
            "line-comment",
            "N6",
            "export // note\nnamespace N6 {}\n",
            false,
        ),
    ];
    for (label, namespace, source, grammar_has_export_statement) in fixtures {
        let records = collect_native_syntax(
            file_id(&format!("src/v0bfix-namespace-{label}.ts")),
            source.as_bytes(),
        );
        assert_namespace_export_fixture(&records, source, namespace, grammar_has_export_statement);
    }
}
