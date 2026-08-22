#!/usr/bin/env python3
"""Bind the linted topology/policies and emit the proposal-phase handoff."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DESIGN = ROOT / "design"


def load(path: Path) -> object:
    return json.loads(path.read_text())


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def artifact(path: Path, role: str = "proposal") -> dict[str, str]:
    return {
        "path": str(path.relative_to(ROOT)),
        "content_hash": f"sha256:{digest(path)}",
        "role": role,
    }


def main() -> None:
    topology_path = DESIGN / "execution.topology.json"
    lint_path = DESIGN / "graph.analysis.report.json"
    verification_path = DESIGN / "verification.policy.json"
    topology = load(topology_path)
    lint = load(lint_path)
    if lint["topology_id"] != topology["topology_id"]:
        raise SystemExit("lint report is for a different topology")
    if any(item["severity"] == "error" for item in lint["findings"]):
        raise SystemExit("topology has lint errors")

    budget_bindings = []
    for policy_id in topology["budget_policy_ids"]:
        path = DESIGN / f"{policy_id.replace(':', '-')}.json"
        budget_bindings.append({"policy_id": policy_id, "content_hash": digest(path)})

    manifest_path = DESIGN / "deployment-policy-manifest.json"
    write(manifest_path, {
        "schema": "casegraphen.experimental.deployment_policy_manifest.v0",
        "schema_version": 0,
        "topology_id": topology["topology_id"],
        "topology_content_hash": lint["topology_content_hash"],
        "verification_policies": [{
            "policy_id": "verification:m17-independent-codex-judge",
            "content_hash": digest(verification_path),
        }],
        "budget_policies": budget_bindings,
        "expansion_policies": [],
    })

    proposal_paths = [
        topology_path,
        lint_path,
        manifest_path,
        verification_path,
        DESIGN / "runtime.deployment.json",
        DESIGN / "genesis.mapping.proposal.md",
        DESIGN / "execution-plan.mapping.proposal.md",
        DESIGN / "TOPOLOGY_REVIEW.md",
        ROOT / "preregistration.json",
    ]
    proposal_paths.extend(
        DESIGN / f"{policy_id.replace(':', '-')}.json"
        for policy_id in topology["budget_policy_ids"]
    )
    handoff = {
        "schema": "casegraphen.experimental.skill.orchestration_handoff.v0",
        "handoff_id": "handoff:m17:topology-review",
        "phase": "topology_proposed",
        "selected_task_skill": "casegraphen-design",
        "selection_reason": "The completed phase produced an unreviewed CaseGraphen topology for the m17 ReviewGraphen/Qwen experiment.",
        "observed": {
            "case_space_id": topology["case_space_id"],
            "revision_id": None,
            "topology_content_hash": f"sha256:{lint['topology_content_hash']}",
            "report_ids": [f"graph-lint:{lint['topology_content_hash'][:16]}"],
        },
        "artifacts": [artifact(path) for path in proposal_paths],
        "unresolved_evidence": [],
        "seams": [{
            "kind": "topology_review",
            "status": "open",
            "required_actor_or_policy": "independent reviewer of the exact topology and deployment policy manifest",
        }],
        "return_required": True,
        "next_action": {"kind": "return_for_review", "task_skill": None},
        "mutation_performed": False,
        "accepted_state_changed": False,
    }
    write(DESIGN / "orchestration.handoff.json", handoff)


if __name__ == "__main__":
    main()
