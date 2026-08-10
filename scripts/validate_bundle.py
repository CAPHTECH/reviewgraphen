#!/usr/bin/env python3
"""Offline validator for the ReviewGraphen design-document bundle."""
from __future__ import annotations

import json
import hashlib
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


def reject_json_constant(value: str) -> None:
    fail(f"non-standard JSON numeric constant: {value}")


def reject_nonfinite_numbers(value: Any, path: str = "json") -> None:
    if isinstance(value, float) and not math.isfinite(value):
        fail(f"nonfinite JSON number: {path}")
    if isinstance(value, list):
        for index, item in enumerate(value):
            reject_nonfinite_numbers(item, f"{path}[{index}]")
    elif isinstance(value, dict):
        for key, item in value.items():
            reject_nonfinite_numbers(item, f"{path}.{key}")


def load_json(path: Path) -> Any:
    value = json.loads(
        path.read_text(encoding="utf-8"),
        parse_constant=reject_json_constant,
    )
    reject_nonfinite_numbers(value, str(path.relative_to(ROOT)))
    return value


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
        ("reviewgraphen.report.v2.schema.json", "reviewgraphen.report.v2.example.json"),
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


D2_REPORT_MAX_ROWS = 200_000
D2_REPORT_MAX_CANONICAL_BYTES = 67_108_864
D2_REPORT_MAX_LOSSES = 4_096
D2_REPORT_MAX_STRING_BYTES = 16_384
D2_EXECUTION_MAX_CANONICAL_BYTES = 131_072
D2_CLAIM_MAX_CANONICAL_BYTES = 32_768
U64_MAX = (1 << 64) - 1


def canonical_json_bytes(value: Any) -> bytes:
    return json.dumps(
        value,
        allow_nan=False,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")


def validate_d2_float_domain(value: Any, path: str = "report") -> None:
    if isinstance(value, float):
        if not math.isfinite(value) or not 0.0 <= value <= 1.0:
            fail(f"report v2 float outside finite [0,1] domain: {path}")
    elif isinstance(value, list):
        for index, item in enumerate(value):
            validate_d2_float_domain(item, f"{path}[{index}]")
    elif isinstance(value, dict):
        for key, item in value.items():
            validate_d2_float_domain(item, f"{path}.{key}")


def sha256_id(value: Any) -> str:
    return f"sha256:{hashlib.sha256(canonical_json_bytes(value)).hexdigest()}"


def assert_canonical_order(values: list[Any], key: Any, label: str) -> None:
    observed = [key(value) for value in values]
    if observed != sorted(observed) or len(observed) != len(set(observed)):
        fail(f"report v2 noncanonical/duplicate array: {label}")


def check_report_v2_local_limits(rows: int, canonical_bytes: int) -> None:
    for operation, observed, limit in (
        ("report rows", rows, D2_REPORT_MAX_ROWS),
        ("report canonical bytes", canonical_bytes, D2_REPORT_MAX_CANONICAL_BYTES),
    ):
        if observed < 0 or observed > limit:
            fail(f"Incomplete: {operation}: limit={limit}, observed={observed}")


def checked_u64_add(left: int, right: int, operation: str) -> int:
    if left < 0 or right < 0 or left > U64_MAX - right:
        fail(f"Incomplete: {operation}: observed={U64_MAX}")
    return left + right


def checked_u64_mul(left: int, right: int, operation: str) -> int:
    if left < 0 or right < 0 or (right != 0 and left > U64_MAX // right):
        fail(f"Incomplete: {operation}: observed={U64_MAX}")
    return left * right


def checked_u64_sum(values: Iterable[int], operation: str) -> int:
    total = 0
    for value in values:
        total = checked_u64_add(total, value, operation)
    return total


def require_utf8_bytes(value: str, limit: int, operation: str) -> None:
    observed = len(value.encode("utf-8"))
    if observed > limit:
        fail(f"Incomplete: {operation}: limit={limit}, observed={observed}")


def validate_all_string_bytes(value: Any, path: str = "report") -> None:
    if isinstance(value, str):
        require_utf8_bytes(value, D2_REPORT_MAX_STRING_BYTES, path)
    elif isinstance(value, list):
        for index, item in enumerate(value):
            validate_all_string_bytes(item, f"{path}[{index}]")
    elif isinstance(value, dict):
        for key, item in value.items():
            require_utf8_bytes(key, D2_REPORT_MAX_STRING_BYTES, f"{path}.key")
            validate_all_string_bytes(item, f"{path}.{key}")


def reference_owned_charge(value: Any, operation: str = "report reference charge") -> int:
    if value is None:
        return 0
    if isinstance(value, bool):
        return 1
    if isinstance(value, (int, float)):
        return 8
    if isinstance(value, str):
        return len(value.encode("utf-8"))
    if isinstance(value, list):
        total = checked_u64_mul(len(value), 8, operation)
        for item in value:
            total = checked_u64_add(total, reference_owned_charge(item, operation), operation)
        return total
    if isinstance(value, dict):
        total = checked_u64_mul(len(value), 16, operation)
        for key, item in value.items():
            total = checked_u64_add(total, len(key.encode("utf-8")), operation)
            total = checked_u64_add(total, reference_owned_charge(item, operation), operation)
        return total
    fail(f"unsupported capacity value: {type(value).__name__}")
    return 0


def validate_report_v2_contract(
    report: dict[str, Any] | None = None,
    *,
    run_mutations: bool = True,
) -> list[str]:
    if report is None:
        report = load_json(SCHEMAS / "reviewgraphen.report.v2.example.json")
    result = report["result"]
    metadata = report["metadata"]
    scenario = report["scenario"]
    coverage = report["coverage"]
    views = report["projection"]["views"]

    registrations = result["artifact_registrations"]
    executions = result["executions"]
    claims = result["claims"]
    obstructions = result["obstructions"]
    validate_all_string_bytes(report)
    validate_d2_float_domain(report)

    assert_canonical_order(
        registrations,
        lambda row: (row["event_sequence"], row["registration_id"]),
        "artifact_registrations",
    )
    assert_canonical_order(
        executions,
        lambda row: (row["event_sequence"], row["id"]),
        "executions",
    )
    assert_canonical_order(
        claims,
        lambda row: (row["event_sequence"], row["id"]),
        "claims",
    )
    assert_canonical_order(
        obstructions,
        lambda row: canonical_json_bytes(row),
        "obstructions",
    )
    view_order = {"human": 0, "ci": 1, "machine": 2}
    assert_canonical_order(views, lambda row: view_order[row["kind"]], "projection.views")

    id_sets: list[tuple[list[str], str]] = [
        (scenario["selected_obligation_ids"], "scenario.selected_obligation_ids"),
        (scenario["artifact_registration_ids"], "scenario.artifact_registration_ids"),
        (coverage["denominator_obligation_ids"], "coverage.denominator_obligation_ids"),
        (coverage["visited_obligation_ids"], "coverage.visited_obligation_ids"),
        (coverage["completed_obligation_ids"], "coverage.completed_obligation_ids"),
    ]
    for execution in executions:
        id_sets.extend(
            [
                (execution["obligation_ids"], f"{execution['id']}.obligation_ids"),
                (execution["parsed_claim_ids"], f"{execution['id']}.parsed_claim_ids"),
            ]
        )
    for claim in claims:
        id_sets.extend(
            [
                (claim["obligation_ids"], f"{claim['id']}.obligation_ids"),
                (claim["target_refs"], f"{claim['id']}.target_refs"),
                (claim["source_ids"], f"{claim['id']}.source_ids"),
            ]
        )
        assert_canonical_order(claim["assumptions"], lambda value: value, f"{claim['id']}.assumptions")
        assert_canonical_order(
            claim["requested_evidence"],
            lambda value: value,
            f"{claim['id']}.requested_evidence",
        )
        require_utf8_bytes(claim["summary"], 8_192, f"{claim['id']}.summary")
        for value in claim["assumptions"]:
            require_utf8_bytes(value, 2_048, f"{claim['id']}.assumptions")
        for value in claim["requested_evidence"]:
            require_utf8_bytes(value, 2_048, f"{claim['id']}.requested_evidence")
        claim_identity = {
            key: claim[key]
            for key in (
                "assumptions", "execution_id", "obligation_ids", "polarity", "property_id",
                "requested_evidence", "source_ids", "summary", "target_refs",
            )
        }
        if claim["identity_body_hash"] != sha256_id(claim_identity) or claim["id"] != f"claim:{sha256_id(claim_identity)}":
            fail(f"report v2 claim identity mismatch: {claim['id']}")
        claim_body = {
            key: value
            for key, value in claim.items()
            if key not in {"event_sequence", "event_id", "identity_body_hash", "body_hash"}
        }
        claim_bytes = canonical_json_bytes(claim_body)
        if len(claim_bytes) > D2_CLAIM_MAX_CANONICAL_BYTES:
            fail(
                f"Incomplete: claim canonical bytes: limit={D2_CLAIM_MAX_CANONICAL_BYTES}, "
                f"observed={len(claim_bytes)}"
            )
        if claim["body_hash"] != sha256_id(claim_body):
            fail(f"report v2 claim body hash mismatch: {claim['id']}")
    for index, obstruction in enumerate(obstructions):
        id_sets.extend(
            [
                (obstruction["source_ids"], f"obstructions[{index}].source_ids"),
                (obstruction["blocks"], f"obstructions[{index}].blocks"),
            ]
        )
    known_recovery_refs = {
        metadata["report_id"],
        scenario["program_space_ref"],
        scenario["universe_id"],
        scenario["plan_id"],
        *scenario["selected_obligation_ids"],
        *(row["registration_id"] for row in registrations),
        *(row["id"] for row in executions),
        *(row["id"] for row in claims),
    }
    for view in views:
        id_sets.extend(
            [
                (view["source_ids"], f"{view['kind']}.source_ids"),
                (view["payload"]["execution_ids"], f"{view['kind']}.payload.execution_ids"),
                (view["payload"]["claim_ids"], f"{view['kind']}.payload.claim_ids"),
            ]
        )
        assert_canonical_order(
            view["payload"]["obstruction_kinds"],
            lambda value: value,
            f"{view['kind']}.payload.obstruction_kinds",
        )
        if not view["information_loss"]:
            fail(f"report v2 view has empty information loss: {view['kind']}")
        assert_canonical_order(
            view["information_loss"],
            lambda loss: (
                loss["kind"],
                loss["reason"],
                tuple(loss["source_ids"]),
                tuple(loss["affected_properties"]),
            ),
            f"{view['kind']}.information_loss",
        )
        for loss in view["information_loss"]:
            id_sets.append((loss["source_ids"], f"{view['kind']}.{loss['kind']}.source_ids"))
            assert_canonical_order(
                loss["affected_properties"],
                lambda value: value,
                f"{view['kind']}.{loss['kind']}.affected_properties",
            )
            if loss["meaningful"] is not True:
                fail("report v2 information loss must be meaningful")
            if loss["recoverable"]:
                if loss.get("recovery_ref") not in known_recovery_refs:
                    fail("report v2 recoverable loss has an unresolved recovery_ref")
            elif "recovery_ref" in loss:
                fail("report v2 nonrecoverable loss must omit recovery_ref")
    for values, label in id_sets:
        assert_canonical_order(values, lambda value: value, label)

    authority = result["authority_records"]
    if authority["authority_reconciled"] is not False or any(
        authority[field]
        for field in ("evidence_ids", "verification_ids", "decision_ids", "finding_ids")
    ):
        fail("report v2 must remain authority-free")
    if coverage["verified"] != 0 or coverage["accepted"] != 0:
        fail("report v2 verified/accepted coverage must be zero")
    if any(
        claim["disposition"] != "proposed" or claim["review_status"] != "unreviewed"
        for claim in claims
    ):
        fail("report v2 claims must remain proposed/unreviewed")

    execution_ids = {execution["id"] for execution in executions}
    executions_by_id = {execution["id"]: execution for execution in executions}
    claim_ids = {claim["id"] for claim in claims}
    parsed_claim_ids = {
        claim_id for execution in executions for claim_id in execution["parsed_claim_ids"]
    }
    if claim_ids != parsed_claim_ids:
        fail("report v2 claims are not the complete execution claim union")
    claims_by_execution: dict[str, set[str]] = {}
    for claim in claims:
        if claim["execution_id"] not in execution_ids:
            fail(f"report v2 dangling claim execution: {claim['id']}")
        claims_by_execution.setdefault(claim["execution_id"], set()).add(claim["id"])
        execution = executions_by_id[claim["execution_id"]]
        if claim["event_sequence"] != execution["event_sequence"] or claim["event_id"] != execution["event_id"]:
            fail(f"report v2 claim/execution atomic event mismatch: {claim['id']}")
    for execution in executions:
        exact_claims = claims_by_execution.get(execution["id"], set())
        if set(execution["parsed_claim_ids"]) != exact_claims:
            fail(f"report v2 per-execution claim set mismatch: {execution['id']}")
        if (execution["outcome"]["kind"] == "structured") != bool(exact_claims):
            fail(f"report v2 outcome/claim cardinality mismatch: {execution['id']}")

    registration_ids = {row["registration_id"] for row in registrations}
    referenced_registrations = {
        execution["raw_artifact_registration_id"] for execution in executions
    }
    if registration_ids != referenced_registrations or scenario["artifact_registration_ids"] != sorted(registration_ids):
        fail("report v2 raw registration set is not exact")
    registrations_by_id = {row["registration_id"]: row for row in registrations}
    for execution in executions:
        registration = registrations_by_id[execution["raw_artifact_registration_id"]]
        if (
            registration["run_id"] != metadata["run_id"]
            or registration["source"]["run_id"] != metadata["run_id"]
            or registration["cas_hash"] != execution["raw_artifact_hash"]
            or registration["source"]["execution_id"] != execution["id"]
            or registration["source"]["reviewer_id"] != execution["reviewer_id"]
            or registration["source"]["run_id"] != metadata["run_id"]
            or registration["event_sequence"] >= execution["event_sequence"]
        ):
            fail(f"report v2 raw registration closure mismatch: {execution['id']}")

        identity = {
            key: execution[key]
            for key in (
                "attempt", "envelope_id", "inference_settings", "model", "model_revision",
                "obligation_ids", "plan_id", "prompt_template_version", "provider",
                "reviewer_id", "reviewer_kind", "snapshot_id", "system_prompt_version",
                "tool_policy_version", "wave_id",
            )
        }
        if execution["identity_body_hash"] != sha256_id(identity) or execution["id"] != f"execution:{sha256_id(identity)}":
            fail(f"report v2 execution identity mismatch: {execution['id']}")
        body = {
            key: value
            for key, value in execution.items()
            if key not in {"event_sequence", "event_id", "identity_body_hash", "body_hash"}
        }
        outcome = execution["outcome"]
        for field in ("detail", "diagnostic"):
            if field in outcome:
                require_utf8_bytes(outcome[field], 4_096, f"{execution['id']}.outcome.{field}")
        for field in (
            "reviewer_kind", "reviewer_id", "system_prompt_version",
            "prompt_template_version", "tool_policy_version",
        ):
            require_utf8_bytes(execution[field], 256, f"{execution['id']}.{field}")
        body_bytes = canonical_json_bytes(body)
        if len(body_bytes) > D2_EXECUTION_MAX_CANONICAL_BYTES:
            fail(
                f"Incomplete: execution canonical bytes: limit={D2_EXECUTION_MAX_CANONICAL_BYTES}, "
                f"observed={len(body_bytes)}"
            )
        if execution["body_hash"] != sha256_id(body):
            fail(f"report v2 execution body hash mismatch: {execution['id']}")

        registration_identity = {
            key: registration[key]
            for key in ("run_id", "cas_hash", "media_type", "sensitivity", "source")
        }
        if registration["registration_id"] != f"registration:{sha256_id(registration_identity)}":
            fail(f"report v2 registration identity mismatch: {registration['registration_id']}")

        if execution["outcome"]["kind"] == "abstained":
            raw = {
                "abstention": {
                    "detail": execution["outcome"]["detail"],
                    "reason": execution["outcome"]["reason"],
                },
                "claims": [],
                "execution_id": execution["id"],
                "schema": "reviewgraphen.reviewer_output.v1",
            }
            if registration["size"] != len(canonical_json_bytes(raw)) or registration["cas_hash"] != sha256_id(raw):
                fail(f"report v2 raw artifact hash/size mismatch: {execution['id']}")

    event_sequences = [row["event_sequence"] for row in [*registrations, *executions, *claims]]
    if event_sequences and max(event_sequences) > metadata["confirmed_event_count"]:
        fail("report v2 record sequence exceeds its declared event count")

    selected = set(scenario["selected_obligation_ids"])
    denominator = set(coverage["denominator_obligation_ids"])
    visited = {oid for execution in executions for oid in execution["obligation_ids"]} & selected
    completed = set(coverage["completed_obligation_ids"])
    structured = {
        oid
        for execution in executions
        if execution["outcome"]["kind"] == "structured"
        for oid in execution["obligation_ids"]
    }
    if (
        coverage["universe_id"] != scenario["universe_id"]
        or not selected <= denominator
        or set(coverage["visited_obligation_ids"]) != visited
        or not completed <= structured
        or not completed <= visited
    ):
        fail("report v2 coverage ID sets do not match execution outcomes")
    if (
        coverage["selected"] != len(selected)
        or coverage["visited"] != len(visited)
        or coverage["completed"] != len(completed)
    ):
        fail("report v2 coverage counts do not match exact ID sets")
    status = result["status"]
    if not selected:
        fail("report v2 requires a nonempty selected obligation set")
    if status == "failed":
        fail("report v2 failed status is reserved and has no D2 generation condition")
    if status == "completed":
        if not executions or not claims or completed != selected:
            fail("report v2 completed status lacks exact structured/claim/lifecycle coverage")
    elif status == "partial":
        if not executions or not visited or completed >= selected:
            fail("report v2 partial status requires attempts and incomplete lifecycle coverage")
    elif status == "unsupported_input":
        unsupported_blocks = {
            block for obstruction in obstructions for block in obstruction["blocks"]
        }
        if (
            executions
            or claims
            or registrations
            or scenario["artifact_registration_ids"]
            or visited
            or completed
            or not obstructions
            or any(
                obstruction["kind"] != "pre_review_unsupported_input"
                or not obstruction["source_ids"]
                for obstruction in obstructions
            )
            or unsupported_blocks != selected
        ):
            fail("report v2 unsupported_input lacks its exact pre-review obstruction basis")
    else:
        fail(f"report v2 unknown status: {status}")
    for view in views:
        if view["payload"]["status"] != result["status"]:
            fail("report v2 view status differs from result status")
        if set(view["payload"]["execution_ids"]) != execution_ids:
            fail("report v2 view execution IDs are incomplete")
        if set(view["payload"]["claim_ids"]) != claim_ids:
            fail("report v2 view claim IDs are incomplete")
        if set(view["payload"]["obstruction_kinds"]) != {
            obstruction["kind"] for obstruction in obstructions
        }:
            fail("report v2 view obstruction kinds are incomplete")

    loss_count = checked_u64_sum(
        (len(view["information_loss"]) for view in views),
        "report information-loss rows",
    )
    if loss_count > D2_REPORT_MAX_LOSSES:
        fail(
            f"Incomplete: report information losses: limit={D2_REPORT_MAX_LOSSES}, "
            f"observed={loss_count}"
        )
    rows = checked_u64_sum(
        (
            len(registrations),
            len(executions),
            len(claims),
            len(obstructions),
            len(views),
            loss_count,
        ),
        "report rows",
    )
    # Post-serialization fixture check only. Runtime must enforce the same
    # bound with its bounded writer before allocating the complete output.
    encoded = canonical_json_bytes(report)
    check_report_v2_local_limits(rows, len(encoded))

    if not run_mutations:
        return ["report v2 local ordering/cross-record checks: PASS"]

    schema = load_json(SCHEMAS / "reviewgraphen.report.v2.schema.json")
    validator = Draft202012Validator(schema, format_checker=FormatChecker())

    def schema_must_fail(name: str, mutated: dict[str, Any]) -> None:
        if not list(validator.iter_errors(mutated)):
            fail(f"report v2 schema mutation unexpectedly passed: {name}")

    def contract_must_fail(name: str, mutated: dict[str, Any]) -> None:
        if list(validator.iter_errors(mutated)):
            fail(f"report v2 contract mutation is not schema-valid: {name}")
        try:
            validate_report_v2_contract(mutated, run_mutations=False)
        except AssertionError:
            return
        fail(f"report v2 contract mutation unexpectedly passed: {name}")

    empty_loss = deepcopy(report)
    empty_loss["projection"]["views"][0]["information_loss"] = []
    schema_must_fail("empty information loss", empty_loss)

    duplicate_execution = deepcopy(report)
    duplicate_execution["result"]["executions"].append(deepcopy(executions[0]))
    schema_must_fail("duplicate execution", duplicate_execution)

    authority_nonzero = deepcopy(report)
    authority_nonzero["coverage"]["verified"] = 1
    schema_must_fail("nonzero verified coverage", authority_nonzero)

    omitted_execution = deepcopy(report)
    omitted_execution["result"]["executions"] = []
    schema_must_fail("omitted declared execution", omitted_execution)

    reordered_sources = deepcopy(report)
    reordered_sources["projection"]["views"][0]["source_ids"].reverse()
    contract_must_fail("reordered source IDs", reordered_sources)

    status_mismatch = deepcopy(report)
    status_mismatch["projection"]["views"][0]["payload"]["status"] = "failed"
    contract_must_fail("view/result status mismatch", status_mismatch)

    raw_set_omission = deepcopy(report)
    raw_set_omission["scenario"]["artifact_registration_ids"] = []
    contract_must_fail("raw registration omission", raw_set_omission)

    unsupported = deepcopy(report)
    unsupported["result"]["status"] = "unsupported_input"
    unsupported["result"]["artifact_registrations"] = []
    unsupported["result"]["executions"] = []
    unsupported["result"]["claims"] = []
    unsupported["scenario"]["artifact_registration_ids"] = []
    unsupported["coverage"]["visited_obligation_ids"] = []
    unsupported["coverage"]["completed_obligation_ids"] = []
    unsupported["coverage"]["visited"] = 0
    unsupported["coverage"]["completed"] = 0
    unsupported["result"]["obstructions"] = [
        {
            "kind": "pre_review_unsupported_input",
            "message": "Deterministic context construction refused the selected input before review.",
            "source_ids": [unsupported["scenario"]["program_space_ref"]],
            "blocks": list(unsupported["scenario"]["selected_obligation_ids"]),
        }
    ]
    unsupported_view = unsupported["projection"]["views"][0]
    unsupported_view["source_ids"] = sorted(
        [
            unsupported["scenario"]["program_space_ref"],
            *unsupported["scenario"]["selected_obligation_ids"],
        ]
    )
    unsupported_view["information_loss"] = [
        {
            "kind": "unsupported_input_detail_omitted",
            "reason": "The machine payload retains the typed obstruction kind but omits its full source and block trace.",
            "source_ids": list(unsupported_view["source_ids"]),
            "affected_properties": ["review.unsupported_input_basis"],
            "meaningful": True,
            "recoverable": True,
            "recovery_ref": unsupported["metadata"]["report_id"],
        }
    ]
    unsupported_view["payload"] = {
        "status": "unsupported_input",
        "execution_ids": [],
        "claim_ids": [],
        "obstruction_kinds": ["pre_review_unsupported_input"],
    }
    unsupported_errors = list(validator.iter_errors(unsupported))
    if unsupported_errors:
        fail(f"report v2 valid unsupported_input fixture failed schema: {unsupported_errors[0].message}")
    validate_report_v2_contract(unsupported, run_mutations=False)

    unsupported_with_attempt = deepcopy(unsupported)
    unsupported_with_attempt["result"]["artifact_registrations"] = deepcopy(registrations)
    unsupported_with_attempt["result"]["executions"] = deepcopy(executions)
    unsupported_with_attempt["scenario"]["artifact_registration_ids"] = list(
        scenario["artifact_registration_ids"]
    )
    schema_must_fail("unsupported_input with reviewer attempt", unsupported_with_attempt)

    unsupported_without_basis = deepcopy(unsupported)
    unsupported_without_basis["result"]["obstructions"][0]["kind"] = "reviewer_abstained"
    schema_must_fail("unsupported_input without typed pre-review basis", unsupported_without_basis)

    reserved_failed = deepcopy(report)
    reserved_failed["result"]["status"] = "failed"
    reserved_failed["projection"]["views"][0]["payload"]["status"] = "failed"
    contract_must_fail("reserved failed status", reserved_failed)

    false_completed = deepcopy(report)
    false_completed["result"]["status"] = "completed"
    false_completed["projection"]["views"][0]["payload"]["status"] = "completed"
    schema_must_fail("completed without structured claims/lifecycle", false_completed)

    multibyte_detail = deepcopy(report)
    multibyte_detail["result"]["executions"][0]["outcome"]["detail"] = "界" * 4_096
    contract_must_fail("4096 multibyte characters exceed outcome byte cap", multibyte_detail)

    # This Python gate is an arithmetic/reference oracle only. It does not
    # model Rust allocator capacity and therefore does not claim runtime
    # pre-allocation or the normative J+I+Rr / J+I+R+S+O peaks.
    reference_owned_charge(report)
    if checked_u64_add(U64_MAX - 1, 1, "report reference add") != U64_MAX:
        fail("report v2 checked-add exact boundary failed")
    try:
        checked_u64_add(U64_MAX, 1, "report reference add")
    except AssertionError:
        pass
    else:
        fail("report v2 checked-add overflow unexpectedly passed")
    if checked_u64_mul(U64_MAX, 1, "report reference multiply") != U64_MAX:
        fail("report v2 checked-multiply exact boundary failed")
    try:
        checked_u64_mul(U64_MAX, 2, "report reference multiply")
    except AssertionError:
        pass
    else:
        fail("report v2 checked-multiply overflow unexpectedly passed")

    for constant in ("NaN", "Infinity", "-Infinity"):
        try:
            json.loads(
                f'{{"value":{constant}}}',
                parse_constant=reject_json_constant,
            )
        except AssertionError:
            pass
        else:
            fail(f"report v2 parser accepted non-standard {constant}")
    for nonfinite in (math.nan, math.inf, -math.inf):
        try:
            canonical_json_bytes({"value": nonfinite})
        except ValueError:
            pass
        else:
            fail("report v2 canonical serializer accepted a nonfinite float")
    try:
        reject_nonfinite_numbers(json.loads('{"value":1e400}'))
    except AssertionError:
        pass
    else:
        fail("report v2 parser admitted an overflowing exponent as infinity")
    try:
        validate_d2_float_domain({"value": 1.0000001})
    except AssertionError:
        pass
    else:
        fail("report v2 float domain accepted a value above one")

    return ["report v2 local ordering/status/reference-oracle mutations: PASS"]


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
        validate_report_v2_contract,
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
