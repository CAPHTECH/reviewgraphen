use crate::{ContentHash, DomainError, ProgramSpace, Result, StableId, canonical_json};
use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};

/// Immutable source bytes for one accepted `file:*` ProgramSpace artifact.
///
/// These bytes are snapshot input, not an Evidence record and not an inferred
/// ProgramSpace fact. They exist so later projections can be rebuilt without
/// reopening a mutable workspace.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SnapshotSourceEntry {
    artifact_id: StableId,
    path: String,
    content_hash: ContentHash,
    cas_hash: ContentHash,
    bytes: Vec<u8>,
}

impl SnapshotSourceEntry {
    /// Creates an entry whose hashes are subsequently checked against its
    /// bytes and ProgramSpace artifact by [`SnapshotSourceBundle::new`].
    #[must_use]
    pub fn new(
        artifact_id: StableId,
        path: impl Into<String>,
        content_hash: ContentHash,
        cas_hash: ContentHash,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            artifact_id,
            path: path.into(),
            content_hash,
            cas_hash,
            bytes,
        }
    }

    /// The accepted `file:*` artifact named by these bytes.
    #[must_use]
    pub fn artifact_id(&self) -> &StableId {
        &self.artifact_id
    }

    /// Normalized repository-relative source path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// M2's SHA-256 hash recorded on the corresponding file artifact.
    #[must_use]
    pub fn content_hash(&self) -> &ContentHash {
        &self.content_hash
    }

    /// SHA-256 hash intended for later CAS admission.
    #[must_use]
    pub fn cas_hash(&self) -> &ContentHash {
        &self.cas_hash
    }

    /// Exact immutable bytes read from the snapshot.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Deterministically ordered source bytes for exactly one accepted snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SnapshotSourceBundle {
    snapshot_id: StableId,
    entries: Vec<SnapshotSourceEntry>,
    total_bytes: u64,
}

struct SnapshotSourceEntryStreamingRef<'a>(&'a SnapshotSourceEntry);

impl Serialize for SnapshotSourceEntryStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("SnapshotSourceEntry", 5)?;
        state.serialize_field("artifact_id", &value.artifact_id)?;
        state.serialize_field("bytes", &value.bytes)?;
        state.serialize_field("cas_hash", &value.cas_hash)?;
        state.serialize_field("content_hash", &value.content_hash)?;
        state.serialize_field("path", &value.path)?;
        state.end()
    }
}

struct SnapshotSourceEntriesStreamingRef<'a>(&'a [SnapshotSourceEntry]);

impl Serialize for SnapshotSourceEntriesStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for entry in self.0 {
            sequence.serialize_element(&SnapshotSourceEntryStreamingRef(entry))?;
        }
        sequence.end()
    }
}

struct SnapshotSourceBundleStreamingRef<'a>(&'a SnapshotSourceBundle);

impl Serialize for SnapshotSourceBundleStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("SnapshotSourceBundle", 3)?;
        state.serialize_field(
            "entries",
            &SnapshotSourceEntriesStreamingRef(&value.entries),
        )?;
        state.serialize_field("snapshot_id", &value.snapshot_id)?;
        state.serialize_field("total_bytes", &value.total_bytes)?;
        state.end()
    }
}

impl SnapshotSourceBundle {
    /// Validates source bytes against the file artifacts accepted in the same
    /// ProgramSpace. Construction is all-or-nothing: missing, extra,
    /// duplicate, path-mismatched, or hash-mismatched entries are rejected.
    pub fn new(
        program_space: &ProgramSpace,
        mut entries: Vec<SnapshotSourceEntry>,
    ) -> Result<Self> {
        let expected = program_space
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
            .map(|artifact| {
                let path = artifact
                    .location
                    .as_ref()
                    .map(|location| location.path.clone())
                    .ok_or_else(|| DomainError::InvalidSnapshotSourceBundle {
                        artifact_id: artifact.id.clone(),
                        reason: "accepted file artifact has no source location".to_owned(),
                    })?;
                let content_hash = artifact.content_hash.clone().ok_or_else(|| {
                    DomainError::InvalidSnapshotSourceBundle {
                        artifact_id: artifact.id.clone(),
                        reason: "accepted file artifact has no content hash".to_owned(),
                    }
                })?;
                Ok((artifact.id.clone(), (path, content_hash)))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;

        entries.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.artifact_id.cmp(&right.artifact_id))
        });

        let mut actual_ids = BTreeSet::new();
        let mut actual_paths = BTreeSet::new();
        let mut total_bytes = 0_u64;
        for entry in &entries {
            if !actual_ids.insert(entry.artifact_id.clone()) {
                return Err(DomainError::InvalidSnapshotSourceBundle {
                    artifact_id: entry.artifact_id.clone(),
                    reason: "duplicate artifact entry".to_owned(),
                });
            }
            if !actual_paths.insert(entry.path.clone()) {
                return Err(DomainError::InvalidSnapshotSourceBundle {
                    artifact_id: entry.artifact_id.clone(),
                    reason: "duplicate source path".to_owned(),
                });
            }
            let Some((expected_path, expected_hash)) = expected.get(&entry.artifact_id) else {
                return Err(DomainError::InvalidSnapshotSourceBundle {
                    artifact_id: entry.artifact_id.clone(),
                    reason: "entry does not name an accepted file artifact".to_owned(),
                });
            };
            if &entry.path != expected_path {
                return Err(DomainError::InvalidSnapshotSourceBundle {
                    artifact_id: entry.artifact_id.clone(),
                    reason: "entry path does not match the accepted file artifact".to_owned(),
                });
            }
            let actual_hash = ContentHash::sha256(&entry.bytes);
            if entry.content_hash != actual_hash
                || entry.cas_hash != actual_hash
                || &entry.content_hash != expected_hash
            {
                return Err(DomainError::InvalidSnapshotSourceBundle {
                    artifact_id: entry.artifact_id.clone(),
                    reason: "entry hashes do not match the accepted snapshot bytes".to_owned(),
                });
            }
            total_bytes = total_bytes
                .checked_add(u64::try_from(entry.bytes.len()).map_err(|_| {
                    DomainError::InvalidSnapshotSourceBundle {
                        artifact_id: entry.artifact_id.clone(),
                        reason: "entry length does not fit the source bundle byte counter"
                            .to_owned(),
                    }
                })?)
                .ok_or_else(|| DomainError::InvalidSnapshotSourceBundle {
                    artifact_id: entry.artifact_id.clone(),
                    reason: "aggregate source bytes overflow u64".to_owned(),
                })?;
        }

        let expected_ids = expected.keys().cloned().collect::<BTreeSet<_>>();
        if actual_ids != expected_ids {
            let artifact_id = expected_ids
                .difference(&actual_ids)
                .next()
                .or_else(|| actual_ids.difference(&expected_ids).next())
                .expect("non-equal sets have one differing ID")
                .clone();
            return Err(DomainError::InvalidSnapshotSourceBundle {
                artifact_id,
                reason: "entries do not exactly match accepted file artifacts".to_owned(),
            });
        }

        Ok(Self {
            snapshot_id: program_space.snapshot_id().clone(),
            entries,
            total_bytes,
        })
    }

    /// Snapshot identity shared with the ProgramSpace used for validation.
    #[must_use]
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }

    /// Entries in canonical repository-path order.
    #[must_use]
    pub fn entries(&self) -> &[SnapshotSourceEntry] {
        &self.entries
    }

    /// Exact sum of every entry's byte length.
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Canonical, byte-stable serialization for deterministic handoff tests.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        canonical_json(self)
    }

    pub(crate) fn canonical_digest_bounded(
        &self,
        limit: usize,
        operation: &'static str,
    ) -> Result<ContentHash> {
        let streaming = SnapshotSourceBundleStreamingRef(self);
        crate::canonical::canonical_json_count_bounded(&streaming, limit, operation)?;
        crate::canonical::compact_json_sha256_streaming(&streaming)
    }
}
