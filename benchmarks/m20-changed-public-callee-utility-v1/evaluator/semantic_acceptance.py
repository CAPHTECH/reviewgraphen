"""Hash-bound Wave-5 option-C semantic-acceptance reference."""
from pathlib import Path
from typing import Any

from .canonical import hash_json, sha256_bytes


ALGORITHM_ID = "context.subject_windows.v3.semantic_acceptance.option_c@1"
PRE_ORACLE_FREEZE_MANIFEST_SHA256 = (
    "sha256:95b75a62353b56111fe8913700c72bf9881f93efdc99629e72adb3e2c6a9fa73"
)
WAVE7_SPEC_CONTRACT_MISMATCH_FREEZE_SHA256 = (
    "sha256:eb207a22a41ac6635b68affa1fb8febb123799d89c31ba0a01089794753e1a7e"
)
WAVE8_POSTSEAL_SOURCE_BINDING_FREEZE_SHA256 = (
    "sha256:19014d24b608ee51547f0f3a99c9559583b312b0eeaa848d69441876dd51c1e6"
)
SUPERSEDED_FREEZE_MANIFEST_SHA256 = (
    "sha256:f3af4c7b6ec1e3bec422d25daaa51e9252e01d37537282cfc73de948fa0adcb8"
)
MEASUREMENT_SHA256 = "sha256:af68524b891cdd86169a0789ff386a071694e3976cc35238bec5c164fee26cae"
ALGORITHM_SOURCE_SHA256 = {
    "crates/reviewgraphen-core/src/context_validation_oracle.rs":
        "sha256:4afb9ed6e6c07c3c23367c5bd2e8729d413de0064aacc9324ce2b6f492c52a94",
    "crates/reviewgraphen-core/src/context.rs":
        "sha256:e4e49e2b16e5ee7d626117a97eab498ef04a8a4f2aa6e2cb1ff21e6da5b4ee77",
    "crates/reviewgraphen-runtime/src/generic.rs":
        "sha256:2174b53bcdff80e250652ad6c66015350ccddcefcf1d52d594d8b799df1fd6bd",
    "crates/reviewgraphen-runtime/tests/generic_v3.rs":
        "sha256:7579bf62c92a29459e172abc88aa112a3da9639bc561101cc0be8a1ec4876989",
    "crates/reviewgraphen-runtime/tests/fixtures/real-subject-pairs.v1.json":
        "sha256:8e27d0d8fc763e74515723d1100a748d22edc5356a2f6593575e6988f2948e22",
}
REQUIRED_MUTANTS = [
    "accepted-file-omission",
    "reached-file-omission",
    "literate_access.rs-support-anchor-omission",
    "alternate-support-anchor-omission",
    "oracle-bypass",
    "production-helper-reuse",
]
SEMANTIC_ACCEPTANCE_REFERENCE_SHA256 = (
    "sha256:081298c64a946c722846496fb0d4dfd3c949d21c2700445382f09d447ad9d64d"
)


def reference_body() -> dict[str, Any]:
    """Return the closed reference body bound into the evaluator freeze."""
    return {
        "schema": "m20.context-v3-semantic-acceptance-reference.v1",
        "algorithm_id": ALGORITHM_ID,
        "context_policy_id": "context.subject_windows@3",
        "context_policy_sha256":
            "sha256:932bfa18c5d286c63196366d6d2dc1aaf402f50baa1f1ab5f075b1007be55dd8",
        "oracle_domains": [
            "accepted_file_ids",
            "reached_file_ids",
            "support_anchor_ids",
        ],
        "oracle_independence": (
            "full accepted ProgramSpace artifact/relation/containment traversal; "
            "no production context discovery, reached-set, anchor enumeration, "
            "subject-first, or materialization helper"
        ),
        "production_replay_domains": [
            "materialized_commitments",
            "latent_cardinality",
            "subject_outcomes",
            "windows",
            "admitted_anchor_partition",
            "support_loss_partitions",
            "losses",
            "projection_hash",
            "context_id",
            "canonical_bytes",
        ],
        "algorithm_source_sha256": ALGORITHM_SOURCE_SHA256,
        "measurement_path": (
            "benchmarks/m20-changed-public-callee-utility-v1/"
            "wave18-seal4-measurements.json"
        ),
        "measurement_sha256": MEASUREMENT_SHA256,
        "required_mutants": REQUIRED_MUTANTS,
        "freeze_history": [
            PRE_ORACLE_FREEZE_MANIFEST_SHA256,
            WAVE7_SPEC_CONTRACT_MISMATCH_FREEZE_SHA256,
            WAVE8_POSTSEAL_SOURCE_BINDING_FREEZE_SHA256,
            SUPERSEDED_FREEZE_MANIFEST_SHA256,
        ],
    }


def reference_sha256() -> str:
    """Reject a shadow or partially updated semantic reference."""
    observed = hash_json(reference_body())
    if observed != SEMANTIC_ACCEPTANCE_REFERENCE_SHA256:
        raise ValueError("semantic_acceptance_reference_invalid")
    return observed


def verify_workspace_binding(workspace_root: Path) -> dict[str, Any]:
    """Verify the frozen algorithm and measurement bytes without reading outcomes."""
    reference_sha256()
    for relative, expected in ALGORITHM_SOURCE_SHA256.items():
        path = workspace_root / relative
        if not path.is_file() or sha256_bytes(path.read_bytes()) != expected:
            raise ValueError("semantic_acceptance_source_mismatch")
    measurement = workspace_root / reference_body()["measurement_path"]
    if not measurement.is_file() or sha256_bytes(measurement.read_bytes()) != MEASUREMENT_SHA256:
        raise ValueError("semantic_acceptance_measurement_mismatch")
    return reference_body()
