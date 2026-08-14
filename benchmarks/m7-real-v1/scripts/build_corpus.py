#!/usr/bin/env python3
"""Rebuild the private/public M7 real-regression corpus from the read-only FSL repo."""

from __future__ import annotations

import hashlib
import json
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

FSL = Path("/home/rizumita/github/fsl")
HERE = Path(__file__).resolve().parents[1]
EVIDENCE = HERE / "private" / "evidence"


@dataclass(frozen=True)
class Selection:
    fix: str
    test_bin: str
    test_name: str
    mechanisms: tuple[str, ...]
    root_path: str
    root_hunk: int


SELECTIONS = (
    Selection("b8060376bc0399d534a642d89fd2754326859575", "issue_726_domain_analyze_lowering_guard", "domain_analyze_rejects_unlowerable_constructs_like_check", ("cross_file_contract",), "rust/fsl-tools/src/domain.rs", 0),
    Selection("92a701418d4466fbf223fb36e6c4a4a6f926d2ba", "domain_compensation_guard", "compensation_rejects_when_only_trigger_event_was_observed", ("state_transition_gap", "check_write_gap"), "rust/fsl-core/src/domain.rs", 0),
    Selection("18e3527906d3d2f8fd33dae19d91fb9f4aa33747", "issue_690_can_precedence_false_green", "rendered_can_expansion_parenthesizes_each_piece", ("cross_file_contract",), "rust/fsl-core/src/domain.rs", 0),
    Selection("693bddb74c402c9e49d110321da2229cd807206b", "issue_663_kernel_projection_owner", "db_check_carries_replayable_evidence_for_an_inconclusive_kernel", ("cross_file_contract",), "rust/fslc/src/main.rs", 0),
    Selection("de79983bee04b97331592e90f29e6ebae75d586f", "issue_681_precedence_policy", "native_bmc_rejects_a_bypass_with_policy_attribution_and_replay", ("state_transition_gap", "check_write_gap"), "rust/fsl-core/src/dialect.rs", 3),
    Selection("6d1964f698941749ec05ea59705305a34b1fd0ed", "issue_640_saga_observation_correlation", "paid_is_not_reachable_without_requesting_the_effect", ("cross_file_effect", "check_write_gap"), "rust/fsl-core/src/domain.rs", 1),
    Selection("ed28f9ad5457f01be4f4e4f999bb2f41c289ee63", "issue_641_domain_check_kernel_warnings", "domain_check_preserves_kernel_warnings", ("cross_file_contract",), "rust/fslc/src/main.rs", 0),
    Selection("4af57389812aef3506cc5034cbb96a8287fed13d", "untagged_hint_names_canonical_tag", "untagged_hint_proposes_the_typed_annotation_and_not_the_string_slot", ("cross_file_contract",), "rust/fslc/src/main.rs", 0),
    Selection("fd5b8c68d2f03f19e7c75c4389e5ea7db6a8f7fe", "issue_600_db_check_folds_kernel_verdict", "db_check_folds_an_inconclusive_kernel_into_its_top_level_verdict", ("cross_file_contract", "state_transition_gap"), "rust/fslc/src/main.rs", 14),
    Selection("1130cf38d6976188c7ae8e7b2a47235cca0c3516", "issue_570_reserved_declaration_names", "a_reserved_state_variable_is_a_check_error", ("check_write_gap",), "rust/fsl-core/src/model.rs", 0),
    Selection("5503e2a3ddd0709f3e133b551be6891e3b778b96", "issue_563_ai_check_project_fields", "every_frozen_reference_field_is_present", ("cross_file_contract",), "rust/fslc/src/main.rs", 0),
    Selection("1bc5cf2f353f942d1daa9f69034f81d69138d059", "issue_554_mutate_exit_status", "a_violated_baseline_exits_one", ("cross_file_effect", "state_transition_gap"), "rust/fslc/src/main.rs", 0),
    Selection("0b59bd38462ea8a4aa78e646a5d0b255019f10dd", "issue_562_ai_project_clause_loc", "a_statistical_clause_reports_its_own_line_and_column", ("cross_file_contract",), "rust/fsl-syntax/src/ai_project.rs", 12),
    Selection("daa20e7f2dbefcaf9bf2d12e3663fc78cddbe1e0", "issue_558_manifest_tsg_vocabulary", "manifest_and_standalone_input_agree_on_the_graph_vocabulary", ("cross_file_contract",), "rust/fslc/src/main.rs", 1),
    Selection("169fc2096ec59fee26020bd17ce39a85da707b02", "issue_542_ai_check_rejects_unparseable_clauses", "ai_check_rejects_an_unparseable_statistical_require_clause", ("check_write_gap",), "rust/fslc/src/frontend_output.rs", 0),
    Selection("d31ce375a119200648166205892f3650a3f16473", "issue_502_reachable_reflexivity", "reachable_is_non_reflexive_for_an_empty_relation_on_both_engines", ("state_transition_gap",), "rust/fsl-runtime/src/lib.rs", 2),
    Selection("fc2be19e9fdd5078634a06010a687c822b78073a", "issue_519_monitor_deterministic_init", "replay_accepts_a_bmc_valid_initial_state_that_leaves_a_component_free", ("state_transition_gap",), "rust/fsl-runtime/src/lib.rs", 3),
    Selection("7f48ef66bcfcdb8a88635d4cc31be5479458de19", "issue_516_domain_cli_extra_args", "domain_replay_rejects_an_unknown_trailing_flag", ("check_write_gap",), "rust/fslc/src/main.rs", 0),
    Selection("1fa07e6ff5e782e40621f67834f5db28f111c96d", "issue_512_refine_map_partial_op", "refine_reports_map_partial_op_for_a_zero_divisor_in_a_correspondence_argument", ("check_write_gap",), "rust/fsl-runtime/src/lib.rs", 1),
    Selection("9a379cba9426eed6168c0a35a20e205a3a260bfc", "issue_486_vacuous_forall_implication", "a_forall_wrapped_implication_is_flagged_vacuous_by_default", ("check_write_gap",), "rust/fsl-runtime/src/lib.rs", 0),
)


def git(*args: str, binary: bool = False) -> str | bytes:
    result = subprocess.run(
        ["git", "-C", str(FSL), *args], check=True, capture_output=True
    ).stdout
    return result if binary else result.decode()


def canonical(value: object) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def sha(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(canonical(value))


def blob(revision: str, path: str) -> bytes:
    return git("show", f"{revision}:{path}", binary=True)  # type: ignore[return-value]


ISSUE_REFERENCE = re.compile(
    r"(?i)\bissue\s*#?\s*[0-9]+|(?<![A-Za-z0-9_])#[0-9]{3,}\b"
)
RUST_TEST_MODULE = re.compile(
    r"(?m)^[ \t]*#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\][ \t]*\r?\n"
    r"(?:[ \t]*#\s*\[[^\n]+\][ \t]*\r?\n)*"
    r"[ \t]*(?:(?:pub(?:\([^)]*\))?)[ \t]+)?mod[ \t]+"
    r"[A-Za-z_][A-Za-z0-9_]*[ \t]*\{"
)
RUST_TEST_ATTRIBUTE = re.compile(
    r"(?m)^\s*#\s*\[\s*(?:cfg\s*\([^]]*\btest\b[^]]*\)|"
    r"(?:[A-Za-z_][A-Za-z0-9_]*::)*test(?:\s*\([^]]*\))?)\s*\]"
)


def rust_test_module_end(text: str, opening_brace: int) -> int:
    """Find a test module's closing brace while ignoring Rust literals/comments."""
    depth = 0
    index = opening_brace
    length = len(text)
    while index < length:
        if text.startswith("//", index):
            newline = text.find("\n", index + 2)
            index = length if newline < 0 else newline + 1
            continue
        if text.startswith("/*", index):
            comment_depth = 1
            index += 2
            while index < length and comment_depth:
                if text.startswith("/*", index):
                    comment_depth += 1
                    index += 2
                elif text.startswith("*/", index):
                    comment_depth -= 1
                    index += 2
                else:
                    index += 1
            if comment_depth:
                raise RuntimeError("unterminated block comment in Rust test module")
            continue
        raw = re.match(r"(?:b|c)?r(#{0,255})\"", text[index:])
        if raw:
            terminator = '"' + raw.group(1)
            end = text.find(terminator, index + raw.end())
            if end < 0:
                raise RuntimeError("unterminated raw string in Rust test module")
            index = end + len(terminator)
            continue
        string_prefix = 2 if text.startswith(("b\"", "c\""), index) else 1
        if text[index] == '"' or string_prefix == 2:
            index += string_prefix
            while index < length:
                if text[index] == "\\":
                    index += 2
                elif text[index] == '"':
                    index += 1
                    break
                else:
                    index += 1
            else:
                raise RuntimeError("unterminated string in Rust test module")
            continue
        if text[index] == "'":
            end = index + 1
            escaped = False
            while end < length and text[end] != "\n":
                if text[end] == "'" and not escaped:
                    index = end + 1
                    break
                escaped = text[end] == "\\" and not escaped
                if text[end] != "\\":
                    escaped = False
                end += 1
            else:
                index += 1
            continue
        if text[index] == "{":
            depth += 1
        elif text[index] == "}":
            depth -= 1
            if depth == 0:
                return index + 1
        index += 1
    raise RuntimeError("unterminated Rust test module")


def redact_rust_test_modules(text: str) -> str:
    """Remove final cfg(test) modules while retaining every source line."""
    while match := RUST_TEST_MODULE.search(text):
        opening_brace = text.find("{", match.start(), match.end())
        end = rust_test_module_end(text, opening_brace)
        if text[end:].strip():
            raise RuntimeError("cfg(test) module is not the final Rust item")
        removed = text[match.start() : end]
        replacement = "".join(
            "// [benchmark test code redacted]"
            + ("\n" if line.endswith("\n") else "")
            for line in removed.splitlines(keepends=True)
        )
        text = text[: match.start()] + replacement + text[end:]
    if RUST_TEST_ATTRIBUTE.search(text):
        raise RuntimeError("blind source projection retained a Rust test attribute")
    return text


def redact_blind_source(data: bytes) -> bytes:
    """Remove tests and issue metadata without changing source line numbers."""
    text = data.decode("utf-8")
    text = redact_rust_test_modules(text)
    lines = text.splitlines(keepends=True)
    projected: list[str] = []
    index = 0
    while index < len(lines):
        stripped = lines[index].lstrip()
        if stripped.startswith("//"):
            end = index + 1
            while end < len(lines) and lines[end].lstrip().startswith("//"):
                end += 1
            block = "".join(lines[index:end])
            if ISSUE_REFERENCE.search(block):
                for line in lines[index:end]:
                    newline = "\n" if line.endswith("\n") else ""
                    leading = line[: len(line) - len(line.lstrip())]
                    marker_match = re.match(r"//[/!]?", line.lstrip())
                    marker = marker_match.group(0) if marker_match else "//"
                    projected.append(
                        f"{leading}{marker} [benchmark issue metadata redacted]{newline}"
                    )
            else:
                projected.extend(lines[index:end])
            index = end
            continue
        line = lines[index]
        if ISSUE_REFERENCE.search(line) and "//" in line:
            prefix, _, comment = line.partition("//")
            newline = "\n" if comment.endswith("\n") else ""
            line = f"{prefix}// [benchmark issue metadata redacted]{newline}"
        projected.append(line)
        index += 1
    text = "".join(projected)

    def redact_block_comment(match: re.Match[str]) -> str:
        value = match.group(0)
        if not ISSUE_REFERENCE.search(value):
            return value
        return "/* [benchmark issue metadata redacted]" + "\n" * value.count("\n") + "*/"

    text = re.sub(r"/\*.*?\*/", redact_block_comment, text, flags=re.DOTALL)
    text = re.sub(
        r"(?i)(\bissue\s*#?\s*)[0-9]+",
        r"\1[redacted]",
        text,
    )
    text = re.sub(r"(?<![A-Za-z0-9_])#[0-9]{3,}\b", "#[redacted]", text)
    text = re.sub(
        r"refs/heads/[A-Za-z0-9._/-]+", "[benchmark branch ref redacted]", text
    )
    projected = text.encode("utf-8")
    if ISSUE_REFERENCE.search(text) or b"refs/heads/" in projected:
        raise RuntimeError("blind source projection retained issue or branch metadata")
    if line_count(projected) != line_count(data):
        raise RuntimeError("blind source projection changed line count")
    return projected


def projected_blob(revision: str, path: str) -> bytes:
    return redact_blind_source(blob(revision, path))


def has_blob(revision: str, path: str) -> bool:
    return subprocess.run(
        ["git", "-C", str(FSL), "cat-file", "-e", f"{revision}:{path}"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    ).returncode == 0


def line_count(data: bytes) -> int:
    return max(1, len(data.splitlines(keepends=True)))


def inventory(revision: str, paths: list[str]) -> list[dict[str, object]]:
    return [
        {
            "path": path,
            "content_hash": sha(projected_blob(revision, path)),
            "line_count": line_count(projected_blob(revision, path)),
        }
        for path in paths
    ]


def production_paths(fix: str) -> list[str]:
    changed = git("diff-tree", "--no-commit-id", "--name-only", "-r", fix).splitlines()
    paths = sorted(path for path in changed if re.match(r"^rust/.+/src/.+\.rs$", path))
    if not paths:
        raise RuntimeError(f"no production path for {fix}")
    parent = git("rev-parse", f"{fix}^").strip()
    return [path for path in paths if has_blob(parent, path) and has_blob(fix, path)]


def changed_ranges(fix: str, path: str, hunk_index: int) -> tuple[tuple[int, int], tuple[int, int]]:
    parent = git("rev-parse", f"{fix}^").strip()
    patch = git("diff", "--unified=0", parent, fix, "--", path)
    hunks: list[dict[str, object]] = []
    current: dict[str, object] | None = None
    for raw in patch.splitlines():
        match = re.match(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@", raw)
        if match:
            old_line, old_count, new_line, new_count = (
                int(value or "1") for value in match.groups()
            )
            current = {
                "old_line": old_line,
                "new_line": new_line,
                "fallback": (old_line, max(old_count, 1), new_line, max(new_count, 1)),
                "deleted": [],
                "added": [],
            }
            hunks.append(current)
            continue
        if current is None or raw.startswith(("---", "+++")):
            continue
        if raw.startswith("-"):
            current["deleted"].append(current["old_line"])
            current["old_line"] += 1
        elif raw.startswith("+"):
            current["added"].append(current["new_line"])
            current["new_line"] += 1
        elif raw.startswith(" "):
            current["old_line"] += 1
            current["new_line"] += 1
    if not 0 <= hunk_index < len(hunks):
        raise RuntimeError(f"missing selected code hunk {hunk_index} for {fix} {path}")
    selected = hunks[hunk_index]
    deleted = selected["deleted"]
    added = selected["added"]
    fallback = selected["fallback"]
    old_total = line_count(blob(parent, path))
    new_total = line_count(blob(fix, path))
    old_start = min(deleted) if deleted else min(max(fallback[0], 1), old_total)
    old_end = max(deleted) if deleted else old_start
    new_start = min(added) if added else min(max(fallback[2], 1), new_total)
    new_end = max(added) if added else new_start
    return (old_start, old_end), (new_start, new_end)


def symbol_at(data: bytes, line: int, path: str) -> str:
    lines = data.decode(errors="replace").splitlines()
    patterns = (
        re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)"),
        re.compile(r"\b(?:struct|enum|trait|impl|mod)\s+([A-Za-z_][A-Za-z0-9_]*)"),
    )
    for text in reversed(lines[:line]):
        for pattern in patterns:
            match = pattern.search(text)
            if match:
                return f"rust::{path}::{match.group(1)}"
    return f"rust::{path}::<module>"


def anchor(revision: str, tree: str, path: str, lines: tuple[int, int], mechanisms: tuple[str, ...], anchor_id: str) -> dict[str, object]:
    data = projected_blob(revision, path)
    split = data.splitlines(keepends=True)
    start, end = lines
    span = b"".join(split[start - 1 : end])
    return {
        "anchor_id": anchor_id,
        "tree_hash": "git:" + tree,
        "path": path,
        "file_sha256": sha(data),
        "symbol": symbol_at(data, start, path),
        "start_line": start,
        "end_line": end,
        "span_sha256": sha(span),
        "mechanism_tags": list(mechanisms),
        "origin": "code_fix_hunk",
    }


def run_evidence(selection: Selection, benchmark_unit_id: str, parent: str, parent_tree: str, fix_tree: str) -> dict[str, object]:
    source_dir = EVIDENCE / benchmark_unit_id
    run_index = json.loads((EVIDENCE / "index.json").read_text())
    records = {
        record["benchmark_unit_id"]: record for record in run_index["runs"]
    }
    record = records.get(benchmark_unit_id)
    if record is None:
        raise RuntimeError(f"missing presence-run index entry for {benchmark_unit_id}")
    if (
        record["fix_commit"] != "git:" + selection.fix
        or record["parent_commit"] != "git:" + parent
        or record["test_selector"] != f"{selection.test_bin}::{selection.test_name}"
        or record["fix_exit_status"] != 0
        or record["parent_exit_status"] == 0
    ):
        raise RuntimeError(f"presence evidence failed for {selection.fix}")
    private_dir = HERE / "private" / "evidence" / benchmark_unit_id
    private_dir.mkdir(parents=True, exist_ok=True)
    hashes: dict[str, str] = {}
    for role in ("parent", "fix"):
        stdout = (source_dir / f"{role}.stdout").read_bytes()
        stderr = (source_dir / f"{role}.stderr").read_bytes()
        combined = b"STDOUT\n" + stdout + b"STDERR\n" + stderr
        (private_dir / f"{role}.stdout").write_bytes(stdout)
        (private_dir / f"{role}.stderr").write_bytes(stderr)
        (private_dir / f"{role}.combined").write_bytes(combined)
        hashes[f"{role}_stdout"] = sha(stdout)
        hashes[f"{role}_stderr"] = sha(stderr)
        hashes[f"{role}_combined"] = sha(combined)
    command = ["cargo", "test", "--quiet", "--manifest-path", "rust/Cargo.toml", "-p", "fslc-rust", "--test", selection.test_bin, selection.test_name, "--", "--exact"]
    test_source = blob(selection.fix, f"rust/fslc/tests/{selection.test_bin}.rs")
    if (
        record["command"] != command
        or record["working_directory"] != "repository"
        or record["test_source_sha256"] != sha(test_source)
        or any(record[f"{role}_{stream}_sha256"] != hashes[f"{role}_{stream}"] for role in ("parent", "fix") for stream in ("stdout", "stderr"))
        or any(record[f"{role}_combined_artifact_sha256"] != hashes[f"{role}_combined"] for role in ("parent", "fix"))
    ):
        raise RuntimeError(f"presence artifact binding failed for {selection.fix}")
    def execution(commit: str, tree: str, role: str, status: int) -> dict[str, object]:
        return {
            "commit_hash": "git:" + commit,
            "tree_hash": "git:" + tree,
            "command": command,
            "working_directory": "repository",
            "exit_status": status,
            "stdout_sha256": hashes[f"{role}_stdout"],
            "stderr_sha256": hashes[f"{role}_stderr"],
            "combined_artifact_sha256": hashes[f"{role}_combined"],
        }
    return {
        "schema": "reviewgraphen.benchmark.regression_presence_evidence.v1",
        "benchmark_unit_id": benchmark_unit_id,
        "test_selector": f"{selection.test_bin}::{selection.test_name}",
        "test_source_sha256": sha(test_source),
        "strategy": "fix_regression_test_backported_to_parent",
        "parent_run": execution(parent, parent_tree, "parent", record["parent_exit_status"]),
        "fix_run": execution(selection.fix, fix_tree, "fix", record["fix_exit_status"]),
    }


def main() -> None:
    for child in (HERE / "public", HERE / "private" / "units", HERE / "private" / "oracles"):
        if child.exists():
            shutil.rmtree(child)
    selection_path = HERE / "private" / "selection.json"
    if selection_path.exists():
        selection_path.unlink()
    metadata: list[dict[str, object]] = []
    for index, selection in enumerate(SELECTIONS, 1):
        benchmark_id = f"real-unit-{index:02d}"
        positive_id = f"snapshot-{index * 2 - 1:02d}"
        control_id = f"snapshot-{index * 2:02d}"
        target_id = f"target-{index:02d}"
        parent = git("rev-parse", f"{selection.fix}^").strip()
        parent_tree = git("rev-parse", f"{parent}^{{tree}}").strip()
        fix_tree = git("rev-parse", f"{selection.fix}^{{tree}}").strip()
        paths = production_paths(selection.fix)
        positive_inventory = inventory(parent, paths)
        control_inventory = inventory(selection.fix, paths)
        presence = run_evidence(selection, benchmark_id, parent, parent_tree, fix_tree)
        unit = {
            "schema": "reviewgraphen.benchmark.real_unit.v1",
            "corpus_semantics": "regression_fix_pair",
            "snapshot_semantics": "parent_positive_fix_control",
            "reviewer_input_semantics": "selected_production_snapshot",
            "control_finding_semantics": "unlabeled_requires_adjudication",
            "projection_policy_version": "m7-real-production-paths-blind-redacted.v1",
            "selected_production_paths": paths,
            "positive_source_inventory_hash": sha(canonical(positive_inventory)),
            "control_source_inventory_hash": sha(canonical(control_inventory)),
            "benchmark_unit_id": benchmark_id,
            "target_id": target_id,
            "positive_trial_unit_id": positive_id,
            "control_trial_unit_id": control_id,
            "parent_commit": "git:" + parent,
            "fix_commit": "git:" + selection.fix,
            "positive_tree_hash": "git:" + parent_tree,
            "control_tree_hash": "git:" + fix_tree,
            "mechanism_ontology_version": "reviewgraphen.benchmark.mechanism_ontology.v1",
            "presence_evidence": presence,
        }
        write_json(HERE / "private" / "units" / f"{benchmark_id}.json", unit)
        for trial_id, revision, tree in (
            (positive_id, parent, parent_tree),
            (control_id, selection.fix, fix_tree),
        ):
            packet = {"unit_id": trial_id, "input_tree_hash": "git:" + tree, "files": paths, "language": "rust"}
            write_json(HERE / "public" / trial_id / "packet.json", packet)
            for path in paths:
                destination = HERE / "public" / trial_id / "snapshot" / path
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(projected_blob(revision, path))
        if selection.root_path not in paths:
            raise RuntimeError(f"selected root path is outside production projection: {selection.root_path}")
        root_path = selection.root_path
        positive_range, control_range = changed_ranges(
            selection.fix, root_path, selection.root_hunk
        )
        metadata.append({
            "benchmark_unit_id": benchmark_id,
            "commit_date": git("show", "-s", "--format=%cs", selection.fix).strip(),
            "fix_commit": "git:" + selection.fix,
            "parent_commit": "git:" + parent,
            "test_selector": f"{selection.test_bin}::{selection.test_name}",
            "mechanism_ids": list(selection.mechanisms),
            "root_path": root_path,
            "root_hunk_index": selection.root_hunk,
            "positive_range": list(positive_range),
            "control_range": list(control_range),
        })
    write_json(HERE / "private" / "selection.json", {
        "schema": "reviewgraphen.benchmark.real_selection.v1",
        "source_repository": "git:/home/rizumita/github/fsl",
        "units": metadata,
    })


def finalize_oracles(prepared: Path) -> None:
    selection = json.loads((HERE / "private" / "selection.json").read_text())
    for item in selection["units"]:
        benchmark_id = item["benchmark_unit_id"]
        unit = json.loads((HERE / "private" / "units" / f"{benchmark_id}.json").read_text())
        unit_hash = sha(canonical(unit))
        presence_hash = sha(canonical(unit["presence_evidence"]))
        for role, trial_id, revision, tree, line_key in (
            ("positive_defect_present", unit["positive_trial_unit_id"], unit["parent_commit"][4:], unit["positive_tree_hash"][4:], "positive_range"),
            ("matched_fix_control", unit["control_trial_unit_id"], unit["fix_commit"][4:], unit["control_tree_hash"][4:], "control_range"),
        ):
            manifest_path = prepared / trial_id / "b1" / "replicate-1" / "manifest.json"
            manifest = json.loads(manifest_path.read_text())
            scope = anchor(
                revision,
                tree,
                item["root_path"],
                tuple(item[line_key]),
                tuple(item["mechanism_ids"]),
                f"scope-{benchmark_id}-{role}",
            )
            roots: list[dict[str, object]] = []
            if role == "positive_defect_present":
                root = dict(scope)
                root.pop("anchor_id")
                root.pop("origin")
                root["root_id"] = f"root-{benchmark_id}"
                root["severity"] = "high"
                roots.append(root)
            oracle = {
                "schema": "reviewgraphen.benchmark.real_oracle.v1",
                "benchmark_unit_id": benchmark_id,
                "trial_unit_id": trial_id,
                "target_id": unit["target_id"],
                "revision_role": role,
                "target_expectation": "present" if role == "positive_defect_present" else "absent",
                "control_finding_semantics": "unlabeled_requires_adjudication",
                "real_unit_hash": unit_hash,
                "presence_evidence_hash": presence_hash,
                "input_tree_hash": "git:" + tree,
                "source_bundle_hash": manifest["source_bundle_hash"],
                "target_roots": roots,
                "target_scope_anchors": [scope],
            }
            write_json(HERE / "private" / "oracles" / f"{trial_id}.json", oracle)


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--prepared":
        finalize_oracles(Path(sys.argv[2]).resolve())
    elif len(sys.argv) == 1:
        main()
    else:
        raise SystemExit("usage: build_corpus.py [--prepared PREPARED_DIR]")
