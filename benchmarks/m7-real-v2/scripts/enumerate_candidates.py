#!/usr/bin/env python3
"""Enumerate M7 v2 candidates without executing a reviewer or a test."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path


FSL = Path("/home/rizumita/github/fsl")
ROOT = Path(__file__).resolve().parents[3]
PRIOR_UNITS = ROOT / "benchmarks" / "m7-real-v1" / "private" / "units"
V1_BUILDER = ROOT / "benchmarks" / "m7-real-v1" / "scripts" / "build_corpus.py"
SPLIT_SALT = b"reviewgraphen.m7-real-v2.split.v1\0"
PRODUCTION_PATH = re.compile(r"^rust/[^/]+/src/.+\.rs$")
INTEGRATION_TEST_PATH = re.compile(r"^rust/([^/]+)/tests/(.+)\.rs$")
TEST_FUNCTION = re.compile(
    r"(?ms)#\s*\[\s*(?:(?:[A-Za-z_][A-Za-z0-9_]*::)*test)(?:\s*\([^]]*\))?\s*\]"
    r"(?:\s*#\s*\[[^]]+\])*\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+"
    r"([A-Za-z_][A-Za-z0-9_]*)\s*\("
)


def git(*args: str, check: bool = True) -> bytes:
    result = subprocess.run(
        ["git", "-C", str(FSL), *args],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if check and result.returncode != 0:
        raise RuntimeError(
            f"git {' '.join(args)} failed: {result.stderr.decode(errors='replace')}"
        )
    return result.stdout


def canonical(value: object) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode()


def sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def blob(revision: str, path: str) -> bytes | None:
    result = subprocess.run(
        ["git", "-C", str(FSL), "show", f"{revision}:{path}"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    return result.stdout if result.returncode == 0 else None


def prior_fixes() -> set[str]:
    fixes: set[str] = set()
    for path in sorted(PRIOR_UNITS.glob("*.json")):
        value = json.loads(path.read_bytes())
        fixes.add(str(value["fix_commit"]).removeprefix("git:"))
    return fixes


def changed_paths(parent: str, fix: str) -> list[tuple[str, str]]:
    rows: list[tuple[str, str]] = []
    for line in git("diff", "--name-status", "--find-renames=0", parent, fix).decode().splitlines():
        fields = line.split("\t")
        if len(fields) == 2:
            rows.append((fields[0], fields[1]))
    return rows


def changed_lines(parent: str, fix: str, paths: list[str]) -> tuple[int, dict[str, int]]:
    if not paths:
        return 0, {}
    selected = set(paths)
    total = 0
    by_path: dict[str, int] = {}
    output = git("diff", "--numstat", "--find-renames=0", parent, fix, "--", *paths).decode()
    for line in output.splitlines():
        added, deleted, path = line.split("\t", 2)
        if path not in selected or added == "-" or deleted == "-":
            continue
        count = int(added) + int(deleted)
        by_path[path] = count
        total += count
    return total, by_path


def hunk_count(parent: str, fix: str, paths: list[str]) -> int:
    if not paths:
        return 0
    patch = git("diff", "--unified=0", "--find-renames=0", parent, fix, "--", *paths)
    return sum(1 for line in patch.splitlines() if line.startswith(b"@@ "))


def package_name(revision: str, crate_dir: str) -> str | None:
    manifest = blob(revision, f"rust/{crate_dir}/Cargo.toml")
    if manifest is None:
        return None
    try:
        value = tomllib.loads(manifest.decode())
    except (UnicodeDecodeError, tomllib.TOMLDecodeError):
        return None
    package = value.get("package")
    return str(package.get("name")) if isinstance(package, dict) and package.get("name") else None


def test_functions(data: bytes | None) -> set[str]:
    if data is None:
        return set()
    return set(TEST_FUNCTION.findall(data.decode(errors="replace")))


def load_projection_module():
    spec = importlib.util.spec_from_file_location("m7_v1_builder", V1_BUILDER)
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load m7 v1 projection implementation")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit(f"output exists: {args.output}")

    projection = load_projection_module()
    excluded = prior_fixes()
    log = git(
        "log",
        "--all",
        "--since=2026-06-11T00:00:00+09:00",
        "--until=2026-08-09T23:59:59+09:00",
        "--no-merges",
        "--date=short",
        "--pretty=format:%H%x00%ad%x00%s%x00",
    ).decode(errors="strict")
    fields = log.split("\0")
    if fields and fields[-1] == "":
        fields.pop()
    if len(fields) % 3:
        raise RuntimeError("unexpected git log framing")

    records: list[dict[str, object]] = []
    frame_counts: dict[str, int] = {}
    for index in range(0, len(fields), 3):
        fix, commit_date, subject = fields[index : index + 3]
        fix = fix.lstrip("\n")
        status = "candidate"
        reasons: list[str] = []
        if not subject.startswith("fix"):
            status = "excluded"
            reasons.append("not_fix_subject")
        if fix in excluded:
            status = "excluded"
            reasons.append("used_by_m7_real_v1")
        ancestry = git("rev-list", "--parents", "-n", "1", fix).decode().split()
        if len(ancestry) != 2:
            continue
        parent = ancestry[1]
        paths = changed_paths(parent, fix)
        production = sorted(
            path
            for _, path in paths
            if PRODUCTION_PATH.match(path)
            and blob(parent, path) is not None
            and blob(fix, path) is not None
        )
        test_paths = sorted(
            path for _, path in paths if INTEGRATION_TEST_PATH.match(path)
        )
        if not production:
            status = "excluded"
            reasons.append("no_parent_and_fix_production_rust_path")
        if not test_paths:
            status = "excluded"
            reasons.append("no_changed_rust_integration_test")

        test_options: list[dict[str, str]] = []
        for path in test_paths:
            match = INTEGRATION_TEST_PATH.match(path)
            assert match is not None
            crate_dir, relative_bin = match.groups()
            package = package_name(fix, crate_dir)
            if package is None or "/" in relative_bin:
                continue
            before = test_functions(blob(parent, path))
            after_data = blob(fix, path)
            for name in sorted(test_functions(after_data) - before):
                test_options.append(
                    {
                        "package": package,
                        "test_bin": relative_bin,
                        "test_name": name,
                        "test_path": path,
                        "test_source_sha256": sha256(after_data or b""),
                    }
                )
        if not test_options:
            status = "excluded"
            reasons.append("no_added_exact_integration_test_function")

        projection_error = None
        projected_bytes = 0
        if production:
            try:
                projected_bytes = sum(
                    len(projection.projected_blob(parent, path)) for path in production
                )
                for path in production:
                    projection.projected_blob(fix, path)
            except (RuntimeError, UnicodeDecodeError) as error:
                projection_error = str(error)
                status = "excluded"
                reasons.append("blind_projection_failed")

        prod_lines, prod_by_path = changed_lines(parent, fix, production)
        test_lines, _ = changed_lines(parent, fix, test_paths)
        crates = sorted({path.split("/", 2)[1] for path in production})
        features = {
            "production_changed_lines": prod_lines,
            "production_file_count": len(production),
            "production_hunk_count": hunk_count(parent, fix, production),
            "production_crate_count": len(crates),
            "production_cross_crate": len(crates) > 1,
            "largest_production_file_changed_line_fraction": (
                max(prod_by_path.values(), default=0) / prod_lines if prod_lines else 0.0
            ),
            "test_changed_lines": test_lines,
            "test_to_production_changed_line_ratio": (
                test_lines / prod_lines if prod_lines else 0.0
            ),
            "projected_production_file_count": len(production),
            "projected_production_bytes": projected_bytes,
        }
        split_hash = hashlib.sha256(SPLIT_SALT + bytes.fromhex(fix)).hexdigest()
        record = {
            "fix_commit": "git:" + fix,
            "parent_commit": "git:" + parent,
            "commit_date": commit_date,
            "subject_private": subject,
            "frame_status": status,
            "exclusion_reasons": sorted(set(reasons)),
            "production_paths": production,
            "test_paths": test_paths,
            "test_options": sorted(
                test_options,
                key=lambda value: (
                    value["test_path"], value["test_bin"], value["test_name"]
                ),
            ),
            "features": features,
            "split_hash": "sha256:" + split_hash,
            "projection_error_private": projection_error,
        }
        records.append(record)
        frame_counts[status] = frame_counts.get(status, 0) + 1

    records.sort(key=lambda value: str(value["fix_commit"]))
    output = {
        "schema": "reviewgraphen.benchmark.m7_real_v2_candidate_frame.v1",
        "source_head": "git:" + git("rev-parse", "HEAD").decode().strip(),
        "source_status_clean": not bool(git("status", "--short")),
        "prior_fix_count": len(excluded),
        "record_count": len(records),
        "frame_counts": frame_counts,
        "records": records,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(canonical(output))


if __name__ == "__main__":
    main()
