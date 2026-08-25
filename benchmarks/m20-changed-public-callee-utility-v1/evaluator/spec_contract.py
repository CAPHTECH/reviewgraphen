"""Executable agreement between the normative freeze literals and implementation."""
from pathlib import Path
from typing import Any

from .canonical import hash_json, parse_json_bytes


EXECUTION_PREIMAGE_KEYS = (
    "schema",
    "evaluator_bundle_sha256",
    "runtime_requirements_sha256",
    "semantic_acceptance_reference_sha256",
)
FREEZE_MANIFEST_KEYS = (
    "schema",
    "evaluator_version",
    "design_spec_sha256",
    "files",
    "evaluator_bundle_sha256",
    "runtime_requirements",
    "runtime_requirements_sha256",
    "semantic_acceptance_reference_sha256",
    "measurement_record_sha256",
    "supersedes_freeze_manifest_sha256",
    "evaluator_execution_sha256",
    "runtime_provenance",
    "generated_fixture_inventory_sha256",
    "reference_vector_set_sha256",
    "mutation_manifest_sha256",
)
EXECUTION_KEYS_PREFIX = "M20_EXECUTION_PREIMAGE_KEYS_JSON="
MANIFEST_KEYS_PREFIX = "M20_FREEZE_MANIFEST_KEYS_JSON="


def execution_preimage(
    evaluator_bundle_sha256: str,
    runtime_requirements_sha256: str,
    semantic_acceptance_reference_sha256: str,
) -> dict[str, str]:
    return {
        "schema": "m20.evaluator_execution.v1",
        "evaluator_bundle_sha256": evaluator_bundle_sha256,
        "runtime_requirements_sha256": runtime_requirements_sha256,
        "semantic_acceptance_reference_sha256": semantic_acceptance_reference_sha256,
    }


def execution_hash(
    evaluator_bundle_sha256: str,
    runtime_requirements_sha256: str,
    semantic_acceptance_reference_sha256: str,
) -> str:
    return hash_json(
        execution_preimage(
            evaluator_bundle_sha256,
            runtime_requirements_sha256,
            semantic_acceptance_reference_sha256,
        )
    )


def _one_literal(text: str, prefix: str) -> Any:
    rows = [line[len(prefix):] for line in text.splitlines() if line.startswith(prefix)]
    if len(rows) != 1:
        raise ValueError("spec_contract_literal_missing")
    return parse_json_bytes(rows[0].encode("utf-8"))


def verify_spec_contract(design_spec: Path) -> dict[str, Any]:
    """Reject documentation/implementation drift before any manifest can seal."""
    text = design_spec.read_text(encoding="utf-8")
    execution_keys = _one_literal(text, EXECUTION_KEYS_PREFIX)
    manifest_keys = _one_literal(text, MANIFEST_KEYS_PREFIX)
    if execution_keys != list(EXECUTION_PREIMAGE_KEYS):
        raise ValueError("spec_execution_formula_mismatch")
    if manifest_keys != list(FREEZE_MANIFEST_KEYS):
        raise ValueError("spec_manifest_keys_mismatch")
    probe = execution_preimage("bundle", "runtime", "semantic")
    if list(probe) != list(EXECUTION_PREIMAGE_KEYS):
        raise ValueError("implementation_execution_formula_mismatch")
    return {
        "execution_preimage_keys": execution_keys,
        "freeze_manifest_keys": manifest_keys,
    }
