//! Acceptance for the minimal, source-derived declaration identity constructor.

use reviewgraphen_core::source_review::ids::{
    CanonicalFileKey, DeclarationId, SourceFileId, SourceRange,
};

fn source_file_id(basis_path: &str) -> SourceFileId {
    SourceFileId::from_basis_file_key(
        CanonicalFileKey::from_basis_path(basis_path).expect("fixed basis path must be valid"),
    )
}

#[test]
fn declaration_id_is_exactly_the_admitted_file_and_declaration_range() {
    let admitted_file = source_file_id("src/entry.ts");
    let other_admitted_file = source_file_id("src/other.ts");
    let declaration_range = SourceRange::new(12, 48).expect("fixed range must be valid");
    let other_declaration_range = SourceRange::new(13, 48).expect("fixed range must be valid");

    let declaration = DeclarationId::from_source(admitted_file.clone(), declaration_range);

    assert_eq!(
        declaration,
        DeclarationId::from_source(admitted_file.clone(), declaration_range)
    );
    assert_ne!(
        declaration,
        DeclarationId::from_source(other_admitted_file, declaration_range)
    );
    assert_ne!(
        declaration,
        DeclarationId::from_source(admitted_file, other_declaration_range)
    );
}
