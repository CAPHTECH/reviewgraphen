//! Compile-red acceptance for the V2 admitted Parsed canonical-basis-path seam.

use reviewgraphen_core::source_review::admitted_source::{
    AdmittedSourceBundleV1, AdmittedSourceFileV1,
};
use reviewgraphen_core::source_review::basis::{
    SourceFileOutcome, SourceReviewBasisV1, SourceReviewFileV1,
};
use reviewgraphen_core::source_review::ids::{CanonicalFileKey, SourceFileId, SourceHash};
use reviewgraphen_core::source_review::registry::typescript_registry_binding;

const MAIN_PATH: &str = "src/main.ts";
const DEPENDENCY_PATH: &str = "src/dependency.ts";
const UNADMITTED_PATH: &str = "src/unadmitted.ts";
const MAIN_BYTES: &[u8] = b"export function main() {}\n";
const DEPENDENCY_BYTES: &[u8] = b"export function dependency() {}\n";

fn id_from_fixed_basis_path(path: &str) -> SourceFileId {
    SourceFileId::from_basis_file_key(
        CanonicalFileKey::from_basis_path(path).expect("fixed canonical basis path is valid"),
    )
}

#[test]
fn admitted_parsed_files_keep_their_fixed_canonical_basis_paths() {
    let main_id = id_from_fixed_basis_path(MAIN_PATH);
    let dependency_id = id_from_fixed_basis_path(DEPENDENCY_PATH);
    let unadmitted_id = id_from_fixed_basis_path(UNADMITTED_PATH);
    assert_ne!(main_id, dependency_id);

    let basis = SourceReviewBasisV1::new(
        typescript_registry_binding(),
        vec![
            SourceReviewFileV1 {
                path: MAIN_PATH.into(),
                language: "typescript".into(),
                outcome: SourceFileOutcome::Parsed,
            },
            SourceReviewFileV1 {
                path: DEPENDENCY_PATH.into(),
                language: "typescript".into(),
                outcome: SourceFileOutcome::Parsed,
            },
        ],
        vec![],
    )
    .expect("fixed registry-bound parsed basis is valid");
    let bundle = AdmittedSourceBundleV1::new(
        &basis,
        vec![
            AdmittedSourceFileV1 {
                file_id: main_id.clone(),
                bytes: MAIN_BYTES.to_vec(),
                source_hash: SourceHash::from_source_bytes(MAIN_BYTES),
            },
            AdmittedSourceFileV1 {
                file_id: dependency_id.clone(),
                bytes: DEPENDENCY_BYTES.to_vec(),
                source_hash: SourceHash::from_source_bytes(DEPENDENCY_BYTES),
            },
        ],
    )
    .expect("two fixed parsed sources are admitted");

    assert_eq!(bundle.canonical_basis_path(&main_id), Some(MAIN_PATH));
    assert_eq!(
        bundle.canonical_basis_path(&dependency_id),
        Some(DEPENDENCY_PATH)
    );
    assert_ne!(bundle.canonical_basis_path(&main_id), Some(DEPENDENCY_PATH));
    assert_ne!(bundle.canonical_basis_path(&dependency_id), Some(MAIN_PATH));
    assert_eq!(bundle.canonical_basis_path(&unadmitted_id), None);

    let main_file = bundle
        .file_by_canonical_basis_path(MAIN_PATH)
        .expect("fixed main path resolves to its admitted record");
    assert_eq!(&main_file.file_id, &main_id);
    assert_eq!(main_file.bytes.as_slice(), MAIN_BYTES);

    let dependency_file = bundle
        .file_by_canonical_basis_path(DEPENDENCY_PATH)
        .expect("fixed dependency path resolves to its admitted record");
    assert_eq!(&dependency_file.file_id, &dependency_id);
    assert_eq!(dependency_file.bytes.as_slice(), DEPENDENCY_BYTES);

    assert_eq!(bundle.file_by_canonical_basis_path(UNADMITTED_PATH), None);
    assert_ne!(
        bundle
            .file_by_canonical_basis_path(MAIN_PATH)
            .map(|file| &file.file_id),
        Some(&dependency_id)
    );
    assert_ne!(
        bundle
            .file_by_canonical_basis_path(DEPENDENCY_PATH)
            .map(|file| &file.file_id),
        Some(&main_id)
    );
}
