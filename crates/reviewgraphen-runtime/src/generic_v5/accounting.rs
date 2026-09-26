//! Bidirectional source-validated accounting closure for G1-S6.
//!
//! The design requires disjoint set equality in both
//! directions. A self-comparison or matching count must not become acceptance.

use reviewgraphen_core::source_review::ids::AccountingMismatch;
use std::collections::BTreeSet;
use std::fmt::Display;

/// A materialized proof of a declared set's disjoint partition. It preserves
/// both expected and actual sets so acceptance can inspect the preimage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactPartition<T> {
    pub declared: BTreeSet<T>,
    pub parts: Vec<BTreeSet<T>>,
}

/// Checks one raw finite partition bidirectionally.
///
/// This is a pure collection check and intentionally returns no source
/// capability. Runtime A6 owns the concrete N/D/G comparison against its
/// source-derived expected closure.
///
/// `missing` lists declared members in no part; `extra` lists members that
/// are not declared or that occur in more than one part. Equal counts never
/// substitute for set equality.
pub fn validate_disjoint_cover<T>(
    declared: BTreeSet<T>,
    parts: Vec<BTreeSet<T>>,
) -> Result<ExactPartition<T>, AccountingMismatch>
where
    T: Clone + Ord + Display,
{
    let mut covered = BTreeSet::new();
    let mut extra = BTreeSet::new();
    for member in parts.iter().flatten() {
        if !declared.contains(member) || !covered.insert(member) {
            extra.insert(member.to_string());
        }
    }
    let missing: Vec<String> = declared
        .iter()
        .filter(|member| !covered.contains(member))
        .map(ToString::to_string)
        .collect();
    if !missing.is_empty() || !extra.is_empty() {
        return Err(AccountingMismatch {
            missing,
            extra: extra.into_iter().collect(),
        });
    }
    Ok(ExactPartition { declared, parts })
}
