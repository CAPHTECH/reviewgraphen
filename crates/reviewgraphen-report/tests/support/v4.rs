//! Public-pipeline M5 fixture for Report V4 integration tests.

use reviewgraphen_core::StableId;
use reviewgraphen_report::{ReportRequestV4, generate_v4};
use reviewgraphen_runtime::m5_gluing::run_double_submit_payment_profile_conflict_v4;
use reviewgraphen_store::test_support::{
    FixtureAssignmentValueV4, FixtureAssignmentsV4, FixtureBaseRootsV4,
    materialize_m5_m4_prefix_v4, materialize_short_extractor_hash_m5_m4_prefix_v4,
    materialize_short_metadata_hash_m5_m4_prefix_v4,
    materialize_short_repository_source_hash_m5_m4_prefix_v4,
};
use reviewgraphen_store::{
    DerivedIndexV5, JournalIdentity, M5ReportAuthorityInspectionV4, StoreLimits, StoreRoot,
};
use std::collections::{BTreeMap, BTreeSet};

pub struct V4Fixture {
    _workspace: tempfile::TempDir,
    pub root: StoreRoot,
    pub identity: JournalIdentity,
    pub base_roots: FixtureBaseRootsV4,
    pub assignments: FixtureAssignmentsV4,
    pub obligation_id: StableId,
    pub plan_id: StableId,
    pub snapshot_id: StableId,
}

impl V4Fixture {
    pub fn request(&self, report_id: &str) -> ReportRequestV4 {
        ReportRequestV4 {
            report_id: StableId::parse(report_id).unwrap(),
            repository_id: self.base_roots.repository_id.clone(),
            program_space_ref: StableId::parse(format!("program-space:{}", self.snapshot_id))
                .unwrap(),
            plan_id: self.plan_id.clone(),
            selected_obligation_ids: BTreeSet::from([self.obligation_id.clone()]),
            tool_versions: BTreeMap::from([
                ("reviewgraphen.runtime".into(), "0.1.0".into()),
                ("reviewgraphen.report".into(), "0.1.0".into()),
            ]),
        }
    }

    pub fn generated(&self, report_id: &str) -> reviewgraphen_report::GeneratedReport {
        generate_v4(
            &self.root,
            self.identity.clone(),
            self.base_roots.build().unwrap(),
            self.assignments.build().unwrap(),
            &self.request(report_id),
        )
        .unwrap()
    }
}

pub fn completed_fixture() -> V4Fixture {
    completed_fixture_with_mode(GluingMode::Conflict)
}

pub fn candidate_fixture() -> V4Fixture {
    completed_fixture_with_mode(GluingMode::Candidate)
}

pub fn unknown_fixture() -> V4Fixture {
    completed_fixture_with_mode(GluingMode::Unknown)
}

pub fn short_metadata_hash_fixture() -> V4Fixture {
    completed_fixture_with_mode(GluingMode::ShortMetadataHash)
}

pub fn short_extractor_hash_fixture() -> V4Fixture {
    completed_fixture_with_mode(GluingMode::ShortExtractorHash)
}

pub fn short_repository_source_hash_fixture() -> V4Fixture {
    completed_fixture_with_mode(GluingMode::ShortRepositorySourceHash)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum GluingMode {
    Conflict,
    Candidate,
    Unknown,
    ShortMetadataHash,
    ShortExtractorHash,
    ShortRepositorySourceHash,
}

fn completed_fixture_with_mode(mode: GluingMode) -> V4Fixture {
    let workspace = tempfile::tempdir().unwrap();
    let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
    let fixture = match mode {
        GluingMode::ShortMetadataHash => {
            materialize_short_metadata_hash_m5_m4_prefix_v4(&root).unwrap()
        }
        GluingMode::ShortExtractorHash => {
            materialize_short_extractor_hash_m5_m4_prefix_v4(&root).unwrap()
        }
        GluingMode::ShortRepositorySourceHash => {
            materialize_short_repository_source_hash_m5_m4_prefix_v4(&root).unwrap()
        }
        _ => materialize_m5_m4_prefix_v4(&root).unwrap(),
    };
    let (journal, base_roots, fixture_assignments) = fixture.into_parts();
    let assignments = match mode {
        GluingMode::Conflict
        | GluingMode::ShortMetadataHash
        | GluingMode::ShortExtractorHash
        | GluingMode::ShortRepositorySourceHash => fixture_assignments,
        GluingMode::Candidate => FixtureAssignmentsV4 {
            payment: FixtureAssignmentValueV4::Satisfied,
            ui_event: FixtureAssignmentValueV4::Satisfied,
        },
        GluingMode::Unknown => FixtureAssignmentsV4 {
            payment: FixtureAssignmentValueV4::Unknown,
            ui_event: FixtureAssignmentValueV4::Satisfied,
        },
    };
    let identity = journal.reader().unwrap().identity().clone();
    if matches!(
        mode,
        GluingMode::Conflict
            | GluingMode::ShortMetadataHash
            | GluingMode::ShortExtractorHash
            | GluingMode::ShortRepositorySourceHash
    ) {
        run_double_submit_payment_profile_conflict_v4(
            &journal,
            base_roots.build().unwrap(),
            assignments.build().unwrap(),
        )
        .unwrap();
    } else {
        journal
            .with_m5_gluing_profile_session(
                base_roots.build().unwrap(),
                assignments.build().unwrap(),
                |profile| {
                    while profile.publish_next_gluing_input()?.is_some() {}
                    profile.append_gluing_bundle()?;
                    Ok(())
                },
            )
            .unwrap();
    }
    let authority = match journal
        .inspect_m5_report_authority_v4(base_roots.build().unwrap(), assignments.build().unwrap())
        .unwrap()
    {
        M5ReportAuthorityInspectionV4::Complete(authority) => authority,
        M5ReportAuthorityInspectionV4::Incomplete { .. } => {
            panic!("runtime did not complete the M5 fixture")
        }
    };
    let index = DerivedIndexV5::open(&root).unwrap();
    authority.rebuild_v5(&index, &journal).unwrap();
    let validated = authority.validated_snapshot_v5(&index, &journal).unwrap();
    let snapshot = validated.snapshot();
    let universe = snapshot.universe.as_ref().unwrap();
    let cover = &snapshot.context_covers[0].cover;
    let obligation_id =
        StableId::parse(cover["selected_obligation_ids"][0].as_str().unwrap()).unwrap();
    let plan_id = StableId::parse(cover["plan_id"].as_str().unwrap()).unwrap();
    let snapshot_id = universe.snapshot_id.clone();
    drop(validated);
    drop(authority);
    drop(index);
    drop(journal);
    V4Fixture {
        _workspace: workspace,
        root,
        identity,
        base_roots,
        assignments,
        obligation_id,
        plan_id,
        snapshot_id,
    }
}
