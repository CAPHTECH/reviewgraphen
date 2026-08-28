#!/usr/bin/env python3
"""Generate the proposal-only m18 topology from the reviewed m17 contract."""

from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKSPACE = ROOT.parents[1]
M17 = WORKSPACE / "benchmarks/m17-casegraphen-controlled-review-v1"
DESIGN = ROOT / "design"


def replace(value):
    if isinstance(value, str):
        return value.replace("4bit-low-r1", "8bit-low-r1").replace("m17", "m18")
    if isinstance(value, list):
        return [replace(item) for item in value]
    if isinstance(value, dict):
        return {key: replace(item) for key, item in value.items()}
    return value


def write(path: Path, value) -> str:
    payload = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
    path.write_bytes(payload)
    return hashlib.sha256(payload).hexdigest()


def main() -> None:
    DESIGN.mkdir(parents=True, exist_ok=True)
    source = json.loads((M17 / "design/execution.topology.json").read_text())
    selected_ids = {
        node["node_id"] for node in source["nodes"]
        if node["node_id"].startswith("node:4bit-low-r1:")
    }
    nodes = [replace(copy.deepcopy(node)) for node in source["nodes"] if node["node_id"] in selected_ids]
    edges = [
        replace(copy.deepcopy(edge)) for edge in source["edges"]
        if edge["from"] in selected_ids and edge["to"] in selected_ids
    ]
    aggregate_template = next(node for node in source["nodes"] if node["node_id"] == "node:aggregate")
    compare = replace(copy.deepcopy(aggregate_template))
    compare.update({
        "node_id": "node:compare",
        "work_cell_id": "work:compare",
        "purpose": "Compare the m18 8bit/low observation with the frozen m17 8bit/high and 4bit/low observations without causal promotion",
        "idempotency_key": "node:compare:<bound-input-hash>",
        "inputs": [
            {"name": "8bit_low", "schema_id": "schema:m18-arm-result", "artifact_selector": "node:8bit-low-r1:outcome#arm_result"},
            {"name": "m17_observations", "schema_id": "schema:m17-experiment-result", "artifact_selector": "benchmark:m17#aggregate"}
        ],
        "outputs": [{"name": "experiment_result", "schema_id": "schema:m18-experiment-result"}],
    })
    nodes.append(compare)
    edge_template = next(edge for edge in source["edges"] if edge["edge_id"] == "edge:4bit-low-r1:outcome-aggregate")
    compare_edge = replace(copy.deepcopy(edge_template))
    compare_edge.update({
        "edge_id": "edge:8bit-low-r1:outcome-compare",
        "to": "node:compare",
        "input": "8bit_low",
        "schema_id": "schema:m18-arm-result",
        "blocking_predicate": "the m18 8bit/low arm result is absent",
        "dependency_witness": "the comparison requires the validated m18 arm result",
        "removal_counterexample": "without this edge the comparison could omit the new intervention result",
    })
    edges.append(compare_edge)
    topology = replace(copy.deepcopy(source))
    topology.update({
        "topology_id": "topology:m18-8bit-low-checkpoint-review-v1",
        "case_space_id": "case-space:m18-8bit-low-checkpoint-review-v1",
        "nodes": nodes,
        "edges": edges,
        "provenance": {"created_by": "actor:codex-experiment-designer", "source": "benchmark:m18-preregistration"},
    })
    write(DESIGN / "execution.topology.json", topology)

    for path in sorted((M17 / "design").glob("budget-m17-*.json")):
        value = replace(json.loads(path.read_text()))
        write(DESIGN / path.name.replace("m17", "m18"), value)
    verification = replace(json.loads((M17 / "design/verification.policy.json").read_text()))
    write(DESIGN / "verification.policy.json", verification)
    runtime = replace(json.loads((M17 / "design/runtime.deployment.json").read_text()))
    runtime["external_runtime"]["max_parallel"] = 1
    runtime["casegraphen"]["governed_work_cells"] = ["work:8bit-low-r1", "work:compare"]
    write(DESIGN / "runtime.deployment.json", runtime)


if __name__ == "__main__":
    main()
