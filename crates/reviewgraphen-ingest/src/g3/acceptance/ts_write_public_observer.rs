//! TS-write@2: the public parser has only raw syntax; it cannot bind a Git file.
use crate::typescript::g3_syntax::{
    RawTypeScriptParseObstructionV1, TypeScriptLhsKindV1, parse_g3_syntax,
};
use reviewgraphen_core::source_review::ids::{SourceHash, SourceRange};
use tree_sitter::{Node, Parser};

const SOURCE: &[u8] = b"export function callee() {}\nexport function caller() {\n  let value = 0;\n  value = 1;\n  callee();\n}\nit(\"marker\", () => {});\n";
fn range(start: u64, end: u64) -> SourceRange {
    SourceRange::new(start, end).unwrap()
}

fn ast(bytes: &[u8]) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();
    let tree = parser
        .parse(bytes, None)
        .expect("independent grammar parser");
    assert!(
        !tree.root_node().has_error(),
        "fixture itself is valid syntax"
    );
    tree
}

fn nodes(node: Node<'_>, kind: &str, found: &mut Vec<(usize, usize)>) {
    if node.kind() == kind {
        found.push((node.start_byte(), node.end_byte()));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        nodes(child, kind, found);
    }
}

#[test]
fn raw_public_parser_matches_independently_walked_assignment_without_any_file_identity() {
    assert_eq!(SOURCE.len(), 123);
    assert_eq!(
        SourceHash::from_source_bytes(SOURCE).wire_literal(),
        "sha256:eca129e7be017d0a741b7a35a3183bccb61ae6753916ec83209a4c651c1cf9d7"
    );
    let tree = ast(SOURCE);
    let mut assignments = Vec::new();
    nodes(tree.root_node(), "assignment_expression", &mut assignments);
    assert_eq!(assignments, [(74, 83)]);
    let node = tree.root_node().descendant_for_byte_range(74, 83).unwrap();
    assert_eq!(node.kind(), "assignment_expression");
    let lhs = node.child_by_field_name("left").unwrap();
    assert_eq!(
        (lhs.kind(), lhs.start_byte(), lhs.end_byte()),
        ("identifier", 74, 79)
    );
    assert_eq!(&SOURCE[74..83], b"value = 1");
    let raw = parse_g3_syntax(SOURCE).expect("identity-free raw syntax only");
    assert_eq!(raw.assignments.len(), 1);
    assert_eq!(raw.assignments[0].occurrence_range, range(74, 83));
    assert_eq!(raw.assignments[0].lhs_range, range(74, 79));
    assert_eq!(raw.assignments[0].lhs_kind, TypeScriptLhsKindV1::Identifier);
    assert_eq!(
        raw.census
            .iter()
            .filter(|key| format!("{key:?}").contains("Assignment"))
            .count(),
        1
    );
    let again = parse_g3_syntax(SOURCE).unwrap();
    assert_eq!(again.assignments.len(), raw.assignments.len());
    assert_eq!(
        again.assignments[0].occurrence_range,
        raw.assignments[0].occurrence_range
    );
    assert_eq!(again.assignments[0].lhs_range, raw.assignments[0].lhs_range);
    // A public caller can alter these raw values, but cannot turn them into an admitted row:
    // runtime's sole binding stage accepts provenance, not raw syntax submissions.
    let mut forged = parse_g3_syntax(SOURCE).unwrap();
    forged.assignments[0].occurrence_range = range(0, 123);
    assert_ne!(
        forged.assignments[0].occurrence_range, raw.assignments[0].occurrence_range,
        "raw DTO is untrusted syntax, not a capability"
    );
}

#[test]
fn invalid_utf8_parse_error_field_overlap_and_compound_are_raw_syntax_not_admission() {
    assert!(matches!(
        parse_g3_syntax(b"\xff"),
        Err(RawTypeScriptParseObstructionV1::InvalidUtf8)
    ));
    assert!(matches!(
        parse_g3_syntax(b"let = ;\n"),
        Err(RawTypeScriptParseObstructionV1::ParseFailed)
    ));
    let field = b"let holder = { value: 0 }; holder.value = 1;\n";
    let tree = ast(field);
    let mut assignments = Vec::new();
    nodes(tree.root_node(), "assignment_expression", &mut assignments);
    assert_eq!(assignments.len(), 1);
    let expr = tree
        .root_node()
        .descendant_for_byte_range(assignments[0].0, assignments[0].1)
        .unwrap();
    assert_eq!(
        expr.child_by_field_name("left").unwrap().kind(),
        "member_expression"
    );
    let raw = parse_g3_syntax(field).unwrap();
    assert_eq!(raw.assignments.len(), 1);
    assert_eq!(raw.assignments[0].lhs_kind, TypeScriptLhsKindV1::Field);
    let lhs = raw.assignments[0].lhs_range;
    assert_eq!(
        &field[lhs.start() as usize..lhs.end() as usize],
        b"holder.value"
    );
    let overlap = b"let a = 0, b = 0; a = b = 1; a += 1; a++;\n";
    let tree = ast(overlap);
    let mut parsed = Vec::new();
    nodes(tree.root_node(), "assignment_expression", &mut parsed);
    assert_eq!(parsed.len(), 2, "nested assignments both exist in the AST");
    assert!(parsed[0].0 <= parsed[1].0 && parsed[0].1 >= parsed[1].1);
    let raw = parse_g3_syntax(overlap).expect("raw parser does not decide overlap authority");
    assert_eq!(
        raw.assignments
            .iter()
            .map(|a| (
                a.occurrence_range.start() as usize,
                a.occurrence_range.end() as usize
            ))
            .collect::<Vec<_>>(),
        parsed
    );
    assert!(
        raw.exclusions
            .iter()
            .any(|e| format!("{:?}", e.reason).contains("CompoundAssignment"))
    );
    assert!(
        raw.exclusions
            .iter()
            .any(|e| format!("{:?}", e.reason).contains("UpdateExpression"))
    );
}
