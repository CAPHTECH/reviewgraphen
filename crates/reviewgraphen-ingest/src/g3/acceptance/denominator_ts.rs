//! G3-denominator-ts@1: independently fixed TypeScript source denominator.
//! This test intentionally imports a separately owned, not-yet-present helper.

use crate::g3::g3_test_support::typescript_denominator::{
    TypeScriptDenominatorError, TypeScriptDenominatorKeyV1, TypeScriptOracleRangeV1,
    typescript_denominator,
};
use reviewgraphen_core::ContentHash;
use reviewgraphen_core::source_review::ids::{CanonicalFileKey, SourceFileId, SourceRange};
use std::collections::BTreeSet;
use tree_sitter::Parser;

const SOURCE: &str = concat!(
    "export function callee() {}\n",
    "export function caller() {\n",
    "  let value = 0;\n",
    "  value = 1;\n",
    "  callee();\n",
    "}\n",
    "it(\"marker\", () => {});\n",
);
const PATH: &str = "src/structural.ts";
const LANGUAGE: &str = "typescript";
const SNAPSHOT: &str = "g3-denominator-ts-fixture-snapshot@1";
const SNAPSHOT_SHA256: &str =
    "sha256:0c13cf753f15806afd780d5adbd217725f32a2de23d88765b2edb8877ceb8278";
const SOURCE_SHA256: &str = "eca129e7be017d0a741b7a35a3183bccb61ae6753916ec83209a4c651c1cf9d7";
const SOURCE_LENGTH: usize = 123;

fn oracle_range(start: u64, end: u64) -> TypeScriptOracleRangeV1 {
    // The independent oracle's coordinates must remain valid half-open byte
    // positions in this exact source, never Rust line/column coordinates.
    let checked = SourceRange::new(start, end).expect("frozen half-open byte range");
    assert_eq!((checked.start(), checked.end()), (start, end));
    assert!(end <= SOURCE_LENGTH as u64);
    TypeScriptOracleRangeV1::new(start, end)
}

fn assert_syntax_valid(bytes: &[u8]) {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .expect("locked TypeScript grammar");
    let tree = parser.parse(bytes, None).expect("parser returns a tree");
    assert!(
        !tree.root_node().has_error(),
        "negative must not be parse-confounded"
    );
}

#[test]
fn frozen_fixture_identity_and_complete_five_row_denominator() {
    let bytes = SOURCE.as_bytes();
    assert_eq!(LANGUAGE, "typescript");
    assert_eq!(bytes.len(), SOURCE_LENGTH);

    // This is a test-owned canonical file key, not an A0/I3 admission result.
    let file_id = SourceFileId::from_basis_file_key(
        CanonicalFileKey::from_basis_path(PATH).expect("canonical fixture path"),
    );
    assert_eq!(
        file_id.canonical_key(),
        r#"{"basis_file_key":"src/structural.ts","domain":"source_review_identity.v1","kind":"source_file"}"#
    );
    // The snapshot is only a fixture label: hashing it does NOT establish
    // A0/I3 admission. Neither label nor path/hash enters the byte-only helper.
    assert_eq!(
        ContentHash::sha256(SNAPSHOT.as_bytes()).as_str(),
        SNAPSHOT_SHA256
    );
    assert_eq!(
        ContentHash::sha256(bytes).as_str(),
        format!("sha256:{SOURCE_SHA256}")
    );
    assert_syntax_valid(bytes);

    // Complete set equality detects missing, extra, or shifted rows. The `it`
    // call is only a generic Call candidate, never a test-framework fact.
    let expected = BTreeSet::from([
        TypeScriptDenominatorKeyV1::Declaration {
            range: oracle_range(7, 27),
        },
        TypeScriptDenominatorKeyV1::Containment {
            parent: oracle_range(0, 123),
            child: oracle_range(35, 98),
        },
        TypeScriptDenominatorKeyV1::DirectCall {
            range: oracle_range(87, 95),
        },
        TypeScriptDenominatorKeyV1::Write {
            range: oracle_range(74, 83),
        },
        TypeScriptDenominatorKeyV1::CallCandidate {
            range: oracle_range(99, 121),
        },
    ]);
    assert_eq!(expected.len(), 5);
    assert_eq!(
        typescript_denominator(bytes).expect("frozen fixture accepted"),
        expected
    );
}

#[test]
fn removing_only_final_lf_is_typed_wrong_length() {
    let bytes = SOURCE.as_bytes();
    assert_eq!(bytes.len(), SOURCE_LENGTH);
    assert_eq!(bytes.last(), Some(&b'\n'));
    let without_final_lf = &bytes[..bytes.len() - 1];
    assert_syntax_valid(without_final_lf);
    assert!(matches!(
        typescript_denominator(without_final_lf),
        Err(TypeScriptDenominatorError::WrongLength {
            expected: 123,
            actual: 122
        })
    ));
}

#[test]
fn same_length_valid_source_with_changed_write_is_typed_wrong_hash() {
    let bytes = SOURCE.as_bytes();
    assert_eq!(&bytes[74..83], b"value = 1");
    let mut changed = bytes.to_vec();
    changed[82] = b'2';
    assert_eq!(&changed[74..83], b"value = 2");
    assert_eq!(changed.len(), SOURCE_LENGTH);
    assert_syntax_valid(&changed);
    let changed_hash = ContentHash::sha256(&changed);
    assert_eq!(
        changed_hash.as_str(),
        "sha256:8459012db52d85800fa2b52b0a00d43a01b57cb6d2bf514fe64ea13a5ec1d987"
    );
    assert_ne!(changed_hash, ContentHash::sha256(bytes));
    assert!(matches!(
        typescript_denominator(&changed),
        Err(TypeScriptDenominatorError::WrongHash { .. })
    ));
}
