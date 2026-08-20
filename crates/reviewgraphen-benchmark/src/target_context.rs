//! Deterministic, non-authority target context for implementation research.
//!
//! This is deliberately a benchmark artifact rather than a
//! `ReviewContextEnvelope`: no review obligation, claim, evidence, or accepted
//! state is synthesized.  It projects accepted ProgramSpace facts around one
//! caller-selected symbol and declares the source omitted by the projection.

use reviewgraphen_core::{Artifact, ContentHash, StableId, canonical_json};
use reviewgraphen_ingest::{
    CapabilityState, IngestRequest, IngestionObstruction, ingest_with_sources,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const MAX_SOURCE_BYTES: u64 = 512 * 1024 * 1024;
const WINDOW_PADDING_LINES: u64 = 48;
const MAX_WINDOWS: usize = 8;
const MAX_WINDOW_LINES: u64 = 160;
const AUTHORITY: &str =
    "research_projection_only_not_claim_evidence_verification_or_accepted_state";

#[derive(Debug, Error)]
pub enum TargetContextError {
    #[error("target context ingestion failed: {0}")]
    Ingest(#[from] reviewgraphen_ingest::IngestError),
    #[error("target selector matched no accepted symbol")]
    NoMatch,
    #[error("target selector matched more than one accepted symbol")]
    Ambiguous,
    #[error("target context source bytes are not UTF-8")]
    NonUtf8,
    #[error("target context source entry is missing")]
    MissingSource,
    #[error("target context canonicalization failed: {0}")]
    Canonical(#[from] reviewgraphen_core::DomainError),
}

pub type Result<T> = std::result::Result<T, TargetContextError>;

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetContextProjection {
    schema: &'static str,
    projection_id: StableId,
    projection_hash: ContentHash,
    snapshot_id: StableId,
    selector: String,
    target: ProjectedArtifact,
    accepted_neighborhood: Vec<ProjectedArtifact>,
    accepted_relations: Vec<ProjectedRelation>,
    sources: Vec<ProjectedSource>,
    extraction: ProjectedExtraction,
    information_loss: Vec<String>,
    authority: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectionBody<'a> {
    schema: &'static str,
    snapshot_id: &'a StableId,
    selector: &'a str,
    target: &'a ProjectedArtifact,
    accepted_neighborhood: &'a [ProjectedArtifact],
    accepted_relations: &'a [ProjectedRelation],
    sources: &'a [ProjectedSource],
    extraction: &'a ProjectedExtraction,
    information_loss: &'a [String],
    authority: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectedArtifact {
    id: StableId,
    kind: String,
    label: String,
    location: Option<reviewgraphen_core::Location>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectedRelation {
    id: StableId,
    kind: String,
    source_id: StableId,
    target_ids: BTreeSet<StableId>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectedSource {
    file_artifact_id: StableId,
    path: String,
    content_hash: ContentHash,
    windows: Vec<SourceWindow>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectedExtraction {
    snapshot_id: StableId,
    target_revision: String,
    adapter_set_hash: ContentHash,
    capabilities: BTreeMap<String, CapabilityState>,
    relevant_obstructions: Vec<IngestionObstruction>,
    omitted_obstruction_count: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceWindow {
    start_line: u64,
    end_line: u64,
    anchor_source_ids: BTreeSet<StableId>,
    text: String,
}

#[derive(Clone, Debug)]
struct WindowSeed {
    start_line: u64,
    end_line: u64,
    source_id: StableId,
}

pub fn project_target(request: &IngestRequest, selector: &str) -> Result<TargetContextProjection> {
    let ingested = ingest_with_sources(request, MAX_SOURCE_BYTES)?;
    let program = &ingested.program_space;
    let mut matches = program.artifacts().iter().filter(|artifact| {
        matches!(
            artifact.kind.as_str(),
            "function" | "method" | "type" | "module"
        ) && (artifact.label == selector
            || artifact
                .label
                .strip_suffix(selector)
                .is_some_and(|prefix| prefix.ends_with("::") || prefix.is_empty())
            || artifact.label.split("::").any(|part| part == selector))
    });
    let target = matches.next().ok_or(TargetContextError::NoMatch)?;
    if matches.next().is_some() {
        return Err(TargetContextError::Ambiguous);
    }

    let mut neighborhood_ids = BTreeSet::from([target.id.clone()]);
    let accepted_relations = program
        .relations()
        .iter()
        .filter(|relation| {
            relation.source_id == target.id || relation.target_ids.contains(&target.id)
        })
        .map(|relation| {
            neighborhood_ids.insert(relation.source_id.clone());
            neighborhood_ids.extend(relation.target_ids.iter().cloned());
            ProjectedRelation {
                id: relation.id.clone(),
                kind: relation.kind.clone(),
                source_id: relation.source_id.clone(),
                target_ids: relation.target_ids.clone(),
            }
        })
        .collect::<Vec<_>>();

    let by_id = program
        .artifacts()
        .iter()
        .map(|artifact| (artifact.id.clone(), artifact))
        .collect::<BTreeMap<_, _>>();
    let accepted_neighborhood = neighborhood_ids
        .iter()
        .filter_map(|id| by_id.get(id).copied())
        .map(projected_artifact)
        .collect::<Vec<_>>();
    let relevant_obstructions = ingested
        .extraction_report
        .obstructions
        .iter()
        .filter(|obstruction| !obstruction.source_ids.is_disjoint(&neighborhood_ids))
        .cloned()
        .collect::<Vec<_>>();
    let omitted_obstruction_count = ingested
        .extraction_report
        .obstructions
        .len()
        .saturating_sub(relevant_obstructions.len());
    let extraction = ProjectedExtraction {
        snapshot_id: ingested.extraction_report.snapshot_id.clone(),
        target_revision: ingested.extraction_report.target_revision.clone(),
        adapter_set_hash: ingested.extraction_report.adapter_set_hash.clone(),
        capabilities: ingested.extraction_report.capabilities.clone(),
        relevant_obstructions,
        omitted_obstruction_count,
    };

    let mut seeds_by_path = BTreeMap::<String, Vec<WindowSeed>>::new();
    for artifact in neighborhood_ids
        .iter()
        .filter_map(|id| by_id.get(id).copied())
    {
        let Some(location) = &artifact.location else {
            continue;
        };
        let (Some(start_line), Some(end_line)) = (location.start_line, location.end_line) else {
            continue;
        };
        seeds_by_path
            .entry(location.path.clone())
            .or_default()
            .push(WindowSeed {
                start_line,
                end_line,
                source_id: artifact.id.clone(),
            });
    }

    let file_artifacts = program
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
        .filter_map(|artifact| {
            artifact
                .location
                .as_ref()
                .map(|location| (location.path.clone(), artifact))
        })
        .collect::<BTreeMap<_, _>>();
    let source_entries = ingested
        .source_bundle
        .entries()
        .iter()
        .map(|entry| (entry.path(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut sources = Vec::new();
    let mut omitted_window_count = 0_usize;
    for (path, seeds) in seeds_by_path {
        let entry = source_entries
            .get(path.as_str())
            .ok_or(TargetContextError::MissingSource)?;
        let file = file_artifacts
            .get(&path)
            .copied()
            .ok_or(TargetContextError::MissingSource)?;
        let text = std::str::from_utf8(entry.bytes()).map_err(|_| TargetContextError::NonUtf8)?;
        let lines = text.split_inclusive('\n').collect::<Vec<_>>();
        let (windows, omitted) = build_windows(&lines, seeds);
        omitted_window_count = omitted_window_count.saturating_add(omitted);
        sources.push(ProjectedSource {
            file_artifact_id: file.id.clone(),
            path,
            content_hash: entry.content_hash().clone(),
            windows,
        });
    }

    let mut information_loss = vec![
        "projection includes only the selected accepted symbol and its one-hop accepted ProgramSpace relations"
            .to_owned(),
        "unresolved calls, macro expansion, type resolution, dependency API surfaces, and hidden tests are not reconstructed"
            .to_owned(),
        format!(
            "source is limited to at most {MAX_WINDOWS} windows of {MAX_WINDOW_LINES} lines with {WINDOW_PADDING_LINES}-line target padding"
        ),
    ];
    if omitted_window_count > 0 {
        information_loss.push(format!(
            "{omitted_window_count} lower-priority source windows were omitted by the window cap"
        ));
    }
    information_loss.push(format!(
        "{omitted_obstruction_count} extraction obstructions outside the selected one-hop source-ID neighborhood were omitted"
    ));

    let target = projected_artifact(target);
    let body_bytes = canonical_json(&ProjectionBody {
        schema: "reviewgraphen.benchmark.target_context.v1",
        snapshot_id: program.snapshot_id(),
        selector,
        target: &target,
        accepted_neighborhood: &accepted_neighborhood,
        accepted_relations: &accepted_relations,
        sources: &sources,
        extraction: &extraction,
        information_loss: &information_loss,
        authority: AUTHORITY,
    })?;
    let projection_hash = ContentHash::sha256(&body_bytes);
    let projection_id = StableId::derived(
        "benchmark-target-context",
        &BTreeMap::from([
            (
                "projection_hash".to_owned(),
                serde_json::Value::String(projection_hash.to_string()),
            ),
            (
                "snapshot".to_owned(),
                serde_json::Value::String(program.snapshot_id().to_string()),
            ),
        ]),
    )?;
    Ok(TargetContextProjection {
        schema: "reviewgraphen.benchmark.target_context.v1",
        projection_id,
        projection_hash,
        snapshot_id: program.snapshot_id().clone(),
        selector: selector.to_owned(),
        target,
        accepted_neighborhood,
        accepted_relations,
        sources,
        extraction,
        information_loss,
        authority: AUTHORITY,
    })
}

fn projected_artifact(artifact: &Artifact) -> ProjectedArtifact {
    ProjectedArtifact {
        id: artifact.id.clone(),
        kind: artifact.kind.clone(),
        label: artifact.label.clone(),
        location: artifact.location.clone(),
    }
}

fn build_windows(lines: &[&str], mut seeds: Vec<WindowSeed>) -> (Vec<SourceWindow>, usize) {
    seeds.sort_by(|left, right| {
        left.start_line
            .cmp(&right.start_line)
            .then_with(|| left.end_line.cmp(&right.end_line))
            .then_with(|| left.source_id.cmp(&right.source_id))
    });
    let line_count = u64::try_from(lines.len()).unwrap_or(u64::MAX);
    let mut windows = Vec::<(u64, u64, BTreeSet<StableId>)>::new();
    for seed in seeds {
        let start = seed.start_line.saturating_sub(WINDOW_PADDING_LINES).max(1);
        let desired_end = seed.end_line.saturating_add(WINDOW_PADDING_LINES);
        let end = desired_end
            .min(start.saturating_add(MAX_WINDOW_LINES - 1))
            .min(line_count);
        if let Some((_, previous_end, anchors)) = windows.last_mut()
            && start <= previous_end.saturating_add(1)
        {
            *previous_end = (*previous_end).max(end);
            anchors.insert(seed.source_id);
        } else {
            windows.push((start, end, BTreeSet::from([seed.source_id])));
        }
    }
    let omitted = windows.len().saturating_sub(MAX_WINDOWS);
    windows.truncate(MAX_WINDOWS);
    let projected = windows
        .into_iter()
        .map(|(start_line, end_line, anchor_source_ids)| {
            let start = usize::try_from(start_line.saturating_sub(1)).unwrap_or(usize::MAX);
            let end = usize::try_from(end_line)
                .unwrap_or(usize::MAX)
                .min(lines.len());
            SourceWindow {
                start_line,
                end_line,
                anchor_source_ids,
                text: lines.get(start..end).unwrap_or_default().concat(),
            }
        })
        .collect();
    (projected, omitted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> StableId {
        StableId::parse(format!("artifact:sha256:{value:0>64}")).expect("test ID is valid")
    }

    #[test]
    fn distant_anchors_become_distinct_bounded_windows() {
        let owned = (1..=600)
            .map(|line| format!("line {line}\n"))
            .collect::<Vec<_>>();
        let lines = owned.iter().map(String::as_str).collect::<Vec<_>>();
        let (windows, omitted) = build_windows(
            &lines,
            vec![
                WindowSeed {
                    start_line: 40,
                    end_line: 45,
                    source_id: id("1"),
                },
                WindowSeed {
                    start_line: 500,
                    end_line: 510,
                    source_id: id("2"),
                },
            ],
        );
        assert_eq!(omitted, 0);
        assert_eq!(windows.len(), 2);
        assert!(windows[0].text.contains("line 40"));
        assert!(windows[1].text.contains("line 500"));
        assert!(
            windows
                .iter()
                .all(|window| window.end_line - window.start_line < MAX_WINDOW_LINES)
        );
    }

    #[test]
    fn overlapping_anchors_merge_without_losing_source_ids() {
        let owned = (1..=200)
            .map(|line| format!("line {line}\n"))
            .collect::<Vec<_>>();
        let lines = owned.iter().map(String::as_str).collect::<Vec<_>>();
        let first = id("1");
        let second = id("2");
        let (windows, omitted) = build_windows(
            &lines,
            vec![
                WindowSeed {
                    start_line: 80,
                    end_line: 90,
                    source_id: first.clone(),
                },
                WindowSeed {
                    start_line: 100,
                    end_line: 110,
                    source_id: second.clone(),
                },
            ],
        );
        assert_eq!(omitted, 0);
        assert_eq!(windows.len(), 1);
        assert_eq!(
            windows[0].anchor_source_ids,
            BTreeSet::from([first, second])
        );
    }
}
