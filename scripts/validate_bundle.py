#!/usr/bin/env python3
"""Offline validator for the ReviewGraphen design-document bundle."""
from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
import tomllib
import urllib.parse
from pathlib import Path
from typing import Any, Iterable

try:
    from jsonschema import Draft202012Validator, FormatChecker
except ImportError as exc:  # pragma: no cover
    raise SystemExit("jsonschema is required: python -m pip install jsonschema") from exc

ROOT = Path(__file__).resolve().parents[1]
SCHEMAS = ROOT / "schemas"
EXAMPLE = ROOT / "examples" / "double-submit-payment"


def fail(message: str) -> None:
    raise AssertionError(message)


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def validate_json_and_toml() -> list[str]:
    notes: list[str] = []
    for path in sorted(ROOT.rglob("*.json")):
        load_json(path)
    notes.append(f"parsed JSON: {len(list(ROOT.rglob('*.json')))} files")

    for path in sorted(ROOT.rglob("*.toml")):
        tomllib.loads(path.read_text(encoding="utf-8"))
    notes.append(f"parsed TOML: {len(list(ROOT.rglob('*.toml')))} files")
    return notes


def validate_schemas() -> list[str]:
    pairs = [
        ("reviewgraphen.input.schema.json", "reviewgraphen.input.example.json"),
        ("reviewgraphen.obligation.schema.json", "reviewgraphen.obligation.example.json"),
        ("reviewgraphen.report.schema.json", "reviewgraphen.report.example.json"),
    ]
    for schema_name, example_name in pairs:
        schema = load_json(SCHEMAS / schema_name)
        Draft202012Validator.check_schema(schema)
        instance = load_json(SCHEMAS / example_name)
        errors = sorted(
            Draft202012Validator(schema, format_checker=FormatChecker()).iter_errors(instance),
            key=lambda error: list(error.absolute_path),
        )
        if errors:
            rendered = "\n".join(
                f"{example_name}: /{'/'.join(map(str, e.absolute_path))}: {e.message}"
                for e in errors
            )
            fail(rendered)
    return [f"validated JSON Schema examples: {len(pairs)}"]


def validate_markdown() -> list[str]:
    link_pattern = re.compile(r"(?<!!)\[[^\]]*\]\(([^)]+)\)")
    relative_links = 0
    for path in sorted(ROOT.rglob("*.md")):
        text = path.read_text(encoding="utf-8")
        if text.count("```") % 2 != 0:
            fail(f"unbalanced fenced code block: {path.relative_to(ROOT)}")
        for raw_target in link_pattern.findall(text):
            target = raw_target.strip().split()[0].strip("<>")
            if target.startswith(("http://", "https://", "mailto:", "#")):
                continue
            target = urllib.parse.unquote(target.split("#", 1)[0])
            if not target:
                continue
            relative_links += 1
            resolved = (path.parent / target).resolve()
            if not resolved.exists():
                fail(
                    f"broken link: {path.relative_to(ROOT)} -> {target} "
                    f"({resolved})"
                )
    return [
        f"checked Markdown: {len(list(ROOT.rglob('*.md')))} files",
        f"checked relative links: {relative_links}",
    ]


def collect_ids(records: Iterable[dict[str, Any]]) -> set[str]:
    result: set[str] = set()
    for record in records:
        rid = record.get("id")
        if isinstance(rid, str):
            if rid in result:
                fail(f"duplicate ID in collection: {rid}")
            result.add(rid)
    return result


def assert_refs(refs: Iterable[str], known: set[str], label: str) -> None:
    missing = sorted(set(refs) - known)
    if missing:
        fail(f"{label}: dangling refs: {missing}")


def validate_semantics() -> list[str]:
    program = load_json(SCHEMAS / "reviewgraphen.input.example.json")
    bundle = load_json(SCHEMAS / "reviewgraphen.obligation.example.json")
    report = load_json(SCHEMAS / "reviewgraphen.report.example.json")

    # Checked-in schema fixtures and scenario fixtures must remain equivalent.
    if program != load_json(EXAMPLE / "program-space.json"):
        fail("program-space fixture differs from schema example")
    if report != load_json(EXAMPLE / "review-report.json"):
        fail("review-report fixture differs from schema example")

    program_records = (
        program["artifacts"]
        + program["relations"]
        + program["contexts"]
        + program["invariants"]
        + program["evidence"]
        + program["extraction"]["limitations"]
    )
    program_ids = collect_ids(program_records)
    program_ids |= {program["repository"]["id"], program["snapshot"]["id"]}

    for relation in program["relations"]:
        assert_refs(
            [relation["source_id"], *relation["target_ids"]],
            program_ids,
            relation["id"],
        )
    for context in program["contexts"]:
        assert_refs(context["member_ids"], program_ids, context["id"])
    for invariant in program["invariants"]:
        assert_refs(invariant["scope_ids"], program_ids, invariant["id"])

    obligation_ids = collect_ids(bundle["obligations"])
    declared = set(bundle["universe"]["obligation_ids"])
    if obligation_ids != declared:
        fail("universe obligation_ids do not match obligation records")
    if bundle["universe"]["snapshot_id"] != program["snapshot"]["id"]:
        fail("obligation universe snapshot differs from ProgramSpace snapshot")
    assert_refs(bundle["universe"]["limitation_ids"], program_ids, "universe limitations")

    for obligation in bundle["obligations"]:
        assert_refs(obligation["target"]["refs"], program_ids, obligation["id"])
        assert_refs(obligation["source_ids"], program_ids, obligation["id"])
        assert_refs(
            obligation.get("depends_on_obligation_ids", []),
            obligation_ids,
            obligation["id"],
        )

    result = report["result"]
    if set(result["obligation_ids"]) != obligation_ids:
        fail("report obligation IDs differ from obligation universe")

    execution_ids = collect_ids(result["executions"])
    claim_ids = collect_ids(result["claims"])
    binding_ids = collect_ids(result["evidence_bindings"])
    verification_ids = collect_ids(result["verifications"])
    decision_ids = collect_ids(result["decisions"])
    finding_ids = collect_ids(result["findings"])
    obstruction_ids = collect_ids(result["obstructions"])
    candidate_ids = collect_ids(result["completion_candidates"])
    gluing_ids = collect_ids(result["gluing_results"])

    evidence_ids = {binding["evidence_id"] for binding in result["evidence_bindings"]}
    known_report_ids = (
        program_ids
        | obligation_ids
        | execution_ids
        | claim_ids
        | binding_ids
        | verification_ids
        | decision_ids
        | finding_ids
        | obstruction_ids
        | candidate_ids
        | gluing_ids
        | evidence_ids
        | {
            report["metadata"]["report_id"],
            report["metadata"]["run_id"],
            report["scenario"]["universe_id"],
        }
    )

    for execution in result["executions"]:
        assert_refs(execution["obligation_ids"], obligation_ids, execution["id"])
        assert_refs(execution["claim_ids"], claim_ids, execution["id"])
    for claim in result["claims"]:
        assert_refs([claim["execution_id"]], execution_ids, claim["id"])
        assert_refs(claim["obligation_ids"], obligation_ids, claim["id"])
        assert_refs(claim["source_ids"], program_ids | evidence_ids, claim["id"])
    for binding in result["evidence_bindings"]:
        assert_refs([binding["claim_id"]], claim_ids, binding["id"])
    for verification in result["verifications"]:
        assert_refs([verification["claim_id"]], claim_ids, verification["id"])
        assert_refs(verification["evidence_ids"], evidence_ids, verification["id"])
        if verification["result"] == "passed" and not verification["verifier_id"]:
            fail(f"passed verification lacks verifier: {verification['id']}")
    for decision in result["decisions"]:
        assert_refs([decision["target_id"]], claim_ids | finding_ids | obstruction_ids, decision["id"])
        assert_refs(decision["source_ids"], known_report_ids, decision["id"])
    for finding in result["findings"]:
        assert_refs([finding["claim_id"]], claim_ids, finding["id"])
        assert_refs(finding["evidence_ids"], evidence_ids, finding["id"])
        assert_refs(finding["verification_ids"], verification_ids, finding["id"])
        assert_refs(finding["location_refs"], program_ids, finding["id"])
        assert_refs(finding.get("remediation_candidate_ids", []), candidate_ids, finding["id"])
        if finding["status"] == "accepted":
            decision_id = finding.get("decision_id")
            if not decision_id or decision_id not in decision_ids:
                fail(f"accepted finding lacks decision: {finding['id']}")
    for obstruction in result["obstructions"]:
        assert_refs(obstruction["source_ids"], program_ids | evidence_ids, obstruction["id"])
        assert_refs(obstruction.get("counterexample_refs", []), evidence_ids, obstruction["id"])
    for gluing in result["gluing_results"]:
        assert_refs(gluing["context_ids"], program_ids, gluing["id"])
        assert_refs(gluing["source_ids"], program_ids | evidence_ids, gluing["id"])
        assert_refs(gluing["obstruction_ids"], obstruction_ids, gluing["id"])

    stages = report["coverage"]["stages"]
    expected_total = len(obligation_ids)
    for stage, ratio in stages.items():
        if ratio["numerator"] > ratio["denominator"]:
            fail(f"coverage numerator > denominator: {stage}")
        if ratio["denominator"] != expected_total:
            fail(f"coverage denominator differs from universe: {stage}")
    if report["coverage"]["universe_id"] != report["scenario"]["universe_id"]:
        fail("coverage and scenario reference different universes")

    for name, view in report["projection"].items():
        if not view["source_ids"]:
            fail(f"projection has no source IDs: {name}")
        if not view["information_loss"]:
            fail(f"projection has no information loss: {name}")

    # Source-location paths used by the reference fixture should exist.
    fixture_root = EXAMPLE / "fixture"
    for artifact in program["artifacts"]:
        location = artifact.get("location")
        if location and location.get("path"):
            source_path = fixture_root / location["path"]
            if not source_path.exists():
                fail(f"fixture source path missing: {source_path}")

    return [
        f"semantic fixture IDs: ProgramSpace={len(program_ids)}, obligations={len(obligation_ids)}",
        f"semantic report records: executions={len(execution_ids)}, claims={len(claim_ids)}, verifications={len(verification_ids)}",
    ]


def validate_rust_fixture() -> list[str]:
    cargo = shutil.which("cargo")
    if cargo is None:
        return ["Rust fixture: skipped (cargo not installed)"]
    subprocess.run(
        [cargo, "test", "--quiet"],
        cwd=EXAMPLE / "fixture",
        check=True,
        timeout=60,
    )
    return ["Rust fixture: cargo test passed"]


def main() -> int:
    notes: list[str] = []
    for check in (
        validate_json_and_toml,
        validate_schemas,
        validate_markdown,
        validate_semantics,
        validate_rust_fixture,
    ):
        notes.extend(check())
    print("ReviewGraphen bundle validation: PASS")
    for note in notes:
        print(f"- {note}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, json.JSONDecodeError, tomllib.TOMLDecodeError, subprocess.SubprocessError) as exc:
        print(f"ReviewGraphen bundle validation: FAIL\n- {exc}", file=sys.stderr)
        raise SystemExit(1)
