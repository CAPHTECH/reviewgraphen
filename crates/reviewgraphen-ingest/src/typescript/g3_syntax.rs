//! Identity-free TypeScript G3 syntax observations. This parser admits no source.

use reviewgraphen_core::source_review::ids::SourceRange;
use reviewgraphen_core::source_review::reasons::TypeScriptSyntaxKind;
use tree_sitter::{Node, Parser};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum TypeScriptLhsKindV1 {
    Identifier,
    Field,
    Index,
    Destructure,
    Other,
}

impl TypeScriptLhsKindV1 {
    pub fn wire_literal(self) -> &'static str {
        match self {
            Self::Identifier => "identifier",
            Self::Field => "field",
            Self::Index => "index",
            Self::Destructure => "destructure",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct RawTypeScriptContainmentV1 {
    pub parent_range: SourceRange,
    pub child_range: SourceRange,
    pub child_kind: TypeScriptSyntaxKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct RawTypeScriptAssignmentV1 {
    pub occurrence_range: SourceRange,
    pub lhs_range: SourceRange,
    pub lhs_kind: TypeScriptLhsKindV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct RawTypeScriptCallV1 {
    pub occurrence_range: SourceRange,
    pub kind: TypeScriptSyntaxKind,
}

/// A complete, syntax-only occurrence coordinate, independent of emitted rows.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum RawTypeScriptKeyV1 {
    Containment(RawTypeScriptContainmentV1),
    Assignment(RawTypeScriptAssignmentV1),
    Call(RawTypeScriptCallV1),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum RawTypeScriptExclusionReasonV1 {
    NestedCallable,
    UnsupportedCallableForm,
    CompoundAssignment,
    UpdateExpression,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct RawTypeScriptExclusionV1 {
    pub range: SourceRange,
    pub syntax_kind: String,
    pub reason: RawTypeScriptExclusionReasonV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawTypeScriptFileV1 {
    pub containment: Vec<RawTypeScriptContainmentV1>,
    pub assignments: Vec<RawTypeScriptAssignmentV1>,
    pub calls: Vec<RawTypeScriptCallV1>,
    pub census: Vec<RawTypeScriptKeyV1>,
    pub exclusions: Vec<RawTypeScriptExclusionV1>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawTypeScriptParseObstructionV1 {
    InvalidUtf8,
    ParserUnavailable,
    ParseFailed,
    InvalidRange,
}

/// Parse bytes as the existing `.ts` grammar. Neither input nor output bears file identity.
pub fn parse_g3_syntax(
    bytes: &[u8],
) -> Result<RawTypeScriptFileV1, RawTypeScriptParseObstructionV1> {
    std::str::from_utf8(bytes).map_err(|_| RawTypeScriptParseObstructionV1::InvalidUtf8)?;
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .map_err(|_| RawTypeScriptParseObstructionV1::ParserUnavailable)?;
    let tree = parser
        .parse(bytes, None)
        .ok_or(RawTypeScriptParseObstructionV1::ParseFailed)?;
    let root = tree.root_node();
    if root.has_error() || has_missing(root) {
        return Err(RawTypeScriptParseObstructionV1::ParseFailed);
    }
    let mut result = RawTypeScriptFileV1 {
        containment: Vec::new(),
        assignments: Vec::new(),
        calls: Vec::new(),
        census: Vec::new(),
        exclusions: Vec::new(),
    };
    // A separate AST traversal enumerates the census BEFORE row emission. Its keys
    // retain multiplicity: neither parser nor admission deduplicates occurrences.
    census(root, root, bytes.len(), &mut result.census)?;
    emit(root, root, bytes.len(), &mut result)?;
    Ok(result)
}

fn has_missing(node: Node<'_>) -> bool {
    node.is_missing()
        || (0..node.child_count())
            .any(|index| has_missing(node.child(index).expect("bounded tree child")))
}

fn span(node: Node<'_>, len: usize) -> Result<SourceRange, RawTypeScriptParseObstructionV1> {
    if node.end_byte() > len || node.start_byte() > node.end_byte() {
        return Err(RawTypeScriptParseObstructionV1::InvalidRange);
    }
    SourceRange::new(
        u64::try_from(node.start_byte())
            .map_err(|_| RawTypeScriptParseObstructionV1::InvalidRange)?,
        u64::try_from(node.end_byte())
            .map_err(|_| RawTypeScriptParseObstructionV1::InvalidRange)?,
    )
    .map_err(|_| RawTypeScriptParseObstructionV1::InvalidRange)
}

fn kind(node: Node<'_>) -> Result<TypeScriptSyntaxKind, RawTypeScriptParseObstructionV1> {
    TypeScriptSyntaxKind::parse_wire(node.kind())
        .map_err(|_| RawTypeScriptParseObstructionV1::ParseFailed)
}

fn top_level(node: Node<'_>) -> bool {
    match node.parent() {
        Some(parent) if parent.kind() == "program" => true,
        Some(parent) if parent.kind() == "export_statement" => parent
            .parent()
            .is_some_and(|grandparent| grandparent.kind() == "program"),
        _ => false,
    }
}

fn direct_const_callable(node: Node<'_>) -> bool {
    let Some(declarator) = node.parent() else {
        return false;
    };
    if declarator.kind() != "variable_declarator"
        || declarator.child_by_field_name("value") != Some(node)
    {
        return false;
    }
    let Some(declaration) = declarator.parent() else {
        return false;
    };
    declaration.kind() == "lexical_declaration"
        && (0..declaration.child_count()).any(|i| {
            declaration
                .child(i)
                .is_some_and(|child| child.kind() == "const")
        })
        && top_level(declaration)
}

// Placement alone does not make an initializer a supported native Callable.
// Keep this predicate identical to native_syntax::collect_const_callable's
// supported initializer kinds, including when deciding whether to exclude it.
fn supported_const_callable(node: Node<'_>) -> bool {
    matches!(node.kind(), "arrow_function" | "function_expression") && direct_const_callable(node)
}

fn direct_callable(node: Node<'_>) -> Option<Node<'_>> {
    match node.kind() {
        "function_declaration" | "generator_function_declaration" if top_level(node) => Some(node),
        "variable_declarator" => {
            let value = node.child_by_field_name("value")?;
            supported_const_callable(value).then_some(node)
        }
        "arrow_function" | "function_expression" | "generator_function" => {
            let export = node.parent()?;
            (top_level(node)
                && export.kind() == "export_statement"
                && (0..export.child_count())
                    .any(|i| export.child(i).is_some_and(|c| c.kind() == "default")))
            .then_some(node)
        }
        _ => None,
    }
}

fn lhs_kind(node: Node<'_>) -> TypeScriptLhsKindV1 {
    match node.kind() {
        "identifier" => TypeScriptLhsKindV1::Identifier,
        "member_expression" | "optional_member_expression" => TypeScriptLhsKindV1::Field,
        "subscript_expression" => TypeScriptLhsKindV1::Index,
        "object_pattern" | "array_pattern" | "assignment_pattern" => {
            TypeScriptLhsKindV1::Destructure
        }
        _ => TypeScriptLhsKindV1::Other,
    }
}

fn key(
    node: Node<'_>,
    root: Node<'_>,
    len: usize,
) -> Result<Option<RawTypeScriptKeyV1>, RawTypeScriptParseObstructionV1> {
    if let Some(callable) = direct_callable(node) {
        return Ok(Some(RawTypeScriptKeyV1::Containment(
            RawTypeScriptContainmentV1 {
                parent_range: span(root, len)?,
                child_range: span(callable, len)?,
                child_kind: kind(callable)?,
            },
        )));
    }
    match node.kind() {
        "assignment_expression" => {
            let lhs = node
                .child_by_field_name("left")
                .ok_or(RawTypeScriptParseObstructionV1::ParseFailed)?;
            Ok(Some(RawTypeScriptKeyV1::Assignment(
                RawTypeScriptAssignmentV1 {
                    occurrence_range: span(node, len)?,
                    lhs_range: span(lhs, len)?,
                    lhs_kind: lhs_kind(lhs),
                },
            )))
        }
        "call_expression" => Ok(Some(RawTypeScriptKeyV1::Call(RawTypeScriptCallV1 {
            occurrence_range: span(node, len)?,
            kind: kind(node)?,
        }))),
        _ => Ok(None),
    }
}

fn census(
    node: Node<'_>,
    root: Node<'_>,
    len: usize,
    keys: &mut Vec<RawTypeScriptKeyV1>,
) -> Result<(), RawTypeScriptParseObstructionV1> {
    if let Some(key) = key(node, root, len)? {
        keys.push(key);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        census(child, root, len, keys)?;
    }
    Ok(())
}

fn emit(
    node: Node<'_>,
    root: Node<'_>,
    len: usize,
    raw: &mut RawTypeScriptFileV1,
) -> Result<(), RawTypeScriptParseObstructionV1> {
    if let Some(key) = key(node, root, len)? {
        match key {
            RawTypeScriptKeyV1::Containment(row) => raw.containment.push(row),
            RawTypeScriptKeyV1::Assignment(row) => raw.assignments.push(row),
            RawTypeScriptKeyV1::Call(row) => raw.calls.push(row),
        }
    } else {
        let reason = match node.kind() {
            "augmented_assignment_expression" => {
                Some(RawTypeScriptExclusionReasonV1::CompoundAssignment)
            }
            "update_expression" => Some(RawTypeScriptExclusionReasonV1::UpdateExpression),
            "function_declaration" | "generator_function_declaration" => {
                Some(RawTypeScriptExclusionReasonV1::NestedCallable)
            }
            "arrow_function" | "function_expression" | "generator_function"
                if !supported_const_callable(node) && direct_callable(node).is_none() =>
            {
                Some(RawTypeScriptExclusionReasonV1::UnsupportedCallableForm)
            }
            _ => None,
        };
        if let Some(reason) = reason {
            raw.exclusions.push(RawTypeScriptExclusionV1 {
                range: span(node, len)?,
                syntax_kind: node.kind().to_owned(),
                reason,
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        emit(child, root, len, raw)?;
    }
    Ok(())
}

#[cfg(test)]
mod unsupported_callable_tests {
    use super::{RawTypeScriptExclusionReasonV1, parse_g3_syntax};
    use tree_sitter::{Node, Parser};

    fn initializer<'a>(root: Node<'a>) -> Node<'a> {
        let mut cursor = root.walk();
        let declaration = root
            .named_children(&mut cursor)
            .next()
            .expect("one declaration");
        let mut cursor = declaration.walk();
        let declarator = declaration
            .named_children(&mut cursor)
            .next()
            .expect("const declarator");
        declarator
            .child_by_field_name("value")
            .expect("initializer node")
    }

    #[test]
    fn direct_const_generator_is_explicitly_excluded_not_admitted() {
        let source = b"const generator = function* () {};\n";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .unwrap();
        let tree = parser.parse(source, None).expect("independent fixture AST");
        assert!(!tree.root_node().has_error());
        let generator = initializer(tree.root_node());
        assert_eq!(generator.kind(), "generator_function");
        let span = generator.byte_range();
        assert_eq!(&source[span.clone()], b"function* () {}");

        let raw = parse_g3_syntax(source).expect("valid raw TypeScript syntax");
        assert!(
            raw.containment.is_empty(),
            "generator is not a native direct Callable"
        );
        assert!(
            !raw.census
                .iter()
                .any(|key| format!("{key:?}").contains("Containment"))
        );
        assert_eq!(
            raw.exclusions
                .iter()
                .filter(|entry| {
                    entry.syntax_kind == "generator_function"
                        && entry.range.start() == span.start as u64
                        && entry.range.end() == span.end as u64
                        && entry.reason == RawTypeScriptExclusionReasonV1::UnsupportedCallableForm
                })
                .count(),
            1,
            "unsupported direct const initializer must be counted once"
        );
    }
}
