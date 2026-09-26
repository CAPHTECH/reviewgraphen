//! AST-bound local and relative call-edge contracts for G1-S4.
//!
//! The design requires both the callsite and its
//! enclosing caller range. All write, shadow, eval, and callable decisions take
//! AST nodes plus source bytes; public substring classifiers do not exist.

use super::import_bindings::{CallableClassification, ImportBinding, ResolvedExportBinding};
use super::relative_paths::{RelativeEntry, RelativeEntryOutcome};
use reviewgraphen_core::source_review::ids::{
    CallerId, CallsiteId, DeclarationId, SourceFileId, SourceRange,
};
use reviewgraphen_core::source_review::reasons::{
    CallReason, CallStageV1, ReasonSet, ResolutionKind,
};
use reviewgraphen_core::source_review::registry::typescript_registry_binding;
use std::collections::BTreeMap;
use tree_sitter::Node;

/// The exact callable node enclosing a callsite. Its range must contain the
/// callsite range, preventing file-wide shadow/write attribution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallerScope {
    pub caller_id: CallerId,
    pub caller_range: SourceRange,
}

/// A local call edge made only from source-derived endpoint IDs and callsite ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalCallEdge {
    pub caller_id: CallerId,
    pub callee_id: DeclarationId,
    pub callsite_key: CallsiteId,
    pub resolution_kind: ResolutionKind,
    pub evaluated_stages: Vec<CallStageV1>,
}

/// A relative call edge. The caller and callsite are mandatory output fields so
/// an edge cannot migrate across enclosing callers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelativeCallEdge {
    pub caller_id: CallerId,
    pub callee_id: DeclarationId,
    pub callsite_key: CallsiteId,
    pub resolution_kind: ResolutionKind,
    pub evaluated_stages: Vec<CallStageV1>,
}

/// A non-admitted call occurrence. Module-level calls retain `caller_id: None`
/// and their actual source range instead of receiving a fabricated ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedCall {
    pub caller_id: Option<CallerId>,
    pub callsite_key: CallsiteId,
    pub source_range: SourceRange,
    pub reasons: ReasonSet,
    pub evaluated_stages: Vec<CallStageV1>,
}

/// The complete local-call catalogue: accepted edges and every unresolved
/// occurrence remain distinct.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LocalCallReport {
    pub edges: Vec<LocalCallEdge>,
    pub unresolved: Vec<UnresolvedCall>,
}

/// Inputs for a relative call. The caller scope and callsite identity are
/// required even before candidate path resolution begins.
pub struct RelativeCallInput<'tree, 'source> {
    pub caller: CallerScope,
    pub callsite_key: CallsiteId,
    pub callsite_node: Node<'tree>,
    pub caller_source: &'source [u8],
    pub caller_file_id: SourceFileId,
    pub binding: &'source ImportBinding,
    /// The callee export-surface decision. This is intentionally a result from
    /// `resolve_export_binding`: only its success form carries the private
    /// `ResolvedExportBinding` proof required for an exact edge. An error form
    /// remains a source-recorded unresolved occurrence.
    pub export_resolution: Result<ResolvedExportBinding, ReasonSet>,
    pub candidate_entries: Vec<RelativeEntry>,
    pub tree_complete: bool,
    pub callee_root: Node<'tree>,
    pub callee_source: &'source [u8],
    pub callee_file_id: SourceFileId,
}

/// A relative resolution is either one identity-bound edge or one unresolved
/// occurrence with the complete, order-independent reason set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelativeCallResult {
    pub edge: Option<RelativeCallEdge>,
    pub unresolved: Option<UnresolvedCall>,
    pub candidate_paths: Vec<RelativeEntry>,
    pub evaluated_stages: Vec<CallStageV1>,
}

/// The furthest import-resolution checkpoint reached before a typed failure.
/// Payload construction selects this semantic result form; it never writes an
/// evaluation-stage literal itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportCallAttempt {
    Caller,
    Form,
    LocalBinding,
    Specifier,
    Candidates,
    ExportBinding,
}

/// Records an import-dispatch failure together with the resolver-owned prefix
/// that was actually visited.
#[must_use]
pub fn unresolved_import_call(
    caller_id: Option<CallerId>,
    callsite_key: CallsiteId,
    source_range: SourceRange,
    reasons: impl IntoIterator<Item = CallReason>,
    attempted: ImportCallAttempt,
) -> UnresolvedCall {
    unresolved(
        caller_id,
        callsite_key,
        source_range,
        reasons,
        import_call_stage_prefix(attempted),
    )
}

/// Records a parser-level call failure when no local or import resolver result
/// can exist. The result owns its one visited stage.
#[must_use]
pub fn unresolved_syntax_call(
    caller_id: Option<CallerId>,
    callsite_key: CallsiteId,
    source_range: SourceRange,
) -> UnresolvedCall {
    unresolved(
        caller_id,
        callsite_key,
        source_range,
        [CallReason::UnsupportedSyntax],
        &["c.syntax"],
    )
}

/// Evaluates local calls from the parsed file root. The implementation must
/// record module-level calls as [`UnresolvedCall`] rather than dropping them.
pub fn evaluate_local_calls(
    _root: Node<'_>,
    _source: &[u8],
    _file_id: SourceFileId,
) -> LocalCallReport {
    let mut callables = BTreeMap::<String, Vec<(DeclarationId, Node<'_>)>>::new();
    let mut callable_nodes = Vec::new();
    for kind in [
        "function_declaration",
        "generator_function_declaration",
        "variable_declarator",
    ] {
        collect_nodes_of_kind(_root, kind, &mut callable_nodes);
    }
    for node in callable_nodes {
        let CallableClassification::Callable(id) =
            classify_callable(node, _source, _file_id.clone())
        else {
            continue;
        };
        let Some(name) = binding_name(node, _source) else {
            continue;
        };
        callables.entry(name).or_default().push((id, node));
    }

    let mut calls = Vec::new();
    collect_nodes_of_kind(_root, "call_expression", &mut calls);
    let mut report = LocalCallReport::default();
    for call in calls {
        let range = node_range(call);
        let (caller_id, caller_node) = enclosing_caller(call, _source, _file_id.clone());
        let callsite_key = CallsiteId::from_source(_file_id.clone(), range, caller_id.clone());
        let Some(function) = call.child_by_field_name("function") else {
            report.unresolved.push(unresolved(
                caller_id,
                callsite_key,
                range,
                [CallReason::UnsupportedSyntax],
                &["c.syntax"],
            ));
            continue;
        };
        if function.kind() != "identifier" {
            report.unresolved.push(unresolved(
                caller_id,
                callsite_key,
                range,
                [CallReason::DynamicDispatch],
                &["c.syntax"],
            ));
            continue;
        }
        let name = node_text(function, _source);
        let Some(caller_id) = caller_id else {
            report.unresolved.push(unresolved(
                None,
                callsite_key,
                range,
                [CallReason::UnsupportedCaller],
                &["c.syntax", "c.caller"],
            ));
            continue;
        };
        let Some(caller_node) = caller_node else {
            report.unresolved.push(unresolved(
                Some(caller_id),
                callsite_key,
                range,
                [CallReason::UnsupportedCaller],
                &["c.syntax", "c.caller"],
            ));
            continue;
        };
        let mut reasons = binding_reasons(
            Some(caller_node),
            _source,
            name,
            Some(node_range(caller_node)),
        );
        collect_write_reasons(_root, _source, name, &mut reasons);
        let Some(candidates) = callables.get(name) else {
            reasons.push(CallReason::UnresolvedName);
            report.unresolved.push(UnresolvedCall {
                caller_id: Some(caller_id),
                callsite_key,
                source_range: range,
                reasons: ReasonSet::new(reasons),
                evaluated_stages: call_stage_trace(&[
                    "c.syntax",
                    "c.caller",
                    "c.local_binding",
                    "c.shadow",
                    "c.writes",
                ]),
            });
            continue;
        };
        if candidates.len() != 1 {
            reasons.push(CallReason::ImportBindingAmbiguous);
        }
        if !reasons.is_empty() {
            report.unresolved.push(UnresolvedCall {
                caller_id: Some(caller_id),
                callsite_key,
                source_range: range,
                reasons: ReasonSet::new(reasons),
                evaluated_stages: call_stage_trace(&[
                    "c.syntax",
                    "c.caller",
                    "c.local_binding",
                    "c.shadow",
                    "c.writes",
                ]),
            });
            continue;
        }
        report.edges.push(LocalCallEdge {
            caller_id,
            callee_id: candidates[0].0.clone(),
            callsite_key,
            resolution_kind: ResolutionKind::SyntacticUnique,
            evaluated_stages: call_stage_trace(&[
                "c.syntax",
                "c.caller",
                "c.local_binding",
                "c.shadow",
                "c.writes",
                "c.resolution",
            ]),
        });
    }
    report
}

/// Resolves one relative call while preserving caller and callsite identity in
/// both success and failure forms. The required `export_resolution` means this
/// entry cannot construct an exact callee from a local or export-slot name;
/// only the AST-derived proof from `resolve_export_binding` can do so
///.
pub fn resolve_relative_call(_input: RelativeCallInput<'_, '_>) -> RelativeCallResult {
    let RelativeCallInput {
        caller,
        callsite_key,
        callsite_node,
        caller_source,
        caller_file_id: _,
        binding,
        export_resolution,
        candidate_entries,
        tree_complete,
        callee_root,
        callee_source,
        callee_file_id: _,
    } = _input;
    let call_range = node_range(callsite_node);
    let unresolved_result = |reasons: ReasonSet, stages: &[&str]| RelativeCallResult {
        edge: None,
        unresolved: Some(UnresolvedCall {
            caller_id: Some(caller.caller_id.clone()),
            callsite_key: callsite_key.clone(),
            source_range: call_range,
            reasons,
            evaluated_stages: call_stage_trace(stages),
        }),
        candidate_paths: candidate_entries.clone(),
        evaluated_stages: call_stage_trace(stages),
    };

    if callsite_key
        != CallsiteId::from_source(
            binding.file_id.clone(),
            call_range,
            Some(caller.caller_id.clone()),
        )
        || !contains(caller.caller_range, call_range)
    {
        return unresolved_result(
            ReasonSet::new([CallReason::UnsupportedCaller]),
            &["c.syntax", "c.caller"],
        );
    }
    let Some(function) = callsite_node.child_by_field_name("function") else {
        return unresolved_result(
            ReasonSet::new([CallReason::UnsupportedSyntax]),
            &["c.syntax"],
        );
    };
    if function.kind() != "identifier" {
        let mut reasons = vec![CallReason::DynamicDispatch];
        if matches!(
            binding.import_kind,
            super::import_bindings::ImportKind::Namespace
        ) {
            reasons.push(CallReason::ImportResolutionUnavailable);
        }
        return unresolved_result(
            ReasonSet::new(reasons),
            &["c.syntax", "c.caller", "c.import_form"],
        );
    }
    if node_text(function, caller_source)
        != source_slice(caller_source, binding.local_binding_range)
    {
        return unresolved_result(
            ReasonSet::new([CallReason::ImportBindingAmbiguous]),
            &["c.syntax", "c.caller", "c.import_form", "c.local_binding"],
        );
    }
    let caller_reasons = binding_reasons(
        callsite_node
            .parent()
            .and_then(|_| enclosing_declaration_node(callsite_node)),
        caller_source,
        &source_slice(caller_source, binding.local_binding_range),
        Some(caller.caller_range),
    );
    if !caller_reasons.is_empty() {
        return unresolved_result(
            ReasonSet::new(caller_reasons),
            &[
                "c.syntax",
                "c.caller",
                "c.import_form",
                "c.local_binding",
                "c.shadow",
                "c.writes",
            ],
        );
    }
    let candidate_reasons = relative_candidate_reasons(&candidate_entries, tree_complete);
    if !candidate_reasons.is_empty() {
        return unresolved_result(
            ReasonSet::new(candidate_reasons),
            &[
                "c.syntax",
                "c.caller",
                "c.import_form",
                "c.local_binding",
                "c.specifier",
                "c.candidates",
            ],
        );
    }
    let resolved = match export_resolution {
        Ok(resolved) => resolved,
        Err(reasons) => {
            return unresolved_result(
                reasons,
                &[
                    "c.syntax",
                    "c.caller",
                    "c.import_form",
                    "c.local_binding",
                    "c.specifier",
                    "c.candidates",
                    "c.export_binding",
                ],
            );
        }
    };
    let callee_name = source_slice(callee_source, resolved.local_binding_range());
    let callee_reasons = binding_reasons(
        Some(callee_root),
        callee_source,
        &callee_name,
        Some(resolved.callee_declaration_range()),
    );
    if !callee_reasons.is_empty() {
        return unresolved_result(
            ReasonSet::new(callee_reasons),
            &[
                "c.syntax",
                "c.caller",
                "c.import_form",
                "c.local_binding",
                "c.specifier",
                "c.candidates",
                "c.export_binding",
                "c.shadow",
                "c.writes",
            ],
        );
    }
    RelativeCallResult {
        edge: Some(RelativeCallEdge {
            caller_id: caller.caller_id,
            callee_id: resolved.callee_id().clone(),
            callsite_key,
            resolution_kind: ResolutionKind::SyntacticUniqueRelativeImportV1,
            evaluated_stages: call_stage_trace(&[
                "c.syntax",
                "c.caller",
                "c.import_form",
                "c.local_binding",
                "c.specifier",
                "c.candidates",
                "c.export_binding",
                "c.shadow",
                "c.writes",
                "c.resolution",
            ]),
        }),
        unresolved: None,
        candidate_paths: candidate_entries,
        evaluated_stages: call_stage_trace(&[
            "c.syntax",
            "c.caller",
            "c.import_form",
            "c.local_binding",
            "c.specifier",
            "c.candidates",
            "c.export_binding",
            "c.shadow",
            "c.writes",
            "c.resolution",
        ]),
    }
}

/// Computes write/shadow/eval reasons only inside `caller.caller_range`, using
/// AST bindings, updates, destructuring, catch bindings, and direct eval/with
/// nodes. It must not inspect source text with substring matching.
pub fn caller_binding_reasons(
    _caller: &CallerScope,
    _caller_node: Node<'_>,
    _source: &[u8],
    _binding: &ImportBinding,
) -> ReasonSet {
    if node_range(_caller_node) != _caller.caller_range {
        return ReasonSet::new([CallReason::UnsupportedCaller]);
    }
    ReasonSet::new(binding_reasons(
        Some(_caller_node),
        _source,
        &source_slice(_source, _binding.local_binding_range),
        Some(_caller.caller_range),
    ))
}

/// Computes the callee-side binding reasons from the resolved declaration AST.
/// Callee writes, shadows, and eval/with invalidate the relative edge too
///.
pub fn callee_binding_reasons(
    _callee_declaration: Node<'_>,
    _callee_source: &[u8],
    _callee_id: &DeclarationId,
    _binding: &ImportBinding,
) -> ReasonSet {
    let Some(name) = binding_name(_callee_declaration, _callee_source) else {
        return ReasonSet::new([CallReason::ExportBindingUnsupported]);
    };
    let mut root = _callee_declaration;
    while let Some(parent) = root.parent() {
        root = parent;
    }
    ReasonSet::new(binding_reasons(
        Some(root),
        _callee_source,
        &name,
        Some(node_range(_callee_declaration)),
    ))
}

fn unresolved(
    caller_id: Option<CallerId>,
    callsite_key: CallsiteId,
    source_range: SourceRange,
    reasons: impl IntoIterator<Item = CallReason>,
    stage_wires: &[&str],
) -> UnresolvedCall {
    UnresolvedCall {
        caller_id,
        callsite_key,
        source_range,
        reasons: ReasonSet::new(reasons),
        evaluated_stages: call_stage_trace(stage_wires),
    }
}

fn call_stage_trace(stage_wires: &[&str]) -> Vec<CallStageV1> {
    let binding = typescript_registry_binding();
    stage_wires
        .iter()
        .map(|stage| CallStageV1::parse_wire(&binding, stage).expect("frozen call stage"))
        .collect()
}

fn import_call_stage_prefix(attempted: ImportCallAttempt) -> &'static [&'static str] {
    match attempted {
        ImportCallAttempt::Caller => &["c.syntax", "c.caller"],
        ImportCallAttempt::Form => &["c.syntax", "c.caller", "c.import_form"],
        ImportCallAttempt::LocalBinding => {
            &["c.syntax", "c.caller", "c.import_form", "c.local_binding"]
        }
        ImportCallAttempt::Specifier => &[
            "c.syntax",
            "c.caller",
            "c.import_form",
            "c.local_binding",
            "c.specifier",
        ],
        ImportCallAttempt::Candidates => &[
            "c.syntax",
            "c.caller",
            "c.import_form",
            "c.local_binding",
            "c.specifier",
            "c.candidates",
        ],
        ImportCallAttempt::ExportBinding => &[
            "c.syntax",
            "c.caller",
            "c.import_form",
            "c.local_binding",
            "c.specifier",
            "c.candidates",
            "c.export_binding",
        ],
    }
}

fn classify_callable(
    node: Node<'_>,
    source: &[u8],
    file_id: SourceFileId,
) -> CallableClassification {
    if is_supported_callable(node, source) && is_top_level_callable(node) {
        CallableClassification::Callable(DeclarationId::from_source(file_id, node_range(node)))
    } else {
        CallableClassification::Rejected(ReasonSet::new([CallReason::UnsupportedCaller]))
    }
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

fn enclosing_caller<'tree>(
    call: Node<'tree>,
    source: &[u8],
    file_id: SourceFileId,
) -> (Option<CallerId>, Option<Node<'tree>>) {
    let mut current = call.parent();
    let mut nested_callable = false;
    while let Some(node) = current {
        if matches!(
            node.kind(),
            "arrow_function" | "function_expression" | "generator_function"
        ) {
            nested_callable = true;
        }
        if matches!(
            node.kind(),
            "function_declaration" | "generator_function_declaration" | "variable_declarator"
        ) {
            if nested_callable && !matches!(node.kind(), "variable_declarator") {
                return (None, None);
            }
            if let CallableClassification::Callable(id) =
                classify_callable(node, source, file_id.clone())
            {
                return (Some(CallerId::from_declaration(id)), Some(node));
            }
            if matches!(
                node.kind(),
                "function_declaration" | "generator_function_declaration"
            ) {
                return (None, None);
            }
        }
        if matches!(node.kind(), "method_definition" | "class_declaration") {
            return (None, None);
        }
        current = node.parent();
    }
    (None, None)
}

fn enclosing_declaration_node(call: Node<'_>) -> Option<Node<'_>> {
    let mut current = call.parent();
    while let Some(node) = current {
        if matches!(
            node.kind(),
            "function_declaration" | "generator_function_declaration" | "variable_declarator"
        ) && is_top_level_callable(node)
        {
            return Some(node);
        }
        current = node.parent();
    }
    None
}

fn relative_candidate_reasons(entries: &[RelativeEntry], tree_complete: bool) -> Vec<CallReason> {
    let mut reasons = Vec::new();
    if entries.len() > 1 {
        reasons.push(CallReason::RelativeTargetAmbiguous);
    }
    if entries
        .iter()
        .any(|entry| entry.outcome == RelativeEntryOutcome::Excluded)
    {
        reasons.push(CallReason::RelativeTargetExcluded);
    }
    if entries.iter().any(|entry| {
        matches!(
            entry.outcome,
            RelativeEntryOutcome::Unread | RelativeEntryOutcome::ParseFailed
        )
    }) {
        reasons.push(CallReason::RelativeTargetUnread);
    }
    if entries
        .iter()
        .any(|entry| entry.outcome == RelativeEntryOutcome::ParseFailed)
    {
        reasons.push(CallReason::ParseFailure);
    }
    if !tree_complete {
        reasons.push(CallReason::RelativeTargetUnread);
    }
    if entries.is_empty() && tree_complete {
        reasons.push(CallReason::RelativeTargetMissing);
    }
    reasons
}

fn binding_reasons(
    root: Option<Node<'_>>,
    source: &[u8],
    name: &str,
    declaration_range: Option<SourceRange>,
) -> Vec<CallReason> {
    let Some(root) = root else {
        return vec![CallReason::UnsupportedCaller];
    };
    let mut reasons = Vec::new();
    collect_binding_reasons(root, source, name, declaration_range, &mut reasons);
    reasons
}

fn collect_binding_reasons(
    node: Node<'_>,
    source: &[u8],
    name: &str,
    declaration_range: Option<SourceRange>,
    reasons: &mut Vec<CallReason>,
) {
    match node.kind() {
        "with_statement" => reasons.push(CallReason::UnsupportedSyntax),
        "call_expression"
            if node
                .child_by_field_name("function")
                .is_some_and(|function| {
                    function.kind() == "identifier" && node_text(function, source) == "eval"
                }) =>
        {
            reasons.push(CallReason::UnsupportedSyntax)
        }
        "assignment_expression" | "augmented_assignment_expression"
            if node
                .child_by_field_name("left")
                .is_some_and(|left| contains_identifier(left, source, name)) =>
        {
            reasons.push(CallReason::WrittenBinding);
        }
        "update_expression"
            if node
                .child_by_field_name("argument")
                .is_some_and(|argument| contains_identifier(argument, source, name)) =>
        {
            reasons.push(CallReason::WrittenBinding);
        }
        "variable_declarator"
            if Some(node_range(node)) != declaration_range
                && node
                    .child_by_field_name("name")
                    .is_some_and(|binding| contains_identifier(binding, source, name)) =>
        {
            reasons.push(CallReason::ShadowedBinding);
        }
        "formal_parameters" | "catch_clause" if contains_identifier(node, source, name) => {
            reasons.push(CallReason::ShadowedBinding);
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_binding_reasons(child, source, name, declaration_range, reasons);
    }
}

fn collect_write_reasons(node: Node<'_>, source: &[u8], name: &str, reasons: &mut Vec<CallReason>) {
    match node.kind() {
        "assignment_expression" | "augmented_assignment_expression"
            if node
                .child_by_field_name("left")
                .is_some_and(|left| contains_identifier(left, source, name)) =>
        {
            reasons.push(CallReason::WrittenBinding);
        }
        "update_expression"
            if node
                .child_by_field_name("argument")
                .is_some_and(|argument| contains_identifier(argument, source, name)) =>
        {
            reasons.push(CallReason::WrittenBinding);
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_write_reasons(child, source, name, reasons);
    }
}

fn contains_identifier(node: Node<'_>, source: &[u8], name: &str) -> bool {
    if matches!(
        node.kind(),
        "identifier" | "shorthand_property_identifier_pattern"
    ) && node_text(node, source) == name
    {
        return true;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .any(|child| contains_identifier(child, source, name))
}

fn binding_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .map(|name| node_text(name, source).to_owned())
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

fn is_top_level_callable(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    match parent.kind() {
        "program" => true,
        "export_statement" => parent
            .parent()
            .is_some_and(|grandparent| grandparent.kind() == "program"),
        "lexical_declaration" => parent.parent().is_some_and(|grandparent| {
            grandparent.kind() == "program"
                || (grandparent.kind() == "export_statement"
                    && grandparent
                        .parent()
                        .is_some_and(|program| program.kind() == "program"))
        }),
        _ => false,
    }
}

fn contains(outer: SourceRange, inner: SourceRange) -> bool {
    outer.start() <= inner.start() && inner.end() <= outer.end()
}

fn node_range(node: Node<'_>) -> SourceRange {
    SourceRange::new(node.start_byte() as u64, node.end_byte() as u64)
        .expect("tree-sitter node ranges are ordered")
}

fn source_slice(source: &[u8], range: SourceRange) -> String {
    node_text_bytes(&source[range.start() as usize..range.end() as usize]).to_owned()
}

fn node_text<'source>(node: Node<'_>, source: &'source [u8]) -> &'source str {
    node_text_bytes(&source[node.byte_range()])
}

fn node_text_bytes(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("TypeScript parser receives UTF-8 source")
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
