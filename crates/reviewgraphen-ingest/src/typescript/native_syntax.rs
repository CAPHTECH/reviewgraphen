//! Source-native TypeScript syntax cataloguing for the I2 V0 slice.
//!
//! This module observes syntax only. It does not admit facts, mint authority,
//! or construct `SourceValidated` values.

use reviewgraphen_core::source_review::ids::{SourceFileId, SourceRange};
use reviewgraphen_core::source_review::reasons::TypeScriptSyntaxKind;
use tree_sitter::{Node, Parser};

use super::payload::{PayloadImportKindV1, ScopeKindV1, VisibilityValueV1};

/// One source-native syntax record observed in a parsed TypeScript file.
///
/// Each role is a separate variant, so a record cannot carry another role's
/// source clues. `kind`, `file_id`, and `range` deliberately repeat in every
/// variant rather than adding a public common struct: all records require the
/// registered kind and exact file/range,
/// while the variant itself is the required role.
///
/// Every range is UTF-8 byte half-open. An `Option`
/// is required-and-nullable in the eventual wire record; it never means an
/// inferred name, visibility, resolution, or accepted fact.
#[derive(Clone, Debug, Eq, PartialEq)]
/// Boundary: each record holds only unresolved clues observable from one file's
/// parse tree. It carries no resolved values (caller, callee, or declaration IDs,
/// candidates, resolution kinds, evaluation stages, or export-slot resolution);
/// later slices' payloads own resolution results.
/// Binding keys and export slots are the source spellings observed in this file.
/// Registry-bound binding-key and export-slot types belong to later payload
/// slices (A2 to A4) and the registry literal freeze step.
pub enum NativeSyntaxRecord {
    /// A callable source form.
    ///
    /// `range` is the required declaration range. For a
    /// direct `const` callable, it is the entire `variable_declarator`,
    /// including the binding, annotation, and initializer; it excludes sibling
    /// declarators and the shared `const`/`export` prefix
    ///.
    Callable {
        /// Required opaque registered TypeScript syntax kind.
        kind: TypeScriptSyntaxKind,
        /// Required identity of the exact source file.
        file_id: SourceFileId,
        /// Required primary AST-node range in the exact input bytes.
        range: SourceRange,
        /// Required-and-nullable binding key. An unsupported unnamed candidate
        /// has no binding key.
        binding_key: Option<String>,
        /// Required-and-nullable implementation range; declarations without an
        /// implementation retain `None` rather than synthesizing a body
        ///.
        implementation_range: Option<SourceRange>,
        /// Required source visibility observation; `Unknown` is distinct from
        /// `NonExported`.
        visibility: VisibilityValueV1,
    },
    /// A call source form.
    Call {
        /// Required opaque registered TypeScript syntax kind.
        kind: TypeScriptSyntaxKind,
        /// Required identity of the exact source file.
        file_id: SourceFileId,
        /// Required callsite AST-node range in the exact input bytes.
        range: SourceRange,
        /// Required-and-nullable import binding key.
        binding_key: Option<String>,
    },
    /// A static-import binding source form.
    Binding {
        /// Required opaque registered TypeScript syntax kind.
        kind: TypeScriptSyntaxKind,
        /// Required identity of the exact source file.
        file_id: SourceFileId,
        /// Required import AST-node range in the exact input bytes.
        range: SourceRange,
        /// Required local import spelling.
        local_name: String,
        /// Required static-import form.
        import_kind: PayloadImportKindV1,
        /// Required-and-nullable export slot.
        export_slot: Option<String>,
        /// Required original module-specifier spelling and its exact source
        /// range.
        specifier: String,
        specifier_range: SourceRange,
    },
    /// An export-surface source form.
    Surface {
        /// Required opaque registered TypeScript syntax kind.
        kind: TypeScriptSyntaxKind,
        /// Required identity of the exact source file.
        file_id: SourceFileId,
        /// Required export AST-node range in the exact input bytes.
        range: SourceRange,
        /// Required export-slot sequence, which may be empty.
        export_slots: Vec<String>,
        /// Required local-name sequence, separate from slots and possibly
        /// empty.
        local_names: Vec<String>,
        /// Required-and-nullable re-export target module specifier.
        target_specifier: Option<String>,
    },
    /// The sole file-lexical scope source form.
    Scope {
        /// Required opaque registered TypeScript syntax kind.
        kind: TypeScriptSyntaxKind,
        /// Required identity of the exact source file.
        file_id: SourceFileId,
        /// Required file-lexical AST-node range in the exact input bytes.
        range: SourceRange,
        /// Required first-cohort file-lexical scope kind.
        scope_kind: ScopeKindV1,
        /// Required member syntax-key sequence, which may be empty.
        /// Byte ranges of the member declarations observed in the sole
        /// file-lexical scope. Ranges, not registry-bound syntax keys:
        /// keys depend on the snapshot and registry binding, not on this file alone.
        member_ranges: Vec<SourceRange>,
    },
}

/// Collects all five roles of source-native syntax from one parsed `.ts` file.
///
/// The implementation must use the same Tree-sitter TypeScript parser policy
/// as [`super::syntax::parse_typescript`]. If that parser rejects the bytes
/// (including `ERROR` or `MISSING` nodes), this function returns no records.
/// It consumes only the supplied exact bytes and file identity: it neither
/// returns `SourceValidated` nor confers admission authority.
#[must_use]
pub fn collect_native_syntax(file_id: SourceFileId, source: &[u8]) -> Vec<NativeSyntaxRecord> {
    let Ok(text) = std::str::from_utf8(source) else {
        return Vec::new();
    };
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let root = tree.root_node();
    if root.has_error() || has_missing(root) {
        return Vec::new();
    }

    let exported_bindings = same_file_exported_bindings(root, text);
    let mut records = Vec::new();
    collect_records(
        root,
        text,
        source.len(),
        &file_id,
        &exported_bindings,
        &mut records,
    );
    let (Some(kind), Some(range)) = (syntax_kind(root), source_range(root, source.len())) else {
        return Vec::new();
    };
    records.push(NativeSyntaxRecord::Scope {
        kind,
        file_id,
        range,
        scope_kind: ScopeKindV1::FileLexical,
        member_ranges: named_child_ranges(root, source.len()),
    });
    records
}

fn collect_records(
    node: Node<'_>,
    source: &str,
    source_length: usize,
    file_id: &SourceFileId,
    exported_bindings: &[String],
    records: &mut Vec<NativeSyntaxRecord>,
) {
    match node.kind() {
        "variable_declarator" => {
            collect_const_callable(
                node,
                source,
                source_length,
                file_id,
                exported_bindings,
                records,
            );
        }
        "function_declaration" | "generator_function_declaration" => {
            collect_function_callable(
                node,
                source,
                source_length,
                file_id,
                exported_bindings,
                records,
            );
        }
        "arrow_function" | "function_expression" | "generator_function" => {
            collect_anonymous_default_callable(node, source_length, file_id, records);
        }
        "call_expression" => collect_call(node, source, source_length, file_id, records),
        "import_statement" => {
            collect_import_bindings(node, source, source_length, file_id, records)
        }
        "export_statement" if is_top_level_export(node) => {
            collect_surface(node, source, source_length, file_id, records);
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_records(
            child,
            source,
            source_length,
            file_id,
            exported_bindings,
            records,
        );
    }
}

fn collect_const_callable(
    declarator: Node<'_>,
    source: &str,
    source_length: usize,
    file_id: &SourceFileId,
    exported_bindings: &[String],
    records: &mut Vec<NativeSyntaxRecord>,
) {
    if !is_top_level_callable(declarator) {
        return;
    }
    let Some(declaration) = declarator.parent() else {
        return;
    };
    if declaration.kind() != "lexical_declaration" || !has_direct_child_kind(declaration, "const") {
        return;
    }
    let Some(value) = declarator.child_by_field_name("value") else {
        return;
    };
    if !matches!(value.kind(), "arrow_function" | "function_expression") {
        return;
    }
    let (Some(kind), Some(range), Some(implementation_range)) = (
        syntax_kind(declarator),
        source_range(declarator, source_length),
        source_range(value, source_length),
    ) else {
        return;
    };

    let binding_key = declarator
        .child_by_field_name("name")
        .map(|name| node_text(name, source).to_owned());
    records.push(NativeSyntaxRecord::Callable {
        kind,
        file_id: file_id.clone(),
        range,
        binding_key: binding_key.clone(),
        implementation_range: Some(implementation_range),
        visibility: visibility(declarator, binding_key.as_deref(), exported_bindings),
    });
}

fn collect_function_callable(
    declaration: Node<'_>,
    source: &str,
    source_length: usize,
    file_id: &SourceFileId,
    exported_bindings: &[String],
    records: &mut Vec<NativeSyntaxRecord>,
) {
    if !is_top_level_callable(declaration) {
        return;
    }
    let (Some(kind), Some(range)) = (
        syntax_kind(declaration),
        source_range(declaration, source_length),
    ) else {
        return;
    };

    let binding_key = declaration
        .child_by_field_name("name")
        .map(|name| node_text(name, source).to_owned());
    records.push(NativeSyntaxRecord::Callable {
        kind,
        file_id: file_id.clone(),
        range,
        binding_key: binding_key.clone(),
        implementation_range: declaration
            .child_by_field_name("body")
            .and_then(|body| source_range(body, source_length)),
        visibility: visibility(declaration, binding_key.as_deref(), exported_bindings),
    });
}

fn collect_anonymous_default_callable(
    callable: Node<'_>,
    source_length: usize,
    file_id: &SourceFileId,
    records: &mut Vec<NativeSyntaxRecord>,
) {
    if !is_anonymous_default_callable(callable) {
        return;
    }
    let (Some(kind), Some(range)) = (syntax_kind(callable), source_range(callable, source_length))
    else {
        return;
    };
    let implementation_range = match callable.kind() {
        "arrow_function" => Some(range),
        _ => callable
            .child_by_field_name("body")
            .and_then(|body| source_range(body, source_length)),
    };
    records.push(NativeSyntaxRecord::Callable {
        kind,
        file_id: file_id.clone(),
        range,
        binding_key: None,
        implementation_range,
        visibility: VisibilityValueV1::Exported,
    });
}

fn collect_call(
    call: Node<'_>,
    source: &str,
    source_length: usize,
    file_id: &SourceFileId,
    records: &mut Vec<NativeSyntaxRecord>,
) {
    let (Some(kind), Some(range)) = (syntax_kind(call), source_range(call, source_length)) else {
        return;
    };
    let binding_key = call
        .child_by_field_name("function")
        .filter(|callee| callee.kind() == "identifier")
        .map(|callee| node_text(callee, source).to_owned());
    records.push(NativeSyntaxRecord::Call {
        kind,
        file_id: file_id.clone(),
        range,
        binding_key,
    });
}

fn collect_import_bindings(
    import_statement: Node<'_>,
    source: &str,
    source_length: usize,
    file_id: &SourceFileId,
    records: &mut Vec<NativeSyntaxRecord>,
) {
    let Some(specifier_node) = import_statement.child_by_field_name("source") else {
        return;
    };
    let Some(import_clause) = direct_named_child(import_statement, "import_clause") else {
        return;
    };
    let type_only_clause = has_direct_child_kind(import_statement, "type");
    let mut cursor = import_clause.walk();
    for child in import_clause.named_children(&mut cursor) {
        match child.kind() {
            "identifier" => collect_default_binding(
                child,
                specifier_node,
                type_only_clause,
                source,
                source_length,
                file_id,
                records,
            ),
            "namespace_import" => collect_namespace_binding(
                child,
                specifier_node,
                type_only_clause,
                source,
                source_length,
                file_id,
                records,
            ),
            "named_imports" => collect_named_bindings(
                child,
                specifier_node,
                type_only_clause,
                source,
                source_length,
                file_id,
                records,
            ),
            _ => {}
        }
    }
}

fn collect_named_bindings(
    named_imports: Node<'_>,
    specifier_node: Node<'_>,
    type_only_clause: bool,
    source: &str,
    source_length: usize,
    file_id: &SourceFileId,
    records: &mut Vec<NativeSyntaxRecord>,
) {
    let mut cursor = named_imports.walk();
    for binding in named_imports.named_children(&mut cursor) {
        if binding.kind() != "import_specifier" {
            continue;
        }
        let Some(name) = binding.child_by_field_name("name") else {
            continue;
        };
        let (Some(kind), Some(range), Some(specifier_range)) = (
            syntax_kind(binding),
            source_range(binding, source_length),
            source_range(specifier_node, source_length),
        ) else {
            continue;
        };
        let local = binding.child_by_field_name("alias").unwrap_or(name);
        let type_only = type_only_clause || has_direct_child_kind(binding, "type");
        records.push(NativeSyntaxRecord::Binding {
            kind,
            file_id: file_id.clone(),
            range,
            local_name: node_text(local, source).to_owned(),
            import_kind: if type_only {
                PayloadImportKindV1::TypeOnly
            } else {
                PayloadImportKindV1::Named
            },
            export_slot: Some(node_text(name, source).to_owned()),
            specifier: string_contents(specifier_node, source),
            specifier_range,
        });
    }
}

fn collect_default_binding(
    binding: Node<'_>,
    specifier_node: Node<'_>,
    type_only_clause: bool,
    source: &str,
    source_length: usize,
    file_id: &SourceFileId,
    records: &mut Vec<NativeSyntaxRecord>,
) {
    let (Some(kind), Some(range), Some(specifier_range)) = (
        syntax_kind(binding),
        source_range(binding, source_length),
        source_range(specifier_node, source_length),
    ) else {
        return;
    };
    records.push(NativeSyntaxRecord::Binding {
        kind,
        file_id: file_id.clone(),
        range,
        local_name: node_text(binding, source).to_owned(),
        import_kind: if type_only_clause {
            PayloadImportKindV1::TypeOnly
        } else {
            PayloadImportKindV1::Default
        },
        export_slot: Some("default".to_owned()),
        specifier: string_contents(specifier_node, source),
        specifier_range,
    });
}

fn collect_namespace_binding(
    binding: Node<'_>,
    specifier_node: Node<'_>,
    type_only_clause: bool,
    source: &str,
    source_length: usize,
    file_id: &SourceFileId,
    records: &mut Vec<NativeSyntaxRecord>,
) {
    let Some(local) = direct_named_child(binding, "identifier") else {
        return;
    };
    let (Some(kind), Some(range), Some(specifier_range)) = (
        syntax_kind(binding),
        source_range(binding, source_length),
        source_range(specifier_node, source_length),
    ) else {
        return;
    };
    records.push(NativeSyntaxRecord::Binding {
        kind,
        file_id: file_id.clone(),
        range,
        local_name: node_text(local, source).to_owned(),
        import_kind: if type_only_clause {
            PayloadImportKindV1::TypeOnly
        } else {
            PayloadImportKindV1::Namespace
        },
        export_slot: None,
        specifier: string_contents(specifier_node, source),
        specifier_range,
    });
}

fn collect_surface(
    export_statement: Node<'_>,
    source: &str,
    source_length: usize,
    file_id: &SourceFileId,
    records: &mut Vec<NativeSyntaxRecord>,
) {
    let (Some(kind), Some(range)) = (
        syntax_kind(export_statement),
        source_range(export_statement, source_length),
    ) else {
        return;
    };
    let mut export_slots = Vec::new();
    let mut local_names = Vec::new();
    collect_export_specifiers(
        export_statement,
        source,
        has_direct_child_kind(export_statement, "type"),
        &mut export_slots,
        &mut local_names,
    );
    if !has_descendant_kind(export_statement, "export_specifier") {
        collect_direct_surface_fields(
            export_statement,
            source,
            &mut export_slots,
            &mut local_names,
        );
    }
    let target_specifier = export_statement
        .child_by_field_name("source")
        .map(|specifier| string_contents(specifier, source));
    records.push(NativeSyntaxRecord::Surface {
        kind,
        file_id: file_id.clone(),
        range,
        export_slots,
        local_names,
        target_specifier,
    });
}

fn collect_direct_surface_fields(
    export_statement: Node<'_>,
    source: &str,
    export_slots: &mut Vec<String>,
    local_names: &mut Vec<String>,
) {
    if has_direct_child_kind(export_statement, "default") {
        export_slots.push("default".to_owned());
        if let Some(local) = default_local_name(export_statement, source) {
            local_names.push(local);
        }
        return;
    }
    if let Some(namespace_export) = direct_named_child(export_statement, "namespace_export") {
        if let Some(alias) = direct_named_child(namespace_export, "identifier") {
            export_slots.push(node_text(alias, source).to_owned());
        }
        return;
    }

    let Some(declaration) = export_statement
        .child_by_field_name("declaration")
        .or_else(|| direct_export_declaration(export_statement))
    else {
        return;
    };
    if matches!(
        declaration.kind(),
        "lexical_declaration" | "variable_declaration"
    ) {
        let mut declaration_cursor = declaration.walk();
        for declarator in declaration.named_children(&mut declaration_cursor) {
            if declarator.kind() != "variable_declarator" {
                continue;
            }
            if let Some(name) = declarator.child_by_field_name("name") {
                let name = node_text(name, source).to_owned();
                export_slots.push(name.clone());
                local_names.push(name);
            }
        }
    } else if let Some(name) = declaration.child_by_field_name("name") {
        let name = node_text(name, source).to_owned();
        export_slots.push(name.clone());
        local_names.push(name);
    }
}

fn collect_export_specifiers(
    node: Node<'_>,
    source: &str,
    type_only_clause: bool,
    export_slots: &mut Vec<String>,
    local_names: &mut Vec<String>,
) {
    if node.kind() == "export_specifier" {
        if type_only_clause || has_direct_child_kind(node, "type") {
            return;
        }
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let slot = node.child_by_field_name("alias").unwrap_or(name);
        export_slots.push(node_text(slot, source).to_owned());
        local_names.push(node_text(name, source).to_owned());
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_export_specifiers(child, source, type_only_clause, export_slots, local_names);
    }
}

fn same_file_exported_bindings(root: Node<'_>, source: &str) -> Vec<String> {
    let mut bindings = Vec::new();
    let mut cursor = root.walk();
    for export_statement in root
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "export_statement")
    {
        if export_statement.child_by_field_name("source").is_some() {
            continue;
        }
        collect_exported_binding_names(
            export_statement,
            source,
            has_direct_child_kind(export_statement, "type"),
            &mut bindings,
        );
        if let Some(name) = default_local_name(export_statement, source) {
            bindings.push(name);
        }
    }
    bindings
}

fn collect_exported_binding_names(
    node: Node<'_>,
    source: &str,
    type_only_clause: bool,
    bindings: &mut Vec<String>,
) {
    if node.kind() == "export_specifier" {
        if type_only_clause || has_direct_child_kind(node, "type") {
            return;
        }
        if let Some(name) = node.child_by_field_name("name") {
            bindings.push(node_text(name, source).to_owned());
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_exported_binding_names(child, source, type_only_clause, bindings);
    }
}

fn is_top_level_export(node: Node<'_>) -> bool {
    node.kind() == "export_statement"
        && node
            .parent()
            .is_some_and(|parent| parent.kind() == "program")
}

fn is_top_level_callable(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    match parent.kind() {
        "program" => true,
        "export_statement" => is_top_level_export(parent),
        "lexical_declaration" => match parent.parent() {
            Some(grandparent) if grandparent.kind() == "program" => true,
            Some(grandparent) if grandparent.kind() == "export_statement" => {
                is_top_level_export(grandparent)
            }
            _ => false,
        },
        _ => false,
    }
}

fn is_anonymous_default_callable(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        is_top_level_export(parent) && has_direct_child_kind(parent, "default")
    })
}

fn direct_export_ancestor(mut node: Node<'_>) -> bool {
    while let Some(parent) = node.parent() {
        if is_top_level_export(parent) {
            return true;
        }
        node = parent;
    }
    false
}

fn visibility(
    node: Node<'_>,
    binding_key: Option<&str>,
    exported_bindings: &[String],
) -> VisibilityValueV1 {
    if direct_export_ancestor(node)
        || binding_key.is_some_and(|binding| exported_bindings.iter().any(|item| item == binding))
    {
        VisibilityValueV1::Exported
    } else {
        VisibilityValueV1::NonExported
    }
}

fn direct_named_child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn has_direct_child_kind(node: Node<'_>, kind: &str) -> bool {
    (0..node.child_count()).any(|index| node.child(index).is_some_and(|child| child.kind() == kind))
}

fn direct_export_declaration(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(|child| {
        matches!(
            child.kind(),
            "class_declaration"
                | "enum_declaration"
                | "function_declaration"
                | "generator_function_declaration"
                | "interface_declaration"
                | "internal_module"
                | "lexical_declaration"
                | "type_alias_declaration"
                | "variable_declaration"
        )
    })
}

fn default_local_name(export_statement: Node<'_>, source: &str) -> Option<String> {
    if !has_direct_child_kind(export_statement, "default") {
        return None;
    }
    let declaration_or_value = export_statement
        .child_by_field_name("declaration")
        .or_else(|| export_statement.child_by_field_name("value"))?;
    let name = if declaration_or_value.kind() == "identifier" {
        Some(declaration_or_value)
    } else {
        declaration_or_value.child_by_field_name("name")
    }?;
    Some(node_text(name, source).to_owned())
}

fn has_descendant_kind(node: Node<'_>, kind: &str) -> bool {
    if node.kind() == kind {
        return true;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .any(|child| has_descendant_kind(child, kind))
}

fn has_missing(node: Node<'_>) -> bool {
    node.is_missing()
        || (0..node.child_count())
            .any(|index| has_missing(node.child(index).expect("bounded child index")))
}

fn named_child_ranges(node: Node<'_>, source_length: usize) -> Vec<SourceRange> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter_map(|child| source_range(child, source_length))
        .collect()
}

fn source_range(node: Node<'_>, source_length: usize) -> Option<SourceRange> {
    let start = node.start_byte();
    let end = node.end_byte();
    if end > source_length {
        return None;
    }
    SourceRange::new(start as u64, end as u64).ok()
}

fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn string_contents(node: Node<'_>, source: &str) -> String {
    let text = node_text(node, source);
    if text.len() >= 2
        && matches!(text.as_bytes()[0], b'\'' | b'\"')
        && text.as_bytes()[0] == text.as_bytes()[text.len() - 1]
    {
        text[1..text.len() - 1].to_owned()
    } else {
        text.to_owned()
    }
}

fn syntax_kind(node: Node<'_>) -> Option<TypeScriptSyntaxKind> {
    TypeScriptSyntaxKind::parse_wire(node.kind()).ok()
}

#[cfg(test)]
mod source_range_tests {
    use super::{Parser, source_range};

    #[test]
    fn source_range_rejects_node_past_supplied_byte_length() {
        let source = b"const value = 1;";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .expect("TypeScript parser language is available");
        let tree = parser
            .parse(source, None)
            .expect("complete source produces a parse tree");

        assert!(source_range(tree.root_node(), source.len() - 1).is_none());
    }
}
