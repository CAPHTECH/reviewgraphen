//! Additive contract for real regression-fix pairs.
//!
//! The pilot-v2 artifact family remains frozen. These records are private
//! research bindings and never enter accepted review state or reviewer input.

use super::*;

pub const REAL_UNIT_SCHEMA: &str = "reviewgraphen.benchmark.real_unit.v1";
pub const REAL_ORACLE_SCHEMA: &str = "reviewgraphen.benchmark.real_oracle.v1";
pub const REAL_SCORE_SCHEMA: &str = "reviewgraphen.benchmark.real_score.v1";
pub const REAL_INVENTORY_SCHEMA: &str = "reviewgraphen.benchmark.real_trial_inventory.v1";
pub const REAL_RUN_SUMMARY_SCHEMA: &str = "reviewgraphen.benchmark.real_run_summary.v1";
pub const REAL_FULL_RUN_SUMMARY_SCHEMA: &str = "reviewgraphen.benchmark.real_full_run_summary.v1";
pub const PRESENCE_EVIDENCE_SCHEMA: &str =
    "reviewgraphen.benchmark.regression_presence_evidence.v1";

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionRole {
    PositiveDefectPresent,
    MatchedFixControl,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetExpectation {
    Present,
    Absent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CorpusSemantics {
    RegressionFixPair,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotSemantics {
    ParentPositiveFixControl,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerInputSemantics {
    SelectedProductionSnapshot,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlFindingSemantics {
    UnlabeledRequiresAdjudication,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionTestStrategy {
    FixRegressionTestBackportedToParent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TestExecutionEvidence {
    pub commit_hash: ContentHash,
    pub tree_hash: ContentHash,
    pub command: Vec<String>,
    pub working_directory: String,
    pub exit_status: i32,
    pub stdout_sha256: ContentHash,
    pub stderr_sha256: ContentHash,
    pub combined_artifact_sha256: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegressionPresenceEvidence {
    pub schema: String,
    pub benchmark_unit_id: String,
    pub test_selector: String,
    pub test_source_sha256: ContentHash,
    pub strategy: RegressionTestStrategy,
    pub parent_run: TestExecutionEvidence,
    pub fix_run: TestExecutionEvidence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RealUnitContract {
    pub schema: String,
    pub corpus_semantics: CorpusSemantics,
    pub snapshot_semantics: SnapshotSemantics,
    pub reviewer_input_semantics: ReviewerInputSemantics,
    pub control_finding_semantics: ControlFindingSemantics,
    pub projection_policy_version: String,
    pub selected_production_paths: BTreeSet<String>,
    pub positive_source_inventory_hash: ContentHash,
    pub control_source_inventory_hash: ContentHash,
    pub benchmark_unit_id: String,
    pub target_id: String,
    pub positive_trial_unit_id: String,
    pub control_trial_unit_id: String,
    pub parent_commit: ContentHash,
    pub fix_commit: ContentHash,
    pub positive_tree_hash: ContentHash,
    pub control_tree_hash: ContentHash,
    pub mechanism_ontology_version: String,
    pub presence_evidence: RegressionPresenceEvidence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorOrigin {
    CodeFixHunk,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetScopeAnchor {
    pub anchor_id: String,
    pub tree_hash: ContentHash,
    pub path: String,
    pub file_sha256: ContentHash,
    pub symbol: String,
    pub start_line: u32,
    pub end_line: u32,
    pub span_sha256: ContentHash,
    pub mechanism_tags: BTreeSet<MechanismId>,
    pub origin: AnchorOrigin,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RealPrivateOracle {
    pub schema: String,
    pub benchmark_unit_id: String,
    pub trial_unit_id: String,
    pub target_id: String,
    pub revision_role: RevisionRole,
    pub target_expectation: TargetExpectation,
    pub control_finding_semantics: ControlFindingSemantics,
    pub real_unit_hash: ContentHash,
    pub presence_evidence_hash: ContentHash,
    pub input_tree_hash: ContentHash,
    pub source_bundle_hash: ContentHash,
    pub target_roots: Vec<OracleRoot>,
    pub target_scope_anchors: Vec<TargetScopeAnchor>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RealScore {
    pub schema: String,
    pub trial_id: String,
    pub benchmark_unit_id: String,
    pub trial_unit_id: String,
    pub target_id: String,
    pub arm: Arm,
    pub replicate: u32,
    pub revision_role: RevisionRole,
    pub target_expectation: TargetExpectation,
    pub control_finding_semantics: ControlFindingSemantics,
    pub manifest_hash: ContentHash,
    pub candidate_hash: ContentHash,
    pub oracle_hash: ContentHash,
    pub real_unit_hash: ContentHash,
    pub presence_evidence_hash: ContentHash,
    pub input_tree_hash: ContentHash,
    pub protocol_version: String,
    pub mechanism_ontology_version: String,
    pub paired_configuration_hash: ContentHash,
    pub target_root_count: u32,
    pub detected_target_root_count: u32,
    pub candidate_count: u32,
    pub target_anchor_matched_candidate_count: u32,
    pub unlabeled_candidate_count: u32,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RealInventoryTrial {
    pub trial_id: String,
    pub benchmark_unit_id: String,
    pub trial_unit_id: String,
    pub revision_role: RevisionRole,
    pub arm: Arm,
    pub replicate: u32,
    pub manifest_hash: ContentHash,
    pub paired_configuration_hash: ContentHash,
    pub real_unit_hash: ContentHash,
    pub presence_evidence_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RealTrialInventory {
    pub schema: String,
    pub corpus_semantics: CorpusSemantics,
    pub control_finding_semantics: ControlFindingSemantics,
    pub trials: Vec<RealInventoryTrial>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RealRunSummary {
    pub schema: String,
    pub prepared_trials: u32,
    pub valid_collections: u32,
    pub protocol_invalid_trials: u32,
    pub collection_binding_invalid_trials: u32,
    pub missing_trials: u32,
    pub eligible_fix_pairs: u32,
    pub excluded_fix_pairs: u32,
    pub exclusion_reason_counts: BTreeMap<String, u32>,
    pub b1_positive_detected_targets: u32,
    pub g3_proxy_positive_detected_targets: u32,
    pub total_positive_targets_per_arm: u32,
    pub g3_minus_b1_positive_detected_targets: i64,
    pub b1_control_unlabeled_findings: u32,
    pub g3_proxy_control_unlabeled_findings: u32,
    pub b1_control_target_anchor_allegations: u32,
    pub g3_proxy_control_target_anchor_allegations: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RealFullRunSummary {
    pub schema: String,
    pub prepared_trials: u32,
    pub valid_collections: u32,
    pub protocol_invalid_trials: u32,
    pub collection_binding_invalid_trials: u32,
    pub missing_trials: u32,
    pub eligible_fix_pairs: u32,
    pub excluded_fix_pairs: u32,
    pub exclusion_reason_counts: BTreeMap<String, u32>,
    pub full_positive_detected_targets: u32,
    pub total_positive_targets: u32,
    pub full_positive_findings: u32,
    pub full_positive_unlabeled_findings: u32,
    pub full_control_findings: u32,
    pub full_control_unlabeled_findings: u32,
    pub full_control_target_anchor_allegations: u32,
}

fn validate_test_run(run: &TestExecutionEvidence) -> Result<()> {
    if !exact_git_hash(&run.commit_hash)
        || !exact_git_hash(&run.tree_hash)
        || run.command.is_empty()
        || run.command.len() > 64
        || run.command.iter().any(|part| !text(part))
        || !safe_relative_path(&run.working_directory)
        || !(0..=255).contains(&run.exit_status)
        || !exact_sha256(&run.stdout_sha256)
        || !exact_sha256(&run.stderr_sha256)
        || !exact_sha256(&run.combined_artifact_sha256)
    {
        return Err(BenchmarkError::Validation(
            "invalid regression test execution",
        ));
    }
    Ok(())
}

impl RegressionPresenceEvidence {
    pub fn validate(&self) -> Result<()> {
        validate_test_run(&self.parent_run)?;
        validate_test_run(&self.fix_run)?;
        if self.schema != PRESENCE_EVIDENCE_SCHEMA
            || !text(&self.benchmark_unit_id)
            || !text(&self.test_selector)
            || !exact_sha256(&self.test_source_sha256)
            || self.parent_run.commit_hash == self.fix_run.commit_hash
            || self.parent_run.tree_hash == self.fix_run.tree_hash
            || self.parent_run.command != self.fix_run.command
            || self.parent_run.working_directory != self.fix_run.working_directory
            || self.parent_run.exit_status == 0
            || self.fix_run.exit_status != 0
        {
            return Err(BenchmarkError::Validation(
                "presence evidence must prove parent-fails and fix-passes",
            ));
        }
        Ok(())
    }
}

impl RealUnitContract {
    pub fn validate(&self) -> Result<()> {
        self.presence_evidence.validate()?;
        if self.schema != REAL_UNIT_SCHEMA
            || !text(&self.projection_policy_version)
            || self.selected_production_paths.is_empty()
            || self.selected_production_paths.len() > 4096
            || self
                .selected_production_paths
                .iter()
                .any(|path| !safe_relative_path(path))
            || !exact_sha256(&self.positive_source_inventory_hash)
            || !exact_sha256(&self.control_source_inventory_hash)
            || !text(&self.benchmark_unit_id)
            || !text(&self.target_id)
            || !text(&self.positive_trial_unit_id)
            || !text(&self.control_trial_unit_id)
            || self.positive_trial_unit_id == self.control_trial_unit_id
            || !exact_git_hash(&self.parent_commit)
            || !exact_git_hash(&self.fix_commit)
            || !exact_git_hash(&self.positive_tree_hash)
            || !exact_git_hash(&self.control_tree_hash)
            || self.parent_commit == self.fix_commit
            || self.positive_tree_hash == self.control_tree_hash
            || self.mechanism_ontology_version != MECHANISM_ONTOLOGY_VERSION
            || self.presence_evidence.benchmark_unit_id != self.benchmark_unit_id
            || self.presence_evidence.parent_run.commit_hash != self.parent_commit
            || self.presence_evidence.fix_run.commit_hash != self.fix_commit
            || self.presence_evidence.parent_run.tree_hash != self.positive_tree_hash
            || self.presence_evidence.fix_run.tree_hash != self.control_tree_hash
        {
            return Err(BenchmarkError::Validation("invalid real fix-pair unit"));
        }
        Ok(())
    }
}

fn validate_scope_anchor(anchor: &TargetScopeAnchor) -> Result<()> {
    if !text(&anchor.anchor_id)
        || !exact_git_hash(&anchor.tree_hash)
        || !safe_relative_path(&anchor.path)
        || !exact_sha256(&anchor.file_sha256)
        || !text(&anchor.symbol)
        || !range(anchor.start_line, anchor.end_line)
        || !exact_sha256(&anchor.span_sha256)
        || !tags(&anchor.mechanism_tags)
    {
        return Err(BenchmarkError::Validation("invalid target scope anchor"));
    }
    Ok(())
}

impl RealPrivateOracle {
    pub fn validate(&self) -> Result<()> {
        if self.schema != REAL_ORACLE_SCHEMA
            || !text(&self.benchmark_unit_id)
            || !text(&self.trial_unit_id)
            || !text(&self.target_id)
            || !exact_sha256(&self.real_unit_hash)
            || !exact_sha256(&self.presence_evidence_hash)
            || !exact_git_hash(&self.input_tree_hash)
            || !exact_sha256(&self.source_bundle_hash)
            || self.target_roots.len() > 1024
            || self.target_scope_anchors.is_empty()
            || self.target_scope_anchors.len() > 1024
        {
            return Err(BenchmarkError::Validation("invalid real oracle"));
        }
        let mut root_ids = BTreeSet::new();
        for root in &self.target_roots {
            if !text(&root.root_id)
                || !root_ids.insert(&root.root_id)
                || root.tree_hash != self.input_tree_hash
                || !safe_relative_path(&root.path)
                || !exact_sha256(&root.file_sha256)
                || !text(&root.symbol)
                || !range(root.start_line, root.end_line)
                || !exact_sha256(&root.span_sha256)
                || !tags(&root.mechanism_tags)
                || !matches!(
                    root.severity.as_str(),
                    "critical" | "high" | "medium" | "low"
                )
            {
                return Err(BenchmarkError::Validation("invalid real target root"));
            }
        }
        let mut anchor_ids = BTreeSet::new();
        for anchor in &self.target_scope_anchors {
            validate_scope_anchor(anchor)?;
            if anchor.tree_hash != self.input_tree_hash || !anchor_ids.insert(&anchor.anchor_id) {
                return Err(BenchmarkError::Validation(
                    "invalid real target anchor binding",
                ));
            }
        }
        match (&self.revision_role, &self.target_expectation) {
            (RevisionRole::PositiveDefectPresent, TargetExpectation::Present)
                if !self.target_roots.is_empty() => {}
            (RevisionRole::MatchedFixControl, TargetExpectation::Absent)
                if self.target_roots.is_empty() => {}
            _ => {
                return Err(BenchmarkError::Validation(
                    "real oracle role, expectation, and roots disagree",
                ));
            }
        }
        Ok(())
    }
}

fn validate_anchor_against_manifest(
    manifest: &TrialManifest,
    path: &str,
    file_sha256: &ContentHash,
    end_line: u32,
) -> Result<()> {
    let Some(source) = manifest
        .source_inventory
        .iter()
        .find(|source| source.path == path)
    else {
        return Err(BenchmarkError::Validation(
            "real anchor outside source inventory",
        ));
    };
    if &source.content_hash != file_sha256 || end_line > source.line_count {
        return Err(BenchmarkError::Validation(
            "real anchor differs from source inventory",
        ));
    }
    Ok(())
}

pub fn validate_real_oracle_bindings(
    manifest: &TrialManifest,
    oracle: &RealPrivateOracle,
    unit: &RealUnitContract,
) -> Result<()> {
    manifest.validate()?;
    oracle.validate()?;
    unit.validate()?;
    let unit_hash = canonical_hash(unit)?;
    let evidence_hash = canonical_hash(&unit.presence_evidence)?;
    let (trial_unit_id, tree_hash, inventory_hash) = match oracle.revision_role {
        RevisionRole::PositiveDefectPresent => (
            &unit.positive_trial_unit_id,
            &unit.positive_tree_hash,
            &unit.positive_source_inventory_hash,
        ),
        RevisionRole::MatchedFixControl => (
            &unit.control_trial_unit_id,
            &unit.control_tree_hash,
            &unit.control_source_inventory_hash,
        ),
    };
    let actual_inventory_hash = canonical_hash(&manifest.source_inventory)?;
    let actual_paths = manifest
        .source_inventory
        .iter()
        .map(|source| source.path.clone())
        .collect::<BTreeSet<_>>();
    if oracle.benchmark_unit_id != unit.benchmark_unit_id
        || oracle.target_id != unit.target_id
        || &oracle.trial_unit_id != trial_unit_id
        || manifest.unit_id != oracle.trial_unit_id
        || manifest.input_tree_hash != *tree_hash
        || actual_inventory_hash != *inventory_hash
        || actual_paths != unit.selected_production_paths
        || oracle.input_tree_hash != manifest.input_tree_hash
        || oracle.source_bundle_hash != manifest.source_bundle_hash
        || oracle.real_unit_hash != unit_hash
        || oracle.presence_evidence_hash != evidence_hash
    {
        return Err(BenchmarkError::Validation("real oracle binding mismatch"));
    }
    for root in &oracle.target_roots {
        validate_anchor_against_manifest(manifest, &root.path, &root.file_sha256, root.end_line)?;
    }
    for anchor in &oracle.target_scope_anchors {
        validate_anchor_against_manifest(
            manifest,
            &anchor.path,
            &anchor.file_sha256,
            anchor.end_line,
        )?;
    }
    Ok(())
}

fn finding_matches_scope(finding: &CandidateFinding, anchor: &TargetScopeAnchor) -> bool {
    finding
        .mechanism_tags
        .intersection(&anchor.mechanism_tags)
        .next()
        .is_some()
        && finding.locations.iter().any(|location| {
            location.path == anchor.path
                && location.start_line <= anchor.end_line
                && anchor.start_line <= location.end_line
        })
}

fn finding_matches_root(finding: &CandidateFinding, root: &OracleRoot) -> bool {
    finding
        .mechanism_tags
        .intersection(&root.mechanism_tags)
        .next()
        .is_some()
        && finding
            .locations
            .iter()
            .any(|location| overlaps(location, root))
}

pub fn score_real(
    manifest: &TrialManifest,
    candidate: &CandidateOutput,
    oracle: &RealPrivateOracle,
    unit: &RealUnitContract,
) -> Result<RealScore> {
    validate_candidate_against_manifest(manifest, candidate)?;
    validate_real_oracle_bindings(manifest, oracle, unit)?;
    let mut detected_roots = BTreeSet::new();
    let mut target_matched_candidates = 0_u32;
    for finding in &candidate.findings {
        let root_matches = oracle
            .target_roots
            .iter()
            .filter(|root| finding_matches_root(finding, root))
            .map(|root| root.root_id.clone())
            .collect::<BTreeSet<_>>();
        let scope_match = oracle
            .target_scope_anchors
            .iter()
            .any(|anchor| finding_matches_scope(finding, anchor));
        if !root_matches.is_empty() || scope_match {
            target_matched_candidates = target_matched_candidates
                .checked_add(1)
                .ok_or(BenchmarkError::Validation("target match count overflow"))?;
        }
        detected_roots.extend(root_matches);
    }
    let candidate_count = u32::try_from(candidate.findings.len())
        .map_err(|_| BenchmarkError::Validation("candidate count"))?;
    let target_root_count = u32::try_from(oracle.target_roots.len())
        .map_err(|_| BenchmarkError::Validation("target root count"))?;
    let detected_target_root_count = u32::try_from(detected_roots.len())
        .map_err(|_| BenchmarkError::Validation("detected target root count"))?;
    let unlabeled_candidate_count = match oracle.revision_role {
        RevisionRole::PositiveDefectPresent => candidate_count
            .checked_sub(target_matched_candidates)
            .ok_or(BenchmarkError::Validation("unlabeled candidate underflow"))?,
        RevisionRole::MatchedFixControl => candidate_count,
    };
    let value = RealScore {
        schema: REAL_SCORE_SCHEMA.to_owned(),
        trial_id: manifest.trial_id.clone(),
        benchmark_unit_id: unit.benchmark_unit_id.clone(),
        trial_unit_id: manifest.unit_id.clone(),
        target_id: unit.target_id.clone(),
        arm: manifest.arm.clone(),
        replicate: manifest.replicate,
        revision_role: oracle.revision_role.clone(),
        target_expectation: oracle.target_expectation.clone(),
        control_finding_semantics: oracle.control_finding_semantics.clone(),
        manifest_hash: canonical_hash(manifest)?,
        candidate_hash: canonical_hash(candidate)?,
        oracle_hash: canonical_hash(oracle)?,
        real_unit_hash: canonical_hash(unit)?,
        presence_evidence_hash: canonical_hash(&unit.presence_evidence)?,
        input_tree_hash: manifest.input_tree_hash.clone(),
        protocol_version: manifest.protocol_version.clone(),
        mechanism_ontology_version: manifest.mechanism_ontology_version.clone(),
        paired_configuration_hash: manifest.paired_configuration_hash.clone(),
        target_root_count,
        detected_target_root_count,
        candidate_count,
        target_anchor_matched_candidate_count: target_matched_candidates,
        unlabeled_candidate_count,
        outcome: candidate.outcome.clone(),
    };
    value.validate()?;
    Ok(value)
}

fn real_finding_requires_adjudication(
    revision_role: &RevisionRole,
    roots: &[OracleRoot],
    finding: &CandidateFinding,
) -> bool {
    match revision_role {
        RevisionRole::MatchedFixControl => true,
        RevisionRole::PositiveDefectPresent => {
            roots
                .iter()
                .filter(|root| finding_matches_root(finding, root))
                .count()
                != 1
        }
    }
}

/// Produces candidate-only adjudication items for the real corpus. Every
/// control finding is exported because a fixed revision is not empty ground
/// truth. Positive findings are withheld only when they match exactly one
/// deterministic target root; unmatched and ambiguous findings remain blind.
pub fn blind_real_adjudication_export(
    manifest: &TrialManifest,
    candidate: &CandidateOutput,
    oracle: &RealPrivateOracle,
    unit: &RealUnitContract,
) -> Result<(
    Vec<BlindAdjudicationItem>,
    Vec<PrivateAdjudicationReconciliation>,
)> {
    validate_candidate_against_manifest(manifest, candidate)?;
    validate_real_oracle_bindings(manifest, oracle, unit)?;
    let candidate_hash = canonical_hash(candidate)?;
    let mut public = Vec::new();
    let mut private = Vec::new();
    for finding in &candidate.findings {
        if !real_finding_requires_adjudication(&oracle.revision_role, &oracle.target_roots, finding)
        {
            continue;
        }
        let item_id = format!(
            "adj:{}",
            canonical_hash(&(candidate_hash.as_str(), finding.local_id.as_str()))?
        );
        public.push(BlindAdjudicationItem {
            item_id: item_id.clone(),
            finding: BlindFinding {
                locations: finding.locations.clone(),
                mechanism_tags: finding.mechanism_tags.clone(),
            },
        });
        private.push(PrivateAdjudicationReconciliation {
            schema: "reviewgraphen.benchmark.adjudication_reconciliation.v1".to_owned(),
            item_id,
            trial_id: candidate.trial_id.clone(),
            finding_local_id: finding.local_id.clone(),
        });
    }
    validate_reconciliation(&private)?;
    Ok((public, private))
}

impl RealScore {
    pub fn validate(&self) -> Result<()> {
        if self.schema != REAL_SCORE_SCHEMA
            || !text(&self.trial_id)
            || !text(&self.benchmark_unit_id)
            || !text(&self.trial_unit_id)
            || !text(&self.target_id)
            || self.replicate == 0
            || !exact_sha256(&self.manifest_hash)
            || !exact_sha256(&self.candidate_hash)
            || !exact_sha256(&self.oracle_hash)
            || !exact_sha256(&self.real_unit_hash)
            || !exact_sha256(&self.presence_evidence_hash)
            || !exact_git_hash(&self.input_tree_hash)
            || self.protocol_version != PROTOCOL_VERSION
            || self.mechanism_ontology_version != MECHANISM_ONTOLOGY_VERSION
            || !exact_sha256(&self.paired_configuration_hash)
            || self.detected_target_root_count > self.target_root_count
            || self.target_anchor_matched_candidate_count > self.candidate_count
            || self.unlabeled_candidate_count > self.candidate_count
            || (!matches!(self.outcome, Outcome::Structured) && self.candidate_count != 0)
        {
            return Err(BenchmarkError::Validation("invalid real score"));
        }
        match (&self.revision_role, &self.target_expectation) {
            (RevisionRole::PositiveDefectPresent, TargetExpectation::Present)
                if self.target_root_count > 0
                    && self.unlabeled_candidate_count
                        == self
                            .candidate_count
                            .saturating_sub(self.target_anchor_matched_candidate_count) => {}
            (RevisionRole::MatchedFixControl, TargetExpectation::Absent)
                if self.target_root_count == 0
                    && self.detected_target_root_count == 0
                    && self.unlabeled_candidate_count == self.candidate_count => {}
            _ => return Err(BenchmarkError::Validation("invalid real score semantics")),
        }
        Ok(())
    }
}

pub fn parse_real_unit(bytes: &[u8]) -> Result<RealUnitContract> {
    let value: RealUnitContract = serde_json::from_slice(bytes)?;
    value.validate()?;
    Ok(value)
}

pub fn parse_real_oracle(bytes: &[u8]) -> Result<RealPrivateOracle> {
    let value: RealPrivateOracle = serde_json::from_slice(bytes)?;
    value.validate()?;
    Ok(value)
}

pub fn parse_real_score(bytes: &[u8]) -> Result<RealScore> {
    let value: RealScore = serde_json::from_slice(bytes)?;
    value.validate()?;
    Ok(value)
}

pub fn parse_real_inventory(bytes: &[u8]) -> Result<RealTrialInventory> {
    let value: RealTrialInventory = serde_json::from_slice(bytes)?;
    value.validate()?;
    Ok(value)
}

impl RealTrialInventory {
    pub fn validate(&self) -> Result<()> {
        if self.schema != REAL_INVENTORY_SCHEMA || self.trials.is_empty() {
            return Err(BenchmarkError::Validation("invalid real inventory"));
        }
        let mut trial_ids = BTreeSet::new();
        let mut cells =
            BTreeMap::<(String, u32, RevisionRole), BTreeMap<String, &RealInventoryTrial>>::new();
        for trial in &self.trials {
            if !text(&trial.trial_id)
                || !text(&trial.benchmark_unit_id)
                || !text(&trial.trial_unit_id)
                || trial.replicate == 0
                || !exact_sha256(&trial.manifest_hash)
                || !exact_sha256(&trial.paired_configuration_hash)
                || !exact_sha256(&trial.real_unit_hash)
                || !exact_sha256(&trial.presence_evidence_hash)
                || !trial_ids.insert(&trial.trial_id)
            {
                return Err(BenchmarkError::Validation("invalid real inventory trial"));
            }
            let arm = match trial.arm {
                Arm::B1FreeForm => "b1",
                Arm::G3Proxy => "g3",
                Arm::FullReviewGraphen => "full",
            };
            if cells
                .entry((
                    trial.benchmark_unit_id.clone(),
                    trial.replicate,
                    trial.revision_role.clone(),
                ))
                .or_default()
                .insert(arm.to_owned(), trial)
                .is_some()
            {
                return Err(BenchmarkError::Validation("duplicate real inventory cell"));
            }
        }
        let fix_pairs = self
            .trials
            .iter()
            .map(|trial| (trial.benchmark_unit_id.clone(), trial.replicate))
            .collect::<BTreeSet<_>>();
        for key in fix_pairs {
            let positive = cells.get(&(key.0.clone(), key.1, RevisionRole::PositiveDefectPresent));
            let control = cells.get(&(key.0.clone(), key.1, RevisionRole::MatchedFixControl));
            let (Some(positive), Some(control)) = (positive, control) else {
                return Err(BenchmarkError::Validation("incomplete real fix pair"));
            };
            for pair in [positive, control] {
                if pair.len() == 1 && pair.contains_key("full") {
                    continue;
                }
                let (Some(b1), Some(g3)) = (pair.get("b1"), pair.get("g3")) else {
                    return Err(BenchmarkError::Validation("incomplete real arm pair"));
                };
                if b1.paired_configuration_hash != g3.paired_configuration_hash
                    || b1.trial_unit_id != g3.trial_unit_id
                    || b1.real_unit_hash != g3.real_unit_hash
                    || b1.presence_evidence_hash != g3.presence_evidence_hash
                {
                    return Err(BenchmarkError::Validation("real arm pair binding mismatch"));
                }
            }
            let representative = positive.values().next().expect("positive pair nonempty");
            if positive.values().chain(control.values()).any(|trial| {
                trial.real_unit_hash != representative.real_unit_hash
                    || trial.presence_evidence_hash != representative.presence_evidence_hash
            }) {
                return Err(BenchmarkError::Validation("real fix-pair binding mismatch"));
            }
        }
        Ok(())
    }
}

fn add_real_reason(
    reasons: &mut BTreeMap<(String, u32), BTreeSet<String>>,
    trial: &RealInventoryTrial,
    reason: &str,
) {
    reasons
        .entry((trial.benchmark_unit_id.clone(), trial.replicate))
        .or_default()
        .insert(reason.to_owned());
}

pub fn summarize_real_run(
    inventory: &RealTrialInventory,
    collections: &[TrialCollection],
    scores: &[RealScore],
) -> Result<RealRunSummary> {
    inventory.validate()?;
    let expected = inventory
        .trials
        .iter()
        .map(|trial| (trial.trial_id.as_str(), trial))
        .collect::<BTreeMap<_, _>>();
    let mut collection_map = BTreeMap::new();
    for collection in collections {
        collection.validate()?;
        if !expected.contains_key(collection.trial_id.as_str())
            || collection_map
                .insert(collection.trial_id.as_str(), collection)
                .is_some()
        {
            return Err(BenchmarkError::Validation(
                "unknown or duplicate real collection",
            ));
        }
    }
    let mut score_map = BTreeMap::new();
    for score in scores {
        score.validate()?;
        if !expected.contains_key(score.trial_id.as_str())
            || score_map.insert(score.trial_id.as_str(), score).is_some()
        {
            return Err(BenchmarkError::Validation(
                "unknown or duplicate real score",
            ));
        }
    }
    let mut summary = RealRunSummary {
        schema: REAL_RUN_SUMMARY_SCHEMA.to_owned(),
        prepared_trials: u32::try_from(inventory.trials.len())
            .map_err(|_| BenchmarkError::Validation("real prepared trial count"))?,
        valid_collections: 0,
        protocol_invalid_trials: 0,
        collection_binding_invalid_trials: 0,
        missing_trials: 0,
        eligible_fix_pairs: 0,
        excluded_fix_pairs: 0,
        exclusion_reason_counts: BTreeMap::new(),
        b1_positive_detected_targets: 0,
        g3_proxy_positive_detected_targets: 0,
        total_positive_targets_per_arm: 0,
        g3_minus_b1_positive_detected_targets: 0,
        b1_control_unlabeled_findings: 0,
        g3_proxy_control_unlabeled_findings: 0,
        b1_control_target_anchor_allegations: 0,
        g3_proxy_control_target_anchor_allegations: 0,
    };
    let mut reasons = BTreeMap::<(String, u32), BTreeSet<String>>::new();
    let mut cells = BTreeMap::<(String, u32, RevisionRole, String), &RealScore>::new();
    for trial in &inventory.trials {
        let Some(collection) = collection_map.get(trial.trial_id.as_str()) else {
            checked_increment(&mut summary.missing_trials, "real missing trial overflow")?;
            add_real_reason(&mut reasons, trial, "missing_collection");
            continue;
        };
        if collection.manifest_hash != trial.manifest_hash {
            checked_increment(
                &mut summary.collection_binding_invalid_trials,
                "real collection binding count overflow",
            )?;
            add_real_reason(&mut reasons, trial, "collection_manifest_mismatch");
            continue;
        }
        if matches!(collection.outcome, CollectionOutcome::ProtocolInvalid) {
            checked_increment(
                &mut summary.protocol_invalid_trials,
                "real protocol invalid count overflow",
            )?;
            add_real_reason(&mut reasons, trial, "protocol_invalid");
            continue;
        }
        checked_increment(
            &mut summary.valid_collections,
            "real valid collection overflow",
        )?;
        let Some(score) = score_map.get(trial.trial_id.as_str()) else {
            add_real_reason(&mut reasons, trial, "missing_score");
            continue;
        };
        let expected_outcome = match collection.outcome {
            CollectionOutcome::Structured => Outcome::Structured,
            CollectionOutcome::Abstained => Outcome::Abstained,
            CollectionOutcome::ParseFailure => Outcome::ParseFailure,
            CollectionOutcome::ProtocolInvalid => unreachable!("handled above"),
        };
        if score.benchmark_unit_id != trial.benchmark_unit_id
            || score.trial_unit_id != trial.trial_unit_id
            || score.revision_role != trial.revision_role
            || score.arm != trial.arm
            || score.replicate != trial.replicate
            || score.manifest_hash != trial.manifest_hash
            || score.candidate_hash != collection.candidate_hash
            || score.paired_configuration_hash != trial.paired_configuration_hash
            || score.real_unit_hash != trial.real_unit_hash
            || score.presence_evidence_hash != trial.presence_evidence_hash
            || score.outcome != expected_outcome
        {
            add_real_reason(&mut reasons, trial, "score_binding_mismatch");
            continue;
        }
        let arm = match trial.arm {
            Arm::B1FreeForm => "b1",
            Arm::G3Proxy => "g3",
            Arm::FullReviewGraphen => "full",
        };
        cells.insert(
            (
                trial.benchmark_unit_id.clone(),
                trial.replicate,
                trial.revision_role.clone(),
                arm.to_owned(),
            ),
            score,
        );
    }
    let fix_pairs = inventory
        .trials
        .iter()
        .map(|trial| (trial.benchmark_unit_id.clone(), trial.replicate))
        .collect::<BTreeSet<_>>();
    for key in fix_pairs {
        let cell = |role, arm: &str| {
            cells
                .get(&(key.0.clone(), key.1, role, arm.to_owned()))
                .copied()
        };
        let tuple = (
            cell(RevisionRole::PositiveDefectPresent, "b1"),
            cell(RevisionRole::PositiveDefectPresent, "g3"),
            cell(RevisionRole::MatchedFixControl, "b1"),
            cell(RevisionRole::MatchedFixControl, "g3"),
        );
        if !reasons.contains_key(&key) && !matches!(tuple, (Some(_), Some(_), Some(_), Some(_))) {
            reasons
                .entry(key.clone())
                .or_default()
                .insert("incomplete_scored_fix_pair".to_owned());
        }
        if let Some(pair_reasons) = reasons.get(&key) {
            checked_increment(
                &mut summary.excluded_fix_pairs,
                "excluded fix-pair overflow",
            )?;
            for reason in pair_reasons {
                let value = summary
                    .exclusion_reason_counts
                    .entry(reason.clone())
                    .or_insert(0);
                checked_increment(value, "real exclusion reason overflow")?;
            }
            continue;
        }
        let (Some(pb), Some(pg), Some(cb), Some(cg)) = tuple else {
            return Err(BenchmarkError::Validation(
                "eligible real fix pair disappeared",
            ));
        };
        if pb.target_root_count != pg.target_root_count
            || pb.oracle_hash != pg.oracle_hash
            || cb.oracle_hash != cg.oracle_hash
            || pb.real_unit_hash != pg.real_unit_hash
            || pb.real_unit_hash != cb.real_unit_hash
            || pb.real_unit_hash != cg.real_unit_hash
            || pb.presence_evidence_hash != pg.presence_evidence_hash
            || pb.presence_evidence_hash != cb.presence_evidence_hash
            || pb.presence_evidence_hash != cg.presence_evidence_hash
        {
            checked_increment(
                &mut summary.excluded_fix_pairs,
                "excluded fix-pair overflow",
            )?;
            let value = summary
                .exclusion_reason_counts
                .entry("incompatible_real_scores".to_owned())
                .or_insert(0);
            checked_increment(value, "real exclusion reason overflow")?;
            continue;
        }
        checked_increment(
            &mut summary.eligible_fix_pairs,
            "eligible fix-pair overflow",
        )?;
        summary.b1_positive_detected_targets = summary
            .b1_positive_detected_targets
            .checked_add(pb.detected_target_root_count)
            .ok_or(BenchmarkError::Validation("B1 real target total overflow"))?;
        summary.g3_proxy_positive_detected_targets = summary
            .g3_proxy_positive_detected_targets
            .checked_add(pg.detected_target_root_count)
            .ok_or(BenchmarkError::Validation("G3 real target total overflow"))?;
        summary.total_positive_targets_per_arm = summary
            .total_positive_targets_per_arm
            .checked_add(pb.target_root_count)
            .ok_or(BenchmarkError::Validation(
                "real target denominator overflow",
            ))?;
        summary.b1_control_unlabeled_findings = summary
            .b1_control_unlabeled_findings
            .checked_add(cb.unlabeled_candidate_count)
            .ok_or(BenchmarkError::Validation("B1 control finding overflow"))?;
        summary.g3_proxy_control_unlabeled_findings = summary
            .g3_proxy_control_unlabeled_findings
            .checked_add(cg.unlabeled_candidate_count)
            .ok_or(BenchmarkError::Validation("G3 control finding overflow"))?;
        summary.b1_control_target_anchor_allegations = summary
            .b1_control_target_anchor_allegations
            .checked_add(cb.target_anchor_matched_candidate_count)
            .ok_or(BenchmarkError::Validation("B1 control allegation overflow"))?;
        summary.g3_proxy_control_target_anchor_allegations = summary
            .g3_proxy_control_target_anchor_allegations
            .checked_add(cg.target_anchor_matched_candidate_count)
            .ok_or(BenchmarkError::Validation("G3 control allegation overflow"))?;
    }
    summary.g3_minus_b1_positive_detected_targets =
        i64::from(summary.g3_proxy_positive_detected_targets)
            .checked_sub(i64::from(summary.b1_positive_detected_targets))
            .ok_or(BenchmarkError::Validation("real target delta overflow"))?;
    let classified = summary
        .valid_collections
        .checked_add(summary.protocol_invalid_trials)
        .and_then(|value| value.checked_add(summary.collection_binding_invalid_trials))
        .and_then(|value| value.checked_add(summary.missing_trials))
        .ok_or(BenchmarkError::Validation("real trial status overflow"))?;
    if classified != summary.prepared_trials {
        return Err(BenchmarkError::Validation(
            "real trial statuses do not partition inventory",
        ));
    }
    Ok(summary)
}

/// Summarizes an additive full-ReviewGraphen-only result band. The legacy
/// `real_run_summary.v1` remains the frozen paired B1/G3 contract.
pub fn summarize_real_full_run(
    inventory: &RealTrialInventory,
    collections: &[TrialCollection],
    scores: &[RealScore],
) -> Result<RealFullRunSummary> {
    inventory.validate()?;
    if inventory
        .trials
        .iter()
        .any(|trial| trial.arm != Arm::FullReviewGraphen)
    {
        return Err(BenchmarkError::Validation(
            "full real summary requires only full ReviewGraphen trials",
        ));
    }
    let expected = inventory
        .trials
        .iter()
        .map(|trial| (trial.trial_id.as_str(), trial))
        .collect::<BTreeMap<_, _>>();
    let mut collection_map = BTreeMap::new();
    for collection in collections {
        collection.validate()?;
        if !expected.contains_key(collection.trial_id.as_str())
            || collection_map
                .insert(collection.trial_id.as_str(), collection)
                .is_some()
        {
            return Err(BenchmarkError::Validation(
                "unknown or duplicate full real collection",
            ));
        }
    }
    let mut score_map = BTreeMap::new();
    for score in scores {
        score.validate()?;
        if !expected.contains_key(score.trial_id.as_str())
            || score.arm != Arm::FullReviewGraphen
            || score_map.insert(score.trial_id.as_str(), score).is_some()
        {
            return Err(BenchmarkError::Validation(
                "unknown, non-full, or duplicate full real score",
            ));
        }
    }
    let mut summary = RealFullRunSummary {
        schema: REAL_FULL_RUN_SUMMARY_SCHEMA.to_owned(),
        prepared_trials: u32::try_from(inventory.trials.len())
            .map_err(|_| BenchmarkError::Validation("full real prepared trial count"))?,
        valid_collections: 0,
        protocol_invalid_trials: 0,
        collection_binding_invalid_trials: 0,
        missing_trials: 0,
        eligible_fix_pairs: 0,
        excluded_fix_pairs: 0,
        exclusion_reason_counts: BTreeMap::new(),
        full_positive_detected_targets: 0,
        total_positive_targets: 0,
        full_positive_findings: 0,
        full_positive_unlabeled_findings: 0,
        full_control_findings: 0,
        full_control_unlabeled_findings: 0,
        full_control_target_anchor_allegations: 0,
    };
    let mut reasons = BTreeMap::<(String, u32), BTreeSet<String>>::new();
    let mut cells = BTreeMap::<(String, u32, RevisionRole), &RealScore>::new();
    for trial in &inventory.trials {
        let Some(collection) = collection_map.get(trial.trial_id.as_str()) else {
            checked_increment(
                &mut summary.missing_trials,
                "full real missing trial overflow",
            )?;
            add_real_reason(&mut reasons, trial, "missing_collection");
            continue;
        };
        if collection.manifest_hash != trial.manifest_hash {
            checked_increment(
                &mut summary.collection_binding_invalid_trials,
                "full real collection binding count overflow",
            )?;
            add_real_reason(&mut reasons, trial, "collection_manifest_mismatch");
            continue;
        }
        if matches!(collection.outcome, CollectionOutcome::ProtocolInvalid) {
            checked_increment(
                &mut summary.protocol_invalid_trials,
                "full real protocol invalid count overflow",
            )?;
            add_real_reason(&mut reasons, trial, "protocol_invalid");
            continue;
        }
        checked_increment(
            &mut summary.valid_collections,
            "full real valid collection overflow",
        )?;
        let Some(score) = score_map.get(trial.trial_id.as_str()) else {
            add_real_reason(&mut reasons, trial, "missing_score");
            continue;
        };
        let expected_outcome = match collection.outcome {
            CollectionOutcome::Structured => Outcome::Structured,
            CollectionOutcome::Abstained => Outcome::Abstained,
            CollectionOutcome::ParseFailure => Outcome::ParseFailure,
            CollectionOutcome::ProtocolInvalid => unreachable!("handled above"),
        };
        if score.benchmark_unit_id != trial.benchmark_unit_id
            || score.trial_unit_id != trial.trial_unit_id
            || score.revision_role != trial.revision_role
            || score.arm != trial.arm
            || score.replicate != trial.replicate
            || score.manifest_hash != trial.manifest_hash
            || score.candidate_hash != collection.candidate_hash
            || score.paired_configuration_hash != trial.paired_configuration_hash
            || score.real_unit_hash != trial.real_unit_hash
            || score.presence_evidence_hash != trial.presence_evidence_hash
            || score.outcome != expected_outcome
            || cells
                .insert(
                    (
                        trial.benchmark_unit_id.clone(),
                        trial.replicate,
                        trial.revision_role.clone(),
                    ),
                    score,
                )
                .is_some()
        {
            add_real_reason(&mut reasons, trial, "score_binding_mismatch");
        }
    }
    let fix_pairs = inventory
        .trials
        .iter()
        .map(|trial| (trial.benchmark_unit_id.clone(), trial.replicate))
        .collect::<BTreeSet<_>>();
    for key in fix_pairs {
        let positive = cells
            .get(&(key.0.clone(), key.1, RevisionRole::PositiveDefectPresent))
            .copied();
        let control = cells
            .get(&(key.0.clone(), key.1, RevisionRole::MatchedFixControl))
            .copied();
        if !reasons.contains_key(&key) && (positive.is_none() || control.is_none()) {
            reasons
                .entry(key.clone())
                .or_default()
                .insert("incomplete_scored_fix_pair".to_owned());
        }
        if let Some(pair_reasons) = reasons.get(&key) {
            checked_increment(
                &mut summary.excluded_fix_pairs,
                "full real excluded pair overflow",
            )?;
            for reason in pair_reasons {
                let count = summary
                    .exclusion_reason_counts
                    .entry(reason.clone())
                    .or_insert(0);
                checked_increment(count, "full real exclusion reason overflow")?;
            }
            continue;
        }
        let (Some(positive), Some(control)) = (positive, control) else {
            return Err(BenchmarkError::Validation(
                "eligible full real fix pair disappeared",
            ));
        };
        if positive.real_unit_hash != control.real_unit_hash
            || positive.presence_evidence_hash != control.presence_evidence_hash
        {
            checked_increment(
                &mut summary.excluded_fix_pairs,
                "full real excluded pair overflow",
            )?;
            checked_increment(
                summary
                    .exclusion_reason_counts
                    .entry("incompatible_real_scores".to_owned())
                    .or_insert(0),
                "full real exclusion reason overflow",
            )?;
            continue;
        }
        checked_increment(
            &mut summary.eligible_fix_pairs,
            "full real eligible pair overflow",
        )?;
        summary.full_positive_detected_targets = summary
            .full_positive_detected_targets
            .checked_add(positive.detected_target_root_count)
            .ok_or(BenchmarkError::Validation(
                "full real detected target overflow",
            ))?;
        summary.total_positive_targets = summary
            .total_positive_targets
            .checked_add(positive.target_root_count)
            .ok_or(BenchmarkError::Validation(
                "full real target denominator overflow",
            ))?;
        summary.full_positive_findings = summary
            .full_positive_findings
            .checked_add(positive.candidate_count)
            .ok_or(BenchmarkError::Validation(
                "full real positive finding overflow",
            ))?;
        summary.full_positive_unlabeled_findings = summary
            .full_positive_unlabeled_findings
            .checked_add(positive.unlabeled_candidate_count)
            .ok_or(BenchmarkError::Validation(
                "full real positive unlabeled finding overflow",
            ))?;
        summary.full_control_findings = summary
            .full_control_findings
            .checked_add(control.candidate_count)
            .ok_or(BenchmarkError::Validation(
                "full real control finding overflow",
            ))?;
        summary.full_control_unlabeled_findings = summary
            .full_control_unlabeled_findings
            .checked_add(control.unlabeled_candidate_count)
            .ok_or(BenchmarkError::Validation(
                "full real control unlabeled finding overflow",
            ))?;
        summary.full_control_target_anchor_allegations = summary
            .full_control_target_anchor_allegations
            .checked_add(control.target_anchor_matched_candidate_count)
            .ok_or(BenchmarkError::Validation(
                "full real control allegation overflow",
            ))?;
    }
    let classified = summary
        .valid_collections
        .checked_add(summary.protocol_invalid_trials)
        .and_then(|count| count.checked_add(summary.collection_binding_invalid_trials))
        .and_then(|count| count.checked_add(summary.missing_trials))
        .ok_or(BenchmarkError::Validation(
            "full real trial status overflow",
        ))?;
    if classified != summary.prepared_trials
        || summary.eligible_fix_pairs + summary.excluded_fix_pairs
            != u32::try_from(
                inventory
                    .trials
                    .iter()
                    .map(|trial| (&trial.benchmark_unit_id, trial.replicate))
                    .collect::<BTreeSet<_>>()
                    .len(),
            )
            .map_err(|_| BenchmarkError::Validation("full real pair count"))?
    {
        return Err(BenchmarkError::Validation(
            "full real summary partitions do not close",
        ));
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(value: char) -> ContentHash {
        ContentHash::parse(format!("sha256:{}", value.to_string().repeat(64))).expect("hash")
    }
    fn git(value: char) -> ContentHash {
        ContentHash::parse(format!("git:{}", value.to_string().repeat(40))).expect("git hash")
    }
    fn run(commit: char, tree: char, status: i32) -> TestExecutionEvidence {
        TestExecutionEvidence {
            commit_hash: git(commit),
            tree_hash: git(tree),
            command: vec!["cargo".into(), "test".into(), "regression_case".into()],
            working_directory: "workspace".into(),
            exit_status: status,
            stdout_sha256: hash('a'),
            stderr_sha256: hash('b'),
            combined_artifact_sha256: hash('c'),
        }
    }
    fn unit() -> RealUnitContract {
        RealUnitContract {
            schema: REAL_UNIT_SCHEMA.into(),
            corpus_semantics: CorpusSemantics::RegressionFixPair,
            snapshot_semantics: SnapshotSemantics::ParentPositiveFixControl,
            reviewer_input_semantics: ReviewerInputSemantics::SelectedProductionSnapshot,
            control_finding_semantics: ControlFindingSemantics::UnlabeledRequiresAdjudication,
            projection_policy_version: "m7-real-production-paths.v1".into(),
            selected_production_paths: BTreeSet::from(["src/a.rs".into()]),
            positive_source_inventory_hash: hash('e'),
            control_source_inventory_hash: hash('f'),
            benchmark_unit_id: "real:u1".into(),
            target_id: "target:u1".into(),
            positive_trial_unit_id: "opaque:p".into(),
            control_trial_unit_id: "opaque:c".into(),
            parent_commit: git('1'),
            fix_commit: git('2'),
            positive_tree_hash: git('3'),
            control_tree_hash: git('4'),
            mechanism_ontology_version: MECHANISM_ONTOLOGY_VERSION.into(),
            presence_evidence: RegressionPresenceEvidence {
                schema: PRESENCE_EVIDENCE_SCHEMA.into(),
                benchmark_unit_id: "real:u1".into(),
                test_selector: "private regression selector".into(),
                test_source_sha256: hash('d'),
                strategy: RegressionTestStrategy::FixRegressionTestBackportedToParent,
                parent_run: run('1', '3', 101),
                fix_run: run('2', '4', 0),
            },
        }
    }

    #[test]
    fn presence_evidence_requires_parent_fail_and_fix_pass() {
        let mut value = unit();
        value.validate().expect("valid real unit");
        value.presence_evidence.parent_run.exit_status = 0;
        assert!(value.validate().is_err());
        let mut value = unit();
        value
            .presence_evidence
            .fix_run
            .command
            .push("different".into());
        assert!(value.validate().is_err());
    }

    #[test]
    fn control_score_keeps_every_finding_unlabeled() {
        let score = RealScore {
            schema: REAL_SCORE_SCHEMA.into(),
            trial_id: "trial:c:b1".into(),
            benchmark_unit_id: "real:u1".into(),
            trial_unit_id: "opaque:c".into(),
            target_id: "target:u1".into(),
            arm: Arm::B1FreeForm,
            replicate: 1,
            revision_role: RevisionRole::MatchedFixControl,
            target_expectation: TargetExpectation::Absent,
            control_finding_semantics: ControlFindingSemantics::UnlabeledRequiresAdjudication,
            manifest_hash: hash('a'),
            candidate_hash: hash('b'),
            oracle_hash: hash('c'),
            real_unit_hash: hash('d'),
            presence_evidence_hash: hash('e'),
            input_tree_hash: git('4'),
            protocol_version: PROTOCOL_VERSION.into(),
            mechanism_ontology_version: MECHANISM_ONTOLOGY_VERSION.into(),
            paired_configuration_hash: hash('f'),
            target_root_count: 0,
            detected_target_root_count: 0,
            candidate_count: 3,
            target_anchor_matched_candidate_count: 1,
            unlabeled_candidate_count: 3,
            outcome: Outcome::Structured,
        };
        score.validate().expect("control findings remain unlabeled");
        let mut invalid = score;
        invalid.unlabeled_candidate_count = 2;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn ontology_stays_at_frozen_pilot_v2_boundary() {
        assert_eq!(
            MECHANISM_ONTOLOGY_VERSION,
            "reviewgraphen.benchmark.mechanism_ontology.v1"
        );
        assert_eq!(MECHANISM_ONTOLOGY.len(), 8);
    }

    #[test]
    fn full_summary_keeps_its_denominator_when_results_are_missing() {
        let trial = |trial_id: &str, trial_unit_id: &str, revision_role| RealInventoryTrial {
            trial_id: trial_id.into(),
            benchmark_unit_id: "real:u1".into(),
            trial_unit_id: trial_unit_id.into(),
            revision_role,
            arm: Arm::FullReviewGraphen,
            replicate: 1,
            manifest_hash: hash('a'),
            paired_configuration_hash: hash('b'),
            real_unit_hash: hash('c'),
            presence_evidence_hash: hash('d'),
        };
        let inventory = RealTrialInventory {
            schema: REAL_INVENTORY_SCHEMA.into(),
            corpus_semantics: CorpusSemantics::RegressionFixPair,
            control_finding_semantics: ControlFindingSemantics::UnlabeledRequiresAdjudication,
            trials: vec![
                trial(
                    "trial:positive:full",
                    "opaque:p",
                    RevisionRole::PositiveDefectPresent,
                ),
                trial(
                    "trial:control:full",
                    "opaque:c",
                    RevisionRole::MatchedFixControl,
                ),
            ],
        };
        let summary = summarize_real_full_run(&inventory, &[], &[]).expect("summary");
        assert_eq!(summary.prepared_trials, 2);
        assert_eq!(summary.missing_trials, 2);
        assert_eq!(summary.eligible_fix_pairs, 0);
        assert_eq!(summary.excluded_fix_pairs, 1);
        assert_eq!(summary.exclusion_reason_counts["missing_collection"], 1);
    }

    #[test]
    fn real_adjudication_includes_all_controls_and_only_unresolved_positives() {
        let root = OracleRoot {
            root_id: "root:known".into(),
            tree_hash: git('3'),
            path: "src/a.rs".into(),
            file_sha256: hash('a'),
            symbol: "known".into(),
            start_line: 10,
            end_line: 12,
            span_sha256: hash('b'),
            mechanism_tags: BTreeSet::from([MechanismId::CrossFileContract]),
            severity: "high".into(),
        };
        let matched = CandidateFinding {
            local_id: "matched".into(),
            locations: vec![Location {
                path: "src/a.rs".into(),
                start_line: 11,
                end_line: 11,
            }],
            mechanism_tags: BTreeSet::from([MechanismId::CrossFileContract]),
            severity: None,
            rationale: None,
        };
        let unmatched = CandidateFinding {
            local_id: "unmatched".into(),
            locations: vec![Location {
                path: "src/other.rs".into(),
                start_line: 1,
                end_line: 1,
            }],
            mechanism_tags: BTreeSet::from([MechanismId::StateTransitionGap]),
            severity: None,
            rationale: None,
        };
        assert!(real_finding_requires_adjudication(
            &RevisionRole::MatchedFixControl,
            std::slice::from_ref(&root),
            &matched
        ));
        assert!(!real_finding_requires_adjudication(
            &RevisionRole::PositiveDefectPresent,
            std::slice::from_ref(&root),
            &matched
        ));
        assert!(real_finding_requires_adjudication(
            &RevisionRole::PositiveDefectPresent,
            &[root],
            &unmatched
        ));
    }
}
