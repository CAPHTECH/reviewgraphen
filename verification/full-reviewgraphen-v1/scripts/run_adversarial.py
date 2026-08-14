#!/usr/bin/env python3
"""Independent black-box mutations for the supported V5 CLI slice."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path
from typing import Any, Callable


def canonical(value: Any) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode() + b"\n"


def invoke(binary: Path, arguments: list[str], *, stdout_path: Path | None = None) -> dict[str, Any]:
    if stdout_path:
        output = stdout_path.open("wb")
        close_output = True
    else:
        output = subprocess.PIPE
        close_output = False
    try:
        result = subprocess.run(
            [str(binary), *arguments],
            stdout=output,
            stderr=subprocess.PIPE,
            check=False,
        )
    finally:
        if close_output:
            output.close()
    stdout = stdout_path.read_bytes() if stdout_path else result.stdout
    return {
        "exit_code": result.returncode,
        "stdout": stdout.decode("utf-8", errors="replace").strip(),
        "stderr": result.stderr.decode("utf-8", errors="replace").strip(),
    }


def validate_and_gate(binary: Path, path: Path) -> tuple[dict[str, Any], dict[str, Any]]:
    return invoke(binary, ["schema", "validate", str(path)]), invoke(binary, ["gate", str(path)])


def forge_gate(value: dict[str, Any]) -> None:
    value["gate"].update(status="pass", blocking_ids=[], incomplete_ids=[], reasons=[])


def remove_denominator_member(value: dict[str, Any]) -> None:
    coverage = value["coverage"]
    removed = coverage["denominator_obligation_ids"].pop(0)
    axes = [
        ("visited_obligation_ids", "visited"),
        ("completed_obligation_ids", "completed"),
        ("evidence_supported_obligation_ids", "evidence_supported"),
        ("m5_dependent_successor_obligation_ids", "m5_dependent_successors"),
        ("required_human_resolution_obligation_ids", "required_human_resolutions"),
        ("structurally_preserved_obligation_ids", "structurally_preserved"),
        ("native_verified_obligation_ids", "native_verified"),
        ("verified_obligation_ids", "verified"),
        ("fresh_verified_obligation_ids", "fresh_verified"),
        ("accepted_obligation_ids", "accepted"),
    ]
    for ids_key, count_key in axes:
        if removed in coverage[ids_key]:
            coverage[ids_key].remove(removed)
        coverage[count_key] = len(coverage[ids_key])
    coverage["selected"] = len(coverage["denominator_obligation_ids"])


def inflate_denominator(value: dict[str, Any]) -> None:
    coverage = value["coverage"]
    coverage["denominator_obligation_ids"].append("obligation:sha256:" + "0" * 64)
    coverage["denominator_obligation_ids"].sort()
    coverage["selected"] = len(coverage["denominator_obligation_ids"])


def substitute_denominator_member(value: dict[str, Any]) -> None:
    coverage = value["coverage"]
    old = coverage["denominator_obligation_ids"][0]
    new = "obligation:sha256:" + "0" * 64
    for key, members in coverage.items():
        if key.endswith("_obligation_ids") and isinstance(members, list) and old in members:
            members[members.index(old)] = new
            members.sort()


def forge_freshness(value: dict[str, Any]) -> None:
    coverage = value["coverage"]
    denominator = list(coverage["denominator_obligation_ids"])
    for ids_key, count_key in [
        ("native_verified_obligation_ids", "native_verified"),
        ("verified_obligation_ids", "verified"),
        ("fresh_verified_obligation_ids", "fresh_verified"),
    ]:
        coverage[ids_key] = denominator
        coverage[count_key] = len(denominator)
    forge_gate(value)


def mutate_authority(value: dict[str, Any]) -> None:
    decision = value["result"]["decisions"][0]["body"]
    decision["actor"] = "human:untrusted"
    decision["authority_id"] = "untrusted-reviewer"


def mutate_evidence(value: dict[str, Any]) -> None:
    value["result"]["evidence"][0]["body"]["subject_ids"] = ["file:unrelated"]


def promote_ai_claim(value: dict[str, Any]) -> None:
    claim = next(row["body"] for row in value["result"]["claims"] if row["body"]["author_kind"] == "ai")
    claim["disposition"] = "accepted"
    claim["review_status"] = "accepted"


def run() -> dict[str, Any]:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        raise SystemExit(f"binary does not exist: {binary}")

    cases: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="reviewgraphen-full-v1-") as directory:
        work = Path(directory)
        first_path = work / "first.json"
        second_path = work / "second.json"
        first_run = invoke(binary, ["review", "--fixture", "double-submit"], stdout_path=first_path)
        second_run = invoke(binary, ["review", "--fixture", "double-submit"], stdout_path=second_path)
        first = first_path.read_bytes()
        second = second_path.read_bytes()
        baseline = json.loads(first)

        cases.append({
            "id": "determinism.byte_identity",
            "property": "same fixed input produces the same canonical report bytes",
            "attack": "run the complete supported review route twice in separate temporary stores",
            "observed": {
                "first_exit": first_run["exit_code"],
                "second_exit": second_run["exit_code"],
                "first_sha256": hashlib.sha256(first).hexdigest(),
                "second_sha256": hashlib.sha256(second).hexdigest(),
                "byte_equal": first == second,
            },
            "verdict": "not_broken" if first_run["exit_code"] == 0 and second_run["exit_code"] == 0 and first == second else "broken",
        })

        unknown = invoke(binary, ["review", "--fixture", "not-supported"])
        cases.append({
            "id": "cli.closed_surface",
            "property": "unsupported review routes fail closed",
            "attack": "invoke an unregistered fixture name",
            "observed": unknown,
            "verdict": "not_broken" if unknown["exit_code"] == 2 else "broken",
        })

        def mutation_case(
            case_id: str,
            prop: str,
            attack: str,
            mutate: Callable[[dict[str, Any]], None],
            secure_when: Callable[[dict[str, Any], dict[str, Any]], bool],
        ) -> None:
            candidate = copy.deepcopy(baseline)
            mutate(candidate)
            candidate_path = work / f"{case_id}.json"
            candidate_path.write_bytes(canonical(candidate))
            validation, gate = validate_and_gate(binary, candidate_path)
            secure = secure_when(validation, gate)
            cases.append({
                "id": case_id,
                "property": prop,
                "attack": attack,
                "observed": {"schema_validate": validation, "gate": gate},
                "verdict": "not_broken" if secure else "broken",
            })

        reject = lambda validation, gate: validation["exit_code"] != 0 and gate["exit_code"] != 0
        mutation_case(
            "report_schema.unknown_top_level",
            "schema-external values cannot enter a V5 report",
            "add one unknown top-level property",
            lambda value: value.update(not_in_schema=True),
            reject,
        )
        mutation_case(
            "non_authority.ai_claim_promotion",
            "AI reviewer output cannot label itself accepted",
            "change an AI claim from proposed/unreviewed to accepted/accepted",
            promote_ai_claim,
            reject,
        )
        mutation_case(
            "authority.serialized_decision_tamper",
            "a serialized authority decision remains bound to the admitted actor and authority",
            "replace actor and authority_id but retain all derived IDs and body hashes",
            mutate_authority,
            lambda validation, gate: validation["exit_code"] != 0,
        )
        mutation_case(
            "evidence.serialized_body_tamper",
            "serialized evidence body integrity is checked",
            "replace evidence subject_ids but retain body_hash, event_id, and bindings",
            mutate_evidence,
            lambda validation, gate: validation["exit_code"] != 0,
        )
        mutation_case(
            "denominator.omission",
            "coverage denominator omission is detected against source records",
            "remove one denominator member and repair only coverage-local axes/counts",
            remove_denominator_member,
            lambda validation, gate: validation["exit_code"] != 0,
        )
        mutation_case(
            "denominator.inflation",
            "coverage denominator inflation is detected against source records",
            "add a fabricated obligation and repair only the selected count",
            inflate_denominator,
            lambda validation, gate: validation["exit_code"] != 0,
        )
        mutation_case(
            "denominator.substitution",
            "coverage denominator substitution is detected against source records",
            "replace one obligation across coverage-local arrays but not source records",
            substitute_denominator_member,
            lambda validation, gate: validation["exit_code"] != 0,
        )
        mutation_case(
            "freshness.forged_pass",
            "stale or unverified obligations cannot be relabelled fresh to pass",
            "set all denominator members native/verified/fresh and rewrite the gate as pass without changing source records or hashes",
            forge_freshness,
            lambda validation, gate: validation["exit_code"] != 0 or gate["exit_code"] != 0,
        )
        mutation_case(
            "gate.coherent_local_forgery",
            "gate fails closed for a tampered report",
            "rewrite blocked/incomplete/reasons to a locally coherent pass while retaining the original gate ID and body_hash",
            forge_gate,
            lambda validation, gate: validation["exit_code"] != 0 or gate["exit_code"] != 0,
        )

    broken = [case["id"] for case in cases if case["verdict"] == "broken"]
    return {
        "schema": "reviewgraphen.verification.adversarial_results.v1",
        "scope": "reviewgraphen fixed offline V5 CLI slice and serialized report consumer boundary",
        "method": "independent black-box mutation and negative testing; pre-existing unit tests are not counted as Phase B outcomes",
        "case_count": len(cases),
        "broken_count": len(broken),
        "not_broken_count": len(cases) - len(broken),
        "broken_case_ids": broken,
        "cases": cases,
        "limitations": [
            "A not_broken verdict means only that the named attack did not break the property.",
            "The CLI exposes no injection seam into the private Store authority typestate, so this runner tests its serialized report consumer boundary, not every internal constructor.",
            "This runner does not invoke the live-model adapter, so provider isolation and live replay are outside its measured scope.",
            "After ADR 0028, every detached gate invocation is expected to fail as unsupported (exit 2); this demonstrates absence of a pass-producing detached gate, not authentication of the report.",
        ],
    }


if __name__ == "__main__":
    print(canonical(run()).decode(), end="")
