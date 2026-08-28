#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DESIGN = ROOT / "design"


def load(path: Path):
    return json.loads(path.read_text())


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path: Path, value) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def artifact(path: Path) -> dict[str, str]:
    return {"path": str(path.relative_to(ROOT)), "content_hash": f"sha256:{digest(path)}", "role": "proposal"}


def main() -> None:
    topology = load(DESIGN / "execution.topology.json")
    lint = load(DESIGN / "graph.analysis.report.json")
    if lint["topology_id"] != topology["topology_id"] or any(row["severity"] == "error" for row in lint["findings"]):
        raise SystemExit("topology/lint mismatch or lint error")
    budgets = []
    for policy_id in topology["budget_policy_ids"]:
        path = DESIGN / f"{policy_id.replace(':', '-')}.json"
        budgets.append({"policy_id": policy_id, "content_hash": digest(path)})
    manifest = {
        "schema": "casegraphen.experimental.deployment_policy_manifest.v0",
        "schema_version": 0,
        "topology_id": topology["topology_id"],
        "topology_content_hash": lint["topology_content_hash"],
        "verification_policies": [{
            "policy_id": "verification:m18-independent-codex-judge",
            "content_hash": digest(DESIGN / "verification.policy.json"),
        }],
        "budget_policies": budgets,
        "expansion_policies": [],
    }
    write(DESIGN / "deployment-policy-manifest.json", manifest)
    paths = [
        DESIGN / "execution.topology.json", DESIGN / "graph.analysis.report.json",
        DESIGN / "deployment-policy-manifest.json", DESIGN / "verification.policy.json",
        DESIGN / "runtime.deployment.json", DESIGN / "genesis.mapping.proposal.md",
        DESIGN / "execution-plan.mapping.proposal.md", DESIGN / "TOPOLOGY_REVIEW.md",
        ROOT / "preregistration.json",
    ] + [DESIGN / f"{policy_id.replace(':', '-')}.json" for policy_id in topology["budget_policy_ids"]]
    handoff = {
        "schema": "casegraphen.experimental.skill.orchestration_handoff.v0",
        "handoff_id": "handoff:m18:topology-review",
        "phase": "topology_proposed",
        "selected_task_skill": "casegraphen-design",
        "selection_reason": "The phase produced an unreviewed single-arm 8bit/low follow-up topology.",
        "observed": {
            "case_space_id": topology["case_space_id"], "revision_id": None,
            "topology_content_hash": f"sha256:{lint['topology_content_hash']}",
            "report_ids": [f"graph-lint:{lint['topology_content_hash'][:16]}"],
        },
        "artifacts": [artifact(path) for path in paths],
        "unresolved_evidence": [],
        "seams": [{"kind": "topology_review", "status": "open", "required_actor_or_policy": "independent reviewer of the exact m18 topology and deployment policy manifest"}],
        "return_required": True,
        "next_action": {"kind": "return_for_review", "task_skill": None},
        "mutation_performed": False,
        "accepted_state_changed": False,
    }
    write(DESIGN / "orchestration.handoff.json", handoff)


if __name__ == "__main__":
    main()
