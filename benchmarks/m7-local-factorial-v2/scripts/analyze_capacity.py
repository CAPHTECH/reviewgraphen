#!/usr/bin/env python3
"""Derive a conservative token-capacity audit from frozen v2 artifacts."""

from __future__ import annotations

import json
import math
import pathlib


ROOT = pathlib.Path(__file__).resolve().parents[1]
CONTEXT_TOKENS = 262_144
OUTPUT_TOKENS = 65_536


def read(path: pathlib.Path) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> None:
    plan = read(ROOT / "plan.private.json")
    old = read(
        ROOT
        / "results/schema-probe/stage-1/snapshot-06/b1_free_form/generation-metrics.json"
    )
    resumed = ROOT / "results/schema-probe/stage-1-resumed-server-adjustment/trials"
    full_small = read(resumed / "snapshot-06/full_reviewgraphen/generation-metrics.json")
    b1_large = read(resumed / "snapshot-34/b1_free_form/generation-metrics.json")
    observations = [old, full_small, b1_large]
    ratios = [
        value["admitted_input_bytes"] / value["provider_input_tokens"]
        for value in observations
        if isinstance(value.get("provider_input_tokens"), int)
        and value["provider_input_tokens"] > 0
    ]
    minimum = min(ratios)
    maximum = max(ratios)
    rows = []
    for trial in plan["positive"]["trials"]:
        admitted = trial["admitted_input_bytes"]
        low = math.floor(admitted / maximum)
        high = math.ceil(admitted / minimum)
        rows.append(
            {
                "snapshot_id": trial["snapshot_id"],
                "arm": trial["arm"],
                "admitted_input_bytes": admitted,
                "estimated_provider_input_tokens_low": low,
                "estimated_provider_input_tokens_high": high,
                "input_limit_risk": (
                    "definite"
                    if low > CONTEXT_TOKENS
                    else "possible"
                    if high > CONTEXT_TOKENS
                    else "not_indicated_by_empirical_range"
                ),
                "context_budget_risk_with_frozen_output_limit": (
                    "definite"
                    if low + OUTPUT_TOKENS > CONTEXT_TOKENS
                    else "possible"
                    if high + OUTPUT_TOKENS > CONTEXT_TOKENS
                    else "not_indicated_by_empirical_range"
                ),
            }
        )
    summary = {}
    for arm in ("b1_free_form", "full_reviewgraphen"):
        arm_rows = [row for row in rows if row["arm"] == arm]
        summary[arm] = {
            "units": len(arm_rows),
            "input_limit_definite": sum(
                row["input_limit_risk"] == "definite" for row in arm_rows
            ),
            "input_limit_possible": sum(
                row["input_limit_risk"] == "possible" for row in arm_rows
            ),
            "context_budget_definite": sum(
                row["context_budget_risk_with_frozen_output_limit"] == "definite"
                for row in arm_rows
            ),
            "context_budget_possible": sum(
                row["context_budget_risk_with_frozen_output_limit"] == "possible"
                for row in arm_rows
            ),
        }
    output = {
        "schema": "reviewgraphen.benchmark.m7_local_capacity_audit.v1",
        "experiment": "m7-local-factorial-v2",
        "model_context_window_tokens": CONTEXT_TOKENS,
        "frozen_max_output_tokens": OUTPUT_TOKENS,
        "method": {
            "kind": "empirical_admitted_bytes_per_provider_input_token_interval",
            "observed_bytes_per_token_min": minimum,
            "observed_bytes_per_token_max": maximum,
            "warning": "byte ratios vary by content and arm; rows are risk intervals, not exact tokenization",
            "input_limit_definition": "provider_input_tokens > model_context_window_tokens",
            "context_budget_definition": "provider_input_tokens + frozen_max_output_tokens > model_context_window_tokens",
        },
        "observed_pilot": {
            "snapshot_34_b1": {
                "provider_input_tokens": b1_large["provider_input_tokens"],
                "provider_output_tokens": b1_large["provider_output_tokens"],
                "sum": b1_large["provider_input_tokens"]
                + b1_large["provider_output_tokens"],
                "outcome": "context_budget_exhausted_before_final",
            }
        },
        "summary": summary,
        "trials": rows,
    }
    print(json.dumps(output, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
