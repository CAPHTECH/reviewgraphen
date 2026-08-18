#!/usr/bin/env python3
"""Computes the three agreement statistics defined in
CODEX_CROSS_VALIDATION_AMENDMENT.md section 5, comparing the existing
Claude judge results against a completed codex cross-validation pass.

No selective adoption: every finding is included regardless of agreement.
No blending: this never modifies or recomputes anything from the
existing Claude-judged primary result.

usage: analyze_codex_cross_validation.py <claude-results-dir> <codex-results-dir> <pool-dir>
  claude-results-dir: benchmarks/m7-head-local-v1/diagnostics/final-judge-pool/results
  codex-results-dir:  wherever run_codex_cross_validation.sh copied results
  pool-dir:           the built pool (for truth.json's arm attribution)
"""

from __future__ import annotations

import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

UNITS = ["head-local-00", "head-local-01", "head-local-02", "head-local-04"]


def load_judgments(results_dir: Path, unit_id: str) -> dict[str, dict]:
    record_path = results_dir / unit_id / "process-record.json"
    record = json.loads(record_path.read_bytes())
    out = json.loads(record["raw_response"])
    return {j["finding_id"]: j for j in out["judgments"]}


def main() -> None:
    if len(sys.argv) != 4:
        raise SystemExit(
            "usage: analyze_codex_cross_validation.py <claude-results-dir> "
            "<codex-results-dir> <pool-dir>"
        )
    claude_dir = Path(sys.argv[1])
    codex_dir = Path(sys.argv[2])
    pool_dir = Path(sys.argv[3])

    all_rows = []
    for unit_id in UNITS:
        claude_j = load_judgments(claude_dir, unit_id)
        codex_j = load_judgments(codex_dir, unit_id)
        truth = json.loads((pool_dir / unit_id / "truth.json").read_bytes())
        arm_by_fid = {e["finding_id"]: e["contributing_arms"] for e in truth["entries"]}

        if set(claude_j) != set(codex_j):
            raise SystemExit(
                f"finding_id set mismatch for {unit_id}: "
                f"claude has {len(claude_j)}, codex has {len(codex_j)} -- "
                "this must be investigated before reporting anything, not silently reconciled"
            )

        for fid, arms in arm_by_fid.items():
            all_rows.append({
                "unit_id": unit_id,
                "finding_id": fid,
                "arms": arms,
                "claude_disposition": claude_j[fid]["disposition"],
                "codex_disposition": codex_j[fid]["disposition"],
                "agree": claude_j[fid]["disposition"] == codex_j[fid]["disposition"],
                "claude_notes": claude_j[fid]["quality"]["notes"],
                "codex_notes": codex_j[fid]["quality"]["notes"],
            })

    print(f"Total findings compared: {len(all_rows)}")

    overall_agree = sum(row["agree"] for row in all_rows)
    print(f"\n=== Overall agreement ===")
    print(f"{overall_agree}/{len(all_rows)} ({100 * overall_agree / len(all_rows):.1f}%)")

    print(f"\n=== Per-arm agreement ===")
    by_arm = defaultdict(list)
    for row in all_rows:
        for arm in row["arms"]:
            by_arm[arm].append(row["agree"])
    for arm, agreements in sorted(by_arm.items()):
        agree_count = sum(agreements)
        print(f"{arm}: {agree_count}/{len(agreements)} ({100 * agree_count / len(agreements):.1f}%)")

    print(f"\n=== Disagreements (full detail, none discarded) ===")
    disagreements = [row for row in all_rows if not row["agree"]]
    print(f"{len(disagreements)} of {len(all_rows)} findings")
    for row in disagreements:
        print(f"\n--- {row['unit_id']} / {row['finding_id']} (arms: {row['arms']}) ---")
        print(f"  claude: {row['claude_disposition']}")
        print(f"    notes: {row['claude_notes'][:300]}")
        print(f"  codex:  {row['codex_disposition']}")
        print(f"    notes: {row['codex_notes'][:300]}")

    print(f"\n=== Disposition tally, both judges, both arms ===")
    for judge_key in ("claude_disposition", "codex_disposition"):
        print(f"-- {judge_key} --")
        for arm in ("qwen_skill", "claude_skill"):
            rows_for_arm = [row for row in all_rows if arm in row["arms"]]
            tally = Counter(row[judge_key] for row in rows_for_arm)
            print(f"  {arm}: {dict(tally)}")


if __name__ == "__main__":
    main()
