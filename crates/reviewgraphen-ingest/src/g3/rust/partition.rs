use super::types::{
    G3AccountingError as Error, RawKey, RawRustFileV1, RawRustParseObstructionV1,
    RustConstructPartitionV1, RustFileBindingV1, RustFileDispositionV1, RustFileOutcomeV1,
    RustFilePartitionV1, RustG3BatchV1, RustG3ObstructionV1, RustG3OutcomeV1,
    RustG3ReasonV1 as Reason, RustG3RowV1, RustGitExclusionRefV1,
};
use reviewgraphen_core::{ContentHash, ProgramSpace, SnapshotSourceBundle, StableId};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const PROFILE: &str = "rust.g3-admission@2";
const EXTRACTOR: &str = "syn.g3@2";
const OBSERVATION_DOMAIN: &str = "reviewgraphen.g3.rust.observation.v2";
const OBSTRUCTION_DOMAIN: &str = "reviewgraphen.g3.rust.obstruction.v2";
const PARTITION_DOMAIN: &str = "reviewgraphen.g3.rust.partition.v2";

fn id(domain: &'static str, preimage: &BTreeMap<String, Value>) -> Result<StableId, Error> {
    StableId::derived(domain, preimage).map_err(|_| Error::IdDerivation { domain })
}
fn snapshot_value(snapshot: &StableId) -> Value {
    json!({"kind":"rust_program","value":snapshot.as_str()})
}
fn base(
    source: &RustFileBindingV1,
    contract: &str,
    syntax: Value,
    coordinate: &str,
) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("version".to_owned(), json!(2)),
        ("contract".to_owned(), json!(contract)),
        ("profile".to_owned(), json!(PROFILE)),
        ("extractor".to_owned(), json!(EXTRACTOR)),
        ("snapshot".to_owned(), snapshot_value(source.snapshot_id())),
        ("registry".to_owned(), Value::Null),
        ("file_id".to_owned(), json!(source.file_id().as_str())),
        (
            "canonical_source_path".to_owned(),
            json!(source.canonical_path()),
        ),
        (
            "source_hash".to_owned(),
            json!(source.source_hash().as_str()),
        ),
        ("coordinate".to_owned(), json!(coordinate)),
        ("syntax".to_owned(), syntax),
    ])
}
fn obstruct(
    source: &RustFileBindingV1,
    mut preimage: BTreeMap<String, Value>,
    observation_id: &StableId,
    reason: &str,
    detail: Value,
) -> Result<RustG3ObstructionV1, Error> {
    preimage.insert("observation_id".to_owned(), json!(observation_id.as_str()));
    preimage.insert("reason".to_owned(), json!(reason));
    preimage.insert("reason_detail".to_owned(), detail.clone());
    Ok(RustG3ObstructionV1 {
        id: id(OBSTRUCTION_DOMAIN, &preimage)?,
        occurrence_id: observation_id.clone(),
        source: source.clone(),
        reason: reason.to_owned(),
        detail,
    })
}

fn raw_rows(file: &RawRustFileV1) -> Vec<RawKey> {
    file.declarations
        .iter()
        .chain(&file.containment)
        .chain(&file.assignments)
        .chain(&file.test_attributes)
        .cloned()
        .collect()
}

pub(super) fn check_multiset(census: &[RawKey], emitted: &[RawKey]) -> Result<(), Error> {
    let mut expected = BTreeMap::<RawKey, usize>::new();
    for key in census {
        *expected.entry(key.clone()).or_default() += 1;
    }
    if let Some((key, _)) = expected.iter().find(|(_, count)| **count > 1) {
        return Err(Error::DuplicateOccurrence {
            key: format!("{key:?}"),
        });
    }
    let mut actual = BTreeMap::<RawKey, usize>::new();
    for key in emitted {
        *actual.entry(key.clone()).or_default() += 1;
    }
    if let Some((key, _)) = actual.iter().find(|(_, count)| **count > 1) {
        return Err(Error::DuplicateOccurrence {
            key: format!("{key:?}"),
        });
    }
    if let Some(key) = expected.keys().find(|key| !actual.contains_key(*key)) {
        return Err(Error::MissingOccurrence {
            key: format!("{key:?}"),
        });
    }
    if let Some(key) = actual.keys().find(|key| !expected.contains_key(*key)) {
        return Err(Error::UnexpectedOccurrence {
            key: format!("{key:?}"),
        });
    }
    Ok(())
}

#[derive(Default)]
struct Accumulator {
    declarations: Vec<RustG3RowV1>,
    containment: Vec<RustG3RowV1>,
    writes: Vec<RustG3RowV1>,
    test_markers: Vec<RustG3RowV1>,
    files: Vec<RustFileDispositionV1>,
    obstructions: Vec<RustG3ObstructionV1>,
    occurrence_ids: BTreeMap<String, Vec<StableId>>,
    success_ids: BTreeMap<String, Vec<StableId>>,
    obstruction_ids: BTreeMap<String, Vec<StableId>>,
    exclusions: Vec<super::types::RustExclusionV1>,
    limitations: Vec<super::types::RustLimitationV1>,
    observations: BTreeMap<StableId, (RustFileBindingV1, RawKey)>,
    obstruction_index: BTreeMap<StableId, RustG3ObstructionV1>,
}

impl Accumulator {
    fn insert_obstruction(&mut self, obstruction: RustG3ObstructionV1) -> Result<(), Error> {
        if self
            .obstruction_index
            .insert(obstruction.id.clone(), obstruction.clone())
            .is_some()
        {
            return Err(Error::IdentityCollision { id: obstruction.id });
        }
        self.obstructions.push(obstruction);
        Ok(())
    }
    fn insert_row(&mut self, row: RustG3RowV1) -> Result<(), Error> {
        if self
            .observations
            .insert(row.id.clone(), (row.source.clone(), row.key.clone()))
            .is_some()
        {
            return Err(Error::IdentityCollision { id: row.id });
        }
        let contract = row.key.contract().to_owned();
        self.occurrence_ids
            .entry(contract.clone())
            .or_default()
            .push(row.id.clone());
        match &row.outcome {
            RustG3OutcomeV1::ExistingFact(_) => self
                .success_ids
                .entry(contract)
                .or_default()
                .push(row.id.clone()),
            RustG3OutcomeV1::Obstructed { .. } => self
                .obstruction_ids
                .entry(contract)
                .or_default()
                .push(row.id.clone()),
        }
        match &row.key {
            RawKey::Declaration { .. } => self.declarations.push(row),
            RawKey::Containment { .. } => self.containment.push(row),
            RawKey::Assignment { .. } => self.writes.push(row),
            RawKey::TestAttribute { .. } => self.test_markers.push(row),
        }
        Ok(())
    }
}

fn overlap_indices(
    keys: &[RawKey],
    source: &RustFileBindingV1,
) -> Result<BTreeMap<RawKey, Vec<String>>, Error> {
    let mut result = BTreeMap::<RawKey, Vec<String>>::new();
    for left in keys {
        let RawKey::Assignment { occurrence: a, .. } = left else {
            continue;
        };
        let others = keys
            .iter()
            .filter(|right| {
                let RawKey::Assignment { occurrence: b, .. } = right else {
                    return false;
                };
                left != *right && a.overlaps(*b)
            })
            .collect::<Vec<_>>();
        if !others.is_empty() {
            let mut ids = others
                .iter()
                .map(|key| {
                    id(
                        OBSERVATION_DOMAIN,
                        &base(
                            source,
                            key.contract(),
                            key.syntax(),
                            "rust_one_based_inclusive",
                        ),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect::<Vec<_>>();
            ids.sort();
            result.insert(left.clone(), ids);
        }
    }
    Ok(result)
}

fn observe_file(
    source: &RustFileBindingV1,
    file: RawRustFileV1,
    program: &ProgramSpace,
    acc: &mut Accumulator,
) -> Result<(), Error> {
    let emitted = raw_rows(&file);
    check_multiset(&file.census, &emitted)?;
    let overlap = overlap_indices(&emitted, source)?;
    for key in emitted {
        let preimage = base(
            source,
            key.contract(),
            key.syntax(),
            "rust_one_based_inclusive",
        );
        let observation = id(OBSERVATION_DOMAIN, &preimage)?;
        let full = file.accepted_locations.get(&key).copied();
        let decision = if let Some(others) = overlap.get(&key) {
            Err((
                Reason::OverlappingOccurrence,
                json!({"overlapping_occurrence_ids":others}),
            ))
        } else {
            match &key {
                RawKey::Declaration { .. } => {
                    super::declaration::observe(&key, full, source, program)
                }
                RawKey::Containment { .. } => {
                    super::containment::observe(&key, full, source, program)
                }
                RawKey::Assignment { .. } => {
                    super::write::observe(&key, full, &file.legacy_first_writes, source, program)
                }
                RawKey::TestAttribute { .. } => {
                    super::test_marker::observe(&key, full, source, program)
                }
            }
        };
        let outcome = match decision {
            Ok(fact) => RustG3OutcomeV1::ExistingFact(fact),
            Err((reason, detail)) => {
                let obstruction = obstruct(source, preimage, &observation, reason.label(), detail)?;
                let obstruction_id = obstruction.id.clone();
                acc.insert_obstruction(obstruction)?;
                RustG3OutcomeV1::Obstructed {
                    obstruction_id,
                    reason,
                }
            }
        };
        acc.insert_row(RustG3RowV1 {
            id: observation,
            source: source.clone(),
            key,
            outcome,
        })?;
    }
    acc.exclusions.extend(file.exclusions);
    acc.limitations.extend(file.limitations);
    Ok(())
}

fn parse_failure(
    source: &RustFileBindingV1,
    reason: RawRustParseObstructionV1,
    acc: &mut Accumulator,
) -> Result<RustFileOutcomeV1, Error> {
    let label = match reason {
        RawRustParseObstructionV1::InvalidUtf8 => "invalid_utf8@2",
        RawRustParseObstructionV1::ParseFailed => "parse_failed@2",
        RawRustParseObstructionV1::InvalidRange => "invalid_range@2",
    };
    let preimage = base(
        source,
        "R-unparsed-file@2",
        json!({"kind":"unparsed_file"}),
        "unlocated_file",
    );
    let observation = id(OBSERVATION_DOMAIN, &preimage)?;
    acc.insert_obstruction(obstruct(
        source,
        preimage,
        &observation,
        label,
        Value::Null,
    )?)?;
    Ok(RustFileOutcomeV1::ParseObstructed {
        reason: label,
        latent: "unknown",
    })
}

fn sort_ids(ids: &mut BTreeMap<String, Vec<StableId>>) {
    for rows in ids.values_mut() {
        rows.sort();
    }
}

fn verify_dispositions(
    occurrence_ids: &BTreeMap<String, Vec<StableId>>,
    success_ids: &BTreeMap<String, Vec<StableId>>,
    obstruction_ids: &BTreeMap<String, Vec<StableId>>,
) -> Result<(), Error> {
    for (contract, all) in occurrence_ids {
        let success = success_ids
            .get(contract)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let obstruction = obstruction_ids
            .get(contract)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for occurrence in all {
            let count = success.iter().filter(|id| *id == occurrence).count()
                + obstruction.iter().filter(|id| *id == occurrence).count();
            if count != 1 {
                return Err(if count == 0 {
                    Error::MissingOccurrence {
                        key: occurrence.to_string(),
                    }
                } else {
                    Error::DoubleDisposition {
                        key: occurrence.to_string(),
                    }
                });
            }
        }
        for extra in success.iter().chain(obstruction) {
            if !all.contains(extra) {
                return Err(Error::UnexpectedOccurrence {
                    key: extra.to_string(),
                });
            }
        }
    }
    for (contract, rows) in success_ids.iter().chain(obstruction_ids) {
        if !occurrence_ids.contains_key(contract)
            && let Some(extra) = rows.first()
        {
            return Err(Error::UnexpectedOccurrence {
                key: extra.to_string(),
            });
        }
    }
    Ok(())
}

pub(crate) fn observe(
    bundle: &SnapshotSourceBundle,
    program: &ProgramSpace,
    _direct_calls: &[crate::ResolvedDirectCallOccurrence],
    git_exclusions: &[(
        crate::git::GitExcludedEntry,
        crate::IngestionObstructionKind,
        StableId,
    )],
) -> Result<RustG3BatchV1, Error> {
    if bundle.snapshot_id() != program.snapshot_id() {
        return Err(Error::BindingMismatch {
            path: "<snapshot>".to_owned(),
        });
    }
    let mut acc = Accumulator::default();
    let mut paths = BTreeSet::new();
    let mut file_ids = BTreeSet::new();
    for entry in bundle.entries() {
        if !paths.insert(entry.path()) || !file_ids.insert(entry.artifact_id()) {
            return Err(Error::DuplicateFile {
                path: entry.path().to_owned(),
            });
        }
        let Some(file) = program.artifact(entry.artifact_id()) else {
            return Err(Error::BindingMismatch {
                path: entry.path().to_owned(),
            });
        };
        if file.kind != "file"
            || file.content_hash.as_ref() != Some(entry.content_hash())
            || ContentHash::sha256(entry.bytes()) != *entry.content_hash()
            || file
                .location
                .as_ref()
                .is_none_or(|loc| loc.path != entry.path())
        {
            return Err(Error::BindingMismatch {
                path: entry.path().to_owned(),
            });
        }
        let source = RustFileBindingV1 {
            snapshot_id: bundle.snapshot_id().clone(),
            file_id: entry.artifact_id().clone(),
            canonical_path: entry.path().to_owned(),
            source_hash: entry.content_hash().clone(),
        };
        let outcome = if !entry.path().ends_with(".rs") {
            RustFileOutcomeV1::NonRustExcluded
        } else if file.language.as_deref() != Some("rust") {
            return Err(Error::BindingMismatch {
                path: entry.path().to_owned(),
            });
        } else {
            match super::syntax::parse_rust_g3(entry.bytes(), entry.path()) {
                Ok(raw) => {
                    observe_file(&source, raw, program, &mut acc)?;
                    RustFileOutcomeV1::Observed
                }
                Err(reason) => parse_failure(&source, reason, &mut acc)?,
            }
        };
        acc.files.push(RustFileDispositionV1 { source, outcome });
    }
    if acc.files.len() != bundle.entries().len() {
        return Err(Error::MissingOccurrence {
            key: "file partition".to_owned(),
        });
    }
    let mut git_exclusion_refs = Vec::new();
    let mut excluded_paths = BTreeSet::new();
    for (entry, kind, legacy_id) in git_exclusions {
        if paths.contains(entry.path.as_str()) || !excluded_paths.insert(entry.path.as_str()) {
            return Err(Error::GitExclusionMismatch {
                path: entry.path.clone(),
            });
        }
        git_exclusion_refs.push(RustGitExclusionRefV1::new(
            entry.path.clone(),
            entry.kind,
            *kind,
            legacy_id.clone(),
        ));
    }
    git_exclusion_refs.sort_by(|a, b| a.path.cmp(&b.path));
    for ids in [
        &mut acc.occurrence_ids,
        &mut acc.success_ids,
        &mut acc.obstruction_ids,
    ] {
        sort_ids(ids);
    }
    verify_dispositions(&acc.occurrence_ids, &acc.success_ids, &acc.obstruction_ids)?;
    acc.exclusions
        .sort_by(|a, b| (&a.path, a.range, a.kind).cmp(&(&b.path, b.range, b.kind)));
    acc.limitations
        .sort_by(|a, b| (&a.path, a.range, a.kind).cmp(&(&b.path, b.range, b.kind)));
    acc.obstructions.sort_by(|a, b| a.id.cmp(&b.id));
    let files=acc.files.iter().map(|file|json!({"file_id":file.source.file_id().as_str(),
        "canonical_source_path":file.source.canonical_path(),
        "source_hash":file.source.source_hash().as_str(),"outcome":match &file.outcome {
            RustFileOutcomeV1::Observed=>json!("observed"),
            RustFileOutcomeV1::NonRustExcluded=>json!("non_rust_excluded"),
            RustFileOutcomeV1::ParseObstructed {reason,latent}=>json!({"parse_obstructed":reason,"latent":latent}),
        }})).collect::<Vec<_>>();
    let exclusions = acc
        .exclusions
        .iter()
        .map(|row| {
            json!({"path":row.path,
        "range":row.range.value(),"reason":row.kind})
        })
        .collect::<Vec<_>>();
    let mut latent = acc
        .limitations
        .iter()
        .map(|row| {
            json!({"path":row.path,
        "range":row.range.value(),"kind":row.kind,"count":row.latent_occurrence_count})
        })
        .collect::<Vec<_>>();
    latent.extend(git_exclusion_refs.iter().map(|row| {
        json!({"path":row.path,"entry_kind":row.entry_kind,
            "count":row.latent_occurrence_count})
    }));
    let contracts = [
        "R-declaration@2",
        "R-containment@2",
        "R-write@2",
        "R-test-marker@2",
    ]
    .iter()
    .map(|contract| {
        json!({"contract":contract,
            "occurrence_ids":acc.occurrence_ids.get(*contract).cloned().unwrap_or_default(),
            "success_ids":acc.success_ids.get(*contract).cloned().unwrap_or_default(),
            "obstruction_ids":acc.obstruction_ids.get(*contract).cloned().unwrap_or_default(),
            "exclusion_keys":exclusions})
    })
    .collect::<Vec<_>>();
    let preimage = BTreeMap::from([
        ("version".to_owned(), json!(2)),
        ("profile".to_owned(), json!(PROFILE)),
        ("extractor".to_owned(), json!(EXTRACTOR)),
        ("snapshot".to_owned(), snapshot_value(bundle.snapshot_id())),
        ("registry".to_owned(), Value::Null),
        ("files".to_owned(), json!(files)),
        ("contracts".to_owned(), json!(contracts)),
        ("latent".to_owned(), json!(latent)),
    ]);
    let partition_id = id(PARTITION_DOMAIN, &preimage)?;
    Ok(RustG3BatchV1 {
        snapshot_id: bundle.snapshot_id().clone(),
        git_exclusion_refs,
        declarations: acc.declarations,
        containment: acc.containment,
        writes: acc.writes,
        test_markers: acc.test_markers,
        file_partition: RustFilePartitionV1 {
            id: partition_id.clone(),
            files: acc.files,
        },
        construct_partition: RustConstructPartitionV1 {
            id: partition_id,
            occurrence_ids: acc.occurrence_ids,
            success_ids: acc.success_ids,
            obstruction_ids: acc.obstruction_ids,
            exclusions: acc.exclusions,
            limitations: acc.limitations,
        },
        obstructions: acc.obstructions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::g3::rust::types::{RustInclusiveLineColumnRange as R, RustLhsKindV1};
    fn key() -> RawKey {
        RawKey::Assignment {
            occurrence: R::new(1, 1, 1, 5).unwrap(),
            lhs: R::new(1, 1, 1, 1).unwrap(),
            lhs_kind: RustLhsKindV1::Identifier,
            lhs_name: Some("a".into()),
            owner_syntax: None,
            owner_logical_name: None,
        }
    }
    #[test]
    fn multiset_detects_missing_extra_and_duplicate() {
        let k = key();
        assert!(matches!(
            check_multiset(std::slice::from_ref(&k), &[]),
            Err(Error::MissingOccurrence { .. })
        ));
        assert!(matches!(
            check_multiset(&[], std::slice::from_ref(&k)),
            Err(Error::UnexpectedOccurrence { .. })
        ));
        assert!(matches!(
            check_multiset(std::slice::from_ref(&k), &[k.clone(), k.clone()]),
            Err(Error::DuplicateOccurrence { .. })
        ));
    }

    #[test]
    fn partition_rejects_missing_duplicate_and_both_dispositions() {
        let preimage = BTreeMap::from([("test".to_owned(), json!(1))]);
        let only = StableId::derived("reviewgraphen.g3.rust.observation.v2", &preimage).unwrap();
        let all = BTreeMap::from([("R-write@2".to_owned(), vec![only.clone()])]);
        assert!(matches!(
            verify_dispositions(&all, &BTreeMap::new(), &BTreeMap::new()),
            Err(Error::MissingOccurrence { .. })
        ));
        let duplicated =
            BTreeMap::from([("R-write@2".to_owned(), vec![only.clone(), only.clone()])]);
        assert!(matches!(
            verify_dispositions(&all, &duplicated, &BTreeMap::new()),
            Err(Error::DoubleDisposition { .. })
        ));
        let one = BTreeMap::from([("R-write@2".to_owned(), vec![only])]);
        assert!(matches!(
            verify_dispositions(&all, &one, &one),
            Err(Error::DoubleDisposition { .. })
        ));
    }
}

/// Ordinary committed-Git fixture for G3 owner and write regression tests.
/// It enters the same public ingest route as real source; it mints no G3 facts.
#[cfg(test)]
pub(super) mod test_fixture {
    use crate::{IngestRequest, IngestWithSourcesResult, ingest_with_sources};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    pub(in crate::g3::rust) struct Fixture {
        _temporary: tempfile::TempDir,
        workspace: PathBuf,
        root: PathBuf,
        base: String,
        target: String,
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(root)
            .env("HOME", root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
            .args(args)
            .output()
            .expect("fixture Git launches");
        assert!(
            output.status.success(),
            "Git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("Git output UTF-8")
            .trim()
            .to_owned()
    }

    pub(in crate::g3::rust) fn committed(source: &str) -> Fixture {
        let temporary = tempfile::tempdir().expect("temporary workspace");
        let workspace = temporary.path().to_owned();
        let root = workspace.join("repo");
        fs::create_dir(&root).expect("repo directory");
        git(&root, &["init", "--quiet", "--object-format=sha1"]);
        git(&root, &["config", "user.name", "G3 regression"]);
        git(&root, &["config", "user.email", "g3@example.invalid"]);
        fs::create_dir(root.join("src")).expect("src directory");
        fs::write(root.join("src/structural.rs"), "fn baseline() {}\n").expect("base source");
        git(&root, &["add", "."]);
        git(&root, &["commit", "--quiet", "-m", "base"]);
        let base = git(&root, &["rev-parse", "HEAD"]);
        fs::write(root.join("src/structural.rs"), source).expect("target source");
        git(&root, &["add", "."]);
        git(&root, &["commit", "--quiet", "-m", "target"]);
        let target = git(&root, &["rev-parse", "HEAD"]);
        Fixture {
            _temporary: temporary,
            workspace,
            root,
            base,
            target,
        }
    }

    impl Fixture {
        pub(in crate::g3::rust) fn ingest(&self) -> IngestWithSourcesResult {
            let request = IngestRequest::new(
                &self.workspace,
                &self.root,
                "reviewgraphen.test/g3-owner-write-correction",
                &self.base,
                &self.target,
            );
            ingest_with_sources(&request, 1 << 20).expect("actual committed-Git source admission")
        }
    }
}
