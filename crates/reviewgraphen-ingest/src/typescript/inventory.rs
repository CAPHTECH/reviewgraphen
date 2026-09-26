//! File inventory precedes parsing and preserves unread/unknown denominators.

use super::syntax::parse_typescript;
use reviewgraphen_core::{
    ContentHash,
    typescript::profile::{
        TYPESCRIPT_LANGUAGE, TypeScriptPathClassification, classify_typescript_v1_path,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreeEntryKind {
    Regular,
    Symlink,
    Submodule,
    Other,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventoryEntry {
    pub path: String,
    pub kind: TreeEntryKind,
    pub bytes: Option<Vec<u8>>,
}
impl InventoryEntry {
    #[must_use]
    pub fn new(path: impl Into<String>, kind: TreeEntryKind, bytes: Option<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            kind,
            bytes,
        }
    }
    #[must_use]
    pub fn regular(path: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self::new(path, TreeEntryKind::Regular, Some(bytes))
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InventoryLimits {
    pub max_files: usize,
    pub max_file_bytes: usize,
    pub max_total_source_bytes: usize,
}
impl InventoryLimits {
    #[must_use]
    pub const fn new(
        max_files: usize,
        max_file_bytes: usize,
        max_total_source_bytes: usize,
    ) -> Self {
        Self {
            max_files,
            max_file_bytes,
            max_total_source_bytes,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileOutcome {
    NonTargetExtension,
    ProfileExcluded,
    Parsed,
    ParseFailed,
    UnreadBound,
    UnsupportedEntry,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileRecord {
    pub path: String,
    pub language: Option<String>,
    pub outcome: FileOutcome,
    pub bytes_read: bool,
    pub byte_count: Option<usize>,
    pub source_hash: Option<ContentHash>,
    pub latent_callable_count: Option<usize>,
}

pub fn inventory(mut entries: Vec<InventoryEntry>, limits: InventoryLimits) -> Vec<FileRecord> {
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    let mut total = 0_usize;
    let mut read = 0_usize;
    entries
        .into_iter()
        .map(|entry| match classify_typescript_v1_path(&entry.path) {
            Err(_) => record(
                entry.path,
                None,
                FileOutcome::UnsupportedEntry,
                false,
                None,
                None,
                None,
            ),
            Ok(TypeScriptPathClassification::NonTargetExtension) => record(
                entry.path,
                None,
                FileOutcome::NonTargetExtension,
                false,
                None,
                None,
                None,
            ),
            Ok(TypeScriptPathClassification::ProfileExcluded(_)) => record(
                entry.path,
                None,
                FileOutcome::ProfileExcluded,
                false,
                None,
                None,
                None,
            ),
            Ok(TypeScriptPathClassification::Included) => match (entry.kind, entry.bytes) {
                (TreeEntryKind::Regular, Some(bytes))
                    if read < limits.max_files
                        && bytes.len() <= limits.max_file_bytes
                        && total.saturating_add(bytes.len()) <= limits.max_total_source_bytes =>
                {
                    read += 1;
                    total += bytes.len();
                    let parsed = parse_typescript(&entry.path, &bytes);
                    let outcome = if parsed.is_parsed() {
                        FileOutcome::Parsed
                    } else {
                        FileOutcome::ParseFailed
                    };
                    record(
                        entry.path,
                        Some(TYPESCRIPT_LANGUAGE.into()),
                        outcome,
                        true,
                        Some(bytes.len()),
                        Some(ContentHash::sha256(&bytes)),
                        parsed.latent_callable_count,
                    )
                }
                (TreeEntryKind::Regular, _) => record(
                    entry.path,
                    Some(TYPESCRIPT_LANGUAGE.into()),
                    FileOutcome::UnreadBound,
                    false,
                    None,
                    None,
                    None,
                ),
                _ => record(
                    entry.path,
                    None,
                    FileOutcome::UnsupportedEntry,
                    false,
                    None,
                    None,
                    None,
                ),
            },
        })
        .collect()
}
fn record(
    path: String,
    language: Option<String>,
    outcome: FileOutcome,
    bytes_read: bool,
    byte_count: Option<usize>,
    source_hash: Option<ContentHash>,
    latent_callable_count: Option<usize>,
) -> FileRecord {
    FileRecord {
        path,
        language,
        outcome,
        bytes_read,
        byte_count,
        source_hash,
        latent_callable_count,
    }
}
