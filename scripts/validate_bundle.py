#!/usr/bin/env python3
"""Offline validator for the ReviewGraphen design-document bundle."""
from __future__ import annotations

import json
import math
from copy import deepcopy
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
IGNORED_PATH_PARTS = {
    ".git",
    ".reviewgraphen",
    ".venv",
    "__pycache__",
    "target",
}


def fail(message: str) -> None:
    raise AssertionError(message)


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def repository_files(pattern: str) -> list[Path]:
    """Return source-controlled candidates without generated/local state."""
    return [
        path
        for path in sorted(ROOT.rglob(pattern))
        if not IGNORED_PATH_PARTS.intersection(path.relative_to(ROOT).parts)
    ]


def validate_json_and_toml() -> list[str]:
    notes: list[str] = []
    json_files = repository_files("*.json")
    for path in json_files:
        load_json(path)
    notes.append(f"parsed JSON: {len(json_files)} files")

    toml_files = repository_files("*.toml")
    for path in toml_files:
        tomllib.loads(path.read_text(encoding="utf-8"))
    notes.append(f"parsed TOML: {len(toml_files)} files")
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
    markdown_files = repository_files("*.md")
    for path in markdown_files:
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
        f"checked Markdown: {len(markdown_files)} files",
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


def assert_unique_strings(values: Iterable[str], label: str) -> None:
    values = list(values)
    if len(values) != len(set(values)):
        fail(f"duplicate ID in collection: {label}")


def is_typed_block_ref(value: str) -> bool:
    return bool(re.fullmatch(r"(?:gate|global-claim):[A-Za-z0-9][A-Za-z0-9_.:@/+-]*", value))


def validate_semantics(
    program: dict[str, Any] | None = None,
    bundle: dict[str, Any] | None = None,
    report: dict[str, Any] | None = None,
    *,
    check_fixture_equivalence: bool = True,
) -> list[str]:
    program = program or load_json(SCHEMAS / "reviewgraphen.input.example.json")
    bundle = bundle or load_json(SCHEMAS / "reviewgraphen.obligation.example.json")
    report = report or load_json(SCHEMAS / "reviewgraphen.report.example.json")

    # Checked-in schema fixtures and scenario fixtures must remain equivalent.
    if check_fixture_equivalence and program != load_json(EXAMPLE / "program-space.json"):
        fail("program-space fixture differs from schema example")
    if check_fixture_equivalence and report != load_json(EXAMPLE / "review-report.json"):
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
    obligations_by_id = {obligation["id"]: obligation for obligation in bundle["obligations"]}
    declared = set(bundle["universe"]["obligation_ids"])
    if obligation_ids != declared:
        fail("universe obligation_ids do not match obligation records")
    if bundle["universe"]["snapshot_id"] != program["snapshot"]["id"]:
        fail("obligation universe snapshot differs from ProgramSpace snapshot")
    assert_unique_strings(bundle["universe"]["limitation_ids"], "universe limitation_ids")
    assert_refs(bundle["universe"]["limitation_ids"], program_ids, "universe limitations")

    universe_id = bundle["universe"]["id"]
    limitation_ids = set(bundle["universe"]["limitation_ids"])
    scenario_limitations = report["scenario"]["extraction_limitation_ids"]
    assert_unique_strings(scenario_limitations, "scenario extraction_limitation_ids")
    if set(scenario_limitations) != limitation_ids:
        fail("scenario extraction limitations differ from obligation universe")
    if report["scenario"]["universe_id"] != universe_id or report["coverage"]["universe_id"] != universe_id:
        fail("report scenario or coverage universe differs from obligation universe")

    for obligation in bundle["obligations"]:
        assert_refs(obligation["target"]["refs"], program_ids, obligation["id"])
        assert_refs(obligation["source_ids"], program_ids, obligation["id"])
        assert_refs(
            obligation.get("depends_on_obligation_ids", []),
            obligation_ids,
            obligation["id"],
        )

    context_members = {
        context["id"]: set(context["member_ids"])
        for context in program["contexts"]
    }

    def obligation_grounding_ids(obligation: dict[str, Any]) -> set[str]:
        return (
            set(obligation["target"]["refs"])
            | set(obligation["source_ids"])
            | set(obligation["context_requirement"]["context_ids"])
            | set().union(
                *(context_members[context_id] for context_id in obligation["context_requirement"]["context_ids"])
            )
        )

    def binding_connects_to_obligation(binding: dict[str, Any], obligation: dict[str, Any]) -> bool:
        scope = binding["scope"]
        targets = scope.get("target_ids")
        return (
            scope.get("obligation_id") == obligation["id"]
            and scope.get("property_id") == obligation["property"]["id"]
            and isinstance(scope.get("evidence_mode"), str)
            and scope["evidence_mode"] in obligation["evidence_requirement"]["accepted_modes"]
            and isinstance(targets, list)
            and bool(targets)
            and all(isinstance(target, str) and target in program_ids for target in targets)
            and bool(set(targets) & obligation_grounding_ids(obligation))
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
        for obligation_id in claim["obligation_ids"]:
            if not set(claim["source_ids"]) & obligation_grounding_ids(obligations_by_id[obligation_id]):
                fail(f"claim lacks an obligation-specific grounding source: {claim['id']} -> {obligation_id}")
    for binding in result["evidence_bindings"]:
        assert_refs([binding["claim_id"]], claim_ids, binding["id"])
        assert_refs([binding["evidence_id"]], evidence_ids, binding["id"])
        if binding["relation"] in {"supports", "reproduces"}:
            obligation_id = binding["scope"].get("obligation_id")
            if not isinstance(obligation_id, str) or obligation_id not in obligation_ids:
                fail(f"supporting binding lacks an obligation-specific scope: {binding['id']}")
            claim = next(claim for claim in result["claims"] if claim["id"] == binding["claim_id"])
            if obligation_id not in claim["obligation_ids"] or not binding_connects_to_obligation(binding, obligations_by_id[obligation_id]):
                fail(f"supporting binding does not connect to its scoped obligation: {binding['id']}")
    for verification in result["verifications"]:
        assert_refs([verification["claim_id"]], claim_ids, verification["id"])
        assert_refs(verification["evidence_ids"], evidence_ids, verification["id"])
        if verification["result"] == "passed" and not verification["verifier_id"]:
            fail(f"passed verification lacks verifier: {verification['id']}")
        if verification["result"] == "passed" and not verification["evidence_ids"]:
            fail(f"passed verification lacks evidence: {verification['id']}")
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
        for block in obstruction["blocks"]:
            if block not in obligation_ids and not is_typed_block_ref(block):
                fail(f"{obstruction['id']}: invalid obstruction block ref: {block}")
    for gluing in result["gluing_results"]:
        assert_refs(gluing["context_ids"], program_ids, gluing["id"])
        assert_refs(gluing["source_ids"], program_ids | evidence_ids, gluing["id"])
        assert_refs(gluing["obstruction_ids"], obstruction_ids, gluing["id"])

    bindings_by_claim: dict[str, list[dict[str, Any]]] = {}
    for binding in result["evidence_bindings"]:
        bindings_by_claim.setdefault(binding["claim_id"], []).append(binding)
    supporting_relations = {"supports", "reproduces"}
    claims_by_id = {claim["id"]: claim for claim in result["claims"]}
    verifications_by_id = {verification["id"]: verification for verification in result["verifications"]}

    for claim in result["claims"]:
        relations = {binding["relation"] for binding in bindings_by_claim.get(claim["id"], [])}
        if claim["disposition"] in {"supported", "accepted"} and not relations & supporting_relations:
            fail(f"supported claim lacks supporting evidence binding: {claim['id']}")
        if claim["disposition"] == "refuted" and "refutes" not in relations:
            fail(f"refuted claim lacks refuting evidence binding: {claim['id']}")

    for verification in result["verifications"]:
        supporting_evidence = {
            binding["evidence_id"]
            for binding in bindings_by_claim.get(verification["claim_id"], [])
            if binding["relation"] in supporting_relations
        }
        if verification["result"] == "passed" and (
            not verification["evidence_ids"]
            or not set(verification["evidence_ids"]).issubset(supporting_evidence)
        ):
            fail(f"passed verification has no connected supporting binding: {verification['id']}")

    for finding in result["findings"]:
        claim = claims_by_id[finding["claim_id"]]
        supporting_evidence = {
            binding["evidence_id"]
            for binding in bindings_by_claim.get(finding["claim_id"], [])
            if binding["relation"] in supporting_relations
        }
        if not set(finding["evidence_ids"]).issubset(supporting_evidence):
            fail(f"finding evidence is not connected to its claim: {finding['id']}")
        if not set(finding["location_refs"]).issubset(set(claim["source_ids"])):
            fail(f"finding locations are outside claim source trace: {finding['id']}")
        for verification_id in finding["verification_ids"]:
            verification = verifications_by_id[verification_id]
            if verification["claim_id"] != finding["claim_id"] or verification["result"] != "passed":
                fail(f"finding verification is not a passed verification of the finding claim: {finding['id']}")
            if not set(verification["evidence_ids"]).issubset(set(finding["evidence_ids"])):
                fail(f"finding verification evidence is outside finding evidence trace: {finding['id']}")
        if finding["status"] == "accepted":
            decision = next(decision for decision in result["decisions"] if decision["id"] == finding["decision_id"])
            required_trace = {finding["claim_id"], *finding["evidence_ids"], *finding["verification_ids"]}
            if (
                claim["disposition"] != "accepted"
                or claim["review_status"] != "accepted"
                or not finding["evidence_ids"]
                or not finding["verification_ids"]
                or any(
                    verifications_by_id[verification_id]["result"] != "passed"
                    or verifications_by_id[verification_id]["freshness"] != "fresh"
                    or not verifications_by_id[verification_id]["evidence_ids"]
                    for verification_id in finding["verification_ids"]
                )
                or decision["target_id"] != finding["claim_id"]
                or decision["outcome"] != "accept"
                or not required_trace.issubset(set(decision["source_ids"]))
            ):
                fail(f"accepted finding decision lacks the connected evidence trace: {finding['id']}")

    # A capability qualification is not merely an aggregate number: every
    # unknown obligation is represented by its typed, source-traceable blocker.
    obstructions_by_id = {obstruction["id"]: obstruction for obstruction in result["obstructions"]}
    required_partial_obstructions: set[str] = set()
    for obligation in bundle["obligations"]:
        applicability = obligation["applicability"]
        if applicability["status"] != "unknown":
            continue
        for reason in applicability["reasons"]:
            if not reason.startswith("capability_partial:"):
                continue
            matches = [
                obstruction
                for obstruction in result["obstructions"]
                if obstruction["kind"] == "capability_partial"
                and obstruction["source_ids"] == obligation["source_ids"]
                and obstruction["required_resolution"] == [reason]
                and obstruction["blocks"] == [obligation["id"]]
                and obstruction["review_status"] == "unreviewed"
            ]
            if len(matches) != 1:
                fail(f"unknown obligation lacks exactly one typed capability obstruction: {obligation['id']}")
            required_partial_obstructions.add(matches[0]["id"])

    for projection_name in ("ai_view", "human_review", "ci_gate"):
        payload = report["projection"][projection_name]["payload"]
        projected_ids = set(
            payload.get("obstruction_ids", [])
            if projection_name == "ai_view"
            else payload.get("incomplete_reason_ids", [])
        )
        if not required_partial_obstructions.issubset(projected_ids):
            fail(f"{projection_name} omits typed incomplete reasons")
        if not required_partial_obstructions.issubset(set(report["projection"][projection_name]["source_ids"])):
            fail(f"{projection_name} omits typed incomplete-reason source trace")

    report_limitations = report["coverage"]["limitations"]
    assert_unique_strings((limitation["id"] for limitation in report_limitations), "coverage limitations")
    report_limits = {limitation["id"]: limitation for limitation in report_limitations}
    input_limits = {limitation["id"]: limitation for limitation in program["extraction"]["limitations"]}
    if set(report_limits) != limitation_ids or set(input_limits) != limitation_ids:
        fail("scenario, coverage, ProgramSpace, and universe limitation IDs differ")
    for limitation_id in limitation_ids:
        if limitation_id not in report_limits or limitation_id not in input_limits:
            fail(f"universe limitation missing from report or ProgramSpace: {limitation_id}")
        for field in ("kind", "description", "severity", "source_ids"):
            if report_limits[limitation_id][field] != input_limits[limitation_id][field]:
                fail(f"report limitation differs from ProgramSpace extraction limitation: {limitation_id}.{field}")

    stages = report["coverage"]["stages"]
    expected_total = len(obligation_ids)
    weights = {obligation["id"]: obligation["risk"]["weight"] for obligation in bundle["obligations"]}
    total_weight = sum(weights.values())
    visited_obligations = {
        obligation_id
        for execution in result["executions"]
        for obligation_id in execution["obligation_ids"]
    }
    completed_obligations = {
        obligation_id
        for execution in result["executions"]
        if execution["status"] == "completed"
        for obligation_id in execution["obligation_ids"]
    }
    def connected_claims_for_obligation(obligation: dict[str, Any]) -> list[dict[str, Any]]:
        return [
            claim
            for claim in result["claims"]
            if obligation["id"] in claim["obligation_ids"]
            and claim["disposition"] in {"supported", "accepted"}
            and any(
                binding["relation"] in supporting_relations
                and binding_connects_to_obligation(binding, obligation)
                for binding in bindings_by_claim.get(claim["id"], [])
            )
        ]

    def has_connected_verification(obligation: dict[str, Any], freshness: str | None = None) -> bool:
        for claim in connected_claims_for_obligation(obligation):
            supporting_evidence = {
                binding["evidence_id"]
                for binding in bindings_by_claim[claim["id"]]
                if binding["relation"] in supporting_relations
                and binding_connects_to_obligation(binding, obligation)
            }
            if any(
                verification["claim_id"] == claim["id"]
                and verification["result"] == "passed"
                and (freshness is None or verification["freshness"] == freshness)
                and bool(verification["evidence_ids"])
                and set(verification["evidence_ids"]).issubset(supporting_evidence)
                for verification in result["verifications"]
            ):
                return True
        return False

    supported_obligations = {
        obligation["id"]
        for obligation in bundle["obligations"]
        if connected_claims_for_obligation(obligation)
    }
    verified_obligations = {
        obligation["id"]
        for obligation in bundle["obligations"]
        if has_connected_verification(obligation)
    }
    fresh_verified_obligations = {
        obligation["id"]
        for obligation in bundle["obligations"]
        if has_connected_verification(obligation, freshness="fresh")
    }
    expected_stage_ids = {
        "generated": obligation_ids,
        "visited": visited_obligations,
        "completed": completed_obligations,
        "evidence_supported": supported_obligations,
        "verified": verified_obligations,
        "fresh_verified": fresh_verified_obligations,
    }
    for stage, ratio in stages.items():
        if ratio["numerator"] > ratio["denominator"]:
            fail(f"coverage numerator > denominator: {stage}")
        if ratio["denominator"] != expected_total:
            fail(f"coverage denominator differs from universe: {stage}")
        expected_ids = expected_stage_ids[stage]
        if ratio["numerator"] != len(expected_ids):
            fail(f"coverage numerator does not match records: {stage}")
        expected_weighted = sum(weights[obligation_id] for obligation_id in expected_ids) / total_weight
        if not math.isclose(ratio["weighted"], expected_weighted, rel_tol=0.0, abs_tol=1e-12):
            fail(f"coverage weighted value does not match canonical obligation weights: {stage}")
    freshness = report["coverage"]["freshness"]
    expected_freshness = {
        state: sum(1 for verification in result["verifications"] if verification["freshness"] == state)
        for state in ("fresh", "stale", "unknown")
    }
    if freshness != expected_freshness:
        fail("freshness counts must be derived from verification records")
    payload_ref_fields = {
        "human_review": (
            "critical_findings",
            "blocking_obstructions",
            "unresolved_obligation_ids",
            "incomplete_reason_ids",
        ),
        "ai_view": (
            "obligation_ids",
            "claim_ids",
            "verification_ids",
            "finding_ids",
            "obstruction_ids",
        ),
        "audit_trace": ("accepted_decision_ids",),
        "ci_gate": (
            "blocking_finding_ids",
            "blocking_obstruction_ids",
            "incomplete_reason_ids",
        ),
    }
    for name, view in report["projection"].items():
        if not view["source_ids"]:
            fail(f"projection has no source IDs: {name}")
        if not view["information_loss"]:
            fail(f"projection has no information loss: {name}")
        assert_unique_strings(view["source_ids"], f"projection {name} source_ids")
        for source_id in view["source_ids"]:
            if source_id not in known_report_ids and not is_typed_block_ref(source_id):
                fail(f"projection {name} has dangling source ID: {source_id}")
        payload_ids = {
            ref
            for field in payload_ref_fields[name]
            for ref in view["payload"].get(field, [])
        }
        assert_refs(payload_ids, known_report_ids, f"projection {name} payload")
        if not payload_ids.issubset(set(view["source_ids"])):
            fail(f"projection {name} payload refs are missing from source trace")

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


def validate_semantic_mutations() -> list[str]:
    """Prove that schema-valid shortcuts cannot bypass the semantic checks."""
    program = load_json(SCHEMAS / "reviewgraphen.input.example.json")
    bundle = load_json(SCHEMAS / "reviewgraphen.obligation.example.json")
    report = load_json(SCHEMAS / "reviewgraphen.report.example.json")
    report_schema = load_json(SCHEMAS / "reviewgraphen.report.schema.json")
    report_validator = Draft202012Validator(report_schema, format_checker=FormatChecker())

    def must_fail(name: str, mutated: dict[str, Any]) -> None:
        if list(report_validator.iter_errors(mutated)):
            fail(f"semantic mutation is not schema-valid: {name}")
        try:
            validate_semantics(
                deepcopy(program),
                deepcopy(bundle),
                mutated,
                check_fixture_equivalence=False,
            )
        except AssertionError:
            return
        fail(f"semantic mutation unexpectedly passed: {name}")

    scenario_mismatch = deepcopy(report)
    scenario_mismatch["scenario"]["extraction_limitation_ids"] = ["limitation:bounded-concurrency"]
    must_fail("scenario limitation set mismatch", scenario_mismatch)

    accepted_without_acceptance = deepcopy(report)
    accepted_without_acceptance["result"]["claims"][3]["disposition"] = "supported"
    accepted_without_acceptance["result"]["claims"][3]["review_status"] = "human_reviewed"
    must_fail("accepted finding on non-accepted claim", accepted_without_acceptance)

    stale_accepted_evidence = deepcopy(report)
    stale_accepted_evidence["result"]["verifications"][3]["freshness"] = "stale"
    must_fail("accepted finding on stale verification", stale_accepted_evidence)

    empty_passed_evidence = deepcopy(report)
    empty_passed_evidence["result"]["verifications"][0]["evidence_ids"] = []
    must_fail("passed verification without evidence", empty_passed_evidence)

    unrelated_obligation = deepcopy(report)
    guard_obligation = unrelated_obligation["result"]["claims"][0]["obligation_ids"][0]
    unrelated_obligation["result"]["claims"][0]["obligation_ids"] = [
        unrelated_obligation["result"]["claims"][2]["obligation_ids"][0]
    ]
    unrelated_obligation["result"]["claims"][2]["obligation_ids"].append(guard_obligation)
    must_fail("unrelated property binding cannot inflate coverage", unrelated_obligation)

    same_property_reuse = deepcopy(report)
    second_obligation = same_property_reuse["result"]["claims"][1]["obligation_ids"][0]
    same_property_reuse["result"]["executions"][0]["obligation_ids"].append(second_obligation)
    same_property_reuse["result"]["claims"][0]["obligation_ids"].append(second_obligation)
    same_property_reuse["result"]["claims"][1]["disposition"] = "proposed"
    same_property_reuse["result"]["claims"][1]["review_status"] = "unreviewed"
    must_fail("same-property binding cannot be reused for another obligation", same_property_reuse)

    missing_ai_trace = deepcopy(report)
    missing_ai_trace["projection"]["ai_view"]["source_ids"].remove("verification:guard-order")
    must_fail("projection payload requires source trace", missing_ai_trace)

    return ["semantic mutation regressions: 7 rejected"]


def main() -> int:
    notes: list[str] = []
    for check in (
        validate_json_and_toml,
        validate_schemas,
        validate_markdown,
        validate_semantics,
        validate_semantic_mutations,
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
