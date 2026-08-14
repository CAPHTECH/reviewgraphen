#!/usr/bin/env python3
"""Finalize isolated m7-real model runs into an auditable result staging tree."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
from pathlib import Path


def canonical(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(canonical(value))


def sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def run_logged(argv: list[str], stdout_path: Path, stderr_path: Path) -> subprocess.CompletedProcess[bytes]:
    result = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    stdout_path.parent.mkdir(parents=True, exist_ok=True)
    stdout_path.write_bytes(result.stdout)
    stderr_path.write_bytes(result.stderr)
    return result


def trial_slug(manifest: dict[str, object]) -> str:
    arm = "b1" if manifest["arm"] == "b1_free_form" else "g3"
    return f'{manifest["unit_id"]}-{arm}'


def candidate_or_parse_failure(raw: bytes, trial_id: str, binary: Path, validation: Path) -> tuple[dict[str, object], str]:
    reason = "model_output"
    try:
        value = json.loads(raw)
        if not isinstance(value, dict):
            raise ValueError("candidate is not an object")
        probe = validation.with_name(validation.name + ".raw-candidate-probe.json")
        write_json(probe, value)
        checked = run_logged(
            [str(binary), "validate", "candidate", str(probe)],
            validation.with_name(validation.name + ".raw-candidate-validation.json"),
            validation.with_name(validation.name + ".raw-candidate-validation.err"),
        )
        probe.unlink()
        if checked.returncode == 0:
            return value, reason
        reason = "schema_parse_failure"
    except (json.JSONDecodeError, ValueError):
        reason = "json_parse_failure"
    return {
        "schema": "reviewgraphen.benchmark.candidate_output.v1",
        "trial_id": trial_id,
        "outcome": "parse_failure",
        "findings": [],
        "obligation_results": [],
    }, reason


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--prepared", type=Path, required=True)
    parser.add_argument("--runs", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()

    if args.output.exists():
        raise SystemExit(f"output already exists: {args.output}")
    args.output.mkdir(parents=True)
    validation_dir = args.output / "validation"
    inventory = json.loads((args.prepared / "inventory.json").read_bytes())
    manifests_by_id: dict[str, tuple[dict[str, object], Path]] = {}
    for manifest_path in sorted(args.prepared.glob("snapshot-*/**/replicate-1/manifest.json")):
        manifest = json.loads(manifest_path.read_bytes())
        manifests_by_id[str(manifest["trial_id"])] = (manifest, manifest_path)
    if len(manifests_by_id) != len(inventory["trials"]):
        raise SystemExit("prepared manifest count differs from inventory")

    shutil.copy2(args.corpus / "execution-config.json", args.output / "execution-config.json")
    write_json(args.output / "private" / "inventory.json", inventory)
    collections: list[object] = []
    scores: list[object] = []
    public_items: list[dict[str, object]] = []
    reconciliations: list[dict[str, object]] = []
    trial_records: list[dict[str, object]] = []
    agent_input_records: list[dict[str, object]] = []

    for inventory_trial in inventory["trials"]:
        trial_id = str(inventory_trial["trial_id"])
        manifest, manifest_path = manifests_by_id[trial_id]
        slug = trial_slug(manifest)
        relative_trial = manifest_path.parent.relative_to(args.prepared)
        run_dir = args.runs / relative_trial
        raw_path = run_dir / "candidate.raw.json"
        status_path = run_dir / "exit-status"
        if not raw_path.is_file() or not status_path.is_file():
            raise SystemExit(f"missing isolated run output: {relative_trial}")
        raw = raw_path.read_bytes()
        runner_status = int(status_path.read_text().strip())
        copied_manifest = args.output / "manifests" / f"{slug}.json"
        copied_manifest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(manifest_path, copied_manifest)
        agent_input_root = manifest_path.parent / "agent_input"
        agent_input_files = []
        for input_path in sorted(path for path in agent_input_root.rglob("*") if path.is_file()):
            input_bytes = input_path.read_bytes()
            agent_input_files.append({
                "path": input_path.relative_to(agent_input_root).as_posix(),
                "sha256": sha256(input_bytes),
                "size_bytes": len(input_bytes),
            })
        agent_input_records.append({
            "trial_id": trial_id,
            "manifest_sha256": sha256(manifest_path.read_bytes()),
            "files": agent_input_files,
        })
        raw_output = args.output / "raw-responses" / f"{slug}.json"
        raw_output.parent.mkdir(parents=True, exist_ok=True)
        raw_output.write_bytes(raw)
        telemetry_dir = args.output / "telemetry"
        telemetry_dir.mkdir(parents=True, exist_ok=True)
        for source_name, suffix in [("events.jsonl", "events.jsonl"), ("stderr.log", "stderr.log")]:
            source = run_dir / source_name
            if source.is_file():
                shutil.copy2(source, telemetry_dir / f"{slug}.{suffix}")

        validation_base = validation_dir / slug
        candidate, parse_status = candidate_or_parse_failure(
            raw, trial_id, args.binary, validation_base
        )
        candidate_path = args.output / "candidates" / f"{slug}.json"
        write_json(candidate_path, candidate)
        validated = run_logged(
            [str(args.binary), "validate", "candidate", str(candidate_path)],
            validation_dir / f"{slug}.candidate-validation.json",
            validation_dir / f"{slug}.candidate-validation.err",
        )
        if validated.returncode != 0:
            raise SystemExit(f"normalized candidate failed validation: {slug}")

        collection_path = args.output / "collections" / f"{slug}.json"
        collection_path.parent.mkdir(parents=True, exist_ok=True)
        collected = run_logged(
            [str(args.binary), "collect", str(copied_manifest), str(candidate_path), str(collection_path)],
            validation_dir / f"{slug}.collect.json",
            validation_dir / f"{slug}.collect.err",
        )
        if collected.returncode != 0:
            raise SystemExit(f"collection failed: {slug}")
        collection = json.loads(collection_path.read_bytes())
        collections.append(collection)

        oracle_path = args.corpus / "private" / "oracles" / f'{manifest["unit_id"]}.json'
        oracle = json.loads(oracle_path.read_bytes())
        unit_path = args.corpus / "private" / "units" / f'{oracle["benchmark_unit_id"]}.json'
        if collection["outcome"] != "protocol_invalid":
            scored = run_logged(
                [str(args.binary), "score-real", str(copied_manifest), str(candidate_path), str(oracle_path), str(unit_path)],
                validation_dir / f"{slug}.score.json",
                validation_dir / f"{slug}.score.err",
            )
            if scored.returncode != 0:
                raise SystemExit(f"scoring failed: {slug}")
            score = json.loads(scored.stdout)
            score_path = args.output / "private" / "scores" / f"{slug}.json"
            write_json(score_path, score)
            scores.append(score)

            public_path = args.output / "adjudication" / "per-trial-public" / f"{slug}.json"
            private_path = args.output / "adjudication" / "per-trial-private" / f"{slug}.json"
            public_path.parent.mkdir(parents=True, exist_ok=True)
            private_path.parent.mkdir(parents=True, exist_ok=True)
            exported = run_logged(
                [str(args.binary), "export-real-adjudication", str(copied_manifest), str(candidate_path), str(oracle_path), str(unit_path), str(public_path), str(private_path)],
                validation_dir / f"{slug}.adjudication-export.json",
                validation_dir / f"{slug}.adjudication-export.err",
            )
            if exported.returncode != 0:
                raise SystemExit(f"adjudication export failed: {slug}")
            public_items.extend(json.loads(public_path.read_bytes()))
            reconciliations.extend(json.loads(private_path.read_bytes()))

        transcript = run_dir / "stderr.log"
        transcript_bytes = transcript.read_bytes() if transcript.is_file() else b""
        token_match = re.search(rb"tokens used\s*\n([0-9,]+)", transcript_bytes)
        trial_records.append({
            "trial_id": trial_id,
            "runner_exit_status": runner_status,
            "candidate_parse_status": parse_status,
            "raw_response_sha256": sha256(raw),
            "raw_response_size_bytes": len(raw),
            "tool_transcript_sha256": sha256(transcript_bytes),
            "reported_tokens_used": int(token_match.group(1).replace(b",", b"")) if token_match else None,
        })

    write_json(args.output / "collections.json", collections)
    write_json(args.output / "private" / "scores.json", scores)
    summary_result = run_logged(
        [str(args.binary), "summarize-real-run", str(args.output / "private" / "inventory.json"), str(args.output / "collections.json"), str(args.output / "private" / "scores.json")],
        validation_dir / "summary.json",
        validation_dir / "summary.err",
    )
    if summary_result.returncode != 0:
        raise SystemExit("summary failed")
    (args.output / "summary.json").write_bytes(summary_result.stdout.rstrip(b"\n"))

    public_items.sort(key=lambda item: str(item["item_id"]))
    reconciliations.sort(key=lambda item: str(item["item_id"]))
    if len({item["item_id"] for item in public_items}) != len(public_items):
        raise SystemExit("duplicate public adjudication item ID")
    write_json(args.output / "adjudication" / "public" / "items.json", public_items)
    write_json(args.output / "adjudication" / "private" / "reconciliation.json", reconciliations)

    reconciliation_by_id = {item["item_id"]: item for item in reconciliations}
    candidate_by_trial = {
        json.loads(path.read_bytes())["trial_id"]: json.loads(path.read_bytes())
        for path in (args.output / "candidates").glob("*.json")
    }
    for item in public_items:
        item_id = str(item["item_id"])
        mapping = reconciliation_by_id[item_id]
        manifest, manifest_path = manifests_by_id[str(mapping["trial_id"])]
        context_id = item_id.removeprefix("adj:sha256:")
        context_dir = args.output / "adjudication" / "public" / "contexts" / context_id
        write_json(context_dir / "item.json", item)
        source_dir = manifest_path.parent / "agent_input" / "source"
        context_inventory = []
        context_paths = sorted({location["path"] for location in item["finding"]["locations"]})
        for relative_source in context_paths:
            source_path = source_dir / relative_source
            destination = context_dir / "source" / relative_source
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source_path, destination)
            context_inventory.extend(
                entry for entry in manifest["source_inventory"] if entry["path"] == relative_source
            )
        write_json(context_dir / "context-index.json", {
            "schema": "reviewgraphen.benchmark.real_adjudication_context.v1",
            "item_id": item_id,
            "trial_source_bundle_hash": manifest["source_bundle_hash"],
            "context_source_inventory": context_inventory,
        })
        candidate = candidate_by_trial[str(mapping["trial_id"])]
        if not any(finding["local_id"] == mapping["finding_local_id"] for finding in candidate["findings"]):
            raise SystemExit(f"reconciliation finding missing: {item_id}")

    trial_records.sort(key=lambda item: str(item["trial_id"]))
    agent_input_records.sort(key=lambda item: str(item["trial_id"]))
    write_json(args.output / "agent-input-index.json", {
        "schema": "reviewgraphen.benchmark.real_agent_input_index.v1",
        "trials": agent_input_records,
    })
    runner_bytes = (args.corpus / "scripts" / "run_isolated_trial.sh").read_bytes()
    write_json(args.output / "trial-records.json", {
        "schema": "reviewgraphen.benchmark.real_execution_record.v1",
        "corpus": "m7-real-v1",
        "replicate": 1,
        "raw_responses_preserved_without_repair": True,
        "model_revision": "unknown",
        "isolated_runner_sha256": sha256(runner_bytes),
        "trials": trial_records,
    })
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
