//! AST-bound import/export binding contracts for G1-S4.
//!
//! These signatures intentionally accept tree-sitter nodes and their exact
//! source bytes.  Public string-based callable or export classifiers would
//! violate the design.

use reviewgraphen_core::source_review::ids::{DeclarationId, SourceFileId, SourceRange};
use reviewgraphen_core::source_review::reasons::{CallReason, ReasonSet};
use tree_sitter::Node;

/// One static import form permitted to reach the binding resolver.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ImportKind {
    Named,
    Default,
    TypeOnly,
    Namespace,
}

/// The export slot requested by one static import binding.
///
/// `Named` retains the range of the original exported spelling, rather than
/// the local alias. `Default` and `Namespace` remain distinct: namespace
/// dispatch has no first-cohort exact-export resolution. The resolver receives
/// the caller bytes and must read this range from those bytes; it must not pick
/// a callee by a local name or by the callee export surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ImportedExportSlot {
    Named { original_name_range: SourceRange },
    Default,
    Namespace,
}

/// A source-derived import binding. Its source ranges, rather than a local name,
/// are the identity used during edge construction. `imported_slot` preserves
/// named/default/namespace form and, for a named import, the original exported
/// spelling; a local alias is insufficient to choose an export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportBinding {
    pub file_id: SourceFileId,
    pub import_range: SourceRange,
    pub local_binding_range: SourceRange,
    pub import_kind: ImportKind,
    pub imported_slot: ImportedExportSlot,
}

/// The runtime export slot established by one same-file export surface entry.
///
/// The named spelling belongs to the callee source. It is compared with
/// [`ImportedExportSlot::Named`] only by [`resolve_export_binding`], which has
/// both source byte sequences and both AST-derived ranges.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum RuntimeExportSlot {
    Named { exported_name_range: SourceRange },
    Default,
}

/// A source-derived same-file export slot, distinct from callable identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportSlot {
    pub file_id: SourceFileId,
    pub export_range: SourceRange,
    pub local_binding_range: Option<SourceRange>,
    pub runtime_slot: Option<RuntimeExportSlot>,
    pub kind: ExportSlotKind,
}

/// The finite export surface classes relevant to first-cohort resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ExportSlotKind {
    Direct,
    Default,
    ExportList,
    TypeOnly,
    NonCallable,
    ReExport,
    StarReExport,
    NamespaceReExport,
    Unsupported,
}

/// One successful export resolution carries the declaration ID and the exact
/// source ranges of its callee-local binding.  The ranges are not endpoint
/// identity: they let the relative-call resolver apply its existing callee
/// shadow/write scan to the binding the export surface actually selected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedExportBinding {
    callee_id: DeclarationId,
    local_binding_range: SourceRange,
    callee_declaration_range: SourceRange,
    visibility_witnesses: Vec<SourceRange>,
}

impl ResolvedExportBinding {
    /// Returns the callee identity that the callee AST's runtime export surface
    /// proved. There is deliberately no public constructor: a relative exact
    /// edge can receive this value only from [`resolve_export_binding`], not
    /// from an import/local name.
    #[must_use]
    pub fn callee_id(&self) -> &DeclarationId {
        &self.callee_id
    }

    /// Returns the source range of the exact callee-local binding selected by
    /// the resolved export slot.
    #[must_use]
    pub(crate) fn local_binding_range(&self) -> SourceRange {
        self.local_binding_range
    }

    /// Returns the source range of the callable declaration behind that local
    /// binding.
    #[must_use]
    pub(crate) fn callee_declaration_range(&self) -> SourceRange {
        self.callee_declaration_range
    }

    /// Returns the same-file export ranges that proved this exact binding.
    #[must_use]
    pub fn visibility_witnesses(&self) -> &[SourceRange] {
        &self.visibility_witnesses
    }
}

/// Result of classifying a top-level callable declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallableClassification {
    Callable(DeclarationId),
    Rejected(ReasonSet),
}

/// Enumerates the supported top-level callable declarations for the Node rule.
/// The returned IDs are derived from the same AST predicate used by relative
/// callee resolution; a caller cannot substitute a name string.
pub fn collect_top_level_callables(
    _root: Node<'_>,
    _source: &[u8],
    _file_id: SourceFileId,
) -> Vec<CallableClassification> {
    let (root, source, file_id) = (_root, _source, _file_id);
    let mut declarations = Vec::new();
    collect_nodes_of_kind(root, "function_declaration", &mut declarations);
    collect_nodes_of_kind(root, "generator_function_declaration", &mut declarations);
    collect_nodes_of_kind(root, "variable_declarator", &mut declarations);
    declarations
        .into_iter()
        .filter(|node| is_top_level_callable(*node))
        .map(|node| classify_top_level_callable(node, source, file_id.clone()))
        .collect()
}

/// Extracts static imports from the parsed root. The caller supplies the
/// file identity and exact source bytes; no source text search API is exposed.
pub fn collect_imports(
    _root: Node<'_>,
    _source: &[u8],
    _file_id: SourceFileId,
) -> Vec<ImportBinding> {
    let (root, source, file_id) = (_root, _source, _file_id);
    let mut statements = Vec::new();
    collect_nodes_of_kind(root, "import_statement", &mut statements);
    let mut bindings = Vec::new();
    for statement in statements {
        let Some(import_clause) = direct_named_child(statement, "import_clause") else {
            continue;
        };
        let type_only = node_text(statement, source)
            .trim_start()
            .starts_with("import type ");
        let mut cursor = import_clause.walk();
        for child in import_clause.named_children(&mut cursor) {
            match child.kind() {
                "identifier" => bindings.push(ImportBinding {
                    file_id: file_id.clone(),
                    import_range: node_range(statement),
                    local_binding_range: node_range(child),
                    import_kind: if type_only {
                        ImportKind::TypeOnly
                    } else {
                        ImportKind::Default
                    },
                    imported_slot: ImportedExportSlot::Default,
                }),
                "namespace_import" => {
                    let Some(local) = direct_named_child(child, "identifier") else {
                        continue;
                    };
                    bindings.push(ImportBinding {
                        file_id: file_id.clone(),
                        import_range: node_range(statement),
                        local_binding_range: node_range(local),
                        import_kind: if type_only {
                            ImportKind::TypeOnly
                        } else {
                            ImportKind::Namespace
                        },
                        imported_slot: ImportedExportSlot::Namespace,
                    });
                }
                "named_imports" => {
                    let mut named_cursor = child.walk();
                    for specifier in child.named_children(&mut named_cursor) {
                        if specifier.kind() != "import_specifier" {
                            continue;
                        }
                        let Some(name) = specifier.child_by_field_name("name") else {
                            continue;
                        };
                        let local = specifier.child_by_field_name("alias").unwrap_or(name);
                        let is_type = type_only
                            || node_text(specifier, source)
                                .trim_start()
                                .starts_with("type ");
                        bindings.push(ImportBinding {
                            file_id: file_id.clone(),
                            import_range: node_range(statement),
                            local_binding_range: node_range(local),
                            import_kind: if is_type {
                                ImportKind::TypeOnly
                            } else {
                                ImportKind::Named
                            },
                            imported_slot: ImportedExportSlot::Named {
                                original_name_range: node_range(name),
                            },
                        });
                    }
                }
                _ => {}
            }
        }
    }
    bindings
}

/// Extracts the same-file export surface from AST nodes without promoting a
/// re-export, type-only value, overload, or non-callable into a declaration.
pub fn export_slots(_root: Node<'_>, _source: &[u8], _file_id: SourceFileId) -> Vec<ExportSlot> {
    let (root, source, file_id) = (_root, _source, _file_id);
    let mut statements = Vec::new();
    collect_nodes_of_kind(root, "export_statement", &mut statements);
    let mut slots = Vec::new();
    for statement in statements
        .into_iter()
        .filter(|node| is_top_level_export(*node))
    {
        let text = node_text(statement, source).trim_start();
        let source_backed = statement.child_by_field_name("source").is_some();
        if source_backed {
            slots.push(ExportSlot {
                file_id: file_id.clone(),
                export_range: node_range(statement),
                local_binding_range: None,
                runtime_slot: None,
                kind: if text.starts_with("export * as ") {
                    ExportSlotKind::NamespaceReExport
                } else if text.starts_with("export *") {
                    ExportSlotKind::StarReExport
                } else {
                    ExportSlotKind::ReExport
                },
            });
            continue;
        }
        if text.starts_with("export type ") {
            slots.push(ExportSlot {
                file_id: file_id.clone(),
                export_range: node_range(statement),
                local_binding_range: None,
                runtime_slot: None,
                kind: ExportSlotKind::TypeOnly,
            });
            continue;
        }
        let mut specifiers = Vec::new();
        collect_nodes_of_kind(statement, "export_specifier", &mut specifiers);
        if !specifiers.is_empty() {
            for specifier in specifiers {
                let Some(name) = specifier.child_by_field_name("name") else {
                    continue;
                };
                let slot = specifier.child_by_field_name("alias").unwrap_or(name);
                let type_only = node_text(specifier, source)
                    .trim_start()
                    .starts_with("type ");
                slots.push(ExportSlot {
                    file_id: file_id.clone(),
                    export_range: node_range(statement),
                    local_binding_range: Some(node_range(name)),
                    runtime_slot: (!type_only).then_some(RuntimeExportSlot::Named {
                        exported_name_range: node_range(slot),
                    }),
                    kind: if type_only {
                        ExportSlotKind::TypeOnly
                    } else {
                        ExportSlotKind::ExportList
                    },
                });
            }
            continue;
        }
        if text.starts_with("export default ") {
            let local = direct_callable_name(statement)
                .or_else(|| direct_named_child(statement, "identifier"));
            slots.push(ExportSlot {
                file_id: file_id.clone(),
                export_range: node_range(statement),
                local_binding_range: local.map(node_range),
                runtime_slot: Some(RuntimeExportSlot::Default),
                kind: ExportSlotKind::Default,
            });
            continue;
        }
        let mut direct = Vec::new();
        for kind in [
            "function_declaration",
            "generator_function_declaration",
            "variable_declarator",
        ] {
            collect_nodes_of_kind(statement, kind, &mut direct);
        }
        if direct.is_empty() {
            slots.push(ExportSlot {
                file_id: file_id.clone(),
                export_range: node_range(statement),
                local_binding_range: None,
                runtime_slot: None,
                kind: ExportSlotKind::Unsupported,
            });
        }
        for declaration in direct {
            let Some(name) = direct_callable_name(declaration) else {
                continue;
            };
            slots.push(ExportSlot {
                file_id: file_id.clone(),
                export_range: node_range(statement),
                local_binding_range: Some(node_range(name)),
                runtime_slot: Some(RuntimeExportSlot::Named {
                    exported_name_range: node_range(name),
                }),
                kind: ExportSlotKind::Direct,
            });
        }
    }
    slots
}

/// Resolves the requested import slot against the callee AST and source.
///
/// `caller_source` is required because a named [`ImportBinding`] records the
/// source range of its original imported spelling. The implementation compares
/// that caller slice with the callee's [`RuntimeExportSlot`] slice, while
/// default and namespace remain separate typed cases. A successful
/// [`ResolvedExportBinding`] is the proof required by the relative-call entry
/// point before it can construct an exact callee. Rejection returns the full
/// typed reason set; no name/surface guessing path exists.
pub fn resolve_export_binding(
    _caller_source: &[u8],
    _callee_root: Node<'_>,
    _callee_source: &[u8],
    _callee_file_id: SourceFileId,
    _binding: &ImportBinding,
) -> Result<ResolvedExportBinding, ReasonSet> {
    let (caller_source, callee_root, callee_source, callee_file_id, binding) = (
        _caller_source,
        _callee_root,
        _callee_source,
        _callee_file_id,
        _binding,
    );
    if matches!(binding.import_kind, ImportKind::TypeOnly) {
        return Err(ReasonSet::new([CallReason::TypeOnlyBinding]));
    }
    if matches!(binding.import_kind, ImportKind::Namespace) {
        return Err(ReasonSet::new([CallReason::ImportResolutionUnavailable]));
    }
    if contains_kind(callee_root, "function_signature") {
        return Err(ReasonSet::new([CallReason::ExportBindingUnsupported]));
    }
    let wanted = match binding.imported_slot {
        ImportedExportSlot::Named {
            original_name_range,
        } => Some(source_slice(caller_source, original_name_range)),
        ImportedExportSlot::Default => None,
        ImportedExportSlot::Namespace => unreachable!("namespace was rejected above"),
    };
    let matches = export_slots(callee_root, callee_source, callee_file_id.clone())
        .into_iter()
        .filter(|slot| match (wanted.as_deref(), slot.runtime_slot) {
            (
                Some(wanted),
                Some(RuntimeExportSlot::Named {
                    exported_name_range,
                }),
            ) => source_slice(callee_source, exported_name_range) == wanted,
            (None, Some(RuntimeExportSlot::Default)) => true,
            _ => false,
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(ReasonSet::new([CallReason::ExportBindingUnsupported]));
    }
    let slot = &matches[0];
    let Some(local_range) = slot.local_binding_range else {
        return Err(ReasonSet::new([CallReason::ExportBindingUnsupported]));
    };
    let local_name = source_slice(callee_source, local_range);
    let candidates =
        callable_declarations_by_name(callee_root, callee_source, callee_file_id, &local_name);
    if candidates.len() != 1 {
        return Err(ReasonSet::new([CallReason::ExportBindingUnsupported]));
    }
    let (callee_id, callee_declaration_range) =
        candidates.into_iter().next().expect("one checked callable");
    Ok(ResolvedExportBinding {
        callee_id,
        local_binding_range: local_range,
        callee_declaration_range,
        visibility_witnesses: vec![slot.export_range],
    })
}

/// Tests whether this one top-level declaration is a supported callable using
/// the node and source bytes. Parentheses may be unwrapped only as the design
/// permits; wrappers and names alone are never sufficient.
pub fn classify_top_level_callable(
    _declaration: Node<'_>,
    _source: &[u8],
    _file_id: SourceFileId,
) -> CallableClassification {
    let (declaration, source, file_id) = (_declaration, _source, _file_id);
    if !is_top_level_callable(declaration) || !is_supported_callable(declaration, source) {
        return CallableClassification::Rejected(ReasonSet::new([CallReason::UnsupportedCaller]));
    }
    CallableClassification::Callable(DeclarationId::from_source(file_id, node_range(declaration)))
}

/// Rejects a binding candidate because an AST-observed condition applies.
/// This closed enum prevents a caller from minting an arbitrary binding reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum BindingRejection {
    TypeOnly,
    Ambiguous,
    UnsupportedExport,
    NonRuntimeCallable,
}

impl BindingRejection {
    /// Converts this binding result to its registered call reason.
    #[must_use]
    pub fn reason(self) -> CallReason {
        match self {
            Self::TypeOnly => CallReason::TypeOnlyBinding,
            Self::Ambiguous => CallReason::ImportBindingAmbiguous,
            Self::UnsupportedExport | Self::NonRuntimeCallable => {
                CallReason::ExportBindingUnsupported
            }
        }
    }
}

fn callable_declarations_by_name(
    root: Node<'_>,
    source: &[u8],
    file_id: SourceFileId,
    name: &str,
) -> Vec<(DeclarationId, SourceRange)> {
    let mut nodes = Vec::new();
    for kind in [
        "function_declaration",
        "generator_function_declaration",
        "variable_declarator",
    ] {
        collect_nodes_of_kind(root, kind, &mut nodes);
    }
    nodes
        .into_iter()
        .filter(|node| is_top_level_callable(*node) && is_supported_callable(*node, source))
        .filter_map(|node| {
            direct_callable_name(node)
                .filter(|binding| source_slice(source, node_range(*binding)) == name)
                .map(|_| {
                    let declaration_range = node_range(node);
                    (
                        DeclarationId::from_source(file_id.clone(), declaration_range),
                        declaration_range,
                    )
                })
        })
        .collect()
}

fn is_supported_callable(node: Node<'_>, source: &[u8]) -> bool {
    match node.kind() {
        "function_declaration" | "generator_function_declaration" => {
            node.child_by_field_name("body").is_some()
        }
        "variable_declarator" => {
            node.child_by_field_name("value")
                .is_some_and(is_direct_callable_value)
                && node.parent().is_some_and(|parent| {
                    parent.kind() == "lexical_declaration"
                        && node_text(parent, source).trim_start().starts_with("const ")
                })
        }
        _ => false,
    }
}

fn is_direct_callable_value(mut value: Node<'_>) -> bool {
    while value.kind() == "parenthesized_expression" {
        let mut cursor = value.walk();
        let Some(child) = value.named_children(&mut cursor).next() else {
            return false;
        };
        value = child;
    }
    matches!(
        value.kind(),
        "arrow_function" | "function_expression" | "generator_function"
    )
}

fn direct_callable_name(node: Node<'_>) -> Option<Node<'_>> {
    match node.kind() {
        "function_declaration" | "generator_function_declaration" | "variable_declarator" => {
            node.child_by_field_name("name")
        }
        "export_statement" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .find_map(direct_callable_name)
        }
        _ => None,
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
        "lexical_declaration" => parent.parent().is_some_and(|grandparent| {
            grandparent.kind() == "program" || is_top_level_export(grandparent)
        }),
        _ => false,
    }
}

fn direct_named_child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn collect_nodes_of_kind<'tree>(node: Node<'tree>, kind: &str, output: &mut Vec<Node<'tree>>) {
    if node.kind() == kind {
        output.push(node);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_nodes_of_kind(child, kind, output);
    }
}

fn contains_kind(node: Node<'_>, kind: &str) -> bool {
    if node.kind() == kind {
        return true;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .any(|child| contains_kind(child, kind))
}

fn node_range(node: Node<'_>) -> SourceRange {
    SourceRange::new(node.start_byte() as u64, node.end_byte() as u64)
        .expect("tree-sitter node ranges are ordered")
}

fn source_slice(source: &[u8], range: SourceRange) -> String {
    std::str::from_utf8(&source[range.start() as usize..range.end() as usize])
        .expect("TypeScript parser receives UTF-8 source")
        .to_owned()
}

fn node_text<'source>(node: Node<'_>, source: &'source [u8]) -> &'source str {
    std::str::from_utf8(&source[node.byte_range()])
        .expect("TypeScript parser receives UTF-8 source")
}
