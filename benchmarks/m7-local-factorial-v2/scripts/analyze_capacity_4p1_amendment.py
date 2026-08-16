#!/usr/bin/env python3
"""Add the operator-requested 4.1 bytes/token view without replacing the interval audit."""

from __future__ import annotations

import json
import math
import pathlib


ROOT = pathlib.Path(__file__).resolve().parents[1]
POINT_BYTES_PER_TOKEN = 4.1
CONTEXT_TOKENS = 262_144


def read(path: pathlib.Path) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> None:
    plan = read(ROOT / "plan.private.json")
    existing = read(ROOT / "capacity-audit.private.json")
    rows = []
    for trial in plan["positive"]["trials"]:
        if trial["arm"] != "b1_free_form":
            continue
        admitted = trial["admitted_input_bytes"]
        estimate = math.ceil(admitted / POINT_BYTES_PER_TOKEN)
        rows.append(
            {
                "snapshot_id": trial["snapshot_id"],
                "admitted_input_bytes": admitted,
                "estimated_input_tokens_at_4p1_bytes_per_token": estimate,
                "point_estimate_exceeds_262144": estimate > CONTEXT_TOKENS,
            }
        )

    actual_paths = [
        ROOT / "results/schema-probe/stage-1/snapshot-06/b1_free_form/generation-metrics.json",
        ROOT
        / "results/schema-probe/stage-1-resumed-server-adjustment/trials/snapshot-06/full_reviewgraphen/generation-metrics.json",
        ROOT
        / "results/schema-probe/stage-1-resumed-server-adjustment/trials/snapshot-34/b1_free_form/generation-metrics.json",
    ]
    actual = []
    for path in actual_paths:
        value = read(path)
        actual.append(
            {
                "source": str(path.relative_to(ROOT)),
                "admitted_input_bytes": value["admitted_input_bytes"],
                "provider_input_tokens": value["provider_input_tokens"],
                "observed_bytes_per_provider_input_token": value["admitted_input_bytes"]
                / value["provider_input_tokens"],
            }
        )

    print(
        json.dumps(
            {
                "schema": "reviewgraphen.benchmark.m7_local_capacity_point_amendment.v1",
                "experiment": "m7-local-factorial-v2",
                "relationship": "additive_view_does_not_replace_capacity-audit.private.json",
                "point_estimate": {
                    "bytes_per_token": POINT_BYTES_PER_TOKEN,
                    "basis": "operator-requested rounded snapshot-34 B1 observed ratio",
                    "warning": "admitted bytes omit fixed request overhead and tokenization varies by content and arm; this point estimate is not an exact tokenizer result",
                },
                "empirical_interval_from_existing_audit": existing["method"],
                "actual_completed_pilot_requests": actual,
                "summary": {
                    "b1_positive_units": len(rows),
                    "point_estimate_over_input_limit": sum(
                        row["point_estimate_exceeds_262144"] for row in rows
                    ),
                },
                "b1_positive_units": rows,
            },
            indent=2,
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
