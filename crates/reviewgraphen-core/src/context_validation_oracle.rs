//! Deliberately simple semantic-validation oracle for context v3.
//!
//! This module depends only on accepted domain records. It must not call the
//! production context builder, discovery, adjacency, or anchor helpers.

use crate::{DomainError, Obligation, ProgramSpace, Result, StableId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub(super) struct ContextGraphOracleV3 {
    pub(super) accepted_file_ids: BTreeSet<StableId>,
    pub(super) reached_file_ids: BTreeSet<StableId>,
    pub(super) support_anchor_ids: BTreeSet<StableId>,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct WalkState {
    distance: usize,
    tokens: Vec<u8>,
    nodes: Vec<StableId>,
    incoming: u8,
    node: StableId,
    remaining: usize,
}

pub(super) fn rebuild(
    program: &ProgramSpace,
    obligation: &Obligation,
) -> Result<ContextGraphOracleV3> {
    let accepted_file_ids = program
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
        .map(|artifact| artifact.id.clone())
        .collect::<BTreeSet<_>>();

    let mut structural = BTreeSet::new();
    for seed in obligation
        .source_ids()
        .iter()
        .chain(obligation.target_refs())
        .chain(obligation.context_ids())
    {
        if let Some(relation) = program.relation(seed) {
            structural.insert(relation.id.clone());
            structural.insert(relation.source_id.clone());
            structural.extend(relation.target_ids.iter().cloned());
        } else if program.artifact(seed).is_some() {
            structural.insert(seed.clone());
        }
    }
    let seeds = structural.clone();

    // Full relation scan; no production adjacency index or cache is used.
    let mut adjacency = BTreeMap::<StableId, Vec<(u8, StableId, usize)>>::new();
    for relation in program.relations() {
        if !relation.directed {
            continue;
        }
        let Some((forward, reverse, forward_depth, reverse_depth)) = (match relation.kind.as_str() {
            "calls" => Some((0, 1, 3, 2)),
            "contains" => Some((2, 3, usize::MAX, usize::MAX)),
            "covers" => Some((4, 5, 1, 1)),
            _ => None,
        }) else {
            continue;
        };
        for target in &relation.target_ids {
            adjacency
                .entry(relation.source_id.clone())
                .or_default()
                .push((forward, target.clone(), forward_depth));
            adjacency.entry(target.clone()).or_default().push((
                reverse,
                relation.source_id.clone(),
                reverse_depth,
            ));
        }
    }
    for edges in adjacency.values_mut() {
        edges.sort();
        edges.dedup();
    }

    let mut queue = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for seed in &seeds {
        for (token, next, maximum) in adjacency.get(seed).into_iter().flatten() {
            if seeds.contains(next) {
                continue;
            }
            let remaining = initial_remaining(*token, *maximum);
            if visited.insert((*token, next.clone(), remaining)) {
                structural.insert(next.clone());
                queue.insert(WalkState {
                    distance: 1,
                    tokens: vec![*token],
                    nodes: vec![seed.clone(), next.clone()],
                    incoming: *token,
                    node: next.clone(),
                    remaining,
                });
            }
        }
    }
    while let Some(state) = queue.pop_first() {
        for (token, next, maximum) in adjacency.get(&state.node).into_iter().flatten() {
            if seeds.contains(next) || state.nodes.contains(next) {
                continue;
            }
            let next_remaining = if *token == state.incoming {
                if matches!(*token, 2 | 3) {
                    state.remaining
                } else if state.remaining == 0 {
                    continue;
                } else {
                    state.remaining - 1
                }
            } else {
                initial_remaining(*token, *maximum)
            };
            if !visited.insert((*token, next.clone(), next_remaining)) {
                continue;
            }
            structural.insert(next.clone());
            let mut tokens = state.tokens.clone();
            tokens.push(*token);
            let mut nodes = state.nodes.clone();
            nodes.push(next.clone());
            queue.insert(WalkState {
                distance: state.distance + 1,
                tokens,
                nodes,
                incoming: *token,
                node: next.clone(),
                remaining: next_remaining,
            });
        }
    }

    let mut files_for = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    for id in &structural {
        if accepted_file_ids.contains(id) {
            files_for.entry(id.clone()).or_default().insert(id.clone());
        }
        let mut todo = vec![id.clone()];
        let mut seen = BTreeSet::new();
        while let Some(child) = todo.pop() {
            // Deliberate exhaustive containment scan for every frontier item.
            for relation in program.relations() {
                if !relation.directed
                    || relation.kind != "contains"
                    || !relation.target_ids.contains(&child)
                {
                    continue;
                }
                let parent = &relation.source_id;
                if accepted_file_ids.contains(parent) {
                    files_for
                        .entry(id.clone())
                        .or_default()
                        .insert(parent.clone());
                } else if seen.insert(parent.clone()) {
                    todo.push(parent.clone());
                }
            }
        }
    }
    let reached_file_ids = structural
        .iter()
        .flat_map(|id| files_for.get(id).into_iter().flatten().cloned())
        .collect::<BTreeSet<_>>();

    let mut support_anchor_ids = BTreeSet::new();
    for artifact in program
        .artifacts()
        .iter()
        .filter(|artifact| structural.contains(&artifact.id))
    {
        let Some(location) = artifact.location.as_ref() else {
            continue;
        };
        let (Some(start), Some(end)) = (location.start_line, location.end_line) else {
            continue;
        };
        for file_id in files_for.get(&artifact.id).into_iter().flatten() {
            if !reached_file_ids.contains(file_id) {
                continue;
            }
            let file = program.artifact(file_id).ok_or_else(|| {
                DomainError::Validation("oracle containment file disappeared".to_owned())
            })?;
            if file.location.as_ref().map(|value| value.path.as_str())
                == Some(location.path.as_str())
            {
                support_anchor_ids.insert(anchor_id(
                    program.snapshot_id(),
                    file_id,
                    u32::try_from(start).map_err(|_| {
                        DomainError::Validation("oracle anchor start overflow".to_owned())
                    })?,
                    u32::try_from(end).map_err(|_| {
                        DomainError::Validation("oracle anchor end overflow".to_owned())
                    })?,
                    &artifact.id,
                )?);
            }
        }
    }

    Ok(ContextGraphOracleV3 {
        accepted_file_ids,
        reached_file_ids,
        support_anchor_ids,
    })
}

const fn initial_remaining(token: u8, maximum: usize) -> usize {
    if matches!(token, 2 | 3) {
        usize::MAX
    } else {
        maximum.saturating_sub(1)
    }
}

fn anchor_id(
    snapshot_id: &StableId,
    source_artifact_id: &StableId,
    start_line: u32,
    end_line: u32,
    owner_artifact_id: &StableId,
) -> Result<StableId> {
    StableId::derived(
        "context-support-anchor",
        &BTreeMap::from([
            (
                "anchor_contract".to_owned(),
                serde_json::json!("context.support_anchor@1"),
            ),
            ("end_line".to_owned(), serde_json::json!(end_line)),
            (
                "owner_artifact_id".to_owned(),
                serde_json::json!(owner_artifact_id),
            ),
            ("snapshot_id".to_owned(), serde_json::json!(snapshot_id)),
            (
                "source_artifact_id".to_owned(),
                serde_json::json!(source_artifact_id),
            ),
            ("start_line".to_owned(), serde_json::json!(start_line)),
        ]),
    )
}
