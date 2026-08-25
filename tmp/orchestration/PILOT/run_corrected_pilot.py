#!/usr/bin/env python3
"""Contract-corrected, non-preregistered m20 pilot.

The frozen evaluator is imported only from an isolated /tmp workspace supplied
through M20_PILOT_WORKSPACE.  This file never imports or executes the source
benchmark bundle in the ReviewGraphen worktree.
"""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request


ROOT = pathlib.Path("/home/rizumita/workspace/reviewgraphen")
PILOT = ROOT / "tmp/orchestration/PILOT"
OUT = PILOT / "contract-corrected"
ENDPOINT = "http://192.168.68.71:11999"
MODEL = "Qwen3.8-27B-MLX-4bit"
MAX_TOKENS = 32_000
TIMEOUTS = {"low-32k": 1_800, "xhigh-32k": 3_600}
EFFORTS = {"low-32k": "low", "xhigh-32k": "xhigh"}
JUDGE_TIMEOUT = 420
PREFLIGHT_TIMEOUT = 60
REVIEWER_ADAPTER = "m20.pilot-direct-http-reviewer.v2"
JUDGE_ADAPTER = "m20.pilot-codex-blind-judge.v2"
OBLIGATION_SEED = "m20-obligation-selection-v1"

BWRAP = pathlib.Path(
    "/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap"
)
CODEX = pathlib.Path(
    "/home/rizumita/.local/share/mise/installs/codex/0.147.0/bin/codex"
)
CODE_HOST = pathlib.Path(
    "/home/rizumita/.local/share/mise/installs/codex/0.147.0/bin/codex-code-mode-host"
)

PAIRS = {
    "reviewgraphen": {
        "repo": ROOT,
        "base": "a8b6b24d5ed704f53f721b25db42d5d631f946c7",
        "head": "8569a2261e8a62145228872a2fde9f4c48093d00",
        "request": PILOT / "requests/reviewgraphen.v3.json",
        "audit": PILOT / "product/reviewgraphen/audit.run.v3.json",
    },
    "fsl": {
        "repo": pathlib.Path("/home/rizumita/github/fsl"),
        "base": "fbcb62df43d8079ed55ecfe9fc0823eb672babdb",
        "head": "fd5b8c68d2f03f19e7c75c4389e5ea7db6a8f7fe",
        "request": PILOT / "requests/fsl.v3.json",
        "audit": PILOT / "repos/fsl/product-output/audit.run.v3.json",
    },
    "casegraphen": {
        "repo": pathlib.Path("/home/rizumita/github/casegraphen"),
        "base": "9a63d0ad0614035d1309477a2553a770f1d2c94a",
        "head": "56f2ef5d4cc1abe3fa64ddf80a8db79b596fff38",
        "request": PILOT / "requests/casegraphen.v3.json",
        "audit": PILOT / "repos/casegraphen/product-output/audit.run.v3.json",
    },
}


def _isolated_benchmark() -> pathlib.Path:
    workspace = os.environ.get("M20_PILOT_WORKSPACE")
    if not workspace:
        raise RuntimeError("M20_PILOT_WORKSPACE is required")
    root = pathlib.Path(workspace).resolve()
    benchmark = root / "benchmarks/m20-changed-public-callee-utility-v1"
    forbidden = (ROOT / "benchmarks/m20-changed-public-callee-utility-v1").resolve()
    if benchmark.resolve() == forbidden or not benchmark.is_dir():
        raise RuntimeError("isolated evaluator copy is required")
    return benchmark


BENCHMARK = _isolated_benchmark()
sys.path.insert(0, str(BENCHMARK))

from evaluator.artifacts import verify_run  # noqa: E402
from evaluator.canonical import canonical_bytes, hash_json, sha256_bytes, stable_id  # noqa: E402
from evaluator.model_boundary import ModelResult  # noqa: E402
import evaluator.pipeline as frozen_pipeline  # noqa: E402
from evaluator.pipeline import CONTEXT_HASH, RUN  # noqa: E402
from evaluator.repository import GitRepository, PreflightError, valid_path  # noqa: E402
from evaluator.stage0_contract import (  # noqa: E402
    CONTEXT_POLICY_ID,
    PROFILE_HASH,
    build_occurrence_closure,
    validate_context_projection_public,
)
from evaluator.stage0_driver import commit_cluster_id  # noqa: E402


class PilotGitRepository(GitRepository):
    """Frozen adapter with only symlink/gitlink tree entries omitted.

    FSL contains a non-regular tree edge that makes the frozen all-tree walker
    refuse before reaching the Rust production paths used by this pilot.  All
    commit/tree/blob hashes and regular entries retain the frozen checks.
    """

    skipped_tree_edges: list[dict] = []
    baseline_paths_by_root = {
        str(ROOT): {"crates/reviewgraphen-core/src/context.rs"},
        "/home/rizumita/github/fsl": {"rust/fslc/src/outcome.rs"},
        "/home/rizumita/github/casegraphen": {"src/resource_protocol.rs"},
    }

    def tree(self, oid: str) -> dict[str, tuple[str, str]]:
        output: dict[str, tuple[str, str]] = {}

        def visit(tree_oid: str, prefix: str) -> None:
            content, index = self.object(tree_oid, "tree")[1], 0
            local = set()
            while index < len(content):
                space, nul = content.find(b" ", index), content.find(b"\0", index)
                if space <= index or nul <= space or nul + 1 + self.oid_bytes > len(content):
                    raise PreflightError("tree_framing_invalid")
                mode_raw, name_raw = content[index:space], content[space + 1 : nul]
                try:
                    mode, name = mode_raw.decode("ascii"), name_raw.decode("utf-8", "strict")
                except UnicodeError as error:
                    raise PreflightError("tree_edge_invalid") from error
                if not name or "/" in name or name in {".", ".."} or name in local:
                    raise PreflightError("tree_edge_invalid")
                local.add(name)
                child = content[nul + 1 : nul + 1 + self.oid_bytes].hex()
                path = prefix + name
                if not valid_path(path):
                    raise PreflightError("tree_path_invalid")
                if mode == "40000":
                    visit(child, path + "/")
                elif mode in {"100644", "100755"}:
                    if path in output:
                        raise PreflightError("tree_duplicate_path")
                    self.object(child, "blob")
                    output[path] = (mode, child)
                elif mode in {"120000", "160000"}:
                    self.skipped_tree_edges.append(
                        {"repository_root": str(self.root), "tree_oid": tree_oid, "mode": mode, "path": path}
                    )
                else:
                    raise PreflightError("tree_edge_invalid")
                index = nul + 1 + self.oid_bytes

        visit(oid, "")
        return output

    def baseline_specs(self, trees: tuple[dict, dict]) -> list[dict]:
        rows = super().baseline_specs(trees)
        allowed = self.baseline_paths_by_root.get(str(self.root))
        if allowed is None:
            raise PreflightError("pilot_baseline_path_missing")
        return [row for row in rows if row["path"] in allowed]


frozen_pipeline.GitRepository = PilotGitRepository


def json_bytes(value: object) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False
    ).encode("utf-8")


def write_new(path: pathlib.Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as handle:
        handle.write(data)


def read_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def hash_order(identifier: str) -> tuple[bytes, bytes]:
    return (
        hashlib.sha256(OBLIGATION_SEED.encode() + b"\0" + identifier.encode()).digest(),
        identifier.encode(),
    )


def _role_for_window(roles: list[str]) -> tuple[str, str]:
    if "callee" in roles:
        return "changed", "callee"
    if "caller" in roles:
        return "changed", "caller"
    return "support", "support"


def prepare_pair(pair_id: str, config: dict) -> None:
    request = read_json(config["request"])
    audit = read_json(config["audit"])
    base, head = config["base"], config["head"]
    if request["base_revision"] != base or request["target_revision"] != head:
        raise RuntimeError(f"{pair_id}: request commit mismatch")
    applicable = [
        row
        for row in audit["obligation_contract"]
        if row.get("rule_id") == "relation.changed_public_callee@1"
        and row.get("applicability_status") == "applicable"
    ]
    if not applicable:
        raise RuntimeError(f"{pair_id}: no applicable D obligation")
    selected = sorted(applicable, key=lambda row: hash_order(row["id"]))[0]
    contexts = {row["context"]["obligation_id"]: row["context"] for row in audit["contexts"]}
    context = contexts[selected["id"]]
    repository = PilotGitRepository(str(config["repo"]), [str(config["repo"])])
    trees = repository.snapshots(base, head)
    head_tree = trees[1]

    materialized = {row["artifact_id"]: row for row in context["materialized_sources"]}
    endpoint_pairs = [
        {
            "caller_endpoint_id": context["caller_artifact_id"],
            "callee_endpoint_id": context["callee_artifact_id"],
        }
    ]
    sources = []
    windows = []
    required_by_window = {}
    for window in context["windows"]:
        source = materialized[window["source_artifact_id"]]
        path = source["path"]
        if path not in head_tree:
            raise RuntimeError(f"{pair_id}: materialized path absent at head: {path}")
        source_role, projection_role = _role_for_window(window["roles"])
        required_id = stable_id(
            "m20-pilot-required-source",
            {
                "obligation_id": selected["id"],
                "window_id": window["id"],
                "snapshot_side": "head",
                "path": path,
                "start_line": window["range"]["start_line"],
                "end_line": window["range"]["end_line"],
                "role": source_role,
            },
        )
        required_by_window[window["id"]] = required_id
        sources.append(
            {
                "required_id": required_id,
                "role": source_role,
                "snapshot_side": "head",
                "path": path,
                "start_line": window["range"]["start_line"],
                "end_line": window["range"]["end_line"],
                "blob_oid": head_tree[path][1],
            }
        )
        windows.append(
            {
                "window_id": window["id"],
                "source_artifact_id": window["source_artifact_id"],
                "start_line": window["range"]["start_line"],
                "end_line": window["range"]["end_line"],
                "role": projection_role,
                "source_required_id": required_id,
                "support_anchor_ids": sorted(window["support_anchor_ids"], key=str.encode),
            }
        )
    sources.sort(key=lambda row: row["required_id"].encode())
    windows.sort(
        key=lambda row: (
            row["source_artifact_id"].encode(),
            row["start_line"],
            row["end_line"],
            row["window_id"].encode(),
        )
    )
    subject_outcomes = []
    for row in context["subject_outcomes"]:
        if row["state"] != "admitted":
            raise RuntimeError(f"{pair_id}: selected subject not admitted")
        subject_outcomes.append(
            {
                "role": row["role"],
                "status": "admitted",
                "endpoint_id": row["endpoint_id"],
                "source_artifact_id": row["source_artifact_id"],
                "start_line": row["requested_range"]["start_line"],
                "end_line": row["requested_range"]["end_line"],
                "window_id": row["window_id"],
            }
        )
    subject_windows = sorted(
        [
            {
                "subject_id": row["endpoint_id"],
                "window_id": row["window_id"],
                "role": row["role"],
            }
            for row in subject_outcomes
        ],
        key=lambda row: (
            row["subject_id"].encode(), row["window_id"].encode(), row["role"].encode()
        ),
    )
    materialized_projection = []
    for artifact_id, row in materialized.items():
        path = row["path"]
        materialized_projection.append(
            {
                "source_artifact_id": artifact_id,
                "snapshot_side": "head",
                "path": path,
                "blob_oid": head_tree[path][1],
            }
        )
    materialized_projection.sort(key=lambda row: row["source_artifact_id"].encode())
    unknown_ids = sorted(
        [stable_id("m20-pilot-context-unknown", row) for row in context.get("unknowns", [])],
        key=str.encode,
    )
    remaining_source_ids = sorted(
        {source_id for row in context.get("unknowns", []) for source_id in row["source_ids"]},
        key=str.encode,
    )
    projection_body = {
        "policy_id": CONTEXT_POLICY_ID,
        "policy_sha256": CONTEXT_HASH,
        "projection_id": context["context_id"],
        "snapshot_id": context["snapshot_id"],
        "request_id": audit["request_id"],
        "obligation_ids": [selected["id"]],
        "relation_ids": sorted(selected["target_refs"], key=str.encode),
        "endpoint_pairs": endpoint_pairs,
        "accepted_file_denominator": context["accepted_file_denominator"],
        "reached_file_denominator": context["reached_file_denominator"],
        "materialized_source_denominator": context["materialized_source_denominator"],
        "support_anchor_denominator": context["support_anchor_denominator"],
        "latent_cardinality": {
            "state": context["latent_cardinality"]["state"],
            "capability_states": {
                key: value
                for key, value in context["latent_cardinality"]["capability_states"].items()
                if value in {"partial", "unknown"}
            },
            "qualification_ids": context["latent_cardinality"]["qualification_ids"],
        },
        "subject_outcomes": subject_outcomes,
        "materialized_sources": materialized_projection,
        "admitted_windows": windows,
        "support_loss_summaries": [
            {
                "reason": row["reason"],
                "cardinality": row["cardinality"],
                "observed_count": row["observed_count"],
                "sorted_id_set_sha256": row["sorted_anchor_id_set_sha256"],
            }
            for row in context["support_loss_summaries"]
        ],
        "unknown_ids": unknown_ids,
        "remaining_loss_ids": [],
        "remaining_source_ids": remaining_source_ids,
        "source_required_ids": sorted(required_by_window.values(), key=str.encode),
    }
    projection = {**projection_body, "canonical_sha256": hash_json(projection_body)}
    validate_context_projection_public(projection)
    unit_id = commit_cluster_id(request["repository_identity"], base, head)
    obligation = {
        "schema": "m20.frozen_obligation.v1",
        "unit_id": unit_id,
        "rule_id": "relation.changed_public_callee@1",
        "property_id": "rust.callee_contract_review@1",
        "obligation_ids": [selected["id"]],
        "relation_ids": sorted(selected["target_refs"], key=str.encode),
        "endpoint_pairs": endpoint_pairs,
        "subject_windows": subject_windows,
        "sources": sources,
        "required_references": [],
        "projection": projection,
        "bounded_scope_manifest_id": audit["plan"]["universe_id"],
    }
    pair_input = OUT / "inputs" / pair_id
    obligation_path = pair_input / "obligation.json"
    write_new(obligation_path, json_bytes(obligation))
    occurrence = build_occurrence_closure(
        {
            "snapshot_id": context["snapshot_id"],
            "target_revision": head,
            "legacy_program_space_sha256": hash_json(audit["legacy_ingestion"]),
            "legacy_extraction_report_sha256": hash_json(audit["coverage"]),
        },
        [
            {"file_source_id": row["source_artifact_id"], "path": row["path"]}
            for row in materialized_projection
        ],
        [],
        sorted(materialized, key=str.encode),
        {
            "schema": "m20.stage0-occurrence-metrics.v1",
            "cluster_id": unit_id,
            "wall_time_milliseconds": 0,
            "peak_bytes": 0,
        },
    )
    stage = {
        "schema": "m20.stage_manifest.v1",
        "experiment_id": "m20-changed-public-callee-utility-v1",
        "unit_id": unit_id,
        "repository_root": str(config["repo"]),
        "repository_allow_list": [str(config["repo"])],
        "base_commit_oid": base,
        "head_commit_oid": head,
        "frozen_obligation_sha256": sha256_bytes(obligation_path.read_bytes()),
        "profile_id": "rust.production.v1",
        "profile_sha256": PROFILE_HASH,
        "context_policy_id": CONTEXT_POLICY_ID,
        "context_policy_sha256": CONTEXT_HASH,
        "occurrence_closure": occurrence,
        "public_seeds": {
            "arm_order": "m20-arm-order-v1",
            "judge_permutation": "m20-judge-permutation-v1",
        },
        "backend_adapters": {"reviewer": REVIEWER_ADAPTER, "judge": JUDGE_ADAPTER},
    }
    stage_path = pair_input / "stage.json"
    write_new(stage_path, json_bytes(stage))
    launch = {
        "schema": "m20.pipeline_launch.v1",
        "experiment_id": stage["experiment_id"],
        "unit_id": unit_id,
        "repository_root": str(config["repo"]),
        "base_commit_oid": base,
        "head_commit_oid": head,
        "frozen_obligation_path": str(obligation_path),
        "frozen_obligation_sha256": stage["frozen_obligation_sha256"],
        "stage_manifest_path": str(stage_path),
        "stage_manifest_sha256": sha256_bytes(stage_path.read_bytes()),
        "context_policy_id": CONTEXT_POLICY_ID,
        "context_policy_sha256": CONTEXT_HASH,
    }
    write_new(pair_input / "launch.json", json_bytes(launch))
    conversion = {
        "schema": "m20.nonregistered-pilot.input-conversion.v1",
        "pair_id": pair_id,
        "repository_identity": request["repository_identity"],
        "unit_id": unit_id,
        "applicable_obligation_ids": sorted([row["id"] for row in applicable], key=str.encode),
        "selected_obligation_id": selected["id"],
        "selection_seed": OBLIGATION_SEED,
        "context_id": context["context_id"],
        "original_product_projection_hash": context["projection_hash"],
        "pilot_projection_sha256": projection["canonical_sha256"],
        "source_count": len(sources),
        "source_mapping": [
            {
                "required_id": row["required_id"],
                "role": row["role"],
                "snapshot_side": row["snapshot_side"],
                "path": row["path"],
                "start_line": row["start_line"],
                "end_line": row["end_line"],
                "blob_oid": row["blob_oid"],
            }
            for row in sources
        ],
        "source_audit_sha256": sha256_bytes(config["audit"].read_bytes()),
        "request_sha256": sha256_bytes(config["request"].read_bytes()),
        "pilot_baseline_path_allowlist": sorted(
            PilotGitRepository.baseline_paths_by_root[str(config["repo"])], key=str.encode
        ),
        "pilot_git_adapter_skipped_tree_edges": [
            row for row in PilotGitRepository.skipped_tree_edges
            if row["repository_root"] == str(config["repo"])
        ],
    }
    write_new(pair_input / "conversion.json", json_bytes(conversion))


def prepare() -> None:
    if OUT.exists():
        raise RuntimeError("contract-corrected output root already exists")
    OUT.mkdir(parents=True)
    manifest = BENCHMARK / "freeze-manifest.json"
    prereg = BENCHMARK / "preregistration.json"
    write_new(
        OUT / "isolated-bundle.json",
        json_bytes(
            {
                "schema": "m20.nonregistered-pilot.isolated-bundle.v1",
                "workspace": str(BENCHMARK.parents[1]),
                "benchmark": str(BENCHMARK),
                "freeze_manifest_sha256": sha256_bytes(manifest.read_bytes()),
                "evaluator_bundle_sha256": read_json(manifest)["evaluator_bundle_sha256"],
                "preregistration_sha256": sha256_bytes(prereg.read_bytes()),
                "source_bundle_executed": False,
            }
        ),
    )
    for pair_id, config in PAIRS.items():
        prepare_pair(pair_id, config)


def fetch_json(url: str) -> dict:
    with urllib.request.urlopen(url, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def post_chat(payload: dict, timeout_seconds: int) -> tuple[int | None, bytes, str | None]:
    request = urllib.request.Request(
        ENDPOINT + "/v1/chat/completions",
        data=json_bytes(payload),
        headers={"Content-Type": "application/json", "Authorization": "Bearer ollama"},
        method="POST",
    )
    previous = signal.getsignal(signal.SIGALRM)

    def hard_timeout(_signum: int, _frame: object) -> None:
        raise TimeoutError(f"hard wall timeout after {timeout_seconds} seconds")

    signal.signal(signal.SIGALRM, hard_timeout)
    signal.setitimer(signal.ITIMER_REAL, timeout_seconds)
    try:
        try:
            with urllib.request.urlopen(request, timeout=timeout_seconds) as response:
                return response.status, response.read(), None
        except urllib.error.HTTPError as error:
            return error.code, error.read(), f"HTTPError: {error}"
        except Exception as error:
            return None, b"", f"{type(error).__name__}: {error}"
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)


def preflight() -> None:
    listing = fetch_json(ENDPOINT + "/v1/models")
    health = fetch_json(ENDPOINT + "/health")
    payload = {
        "model": MODEL,
        "messages": [{"role": "user", "content": "Reply with OK."}],
        "max_tokens": 8,
        "reasoning_effort": "low",
        "stream": False,
    }
    started = time.monotonic()
    status, raw, error = post_chat(payload, PREFLIGHT_TIMEOUT)
    elapsed = round(time.monotonic() - started, 3)
    response = None
    try:
        response = json.loads(raw)
    except (ValueError, UnicodeDecodeError):
        pass
    choices = bool(isinstance(response, dict) and response.get("choices"))
    record = {
        "schema": "m20.nonregistered-pilot.corrected-preflight.v1",
        "endpoint": ENDPOINT,
        "model": MODEL,
        "model_present": MODEL in [row.get("id") for row in listing.get("data", [])],
        "health_status": health.get("status"),
        "request": payload,
        "http_status": status,
        "transport_error": error,
        "elapsed_seconds": elapsed,
        "choices_present": choices,
        "response": response,
    }
    write_new(OUT / "preflight.json", json_bytes(record))
    if not record["model_present"] or record["health_status"] != "healthy" or not choices:
        raise RuntimeError("preflight failed; no reviewer call sent")


def inspect_inputs() -> None:
    for pair_id, config in PAIRS.items():
        launch = frozen_pipeline._launch(read_json(OUT / "inputs" / pair_id / "launch.json"))
        stage, _ = frozen_pipeline._read_authenticated(
            launch["stage_manifest_path"], launch["stage_manifest_sha256"]
        )
        obligation, _ = frozen_pipeline._read_authenticated(
            launch["frozen_obligation_path"], launch["frozen_obligation_sha256"]
        )
        stage = frozen_pipeline._stage(stage, launch)
        obligation = frozen_pipeline._obligation(obligation, launch["unit_id"])
        repository = PilotGitRepository(launch["repository_root"], stage["repository_allow_list"])
        trees = repository.snapshots(launch["base_commit_oid"], launch["head_commit_oid"])
        baseline_specs = repository.baseline_specs(trees[:2])
        task_id = stable_id(
            "review-task",
            {
                "unit_id": launch["unit_id"],
                "obligation_ids": obligation["obligation_ids"],
                "rule_id": frozen_pipeline.RULE_ID,
                "property_id": frozen_pipeline.PROPERTY_ID,
            },
        )
        arm_ids = [
            stable_id("hidden-arm", {"task_id": task_id, "construction_kind": kind})
            for kind in ("baseline_diff", "subject_windows")
        ]
        baseline_projection = {
            "projection_id": stable_id("baseline-projection", {"unit_id": launch["unit_id"]}),
            "source_required_ids": sorted(
                [row["required_id"] for row in baseline_specs], key=str.encode
            ),
        }
        baseline_projection["canonical_sha256"] = hash_json(baseline_projection)
        arms = [
            frozen_pipeline._arm(
                repository,
                trees[:2],
                baseline_specs,
                [],
                baseline_projection,
                task_id,
                arm_ids[0],
                stable_id("scope", {"task_id": task_id, "arm": "baseline"}),
            ),
            frozen_pipeline._arm(
                repository,
                trees[:2],
                obligation["sources"],
                obligation["required_references"],
                obligation["projection"],
                task_id,
                arm_ids[1],
                obligation["bounded_scope_manifest_id"],
            ),
        ]
        frozen_pipeline._opportunity(arms)
        packets = [frozen_pipeline._packet(arm, task_id) for arm in arms]
        rows = []
        for arm_name, packet, arm in zip(("free-form", "structured"), packets, arms):
            packet_bytes = json_bytes(packet)
            write_new(OUT / "packet-preview" / pair_id / arm_name / "packet.json", packet_bytes)
            rows.append(
                {
                    "arm": arm_name,
                    "packet_schema": packet["schema"],
                    "instruction_sha256": sha256_bytes(packet["instruction"].encode("utf-8")),
                    "response_schema_sha256": hash_json(packet["response_schema"]),
                    "admitted_source_bytes": sum(row["bytes"] for row in arm["sources"]),
                    "packet_bytes": len(packet_bytes),
                    "packet_sha256": sha256_bytes(packet_bytes),
                    "fixed_abstention_instruction_occurrences": packet_bytes.count(
                        b"fixed provider-free abstention"
                    ),
                }
            )
        write_new(
            OUT / "packet-preview" / pair_id / "metrics.json",
            json_bytes(
                {
                    "schema": "m20.nonregistered-pilot.packet-preview.v1",
                    "pair_id": pair_id,
                    "task_id": task_id,
                    "rows": rows,
                }
            ),
        )


def extract_last_balanced_object(text: str) -> tuple[dict | None, int | None, int | None]:
    best = None
    for start, character in enumerate(text):
        if character != "{":
            continue
        depth = 0
        in_string = False
        escaped = False
        for index in range(start, len(text)):
            char = text[index]
            if in_string:
                if escaped:
                    escaped = False
                elif char == "\\":
                    escaped = True
                elif char == '"':
                    in_string = False
                continue
            if char == '"':
                in_string = True
            elif char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    try:
                        value = json.loads(text[start : index + 1])
                    except json.JSONDecodeError:
                        break
                    if isinstance(value, dict) and (best is None or (index + 1, start) > (best[2], best[1])):
                        best = (value, start, index + 1)
                    break
                if depth < 0:
                    break
    return best if best is not None else (None, None, None)


def reviewer_prompt(packet_bytes: bytes) -> str:
    packet = packet_bytes.decode("utf-8", "strict")
    return (
        "Treat the following arm-neutral packet as the complete review task. "
        "Follow its instruction and response_schema exactly. Inspect only its admitted payload text; "
        "do not infer an oracle, expected finding, arm identity, commit message, issue, or later fix. "
        "You have no tools.\n\n"
        + packet
        + "\n\nAnswer ONLY with a single JSON object matching packet.response_schema"
    )


def compatible_output_schema(value: object) -> object:
    if isinstance(value, list):
        return [compatible_output_schema(item) for item in value]
    if not isinstance(value, dict):
        return value
    result = {key: compatible_output_schema(item) for key, item in value.items()}
    if "type" not in result:
        if "const" in result:
            result["type"] = "string" if isinstance(result["const"], str) else "integer"
        elif "enum" in result and result["enum"]:
            result["type"] = "string" if isinstance(result["enum"][0], str) else "integer"
    return result


class PilotTransport:
    def __init__(self, condition: str, pair_id: str):
        self.condition = condition
        self.pair_id = pair_id
        self.root = OUT / "transport" / condition / pair_id
        self.review_count = 0

    def descriptor(self) -> dict:
        return {"reviewer": REVIEWER_ADAPTER, "judge": JUDGE_ADAPTER}

    def review(self, request: bytes, slot: int, frozen_timeout: int) -> ModelResult:
        if slot != self.review_count:
            raise RuntimeError("review slot/order mismatch")
        self.review_count += 1
        call = self.root / f"reviewer-slot-{slot}"
        call.mkdir(parents=True, exist_ok=False)
        prompt = reviewer_prompt(request)
        payload = {
            "model": MODEL,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": MAX_TOKENS,
            "reasoning_effort": EFFORTS[self.condition],
            "stream": False,
        }
        write_new(call / "packet.json", request)
        write_new(call / "prompt.txt", prompt.encode("utf-8"))
        write_new(call / "http-request.json", json_bytes(payload))
        started = time.monotonic()
        status, response_raw, error = post_chat(payload, TIMEOUTS[self.condition])
        elapsed = round(time.monotonic() - started, 3)
        write_new(call / "http-response.bin", response_raw)
        response = None
        message = {}
        finish_reason = None
        content = ""
        reasoning = ""
        usage = {}
        try:
            response = json.loads(response_raw)
            choice = response["choices"][0]
            message = choice.get("message", {})
            finish_reason = choice.get("finish_reason")
            content = message.get("content") or ""
            reasoning = message.get("reasoning_content") or ""
            usage = response.get("usage") or {}
        except (ValueError, KeyError, IndexError, TypeError, UnicodeDecodeError):
            pass
        write_new(call / "content.txt", content.encode("utf-8"))
        write_new(call / "reasoning-content.txt", reasoning.encode("utf-8"))
        extracted, start, end = extract_last_balanced_object(content)
        extracted_bytes = content[start:end].encode("utf-8") if extracted is not None else content.encode("utf-8")
        if extracted is not None:
            write_new(call / "extracted.json", extracted_bytes)
        outside = content[: start or 0] + content[end or len(content) :] if extracted is not None else content
        inline = bool(outside.strip())
        output_tokens = usage.get("completion_tokens")
        transport_error = error
        process_exit = 0 if status == 200 and isinstance(response, dict) else 1
        timed_out = status is None and error is not None and "Timeout" in error
        execution = {
            "schema": "m20.nonregistered-pilot.corrected-reviewer-execution.v1",
            "condition": self.condition,
            "pair_id": self.pair_id,
            "slot": slot,
            "model": MODEL,
            "reasoning_effort": EFFORTS[self.condition],
            "max_tokens": MAX_TOKENS,
            "pilot_timeout_seconds": TIMEOUTS[self.condition],
            "frozen_timeout_argument_seconds": frozen_timeout,
            "retry_count": 0,
            "http_status": status,
            "transport_error": transport_error,
            "elapsed_seconds": elapsed,
            "output_tokens": output_tokens,
            "finish_reason": finish_reason,
            "inline_reasoning": inline,
            "json_extraction_succeeded": extracted is not None,
            "json_extraction_start": start,
            "json_extraction_end": end,
            "content_bytes": len(content.encode("utf-8")),
            "reasoning_content_bytes": len(reasoning.encode("utf-8")),
            "packet_bytes": len(request),
            "packet_sha256": sha256_bytes(request),
            "prompt_bytes": len(prompt.encode("utf-8")),
            "prompt_sha256": sha256_bytes(prompt.encode("utf-8")),
            "response_body_sha256": sha256_bytes(response_raw),
        }
        write_new(call / "execution.json", json_bytes(execution))
        usage_rows = tuple(
            (name, usage[key])
            for name, key in (
                ("input_tokens", "prompt_tokens"),
                ("output_tokens", "completion_tokens"),
                ("cache_tokens", "cache_tokens"),
            )
            if isinstance(usage.get(key), int) and not isinstance(usage.get(key), bool) and usage[key] >= 0
        )
        return ModelResult(
            extracted_bytes if process_exit == 0 else b"",
            process_exit=process_exit,
            timeout=timed_out,
            provider_truncation=finish_reason not in {None, "stop"},
            usage=usage_rows,
        )

    def judge(self, request: bytes, instruction: bytes, frozen_timeout: int) -> ModelResult:
        call = self.root / "judge"
        call.mkdir(parents=True, exist_ok=False)
        prompt = instruction + b"\n" + request
        write_new(call / "formal-prompt.txt", prompt)
        frozen_schema = read_json(BENCHMARK / "evaluator/schemas/judge_batch_output.v1.json")
        compatibility_schema = compatible_output_schema(frozen_schema)
        write_new(call / "output-schema.compat.json", json_bytes(compatibility_schema))
        result = call / "codex-result"
        result.mkdir()
        codex_home = pathlib.Path(tempfile.mkdtemp(prefix="m20-corrected-judge-"))
        os.chmod(codex_home, 0o700)
        shutil.copy2("/home/rizumita/.codex/auth.json", codex_home / "auth.json")
        command = [
            str(BWRAP), "--die-with-parent", "--unshare-pid", "--unshare-ipc", "--unshare-uts",
            "--proc", "/proc", "--dev", "/dev", "--ro-bind", "/usr", "/usr",
            "--ro-bind", "/bin", "/bin", "--ro-bind", "/lib", "/lib",
            "--ro-bind", "/lib64", "/lib64", "--ro-bind", "/etc", "/etc",
            "--dir", "/run", "--dir", "/run/systemd", "--dir", "/run/systemd/resolve",
            "--ro-bind", "/run/systemd/resolve/stub-resolv.conf", "/run/systemd/resolve/stub-resolv.conf",
            "--dir", "/home", "--dir", "/home/codex", "--bind", str(codex_home), "/home/codex/.codex",
            "--ro-bind", str(CODEX), "/codex", "--ro-bind", str(CODE_HOST), "/codex-code-mode-host",
            "--dir", "/workspace", "--ro-bind", str(call / "formal-prompt.txt"), "/workspace/prompt.txt",
            "--ro-bind", str(call / "output-schema.compat.json"), "/workspace/output-schema.json",
            "--bind", str(result), "/output", "--tmpfs", "/tmp", "--chdir", "/workspace",
            "--setenv", "HOME", "/home/codex", "--setenv", "CODEX_HOME", "/home/codex/.codex",
            "/codex", "exec", "--dangerously-bypass-approvals-and-sandbox",
            "--dangerously-bypass-hook-trust", "--ignore-user-config", "--ignore-rules",
            "--ephemeral", "--skip-git-repo-check", "-m", "gpt-5.6-sol",
            "-c", 'model_reasoning_effort="high"', "--output-schema", "/workspace/output-schema.json",
            "-o", "/output/judgment.json", "--json",
            "Read only prompt.txt. Score the two opaque candidates exactly as instructed. Return only the schema-constrained JSON.",
        ]
        started = time.monotonic()
        status = 124
        try:
            with (result / "events.jsonl").open("xb") as stdout, (result / "stderr.log").open("xb") as stderr:
                completed = subprocess.run(
                    command, stdout=stdout, stderr=stderr, timeout=JUDGE_TIMEOUT, check=False
                )
            status = completed.returncode
        except subprocess.TimeoutExpired:
            pass
        finally:
            shutil.rmtree(codex_home)
        elapsed = round(time.monotonic() - started, 3)
        judgment_path = result / "judgment.json"
        raw = judgment_path.read_bytes() if status == 0 and judgment_path.is_file() else b""
        execution = {
            "schema": "m20.nonregistered-pilot.corrected-judge-execution.v1",
            "condition": self.condition,
            "pair_id": self.pair_id,
            "model": "gpt-5.6-sol",
            "reasoning_effort": "high",
            "pilot_timeout_seconds": JUDGE_TIMEOUT,
            "frozen_timeout_argument_seconds": frozen_timeout,
            "retry_count": 0,
            "status": status,
            "elapsed_seconds": elapsed,
            "formal_prompt_bytes": len(prompt),
            "formal_prompt_sha256": sha256_bytes(prompt),
            "raw_bytes": len(raw),
            "raw_sha256": sha256_bytes(raw),
            "compatibility_schema_delta": "type added to const/enum nodes for Codex response-format API only; frozen decoder remains authoritative",
        }
        write_new(call / "execution.json", json_bytes(execution))
        return ModelResult(raw, process_exit=0 if status == 0 else status, timeout=status == 124)


def run_condition(condition: str) -> None:
    if condition not in TIMEOUTS:
        raise RuntimeError("unknown condition")
    (OUT / "runs" / condition).mkdir(parents=True, exist_ok=True)
    for pair_id in PAIRS:
        launch = read_json(OUT / "inputs" / pair_id / "launch.json")
        run_root = OUT / "runs" / condition / pair_id
        result = RUN(launch, PilotTransport(condition, pair_id), run_root)
        write_new(OUT / "runs" / condition / f"{pair_id}.result.json", json_bytes(result))
        verification = verify_run(run_root)
        write_new(OUT / "runs" / condition / f"{pair_id}.verify.json", json_bytes(verification))
        if not verification["ok"]:
            raise RuntimeError(f"{condition}/{pair_id}: verify-run failed")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: run_corrected_pilot.py prepare|inspect|preflight|low-32k|xhigh-32k")
    command = sys.argv[1]
    if command == "prepare":
        prepare()
    elif command == "inspect":
        inspect_inputs()
    elif command == "preflight":
        preflight()
    elif command in TIMEOUTS:
        run_condition(command)
    else:
        raise SystemExit("unknown command")


if __name__ == "__main__":
    main()
