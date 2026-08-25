//! Experimental resource reservation, allocation, and reconciliation protocol.
//!
//! Topology claims, runtime grants, runtime-reported allocations, and derived
//! reconciliation remain separate records. This module is deterministic and
//! performs no locking, scheduling, worktree mutation, cleanup, or liveness
//! inference.

use crate::execution_topology::{
    execution_topology_content_hash, ExecutionTopology, ResourceClaim, ResourceMode,
    WorkspaceStrategy,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const RESOURCE_DECLARATION_SCHEMA: &str = "casegraphen.experimental.resource.declaration.v0";
pub const RESOURCE_RESERVATION_SCHEMA: &str = "casegraphen.experimental.resource.reservation.v0";
pub const RESERVATION_ASSERTION_SCHEMA: &str =
    "casegraphen.experimental.resource.reservation_disposition.v0";
pub const RATE_LIMIT_CAPACITY_SCHEMA: &str =
    "casegraphen.experimental.resource.rate_limit_capacity.v0";
pub const RESOURCE_ALLOCATION_SCHEMA: &str =
    "casegraphen.experimental.runtime.resource_allocation.v0";
pub const RESOURCE_RECONCILIATION_SCHEMA: &str =
    "casegraphen.experimental.resource.reconciliation.v0";
pub const WORKTREE_RECORD_SCHEMA: &str = "casegraphen.experimental.git.worktree_record.v0";
pub const RUNTIME_ALLOCATION_TRUST_BOUNDARY: &str =
    "runtime_reported_untrusted_until_independently_reconciled";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceDeclaration {
    pub schema: String,
    pub schema_version: u32,
    pub declaration_id: String,
    pub runtime_graph_id: String,
    pub runtime_graph_content_hash: String,
    pub node_id: String,
    pub claims: Vec<ResourceClaim>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceGrant {
    pub resource_id: String,
    pub mode: ResourceMode,
    pub rate_limit_group: Option<String>,
    pub rate_limit_units: u32,
    pub workspace_strategy: Option<WorkspaceStrategy>,
    pub network_scope: Vec<String>,
    pub secret_scope: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceReservation {
    pub schema: String,
    pub schema_version: u32,
    pub reservation_id: String,
    pub declaration_id: String,
    pub attempt_id: String,
    pub granted_at: String,
    pub grants: Vec<ResourceGrant>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReservationAssertionKind {
    Release,
    Supersede,
    /// Explicit operator/runtime-controller assertion that an externally
    /// governed lease expired. CaseGraphen never infers this from wall time.
    Expire,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReservationDispositionAssertion {
    pub schema: String,
    pub schema_version: u32,
    pub assertion_id: String,
    pub reservation_id: String,
    pub attempt_id: String,
    pub kind: ReservationAssertionKind,
    pub asserted_by: String,
    pub reason: String,
    pub superseding_reservation_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimitCapacity {
    pub schema: String,
    pub schema_version: u32,
    pub group_id: String,
    pub capacity: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeResourceAllocation {
    pub schema: String,
    pub schema_version: u32,
    pub allocation_id: String,
    pub reservation_id: String,
    pub attempt_id: String,
    pub resource_id: String,
    pub mode: ResourceMode,
    pub rate_limit_group: Option<String>,
    pub rate_limit_units: u32,
    pub workspace_strategy: Option<WorkspaceStrategy>,
    pub network_scope: Vec<String>,
    pub secret_scope: Vec<String>,
    pub worktree_id: Option<String>,
    pub trust_boundary: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceProtocolFinding {
    pub code: String,
    pub resource_id: Option<String>,
    pub reservation_id: Option<String>,
    pub allocation_id: Option<String>,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceReconciliation {
    pub schema: String,
    pub schema_version: u32,
    pub declaration_id: String,
    pub reservation_id: String,
    pub attempt_id: String,
    pub declared_claim_count: u64,
    pub granted_resource_count: u64,
    pub actual_allocation_count: u64,
    pub complete: bool,
    pub findings: Vec<ResourceProtocolFinding>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeState {
    Active,
    Committed,
    CleanupApproved,
    Superseded,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorktreeCleanupPolicy {
    pub method: String,
    pub recoverable: bool,
    pub requires_explicit_assertion: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitWorktreeRecord {
    pub schema: String,
    pub schema_version: u32,
    pub worktree_id: String,
    pub reservation_id: String,
    pub attempt_id: String,
    pub path_identity: String,
    pub base_commit_sha: String,
    pub branch: String,
    pub resulting_commit_sha: Option<String>,
    pub working_tree_clean: bool,
    pub unexpected_write_paths: Vec<String>,
    pub state: WorktreeState,
    pub cleanup: WorktreeCleanupPolicy,
}

/// Converts topology claims without changing their meaning. A named rate-limit
/// group consumes one capacity unit per claim in v0.
pub fn declaration_grants(declaration: &ResourceDeclaration) -> Vec<ResourceGrant> {
    declaration
        .claims
        .iter()
        .map(|claim| ResourceGrant {
            resource_id: claim.resource.clone(),
            mode: claim.mode,
            rate_limit_group: claim.rate_limit_group.clone(),
            rate_limit_units: u32::from(claim.rate_limit_group.is_some()),
            workspace_strategy: claim.workspace_strategy,
            network_scope: claim.network_scope.clone(),
            secret_scope: claim.secret_scope.clone(),
        })
        .collect()
}

/// Returns true until a valid, explicit release or supersede assertion joins
/// both the reservation and attempt. Wall-clock time is deliberately absent.
pub fn reservation_is_active(
    reservation: &ResourceReservation,
    assertions: &[ReservationDispositionAssertion],
) -> bool {
    !assertions.iter().any(|assertion| {
        assertion.schema == RESERVATION_ASSERTION_SCHEMA
            && assertion.schema_version == 0
            && assertion.reservation_id == reservation.reservation_id
            && assertion.attempt_id == reservation.attempt_id
            && !assertion.assertion_id.is_empty()
            && !assertion.asserted_by.is_empty()
            && !assertion.reason.is_empty()
            && match assertion.kind {
                ReservationAssertionKind::Release => assertion.superseding_reservation_id.is_none(),
                ReservationAssertionKind::Supersede => assertion
                    .superseding_reservation_id
                    .as_deref()
                    .is_some_and(|id| !id.is_empty() && id != reservation.reservation_id),
                ReservationAssertionKind::Expire => assertion.superseding_reservation_id.is_none(),
            }
    })
}

/// Validates and grants the exact declared resources if they do not conflict
/// with still-active reservations or rate-limit capacities.
pub fn grant_reservation(
    declaration: &ResourceDeclaration,
    candidate: &ResourceReservation,
    existing: &[ResourceReservation],
    assertions: &[ReservationDispositionAssertion],
    capacities: &[RateLimitCapacity],
) -> Result<ResourceReservation, Vec<ResourceProtocolFinding>> {
    let mut findings = validate_declaration_and_reservation(declaration, candidate);
    let active = existing
        .iter()
        .filter(|reservation| reservation_is_active(reservation, assertions))
        .collect::<Vec<_>>();

    for reservation in &active {
        if reservation.reservation_id == candidate.reservation_id {
            findings.push(finding(
                "duplicate_reservation_id",
                None,
                Some(candidate.reservation_id.clone()),
                None,
                "an active reservation already uses this reservation id",
            ));
        }
        if reservation.attempt_id == candidate.attempt_id {
            findings.push(finding(
                "attempt_already_reserved",
                None,
                Some(candidate.reservation_id.clone()),
                None,
                "an active reservation already joins this attempt id",
            ));
        }
    }

    for grant in &candidate.grants {
        for reservation in &active {
            for held in &reservation.grants {
                if held.resource_id == grant.resource_id && modes_conflict(held.mode, grant.mode) {
                    findings.push(finding(
                        "resource_conflict",
                        Some(grant.resource_id.clone()),
                        Some(reservation.reservation_id.clone()),
                        None,
                        "an active reservation holds a conflicting resource mode",
                    ));
                }
            }
        }
    }

    let capacity_by_group = capacities
        .iter()
        .filter(|capacity| {
            capacity.schema == RATE_LIMIT_CAPACITY_SCHEMA && capacity.schema_version == 0
        })
        .map(|capacity| (capacity.group_id.as_str(), capacity.capacity))
        .collect::<BTreeMap<_, _>>();
    let mut seen_capacity_groups = BTreeSet::new();
    for capacity in capacities {
        if capacity.schema != RATE_LIMIT_CAPACITY_SCHEMA || capacity.schema_version != 0 {
            findings.push(finding(
                "unsupported_rate_limit_capacity_schema",
                None,
                Some(candidate.reservation_id.clone()),
                None,
                "capacity must use resource.rate_limit_capacity.v0",
            ));
        }
        if !is_canonical_id(&capacity.group_id) {
            findings.push(finding(
                "noncanonical_rate_limit_group",
                None,
                Some(candidate.reservation_id.clone()),
                None,
                "rate-limit group must be a canonical namespaced id",
            ));
        }
        if !seen_capacity_groups.insert(capacity.group_id.as_str()) {
            findings.push(finding(
                "duplicate_rate_limit_capacity",
                None,
                Some(candidate.reservation_id.clone()),
                None,
                "rate-limit group has more than one capacity record",
            ));
        }
    }
    let mut used = BTreeMap::<&str, u64>::new();
    for grant in active
        .iter()
        .flat_map(|reservation| reservation.grants.iter())
        .chain(&candidate.grants)
    {
        if let Some(group) = grant.rate_limit_group.as_deref() {
            *used.entry(group).or_default() += u64::from(grant.rate_limit_units);
        }
    }
    for (group, units) in used {
        match capacity_by_group.get(group).copied() {
            Some(capacity) if units <= u64::from(capacity) => {}
            Some(capacity) => findings.push(finding(
                "rate_limit_capacity_exceeded",
                None,
                Some(candidate.reservation_id.clone()),
                None,
                format!(
                    "rate-limit group {group:?} requests {units} units with capacity {capacity}"
                ),
            )),
            None => findings.push(finding(
                "missing_rate_limit_capacity",
                None,
                Some(candidate.reservation_id.clone()),
                None,
                format!("rate-limit group {group:?} has no explicit capacity"),
            )),
        }
    }

    findings.sort_by(finding_order);
    findings.dedup();
    if findings.is_empty() {
        Ok(candidate.clone())
    } else {
        Err(findings)
    }
}

/// Grants a reservation only after the declaration is joined to the exact
/// content-addressed topology node and its complete resource-claim set.
///
/// Runtime adapters should use this entry point. [`grant_reservation`] remains
/// the protocol-level compatibility evaluator for already joined declarations.
pub fn grant_topology_reservation(
    topology: &ExecutionTopology,
    declaration: &ResourceDeclaration,
    candidate: &ResourceReservation,
    existing: &[ResourceReservation],
    assertions: &[ReservationDispositionAssertion],
    capacities: &[RateLimitCapacity],
) -> Result<ResourceReservation, Vec<ResourceProtocolFinding>> {
    let mut findings = validate_resource_declaration(topology, declaration);
    if let Err(mut grant_findings) =
        grant_reservation(declaration, candidate, existing, assertions, capacities)
    {
        findings.append(&mut grant_findings);
    }
    findings.sort_by(finding_order);
    findings.dedup();
    if findings.is_empty() {
        Ok(candidate.clone())
    } else {
        Err(findings)
    }
}

/// Validates the declaration/topology join without granting or reserving.
pub fn validate_resource_declaration(
    topology: &ExecutionTopology,
    declaration: &ResourceDeclaration,
) -> Vec<ResourceProtocolFinding> {
    let mut findings = validate_declaration_record(declaration, None);
    let topology_hash = execution_topology_content_hash(topology)
        .expect("typed execution topology serializes deterministically");
    if declaration.runtime_graph_id != topology.topology_id
        || declaration.runtime_graph_content_hash != topology_hash
    {
        findings.push(finding(
            "declaration_graph_join_mismatch",
            None,
            None,
            None,
            "declaration must join the exact topology id and content hash",
        ));
    }
    match topology
        .nodes
        .iter()
        .find(|node| node.node_id == declaration.node_id)
    {
        None => findings.push(finding(
            "unknown_declaration_node",
            None,
            None,
            None,
            "declaration node_id does not exist in the topology",
        )),
        Some(node) => {
            let declared = declaration.claims.iter().cloned().collect::<BTreeSet<_>>();
            let expected = node
                .resource_claims
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            if declared != expected
                || declared.len() != declaration.claims.len()
                || expected.len() != node.resource_claims.len()
            {
                findings.push(finding(
                    "declaration_topology_claim_mismatch",
                    None,
                    None,
                    None,
                    "declaration claims must exactly equal the topology node resource claims",
                ));
            }
        }
    }
    findings.sort_by(finding_order);
    findings.dedup();
    findings
}

/// Reconciles untrusted actual allocations with the exact declaration and
/// reservation. A mismatch always keeps `complete` false.
pub fn reconcile_resource_allocations(
    declaration: &ResourceDeclaration,
    reservation: &ResourceReservation,
    allocations: &[RuntimeResourceAllocation],
) -> ResourceReconciliation {
    let mut findings = validate_declaration_and_reservation(declaration, reservation);
    let expected = reservation.grants.iter().cloned().collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();
    let mut allocation_ids = BTreeSet::new();
    for allocation in allocations {
        if !allocation_ids.insert(allocation.allocation_id.as_str()) {
            findings.push(finding(
                "duplicate_allocation_id",
                Some(allocation.resource_id.clone()),
                Some(reservation.reservation_id.clone()),
                Some(allocation.allocation_id.clone()),
                "allocation_id appears more than once",
            ));
        }
        if allocation.schema != RESOURCE_ALLOCATION_SCHEMA || allocation.schema_version != 0 {
            findings.push(allocation_finding(
                allocation,
                "unsupported_allocation_schema",
                "runtime allocation must use runtime.resource_allocation.v0",
            ));
        }
        if allocation.trust_boundary != RUNTIME_ALLOCATION_TRUST_BOUNDARY {
            findings.push(allocation_finding(
                allocation,
                "invalid_allocation_trust_boundary",
                "runtime allocation must retain the untrusted marker",
            ));
        }
        if allocation.reservation_id != reservation.reservation_id
            || allocation.attempt_id != reservation.attempt_id
        {
            findings.push(allocation_finding(
                allocation,
                "allocation_join_mismatch",
                "allocation must join the exact reservation and attempt",
            ));
            continue;
        }
        let grant = allocation_as_grant(allocation);
        if !expected.contains(&grant) {
            findings.push(allocation_finding(
                allocation,
                "unexpected_allocation",
                "runtime allocation does not exactly match a granted resource",
            ));
        }
        if !actual.insert(grant) {
            findings.push(allocation_finding(
                allocation,
                "duplicate_allocation",
                "the same granted resource was reported more than once",
            ));
        }
        validate_named_scopes(
            &allocation.network_scope,
            "network_scope",
            Some(allocation),
            &mut findings,
        );
        validate_named_scopes(
            &allocation.secret_scope,
            "secret_scope",
            Some(allocation),
            &mut findings,
        );
    }
    for missing in expected.difference(&actual) {
        findings.push(finding(
            "missing_allocation",
            Some(missing.resource_id.clone()),
            Some(reservation.reservation_id.clone()),
            None,
            "a granted resource has no matching runtime allocation",
        ));
    }
    findings.sort_by(finding_order);
    findings.dedup();
    ResourceReconciliation {
        schema: RESOURCE_RECONCILIATION_SCHEMA.to_owned(),
        schema_version: 0,
        declaration_id: declaration.declaration_id.clone(),
        reservation_id: reservation.reservation_id.clone(),
        attempt_id: reservation.attempt_id.clone(),
        declared_claim_count: declaration.claims.len() as u64,
        granted_resource_count: reservation.grants.len() as u64,
        actual_allocation_count: allocations.len() as u64,
        complete: findings.is_empty(),
        findings,
    }
}

pub fn validate_worktree_record(
    record: &GitWorktreeRecord,
    reservation: &ResourceReservation,
) -> Vec<ResourceProtocolFinding> {
    let mut findings = Vec::new();
    if record.schema != WORKTREE_RECORD_SCHEMA || record.schema_version != 0 {
        findings.push(worktree_finding(
            record,
            "unsupported_worktree_schema",
            "worktree record must use git.worktree_record.v0",
        ));
    }
    if record.reservation_id != reservation.reservation_id
        || record.attempt_id != reservation.attempt_id
    {
        findings.push(worktree_finding(
            record,
            "worktree_join_mismatch",
            "worktree must join the exact reservation and attempt",
        ));
    }
    let expected_resource = format!("git-worktree:{}", record.worktree_id);
    if !reservation.grants.iter().any(|grant| {
        grant.resource_id == expected_resource
            && grant.mode == ResourceMode::Exclusive
            && grant.workspace_strategy == Some(WorkspaceStrategy::IsolatedWorktree)
    }) {
        findings.push(worktree_finding(
            record,
            "worktree_not_reserved",
            "worktree requires an exclusive isolated-worktree grant for its exact identity",
        ));
    }
    for (field, value) in [
        ("worktree_id", record.worktree_id.as_str()),
        ("path_identity", record.path_identity.as_str()),
        ("branch", record.branch.as_str()),
    ] {
        if value.is_empty() {
            findings.push(worktree_finding(
                record,
                "empty_worktree_field",
                format!("{field} must not be empty"),
            ));
        }
    }
    if !is_git_sha(&record.base_commit_sha) {
        findings.push(worktree_finding(
            record,
            "invalid_base_commit_sha",
            "base_commit_sha must be an explicit 40-character lowercase git SHA",
        ));
    }
    if record
        .resulting_commit_sha
        .as_deref()
        .is_some_and(|sha| !is_git_sha(sha))
    {
        findings.push(worktree_finding(
            record,
            "invalid_resulting_commit_sha",
            "resulting_commit_sha must be null or a 40-character lowercase git SHA",
        ));
    }
    if record.working_tree_clean && !record.unexpected_write_paths.is_empty() {
        findings.push(worktree_finding(
            record,
            "clean_worktree_with_unexpected_writes",
            "a clean worktree cannot report unexpected write paths",
        ));
    }
    if !record.unexpected_write_paths.is_empty() {
        findings.push(worktree_finding(
            record,
            "unexpected_worktree_writes",
            format!(
                "worktree reported unexpected writes at {:?}",
                record.unexpected_write_paths
            ),
        ));
    }
    if record.resulting_commit_sha.is_none() {
        findings.push(worktree_finding(
            record,
            "worktree_uncommitted",
            "worktree has no resulting commit SHA",
        ));
    }
    if !record.cleanup.recoverable || !record.cleanup.requires_explicit_assertion {
        findings.push(worktree_finding(
            record,
            "unsafe_cleanup_policy",
            "cleanup must be recoverable and require an explicit assertion",
        ));
    }
    findings.sort_by(finding_order);
    findings
}

fn validate_declaration_and_reservation(
    declaration: &ResourceDeclaration,
    reservation: &ResourceReservation,
) -> Vec<ResourceProtocolFinding> {
    let mut findings =
        validate_declaration_record(declaration, Some(reservation.reservation_id.clone()));
    if reservation.schema != RESOURCE_RESERVATION_SCHEMA || reservation.schema_version != 0 {
        findings.push(finding(
            "unsupported_reservation_schema",
            None,
            Some(reservation.reservation_id.clone()),
            None,
            "reservation must use resource.reservation.v0",
        ));
    }
    if declaration.declaration_id != reservation.declaration_id {
        findings.push(finding(
            "declaration_join_mismatch",
            None,
            Some(reservation.reservation_id.clone()),
            None,
            "reservation must join the exact declaration",
        ));
    }
    let expected = declaration_grants(declaration)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let granted = reservation.grants.iter().cloned().collect::<BTreeSet<_>>();
    if expected != granted
        || expected.len() != declaration.claims.len()
        || granted.len() != reservation.grants.len()
    {
        findings.push(finding(
            "grant_declaration_mismatch",
            None,
            Some(reservation.reservation_id.clone()),
            None,
            "reservation grants must exactly equal the declared resource claims",
        ));
    }
    for grant in &reservation.grants {
        if !is_canonical_id(&grant.resource_id) {
            findings.push(finding(
                "noncanonical_resource_id",
                Some(grant.resource_id.clone()),
                Some(reservation.reservation_id.clone()),
                None,
                "resource id must use a non-empty namespace and value separated by a colon",
            ));
        }
        if grant.rate_limit_group.is_some() != (grant.rate_limit_units > 0) {
            findings.push(finding(
                "invalid_rate_limit_units",
                Some(grant.resource_id.clone()),
                Some(reservation.reservation_id.clone()),
                None,
                "rate-limit units must be positive exactly when a group is named",
            ));
        }
        validate_named_scopes(&grant.network_scope, "network_scope", None, &mut findings);
        validate_named_scopes(&grant.secret_scope, "secret_scope", None, &mut findings);
    }
    findings
}

fn validate_declaration_record(
    declaration: &ResourceDeclaration,
    reservation_id: Option<String>,
) -> Vec<ResourceProtocolFinding> {
    let mut findings = Vec::new();
    if declaration.schema != RESOURCE_DECLARATION_SCHEMA || declaration.schema_version != 0 {
        findings.push(finding(
            "unsupported_declaration_schema",
            None,
            reservation_id,
            None,
            "declaration must use resource.declaration.v0",
        ));
    }
    findings
}

fn validate_named_scopes(
    scopes: &[String],
    field: &str,
    allocation: Option<&RuntimeResourceAllocation>,
    findings: &mut Vec<ResourceProtocolFinding>,
) {
    for scope in scopes {
        if !is_canonical_id(scope) || scope.contains('=') {
            findings.push(match allocation {
                Some(allocation) => allocation_finding(
                    allocation,
                    "invalid_named_scope",
                    format!("{field} must contain named scope ids, never secret values"),
                ),
                None => finding(
                    "invalid_named_scope",
                    None,
                    None,
                    None,
                    format!("{field} must contain named scope ids, never secret values"),
                ),
            });
        }
    }
}

fn allocation_as_grant(allocation: &RuntimeResourceAllocation) -> ResourceGrant {
    ResourceGrant {
        resource_id: allocation.resource_id.clone(),
        mode: allocation.mode,
        rate_limit_group: allocation.rate_limit_group.clone(),
        rate_limit_units: allocation.rate_limit_units,
        workspace_strategy: allocation.workspace_strategy,
        network_scope: allocation.network_scope.clone(),
        secret_scope: allocation.secret_scope.clone(),
    }
}

fn modes_conflict(left: ResourceMode, right: ResourceMode) -> bool {
    left != ResourceMode::Read || right != ResourceMode::Read
}

fn is_canonical_id(value: &str) -> bool {
    let Some((namespace, identity)) = value.split_once(':') else {
        return false;
    };
    !namespace.is_empty()
        && !identity.is_empty()
        && namespace
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        && identity
            .bytes()
            .all(|byte| !byte.is_ascii_whitespace() && !byte.is_ascii_control())
}

fn is_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn allocation_finding(
    allocation: &RuntimeResourceAllocation,
    code: &str,
    detail: impl Into<String>,
) -> ResourceProtocolFinding {
    finding(
        code,
        Some(allocation.resource_id.clone()),
        Some(allocation.reservation_id.clone()),
        Some(allocation.allocation_id.clone()),
        detail,
    )
}

fn worktree_finding(
    record: &GitWorktreeRecord,
    code: &str,
    detail: impl Into<String>,
) -> ResourceProtocolFinding {
    finding(
        code,
        Some(format!("git-worktree:{}", record.worktree_id)),
        Some(record.reservation_id.clone()),
        None,
        detail,
    )
}

fn finding(
    code: &str,
    resource_id: Option<String>,
    reservation_id: Option<String>,
    allocation_id: Option<String>,
    detail: impl Into<String>,
) -> ResourceProtocolFinding {
    ResourceProtocolFinding {
        code: code.to_owned(),
        resource_id,
        reservation_id,
        allocation_id,
        detail: detail.into(),
    }
}

fn finding_order(
    left: &ResourceProtocolFinding,
    right: &ResourceProtocolFinding,
) -> std::cmp::Ordering {
    (
        &left.code,
        &left.resource_id,
        &left.reservation_id,
        &left.allocation_id,
        &left.detail,
    )
        .cmp(&(
            &right.code,
            &right.resource_id,
            &right.reservation_id,
            &right.allocation_id,
            &right.detail,
        ))
}
