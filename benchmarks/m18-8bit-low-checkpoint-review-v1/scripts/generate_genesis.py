#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = ROOT / "casegraphen"
CS = "case-space:m18-8bit-low-checkpoint-review-v1"
SPACE = "space:m18-8bit-low-checkpoint-review-v1"
BOUNDARY = "source-boundary:m18-8bit-low-checkpoint-review-v1"
SOURCE_ID = "source:m18-approved-intent"
REVISION = "revision:m18-genesis"
TOPOLOGY_HASH = "99a8a26c4ff655f91e04117d0fa3e52b569297ea257db02b2f25da429a820c9e"


def provenance(status: str, title: str) -> dict:
    return {"source": {"kind": "human", "title": title}, "confidence": 1.0, "review_status": status}


def cell(cell_id: str, cell_type: str, title: str, lifecycle: str, status: str, metadata=None, summary=None) -> dict:
    value = {
        "id": cell_id, "cell_type": cell_type, "space_id": SPACE, "title": title,
        "lifecycle": lifecycle, "source_ids": [SOURCE_ID], "structure_ids": [],
        "provenance": provenance(status, "User-approved m18 experiment intent"), "metadata": metadata or {},
    }
    if summary:
        value["summary"] = summary
    return value


def capability(cell_id: str, actor: str, operations: list[str], title: str) -> dict:
    return cell(cell_id, "custom:capability", title, "accepted", "accepted", {"actor_ids": [actor], "operations": operations})


def main() -> None:
    topology_path = ROOT / "design/execution.topology.json"
    manifest_path = ROOT / "design/deployment-policy-manifest.json"
    artifact_id = f"artifact:sha256-{hashlib.sha256(topology_path.read_bytes()).hexdigest()}"
    manifest = json.loads(manifest_path.read_text())
    for key in ("verification_policies", "budget_policies", "expansion_policies"):
        manifest[key].sort(key=lambda row: (row["policy_id"], row["content_hash"]))
    manifest_hash = hashlib.sha256(json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    boundary = {
        "id": BOUNDARY,
        "included_sources": [
            "benchmarks/m18-8bit-low-checkpoint-review-v1/preregistration.json",
            "benchmarks/m18-8bit-low-checkpoint-review-v1/design/execution.topology.json",
            "benchmarks/m18-8bit-low-checkpoint-review-v1/design/deployment-policy-manifest.json",
            "user approval of topology hash 99a8a26c on 2026-08-22",
        ],
        "adapters": ["native_genesis.v1", "external_runtime_evidence.v1"],
        "accepted_fact_policy": "Only user-approved intent, exact topology review, and later explicit review morphisms are accepted facts.",
        "inference_policy": "All Qwen, ReviewGraphen runtime, validator, judge, latency, and model-identity reports enter as unreviewed inferred evidence.",
        "information_loss": [
            "CaseGraphen cells represent one arm and one comparison; model calls and token streams remain runtime artifacts.",
            "A resolved work cell does not verify or human-accept any ReviewGraphen finding.",
        ],
    }
    cells = [
        cell("goal:m18-governed-followup", "goal", "Evaluate the 8bit/low ReviewGraphen checkpoint configuration", "active", "accepted", summary="Separate effort and quantization contrasts observationally without causal promotion."),
        cell("work:8bit-low-r1", "work", "Run and evaluate the 8bit/low external-runtime arm", "active", "unreviewed"),
        cell("work:compare", "work", "Compare m18 8bit/low with m17 observations", "active", "unreviewed"),
        capability("capability:m18-human-review", "actor:human-user", ["review"], "Review topology and experiment evidence"),
        capability("capability:m18-runtime-evidence", "actor:m18-runtime-adapter", ["evidence-attach"], "Attach unreviewed runtime evidence"),
        capability("capability:m18-transition", "actor:m18-case-operator", ["cell-transition"], "Transition governed work after review"),
    ]
    relation = {
        "id": "relation:compare-after-8bit-low", "relation_type": "depends_on", "relation_strength": "hard",
        "from_id": "work:compare", "to_id": "work:8bit-low-r1", "evidence_ids": [], "source_ids": [SOURCE_ID],
        "provenance": provenance("accepted", "User-approved m18 execution order"), "metadata": {},
    }
    genesis = {
        "schema": "highergraphen.case.space.v1", "schema_version": 1,
        "case_space_id": CS, "space_id": SPACE, "case_cells": cells, "case_relations": [relation],
        "morphism_log": [{
            "schema": "highergraphen.case.morphism_log_entry.v1", "schema_version": 1,
            "case_space_id": CS, "sequence": 1, "entry_id": "morphism-log-entry:m18-genesis",
            "morphism_id": "morphism:m18-genesis", "target_revision_id": REVISION,
            "morphism": {
                "morphism_id": "morphism:m18-genesis", "morphism_type": "create", "target_revision_id": REVISION,
                "added_ids": [], "updated_ids": [], "retired_ids": [], "preserved_ids": [],
                "violated_invariant_ids": [], "review_status": "accepted", "evidence_ids": [], "source_ids": [SOURCE_ID],
                "metadata": {"lift_semantics": "native_genesis", "source_boundary": boundary, "source_boundary_id": BOUNDARY},
            },
            "actor_id": "actor:human-user", "recorded_at": "2026-08-22T00:00:00+09:00",
            "provenance": provenance("accepted", "User-approved m18 governed experiment"),
            "source_ids": [SOURCE_ID], "replay_checksum": "",
        }],
        "projections": [],
        "revision": {
            "revision_id": REVISION, "case_space_id": CS,
            "applied_entry_ids": ["morphism-log-entry:m18-genesis"], "applied_morphism_ids": ["morphism:m18-genesis"],
            "checksum": "", "created_at": "2026-08-22T00:00:00+09:00", "source_ids": [SOURCE_ID], "metadata": {},
        },
        "metadata": {"source_boundary": boundary, "approved_topology_content_hash": TOPOLOGY_HASH},
    }
    topology_claim = cell(
        "evidence:execution-topology", "evidence", "Exact m18 execution topology and policy manifest approved for execution",
        "proposed", "unreviewed",
        {
            "topology_id": "topology:m18-8bit-low-checkpoint-review-v1",
            "execution_topology_content_hash": TOPOLOGY_HASH,
            "artifact_id": artifact_id,
            "policy_manifest_content_hash": manifest_hash,
            "case_space_id": CS,
        },
        "The exact topology bytes remain unreviewed until topology-review records the user's decision.",
    )
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    (OUT_DIR / "genesis.case.space.json").write_text(json.dumps(genesis, indent=2, sort_keys=True) + "\n")
    (OUT_DIR / "topology.claim.json").write_text(json.dumps(topology_claim, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
