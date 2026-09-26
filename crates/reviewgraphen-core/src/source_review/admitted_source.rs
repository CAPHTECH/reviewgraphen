//! Basis-bound source bytes for independently rebuilding source-review drafts.

use super::basis::SourceFileOutcome;
use super::basis::SourceReviewBasisV1;
use super::ids::{CanonicalFileKey, SourceFileId, SourceHash, SourceIdentityError};
use std::collections::BTreeMap;
use thiserror::Error;

/// One immutable byte sequence admitted for a parsed basis file.
///
/// `source_hash` must be the hash of exactly `bytes`; `file_id` must be the
/// identity derived from the corresponding basis file key.  This is input to
/// [`AdmittedSourceBundleV1::new`], not an admitted source by itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedSourceFileV1 {
    pub file_id: SourceFileId,
    pub bytes: Vec<u8>,
    pub source_hash: SourceHash,
}

/// The immutable source material required to rebuild a basis-bound draft.
///
/// The private representation prevents a caller from bypassing the admission
/// check. The constructor admits exactly one hash-matching byte sequence for
/// every `Parsed` basis file, and rejects a missing, duplicate, unknown, or
/// non-parsed file. Therefore an adapter can parse these bytes and compare a
/// source-derived draft without treating a submitted report as its source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedSourceBundleV1 {
    files: Vec<AdmittedSourceFileV1>,
    canonical_basis_paths: BTreeMap<SourceFileId, String>,
    target_outcomes: BTreeMap<String, SourceFileOutcome>,
}

/// Source-bundle admission failed before a source-derived draft could exist.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AdmittedSourceBundleError {
    #[error(transparent)]
    InvalidBasisIdentity(#[from] SourceIdentityError),
    #[error("source bundle contains a file that is absent from the admitted basis")]
    UnknownBasisFile { file_id: SourceFileId },
    #[error("source bundle contains bytes for a basis file whose outcome is not parsed")]
    SourceForUnparsedFile { file_id: SourceFileId },
    #[error("source bundle is missing bytes for a parsed admitted basis file")]
    MissingParsedFile { file_id: SourceFileId },
    #[error("source bundle contains more than one byte sequence for an admitted file")]
    DuplicateFile { file_id: SourceFileId },
    #[error("source bundle bytes do not match the admitted source hash")]
    HashMismatch { file_id: SourceFileId },
}

impl AdmittedSourceBundleV1 {
    /// Binds source bytes to `basis` before any extraction or payload entry
    /// point can run.
    ///
    /// The accepted set is exactly the parsed file set in `basis`: unknown
    /// files, bytes for every other [`super::basis::SourceFileOutcome`], omissions,
    /// duplicates, and `SourceHash::from_source_bytes(bytes) != source_hash`
    /// each return the corresponding [`AdmittedSourceBundleError`].
    pub fn new(
        basis: &SourceReviewBasisV1,
        files: Vec<AdmittedSourceFileV1>,
    ) -> Result<Self, AdmittedSourceBundleError> {
        let mut expected = BTreeMap::new();
        let mut target_outcomes = BTreeMap::new();
        for file in &basis.files {
            target_outcomes.insert(file.path.clone(), file.outcome);
            if file.outcome == SourceFileOutcome::Parsed {
                let key = CanonicalFileKey::from_basis_path(&file.path)?;
                expected.insert(SourceFileId::from_basis_file_key(key), file.path.clone());
            }
        }

        let mut admitted = BTreeMap::new();
        for file in files {
            if !expected.contains_key(&file.file_id) {
                return Err(AdmittedSourceBundleError::UnknownBasisFile {
                    file_id: file.file_id,
                });
            }
            if SourceHash::from_source_bytes(&file.bytes) != file.source_hash {
                return Err(AdmittedSourceBundleError::HashMismatch {
                    file_id: file.file_id,
                });
            }
            let file_id = file.file_id.clone();
            if admitted.insert(file_id.clone(), file).is_some() {
                return Err(AdmittedSourceBundleError::DuplicateFile { file_id });
            }
        }
        if let Some(file_id) = expected
            .keys()
            .find(|file_id| !admitted.contains_key(*file_id))
        {
            return Err(AdmittedSourceBundleError::MissingParsedFile {
                file_id: file_id.clone(),
            });
        }

        Ok(Self {
            files: admitted.into_values().collect(),
            canonical_basis_paths: expected,
            target_outcomes,
        })
    }

    /// Returns the sole admitted byte/hash pair for `file_id`, if the basis
    /// admitted that parsed file. Consumers must use this material rather than
    /// caller-supplied substitute bytes.
    #[must_use]
    pub fn file(&self, file_id: &SourceFileId) -> Option<&AdmittedSourceFileV1> {
        self.files.iter().find(|file| &file.file_id == file_id)
    }

    /// Returns the validated canonical basis path that admitted `file_id`.
    #[must_use]
    pub fn canonical_basis_path(&self, file_id: &SourceFileId) -> Option<&str> {
        self.canonical_basis_paths.get(file_id).map(String::as_str)
    }

    /// Returns the admitted record for an exact validated canonical basis path.
    #[must_use]
    pub fn file_by_canonical_basis_path(
        &self,
        canonical_path: &str,
    ) -> Option<&AdmittedSourceFileV1> {
        let file_id = self
            .canonical_basis_paths
            .iter()
            .find_map(|(file_id, path)| (path == canonical_path).then_some(file_id))?;
        self.file(file_id)
    }

    /// Returns every admitted parsed-file byte/hash record in canonical basis
    /// order. Source rebuilders need this finite enumeration to parse every
    /// admitted file; they must not discover files from the host filesystem or
    /// infer an omitted source from a submitted report.
    #[must_use]
    pub fn files(&self) -> &[AdmittedSourceFileV1] {
        &self.files
    }

    /// Returns A0's canonical outcome for an exact target-basis path.  The
    /// lookup never discovers files from the host or a submitted report.
    #[must_use]
    pub fn target_outcome_for_path(&self, canonical_path: &str) -> Option<SourceFileOutcome> {
        self.target_outcomes.get(canonical_path).copied()
    }
}
