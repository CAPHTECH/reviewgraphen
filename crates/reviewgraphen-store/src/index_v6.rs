//! Minimal content-addressed projection of a fresh V5 target predecessor.
//!
//! This projection is deliberately not an editable planning index.  It binds
//! the exact confirmed V5 genesis prefix to the accepted ProgramSpace and
//! obligation universe that M6 will use as its target predecessor.  The
//! canonical projection bytes are themselves a CAS object and are re-read
//! before Store can mint an incremental session proof.

use super::IndexError;
use crate::{CasHash, CasStore, EventJournal, StoreRoot, StoreRootIdentity};
use reviewgraphen_core::{ContentHash, ProgramSpace, StableId, canonical_json};
use serde::Serialize;
use std::io::Cursor;

pub const INDEX_SCHEMA_VERSION_V6: u64 = 6;
pub const PROJECTION_CONTRACT_VERSION_V6: &str =
    "reviewgraphen.pre_incremental_index_projection.v6";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreIncrementalIndexMarkerV6 {
    pub index_schema_version: u64,
    pub projection_contract_version: String,
    pub event_contract_version: String,
    pub projection_mode: String,
    pub run_id: StableId,
    pub genesis_hash: ContentHash,
    pub confirmed_offset: u64,
    pub tail_hash: ContentHash,
    pub event_count: u64,
    pub policy_revision_hash: ContentHash,
    pub authority_replay_basis_digest: ContentHash,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreIncrementalIndexSnapshotV6 {
    pub marker: PreIncrementalIndexMarkerV6,
    pub repository_id: StableId,
    pub repository_identity_hash: ContentHash,
    pub snapshot_id: StableId,
    pub universe_id: StableId,
    pub plan_id: StableId,
    pub plan_body_hash: ContentHash,
    pub program_space: ProgramSpace,
    pub obligation_ids: Vec<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexRebuildReceiptV6 {
    pub run_id: StableId,
    pub confirmed_offset: u64,
    pub tail_hash: ContentHash,
    pub event_count: u64,
    pub snapshot_hash: ContentHash,
    pub cas_hash: CasHash,
}

pub struct DerivedIndexV6<'root> {
    root: &'root StoreRoot,
}

/// Opaque current V6 target projection.  The retained replay session keeps
/// the journal prefix locked for the lifetime of this value.
pub struct ValidatedIndexSnapshotV6 {
    _session: crate::journal::ReplayedV5RunSession,
    snapshot: PreIncrementalIndexSnapshotV6,
    canonical_bytes: Vec<u8>,
    snapshot_hash: ContentHash,
    cas_hash: CasHash,
    store_root_identity: StoreRootIdentity,
    observed: TargetProjectionObservedV6,
}

struct TargetProjectionObservedV6 {
    largest_cas_bytes: Vec<u8>,
    largest_cas_hash: Option<CasHash>,
    max_event_line_bytes: u64,
    decoded_owned_bytes: u64,
}

#[derive(Serialize)]
struct TargetPolicyIdentityV6<'a> {
    schema: &'static str,
    profile_id: &'a str,
    profile_version: &'a str,
    policy_version: &'a str,
    rule_set_hash: &'a ContentHash,
    extractor_set_hash: &'a ContentHash,
}

#[derive(Serialize)]
struct TargetReplayBasisV6<'a> {
    schema: &'static str,
    run_id: &'a StableId,
    genesis_hash: &'a ContentHash,
    confirmed_offset: u64,
    tail_hash: &'a ContentHash,
    event_count: u64,
    snapshot_id: &'a StableId,
    universe_id: &'a StableId,
    plan_id: &'a StableId,
    plan_body_hash: &'a ContentHash,
    policy_revision_hash: &'a ContentHash,
}

#[derive(Serialize)]
struct CanonicalRepositoryIdentityV6<'a> {
    repository_id: &'a StableId,
    repository_identity: &'a str,
}

pub(crate) fn repository_identity_hash_v6(
    program: &ProgramSpace,
) -> Result<ContentHash, IndexError> {
    Ok(ContentHash::sha256(
        &canonical_json(&CanonicalRepositoryIdentityV6 {
            repository_id: program.repository_id(),
            repository_identity: program.repository_identity(),
        })
        .map_err(|_| IndexError::ProjectionContractViolation)?,
    ))
}

impl<'root> DerivedIndexV6<'root> {
    pub fn open(root: &'root StoreRoot) -> Result<Self, IndexError> {
        // Admission creates and verifies the CAS hierarchy once; neither this
        // handle nor its callers receive a filesystem path.
        let _ = CasStore::open(root)?;
        Ok(Self { root })
    }

    pub(crate) fn matches_store_root(&self, root: &StoreRoot) -> bool {
        self.root.identity() == root.identity()
    }

    pub fn rebuild_pre_incremental_v6(
        &self,
        journal: &EventJournal<'_>,
    ) -> Result<IndexRebuildReceiptV6, IndexError> {
        let session = journal.replayed_v5_target_session()?;
        if session.store_root_identity() != self.root.identity() {
            return Err(IndexError::ProjectionContractViolation);
        }
        let (snapshot, _) = project_complete_target(&session, self.root)?;
        let bytes =
            canonical_json(&snapshot).map_err(|_| IndexError::ProjectionContractViolation)?;
        let snapshot_hash = ContentHash::sha256(&bytes);
        let cas_hash = CasHash::parse(snapshot_hash.to_string())?;
        let size = u64::try_from(bytes.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
        CasStore::open(self.root)?.put(&cas_hash, Some(size), Cursor::new(&bytes))?;
        // Publication is not authority until the exact object has been
        // reopened and compared with the canonical typed projection.
        CasStore::open(self.root)?.verify_exact_bytes_streaming(&cas_hash, &bytes)?;
        Ok(IndexRebuildReceiptV6 {
            run_id: snapshot.marker.run_id,
            confirmed_offset: snapshot.marker.confirmed_offset,
            tail_hash: snapshot.marker.tail_hash,
            event_count: snapshot.marker.event_count,
            snapshot_hash,
            cas_hash,
        })
    }

    pub fn validated_snapshot_current_v6(
        &self,
        journal: &EventJournal<'_>,
    ) -> Result<ValidatedIndexSnapshotV6, IndexError> {
        let session = journal.replayed_v5_target_session()?;
        self.validated_snapshot_from_session_v6(session)
    }

    pub(crate) fn validated_snapshot_from_session_v6(
        &self,
        session: crate::journal::ReplayedV5RunSession,
    ) -> Result<ValidatedIndexSnapshotV6, IndexError> {
        if session.store_root_identity() != self.root.identity() {
            return Err(IndexError::ProjectionContractViolation);
        }
        let (snapshot, observed) = project_complete_target(&session, self.root)?;
        let canonical_bytes =
            canonical_json(&snapshot).map_err(|_| IndexError::ProjectionContractViolation)?;
        let snapshot_hash = ContentHash::sha256(&canonical_bytes);
        let cas_hash = CasHash::parse(snapshot_hash.to_string())?;
        CasStore::open(self.root)?.verify_exact_bytes_streaming(&cas_hash, &canonical_bytes)?;
        Ok(ValidatedIndexSnapshotV6 {
            _session: session,
            snapshot,
            canonical_bytes,
            snapshot_hash,
            cas_hash,
            store_root_identity: self.root.identity().clone(),
            observed,
        })
    }
}

impl ValidatedIndexSnapshotV6 {
    #[must_use]
    pub const fn snapshot(&self) -> &PreIncrementalIndexSnapshotV6 {
        &self.snapshot
    }

    #[must_use]
    pub fn snapshot_hash(&self) -> &ContentHash {
        &self.snapshot_hash
    }

    pub(crate) fn store_root_identity(&self) -> &StoreRootIdentity {
        &self.store_root_identity
    }

    pub(crate) fn canonical_snapshot_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    pub(crate) const fn decoded_owned_bytes(&self) -> u64 {
        self.observed.decoded_owned_bytes
    }

    pub(crate) const fn max_cas_bytes(&self) -> u64 {
        self.observed.largest_cas_bytes.len() as u64
    }

    pub(crate) fn largest_verified_cas_bytes(&self) -> &[u8] {
        &self.observed.largest_cas_bytes
    }

    pub(crate) const fn max_event_line_bytes(&self) -> u64 {
        self.observed.max_event_line_bytes
    }

    pub(crate) fn confirmed_journal_bytes(&self) -> u64 {
        self._session.confirmed_offset()
    }

    pub(crate) fn retain_confirmed_journal_prefix(&mut self) -> Result<Vec<u8>, IndexError> {
        self._session
            .retain_confirmed_prefix_bytes()
            .map_err(Into::into)
    }

    pub(crate) fn retained_event_bytes(&self) -> Result<u64, IndexError> {
        self._session.retained_event_bytes().map_err(Into::into)
    }

    pub(crate) fn event_log(&self) -> &reviewgraphen_core::EventLogV5 {
        self._session.log()
    }

    pub(crate) fn revalidate_cas(&self, root: &StoreRoot) -> Result<(), IndexError> {
        if root.identity() != &self.store_root_identity
            || ContentHash::sha256(&self.canonical_bytes) != self.snapshot_hash
            || CasHash::parse(self.snapshot_hash.to_string())? != self.cas_hash
        {
            return Err(IndexError::ProjectionContractViolation);
        }
        CasStore::open(root)?
            .verify_exact_bytes_streaming(&self.cas_hash, &self.canonical_bytes)
            .map_err(IndexError::from)?;
        if let Some(hash) = &self.observed.largest_cas_hash {
            CasStore::open(root)?
                .verify_exact_bytes_streaming(hash, &self.observed.largest_cas_bytes)
                .map_err(IndexError::from)?;
        }
        Ok(())
    }
}

fn project_complete_target(
    session: &crate::journal::ReplayedV5RunSession,
    root: &StoreRoot,
) -> Result<(PreIncrementalIndexSnapshotV6, TargetProjectionObservedV6), IndexError> {
    let log = session.log();
    let state = log
        .replay_pre_incremental_state_for_store()
        .map_err(|_| IndexError::ProjectionContractViolation)?;
    let projection = state.projection();
    let program = projection.program_space();
    let universe = projection.universe();
    let plan = projection.plan();
    if universe.snapshot_id() != program.snapshot_id()
        || program.accepted_git_revision_closure().is_none()
    {
        return Err(IndexError::ProjectionContractViolation);
    }
    let reader = crate::CasReader::open_existing(root)?;
    let mut largest_cas_bytes = Vec::new();
    let mut largest_cas_hash = None;
    for registration in projection.registrations() {
        let hash = CasHash::parse(registration.cas_hash().to_string())?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(
                usize::try_from(registration.size()).map_err(|_| IndexError::IntegerOutOfRange)?,
            )
            .map_err(|_| IndexError::Incomplete {
                limit: root.limits().max_index_working_bytes,
                observed: registration.size(),
            })?;
        reader.read_into(&hash, Some(registration.size()), &mut bytes)?;
        if bytes.len() > largest_cas_bytes.len() {
            largest_cas_bytes = bytes;
            largest_cas_hash = Some(hash);
        }
    }
    let policy_revision_hash = ContentHash::sha256(
        &canonical_json(&TargetPolicyIdentityV6 {
            schema: "reviewgraphen.target_policy_identity.v6",
            profile_id: program.profile_id(),
            profile_version: program.profile_version(),
            policy_version: program.policy_version(),
            rule_set_hash: program.rule_set_hash(),
            extractor_set_hash: program.extractor_set_hash(),
        })
        .map_err(|_| IndexError::ProjectionContractViolation)?,
    );
    let event_count =
        u64::try_from(log.envelopes().len()).map_err(|_| IndexError::IntegerOutOfRange)?;
    let plan_body_hash = ContentHash::sha256(
        &plan
            .canonical_bytes()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    );
    let authority_replay_basis_digest = ContentHash::sha256(
        &canonical_json(&TargetReplayBasisV6 {
            schema: "reviewgraphen.target_pre_incremental_replay_basis.v6",
            run_id: log.run_id(),
            genesis_hash: log.genesis_hash(),
            confirmed_offset: session.confirmed_offset(),
            tail_hash: log.tail_hash(),
            event_count,
            snapshot_id: program.snapshot_id(),
            universe_id: universe.id(),
            plan_id: plan.id(),
            plan_body_hash: &plan_body_hash,
            policy_revision_hash: &policy_revision_hash,
        })
        .map_err(|_| IndexError::ProjectionContractViolation)?,
    );
    let snapshot = PreIncrementalIndexSnapshotV6 {
        marker: PreIncrementalIndexMarkerV6 {
            index_schema_version: INDEX_SCHEMA_VERSION_V6,
            projection_contract_version: PROJECTION_CONTRACT_VERSION_V6.to_owned(),
            event_contract_version: "reviewgraphen.review_event.v5".to_owned(),
            projection_mode: "fresh_target_pre_incremental".to_owned(),
            run_id: log.run_id().clone(),
            genesis_hash: log.genesis_hash().clone(),
            confirmed_offset: session.confirmed_offset(),
            tail_hash: log.tail_hash().clone(),
            event_count,
            policy_revision_hash,
            authority_replay_basis_digest,
        },
        repository_id: program.repository_id().clone(),
        repository_identity_hash: repository_identity_hash_v6(program)?,
        snapshot_id: program.snapshot_id().clone(),
        universe_id: universe.id().clone(),
        plan_id: plan.id().clone(),
        plan_body_hash,
        program_space: program.clone(),
        obligation_ids: universe.obligation_ids().iter().cloned().collect(),
    };
    let decoded_owned_bytes = super::v5::recursive_ownership_charge(&snapshot)?;
    let max_event_line_bytes = log
        .maximum_canonical_event_line_bytes_for_store()
        .map_err(|_| IndexError::ProjectionContractViolation)?;
    Ok((
        snapshot,
        TargetProjectionObservedV6 {
            largest_cas_bytes,
            largest_cas_hash,
            max_event_line_bytes,
            decoded_owned_bytes,
        },
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{JournalError, StoreLimits};
    use reviewgraphen_core::{
        ArtifactRegisteredV3, ArtifactSensitivity, ArtifactSourceV3, EventCommand, EventLog,
        EventLogV5, MvpRulePack, PlanBudget, ProgramSpace, ReviewAggregate,
        RunGenesisBootstrapRequestV4, SnapshotSourceRecordEntry, SnapshotSourcesRecorded, plan,
    };
    use serde_json::Value;

    const FIXTURE: &[u8] =
        include_bytes!("../../../examples/double-submit-payment/program-space.json");

    const BASE_COMMIT_OID: &str = "1111111111111111111111111111111111111111";
    const TARGET_COMMIT_OID: &str = "2222222222222222222222222222222222222222";
    const BASE_TREE_HASH: &str = "git:3333333333333333333333333333333333333333";
    const TARGET_TREE_HASH: &str = "git:4444444444444444444444444444444444444444";

    fn upgrade_fixture_to_incremental_v3(
        input: &mut Value,
        base_commit_oid: &str,
        base_tree_hash: &str,
    ) {
        input["schema"] = Value::String("reviewgraphen.program_space.input.v3".to_owned());
        input["source"]["kind"] = Value::String("git".to_owned());
        input["source"]["revision"] = Value::String(TARGET_COMMIT_OID.to_owned());
        input["source"]["content_hash"] = Value::String(TARGET_TREE_HASH.to_owned());
        input["snapshot"]["base_revision"] = Value::String(base_commit_oid.to_owned());
        input["snapshot"]["target_revision"] = Value::String(TARGET_COMMIT_OID.to_owned());
        input["snapshot"]["tree_hash"] = Value::String(TARGET_TREE_HASH.to_owned());
        input["snapshot"]["dirty"] = Value::Bool(false);
        input["snapshot"]["id"] = Value::String("snapshot:v6-target".to_owned());
        for limitation in input["extraction"]["limitations"].as_array_mut().unwrap() {
            for source_id in limitation["source_ids"].as_array_mut().unwrap() {
                if source_id == "snapshot:double-submit-v1" {
                    *source_id = Value::String("snapshot:v6-target".to_owned());
                }
            }
        }

        for relation in input["relations"].as_array_mut().unwrap() {
            relation["ordered_target_ids"] = relation["target_ids"].clone();
        }

        let mut anchors = serde_json::Map::new();
        for artifact in input["artifacts"].as_array_mut().unwrap() {
            let kind = artifact["kind"].as_str().unwrap().to_owned();
            if artifact["language"] == "rust"
                && matches!(kind.as_str(), "function" | "method" | "type")
            {
                let id = artifact["id"].as_str().unwrap().to_owned();
                artifact["provenance"]["extraction_method"] =
                    Value::String("reviewgraphen.ingest.rust_syn.v1".to_owned());
                anchors.insert(
                    id.clone(),
                    serde_json::json!({
                        "descriptor": "reviewgraphen.rust_symbol_anchor@1",
                        "language": "rust",
                        "symbol_kind": kind,
                        "signature_shape_hash": ContentHash::sha256(
                            format!("signature:{id}").as_bytes()
                        ),
                        "normalized_body_hash": ContentHash::sha256(
                            format!("body:{id}").as_bytes()
                        )
                    }),
                );
            }
        }
        input["incremental_facts"] = serde_json::json!({
            "git_revision_closure": {
                "base_commit_oid": base_commit_oid,
                "base_tree_hash": base_tree_hash,
                "target_commit_oid": TARGET_COMMIT_OID,
                "target_tree_hash": TARGET_TREE_HASH
            },
            "rust_anchor_extractor_id": "reviewgraphen.ingest.rust_syn.anchor.v1",
            "rust_anchor_syn_version": "2.0.119",
            "rust_symbol_anchors": anchors
        });
    }

    pub(crate) fn planned_target(root: &StoreRoot) -> EventLogV5 {
        planned_target_with_schema(root, true, BASE_COMMIT_OID, BASE_TREE_HASH)
    }

    pub(crate) fn planned_target_with_base(
        root: &StoreRoot,
        base_commit_oid: &str,
        base_tree_hash: &str,
    ) -> EventLogV5 {
        planned_target_with_schema(root, true, base_commit_oid, base_tree_hash)
    }

    fn planned_target_with_schema(
        root: &StoreRoot,
        incremental_v3: bool,
        base_commit_oid: &str,
        base_tree_hash: &str,
    ) -> EventLogV5 {
        let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
        if incremental_v3 {
            upgrade_fixture_to_incremental_v3(&mut input, base_commit_oid, base_tree_hash);
        }
        let mut bytes_by_path = std::collections::BTreeMap::new();
        for artifact in input["artifacts"].as_array_mut().unwrap() {
            if artifact["kind"] == "file" {
                let path = artifact["location"]["path"].as_str().unwrap().to_owned();
                let bytes = format!("accepted target source: {path}\n").into_bytes();
                artifact["content_hash"] = Value::String(ContentHash::sha256(&bytes).to_string());
                bytes_by_path.insert(path, bytes);
            }
        }
        let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let aggregate = ReviewAggregate::new(program, universe, obligations).unwrap();
        let run_id = StableId::parse("run:v6-target").unwrap();
        let snapshot_id = aggregate.program().snapshot_id().clone();
        let mut local = EventLog::new_v3(run_id.clone(), aggregate).unwrap();
        let genesis_bytes = local
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let mut registrations = Vec::new();
        let mut entries = Vec::new();
        let cas = CasStore::open(root).unwrap();
        let files = local
            .aggregate()
            .program()
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
            .cloned()
            .collect::<Vec<_>>();
        for artifact in files {
            let path = artifact.location.as_ref().unwrap().path.clone();
            let bytes = &bytes_by_path[&path];
            let hash = ContentHash::sha256(bytes);
            let registration = ArtifactRegisteredV3::new(
                run_id.clone(),
                hash.clone(),
                "text/plain",
                u64::try_from(bytes.len()).unwrap(),
                ArtifactSensitivity::WorkspaceSource,
                ArtifactSourceV3::SnapshotIngest {
                    adapter_id: "test-git-adapter".to_owned(),
                    run_id: run_id.clone(),
                    snapshot_id: snapshot_id.clone(),
                },
            )
            .unwrap();
            let cas_hash = CasHash::parse(hash.to_string()).unwrap();
            cas.put(
                &cas_hash,
                Some(u64::try_from(bytes.len()).unwrap()),
                Cursor::new(bytes),
            )
            .unwrap();
            entries.push(
                SnapshotSourceRecordEntry::new(
                    artifact.id.clone(),
                    path,
                    hash.clone(),
                    registration.registration_id().clone(),
                    hash,
                    1,
                )
                .unwrap(),
            );
            local
                .append(EventCommand::artifact_registered_v3(registration.clone()))
                .unwrap();
            registrations.push(registration);
        }
        registrations.sort_by(|left, right| left.registration_id().cmp(right.registration_id()));
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        let sources = SnapshotSourcesRecorded::new(snapshot_id, entries).unwrap();
        local
            .append(EventCommand::snapshot_sources_recorded(sources.clone()))
            .unwrap();
        let review_plan = plan(local.aggregate(), PlanBudget::new(16, 16).unwrap()).unwrap();
        let request = RunGenesisBootstrapRequestV4::new(
            run_id,
            genesis_bytes,
            local.aggregate().program().repository_identity(),
            local.aggregate().program().snapshot_id().clone(),
            local.aggregate().program().profile_id(),
            local.aggregate().program().profile_version(),
        )
        .unwrap();
        EventLogV5::from_planned_bootstrap_request(request, registrations, sources, review_plan)
            .unwrap()
    }

    #[test]
    fn v6_projection_refuses_program_space_v2() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let legacy = planned_target_with_schema(&root, false, BASE_COMMIT_OID, BASE_TREE_HASH);
        let (journal, _) = EventJournal::publish_new_v5(&root, legacy).unwrap();
        let index = DerivedIndexV6::open(&root).unwrap();
        assert!(matches!(
            index.rebuild_pre_incremental_v6(&journal),
            Err(IndexError::ProjectionContractViolation)
        ));
    }

    #[test]
    fn v6_projection_requires_complete_plan_and_rechecks_cas() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let genesis_only = EventLogV5::from_bootstrap_request({
            let complete = planned_target(&root);
            let genesis = complete.canonical_genesis_bytes().to_vec();
            let state =
                reviewgraphen_core::RunGenesisSnapshot::from_canonical_v4_bytes_for_store(&genesis)
                    .unwrap();
            RunGenesisBootstrapRequestV4::new(
                StableId::parse("run:v6-incomplete").unwrap(),
                genesis,
                state.program_space().repository_identity(),
                state.program_space().snapshot_id().clone(),
                state.program_space().profile_id(),
                state.program_space().profile_version(),
            )
            .unwrap()
        })
        .unwrap();
        assert!(matches!(
            EventJournal::publish_new_v5(&root, genesis_only),
            Err(JournalError::Domain(_))
        ));

        let target = planned_target(&root);
        let expected_plan = target
            .replay_pre_incremental_state_for_store()
            .unwrap()
            .projection()
            .plan()
            .id()
            .clone();
        for registration in target
            .replay_pre_incremental_state_for_store()
            .unwrap()
            .projection()
            .registrations()
        {
            let hash = CasHash::parse(registration.cas_hash().to_string()).unwrap();
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(usize::try_from(registration.size()).unwrap())
                .unwrap();
            crate::CasReader::open_existing(&root)
                .unwrap()
                .read_into(&hash, Some(registration.size()), &mut bytes)
                .unwrap();
        }
        let (journal, _) = EventJournal::publish_new_v5(&root, target).unwrap();
        let foreign_workspace = tempfile::tempdir().unwrap();
        let foreign_root =
            StoreRoot::open(foreign_workspace.path(), StoreLimits::default()).unwrap();
        let foreign_index = DerivedIndexV6::open(&foreign_root).unwrap();
        assert!(matches!(
            foreign_index.rebuild_pre_incremental_v6(&journal),
            Err(IndexError::ProjectionContractViolation)
        ));
        let index = DerivedIndexV6::open(&root).unwrap();
        let receipt = index.rebuild_pre_incremental_v6(&journal).unwrap();
        let view = index.validated_snapshot_current_v6(&journal).unwrap();
        assert_eq!(view.snapshot().plan_id, expected_plan);
        assert_eq!(view.snapshot_hash(), &receipt.snapshot_hash);
        drop(view);

        let object = root
            .path()
            .join("artifacts/sha256")
            .join(receipt.cas_hash.prefix())
            .join(receipt.cas_hash.hex());
        std::fs::write(object, b"mutated index projection").unwrap();
        assert!(index.validated_snapshot_current_v6(&journal).is_err());
    }

    #[test]
    fn v6_replay_refuses_mutated_genesis_and_registered_source_cas() {
        for mutate_genesis in [true, false] {
            let workspace = tempfile::tempdir().unwrap();
            let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
            let target = planned_target(&root);
            let object_hash = if mutate_genesis {
                CasHash::parse(target.genesis_hash().to_string()).unwrap()
            } else {
                CasHash::parse(
                    target
                        .replay_pre_incremental_state_for_store()
                        .unwrap()
                        .projection()
                        .registrations()[0]
                        .cas_hash()
                        .to_string(),
                )
                .unwrap()
            };
            let (journal, _) = EventJournal::publish_new_v5(&root, target).unwrap();
            let object = root
                .path()
                .join("artifacts/sha256")
                .join(object_hash.prefix())
                .join(object_hash.hex());
            std::fs::write(object, b"mutated accepted target bytes").unwrap();
            let index = DerivedIndexV6::open(&root).unwrap();
            assert!(index.rebuild_pre_incremental_v6(&journal).is_err());
        }
    }

    #[test]
    fn v6_retains_real_largest_cas_bytes_and_revalidates_their_hash() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let target = planned_target(&root);
        let (journal, _) = EventJournal::publish_new_v5(&root, target).unwrap();
        let index = DerivedIndexV6::open(&root).unwrap();
        index.rebuild_pre_incremental_v6(&journal).unwrap();
        let view = index.validated_snapshot_current_v6(&journal).unwrap();
        let hash = view.observed.largest_cas_hash.clone().unwrap();
        let mut oracle = Vec::new();
        oracle
            .try_reserve_exact(view.observed.largest_cas_bytes.len())
            .unwrap();
        crate::CasReader::open_existing(&root)
            .unwrap()
            .read_into(
                &hash,
                Some(view.observed.largest_cas_bytes.len() as u64),
                &mut oracle,
            )
            .unwrap();
        assert_eq!(view.observed.largest_cas_bytes, oracle);

        let object = root
            .path()
            .join("artifacts/sha256")
            .join(hash.prefix())
            .join(hash.hex());
        std::fs::write(object, b"mutated retained target source").unwrap();
        assert!(view.revalidate_cas(&root).is_err());
    }
}
