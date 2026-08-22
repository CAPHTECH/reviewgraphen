#!/usr/bin/env python3
"""Generate the proposal-only CaseGraphen topology and bound policies for m17."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DESIGN = ROOT / "design"
CREATOR = "actor:codex-experiment-designer"
SOURCE = "benchmark:m17-preregistration"


def write_json(path: Path, value: object) -> str:
    payload = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
    path.write_bytes(payload)
    return hashlib.sha256(payload).hexdigest()


def provenance() -> dict[str, str]:
    return {"created_by": CREATOR, "source": SOURCE}


def io(name: str, schema_id: str, selector: str | None = None) -> dict[str, str]:
    result = {"name": name, "schema_id": schema_id}
    if selector is not None:
        result["artifact_selector"] = selector
    return result


def resource(
    name: str,
    mode: str,
    *,
    workspace: str | None = "ephemeral",
    network: list[str] | None = None,
    rate_limit: str | None = None,
) -> dict[str, object]:
    return {
        "resource": name,
        "mode": mode,
        "rate_limit_group": rate_limit,
        "workspace_strategy": workspace,
        "network_scope": network or [],
        "secret_scope": [],
    }


def node(
    node_id: str,
    purpose: str,
    inputs: list[dict[str, str]],
    outputs: list[dict[str, str]],
    executor: str,
    budget: str,
    duration_ms: int,
    resources: list[dict[str, object]],
    *,
    verification: str | None = None,
    delivery: str = "barrier",
    side_effects: str = "none",
) -> dict[str, object]:
    if node_id.startswith("node:8bit-high-r1:"):
        work_cell_id = "work:8bit-high-r1"
    elif node_id.startswith("node:4bit-low-r1:"):
        work_cell_id = "work:4bit-low-r1"
    elif node_id == "node:aggregate":
        work_cell_id = "work:aggregate"
    else:
        raise ValueError(f"no governed work-cell mapping for {node_id}")
    return {
        "node_id": node_id,
        "work_cell_id": work_cell_id,
        "purpose": purpose,
        "inputs": inputs,
        "outputs": outputs,
        "side_effects": side_effects,
        "resource_claims": resources,
        "executor_class": executor,
        "verification_policy_id": verification,
        "budget_policy_id": budget,
        "idempotency_key": f"{node_id}:<bound-input-hash>",
        "delivery": delivery,
        "expansion_policy_id": None,
        "estimated_duration_ms": duration_ms,
        "provenance": provenance(),
    }


def edge(
    edge_id: str,
    source: str,
    target: str,
    kind: str,
    *,
    output: str | None = None,
    input_: str | None = None,
    schema: str | None = None,
    predicate: str,
    witness: str,
    counterexample: str,
    resource_scope: list[str] | None = None,
) -> dict[str, object]:
    return {
        "edge_id": edge_id,
        "from": source,
        "to": target,
        "kind": kind,
        "output": output,
        "input": input_,
        "schema_id": schema,
        "blocking_predicate": predicate,
        "dependency_witness": witness,
        "removal_counterexample": counterexample,
        "resource_scope": resource_scope or [],
        "provenance": provenance(),
    }


def add_arm(nodes: list[dict[str, object]], edges: list[dict[str, object]], arm: str) -> None:
    prefix = f"node:{arm}"
    deterministic_resources = [resource("artifact:m15-frozen-cards", "read")]
    qwen_resources = [
        resource(
            "api:qwen-local-backend",
            "exclusive",
            network=["endpoint:192.168.68.71:11999"],
            rate_limit="rate:qwen-local-backend",
        )
    ]
    source_resources = [resource("artifact:frozen-compose-source", "read")]

    nodes.append(node(
        f"{prefix}:project-overview",
        f"Create the frozen ReviewGraphen overview Projection for {arm}",
        [io("frozen_cards", "schema:m17-frozen-card-inventory", "benchmark:m15#cards")],
        [io("review_context", "schema:m17-review-context-state")],
        "reviewgraphen-projector",
        "budget:m17-deterministic",
        30_000,
        deterministic_resources,
    ))

    previous = f"{prefix}:project-overview"
    previous_output = "review_context"
    for round_number in range(1, 4):
        select = f"{prefix}:select-{round_number}"
        expand = f"{prefix}:expand-{round_number}"
        nodes.append(node(
            select,
            f"Ask Qwen to select exactly one next ReviewGraphen Projection for {arm}, round {round_number}",
            [io("review_context", "schema:m17-review-context-state", f"{previous}#{previous_output}")],
            [io("expansion_request", "schema:m17-expansion-request")],
            "qwen-route-selector",
            "budget:m17-selection",
            240_000,
            qwen_resources,
            side_effects="external",
        ))
        edges.append(edge(
            f"edge:{arm}:{previous.split(':')[-1]}-select-{round_number}",
            previous,
            select,
            "data",
            output=previous_output,
            input_="review_context",
            schema="schema:m17-review-context-state",
            predicate="the latest ReviewGraphen context state is absent",
            witness="the selector input binds the immediately preceding projection state",
            counterexample="without this edge Qwen could select from a stale or unobserved state",
        ))
        nodes.append(node(
            expand,
            f"Validate the single request and append exactly one frozen ReviewGraphen Projection for {arm}, round {round_number}",
            [
                io("review_context", "schema:m17-review-context-state", f"{previous}#{previous_output}"),
                io("expansion_request", "schema:m17-expansion-request", f"{select}#expansion_request"),
            ],
            [io("review_context", "schema:m17-review-context-state")],
            "reviewgraphen-projector",
            "budget:m17-deterministic",
            30_000,
            deterministic_resources,
        ))
        edges.append(edge(
            f"edge:{arm}:select-{round_number}-expand-{round_number}",
            select,
            expand,
            "data",
            output="expansion_request",
            input_="expansion_request",
            schema="schema:m17-expansion-request",
            predicate="the Qwen expansion request is absent",
            witness="the projector validates the request produced for this exact round",
            counterexample="without this edge the projector could expand a card not selected by Qwen",
        ))
        edges.append(edge(
            f"edge:{arm}:{previous.split(':')[-1]}-expand-{round_number}",
            previous,
            expand,
            "data",
            output=previous_output,
            input_="review_context",
            schema="schema:m17-review-context-state",
            predicate="the prior context and consumed-card ledger are absent",
            witness="the projector needs the prior state to reject duplicates and enforce the round budget",
            counterexample="without this edge duplicate or out-of-budget projections could be accepted",
        ))
        previous = expand
        previous_output = "review_context"

    review = f"{prefix}:final-review"
    validate = f"{prefix}:validate"
    judge = f"{prefix}:judge"
    outcome = f"{prefix}:outcome"
    nodes.extend([
        node(
            review,
            f"Generate the final structured review from only the received ReviewGraphen source IDs for {arm}",
            [io("review_context", "schema:m17-review-context-state", f"{previous}#{previous_output}")],
            [io("review", "schema:m16-review-report")],
            "qwen-reviewer",
            "budget:m17-final-review",
            420_000,
            qwen_resources,
            verification="verification:m17-independent-codex-judge",
            side_effects="external",
        ),
        node(
            validate,
            f"Validate schema, budgets, uniqueness, source trace, and grounding for {arm}",
            [
                io("review_context", "schema:m17-review-context-state", f"{previous}#{previous_output}"),
                io("review", "schema:m16-review-report", f"{review}#review"),
            ],
            [io("validation", "schema:m17-validation-report")],
            "deterministic-review-validator",
            "budget:m17-deterministic",
            30_000,
            deterministic_resources,
        ),
        node(
            judge,
            f"Blindly judge normalized findings against the frozen full source for {arm}",
            [
                io("review", "schema:m16-review-report", f"{review}#review"),
                io("frozen_source", "schema:m17-frozen-source", "benchmark:m16#judge-source"),
            ],
            [io("judgment", "schema:m17-codex-judgment")],
            "independent-codex-judge",
            "budget:m17-judge",
            420_000,
            source_resources,
            verification="verification:m17-independent-codex-judge",
            side_effects="external",
        ),
        node(
            outcome,
            f"Combine protocol validation and blind quality judgment without promoting either to accepted review state for {arm}",
            [
                io("validation", "schema:m17-validation-report", f"{validate}#validation"),
                io("judgment", "schema:m17-codex-judgment", f"{judge}#judgment"),
            ],
            [io("arm_result", "schema:m17-arm-result")],
            "deterministic-reducer",
            "budget:m17-deterministic",
            30_000,
            [],
        ),
    ])
    edges.extend([
        edge(
            f"edge:{arm}:expand-3-final-review", previous, review, "data",
            output="review_context", input_="review_context", schema="schema:m17-review-context-state",
            predicate="the complete bounded context is absent",
            witness="the final reviewer consumes the state after exactly three expansions",
            counterexample="without this edge the review could omit or invent received projections",
        ),
        edge(
            f"edge:{arm}:final-review-validate", review, validate, "data",
            output="review", input_="review", schema="schema:m16-review-report",
            predicate="the raw structured review is absent",
            witness="the deterministic validator checks the exact emitted review artifact",
            counterexample="without this edge protocol compliance could be inferred without validating the output",
        ),
        edge(
            f"edge:{arm}:expand-3-validate", previous, validate, "data",
            output="review_context", input_="review_context", schema="schema:m17-review-context-state",
            predicate="the authoritative projection trace is absent",
            witness="grounding validation compares review citations with the exact received source IDs",
            counterexample="without this edge invented or unreceived source IDs could pass validation",
        ),
        edge(
            f"edge:{arm}:final-review-judge", review, judge, "data",
            output="review", input_="review", schema="schema:m16-review-report",
            predicate="the candidate findings are absent",
            witness="the blind judge evaluates normalized findings from the emitted review",
            counterexample="without this edge the judge could evaluate a different finding set",
        ),
        edge(
            f"edge:{arm}:final-review-judge-authority", review, judge, "review_or_authority",
            predicate="the candidate findings have not crossed the independent judgment seam",
            witness="verification:m17-independent-codex-judge requires a distinct judge actor and session",
            counterexample="without this edge Qwen output could be presented as independently judged",
        ),
        edge(
            f"edge:{arm}:validate-outcome", validate, outcome, "data",
            output="validation", input_="validation", schema="schema:m17-validation-report",
            predicate="the deterministic protocol result is absent",
            witness="the arm outcome reports completion only from the validator artifact",
            counterexample="without this edge a schema-invalid run could be counted as completed",
        ),
        edge(
            f"edge:{arm}:judge-outcome", judge, outcome, "data",
            output="judgment", input_="judgment", schema="schema:m17-codex-judgment",
            predicate="the independent quality judgment is absent",
            witness="the arm outcome keeps quality separate from protocol completion",
            counterexample="without this edge the experiment could report completion without review-quality evidence",
        ),
    ])


def main() -> None:
    DESIGN.mkdir(parents=True, exist_ok=True)
    nodes: list[dict[str, object]] = []
    edges: list[dict[str, object]] = []
    arms = ["8bit-high-r1", "4bit-low-r1"]
    for arm in arms:
        add_arm(nodes, edges, arm)

    edges.append(edge(
        "edge:arm-order",
        "node:8bit-high-r1:outcome",
        "node:4bit-low-r1:select-1",
        "control",
        predicate="the 8bit/high arm has not produced its complete outcome",
        witness="the preregistered order serializes arms to avoid backend interference",
        counterexample="without this edge backend load from the two arms could overlap and confound comparison",
        resource_scope=["api:qwen-local-backend"],
    ))
    nodes.append(node(
        "node:aggregate",
        "Compare CaseGraphen-governed external-runtime arms with their frozen m16 baselines while preserving replicate limits",
        [
            io("8bit_high", "schema:m17-arm-result", "node:8bit-high-r1:outcome#arm_result"),
            io("4bit_low", "schema:m17-arm-result", "node:4bit-low-r1:outcome#arm_result"),
            io("m16_baseline", "schema:m16-aggregate", "benchmark:m16#aggregate"),
        ],
        [io("experiment_result", "schema:m17-experiment-result")],
        "deterministic-reducer",
        "budget:m17-deterministic",
        30_000,
        [resource("artifact:m16-baseline", "read")],
    ))
    for arm in arms:
        edges.append(edge(
            f"edge:{arm}:outcome-aggregate",
            f"node:{arm}:outcome",
            "node:aggregate",
            "data",
            output="arm_result",
            input_=arm.replace("-r1", "").replace("-", "_"),
            schema="schema:m17-arm-result",
            predicate=f"the {arm} arm result is absent",
            witness="the final comparison requires one validated result from each preregistered arm",
            counterexample=f"without this edge the aggregate could silently omit the {arm} intervention result",
        ))

    topology = {
        "schema": "casegraphen.experimental.execution.topology.v0",
        "schema_version": 0,
        "topology_id": "topology:m17-casegraphen-controlled-review-v1",
        "case_space_id": "case-space:m17-casegraphen-controlled-review-v1",
        "nodes": nodes,
        "edges": edges,
        "verification_policy_ids": ["verification:m17-independent-codex-judge"],
        "budget_policy_ids": [
            "budget:m17-deterministic",
            "budget:m17-selection",
            "budget:m17-final-review",
            "budget:m17-judge",
        ],
        "expansion_policy_ids": [],
        "completeness_policy": "all_expected_nodes_reported",
        "provenance": provenance(),
    }

    budgets = {
        "budget:m17-deterministic": {"wall_clock_seconds": 30, "max_attempts": 1},
        "budget:m17-selection": {"wall_clock_seconds": 240, "max_output_tokens": 2000, "max_attempts": 1},
        "budget:m17-final-review": {"wall_clock_seconds": 420, "max_output_tokens": 12000, "max_attempts": 1},
        "budget:m17-judge": {"wall_clock_seconds": 420, "max_output_tokens": 12000, "max_attempts": 1},
    }
    budget_hashes: dict[str, str] = {}
    for policy_id, limits in budgets.items():
        filename = policy_id.replace(":", "-") + ".json"
        budget_hashes[policy_id] = write_json(DESIGN / filename, {
            "schema": "reviewgraphen.benchmark.m17_budget_policy.v1",
            "policy_id": policy_id,
            "status": "proposed_deployment_policy",
            "limits": limits,
        })

    verification = {
        "schema": "casegraphen.experimental.verification_policy.v0",
        "verification_policy_id": "verification:m17-independent-codex-judge",
        "producer_constraints": {"capability_ids": ["capability:qwen-review-producer"]},
        "verifier_constraints": {"capability_ids": ["capability:codex-blind-judge"]},
        "actor_must_differ": True,
        "quorum": {"minimum_accepts": 1, "total_verifiers": 1},
        "required_anchors": ["anchor:frozen-compose-source", "anchor:normalized-findings"],
        "allowed_runtime_attestations": ["separate_session", "model_identity", "prompt_hash"],
        "lenses": ["correctness", "source-validity", "actionability"],
        "provenance": provenance(),
    }
    write_json(DESIGN / "verification.policy.json", verification)
    write_json(DESIGN / "execution.topology.json", topology)


if __name__ == "__main__":
    main()
