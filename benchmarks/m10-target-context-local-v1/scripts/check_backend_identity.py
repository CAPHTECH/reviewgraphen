#!/usr/bin/env python3
"""Gates on BOTH the `/v1/models` listing and `/health`. Either mismatch stops.

Why two, not one:

- The **listing** caught the weights change on 2026-08-19 (LM Studio ->
  mlx-dspark, four models -> one, quantization unstated -> explicitly 4-bit).
- It did **not** catch the 2026-08-20 decode-path change. `mode` went
  `dspark` -> `baseline` and `drafter` -> `null` while `id`, `owned_by` and
  `target` were all unchanged. The listing hash did catch it only because
  this backend happens to expose `x_mlx_dspark` inside the listing; a server
  that did not would have shown nothing.
- `/health` reports the decode path directly: `mode`, `drafter`,
  `max_draft`, `reasoning_effort`, `context_window`, `max_output_tokens`.
  Those are the parameters every timing figure in this program depends on,
  and none of them appears in `response.model` -- the field the original pin
  asserted on, which passed through an entire backend swap.

Both are control-endpoint reads, not generation requests. A responsive
control endpoint still does not mean generation is healthy.

usage: check_backend_identity.py <expected-listing-sha256> <expected-health-sha256> <output-json>
"""

from __future__ import annotations

import hashlib
import json
import sys
import urllib.request
from pathlib import Path

MODELS_URL = "http://192.168.68.71:11999/v1/models"
HEALTH_URL = "http://192.168.68.71:11999/health"

# Volatile fields are excluded from both hashes: `created` is a timestamp,
# and `status` reports liveness rather than configuration.
HEALTH_FIELDS = (
    "model",
    "mode",
    "target",
    "drafter",
    "max_draft",
    "lookup_drafts",
    "confidence_threshold",
    "reasoning_effort",
    "supports_reasoning_effort",
    "context_window",
    "max_output_tokens",
)


def fetch(url: str) -> dict:
    with urllib.request.urlopen(url, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def listing_identity(listing: dict) -> tuple[str, str]:
    entries = []
    for entry in sorted(listing.get("data", []), key=lambda e: e.get("id", "")):
        extra = entry.get("x_mlx_dspark") or {}
        entries.append(
            {
                "id": entry.get("id"),
                "owned_by": entry.get("owned_by"),
                "mode": extra.get("mode"),
                "target": extra.get("target"),
                "drafter": extra.get("drafter"),
            }
        )
    canonical = json.dumps(entries, sort_keys=True, separators=(",", ":"))
    return canonical, hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def health_identity(health: dict) -> tuple[str, str]:
    picked = {k: health.get(k) for k in HEALTH_FIELDS}
    canonical = json.dumps(picked, sort_keys=True, separators=(",", ":"))
    return canonical, hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def main() -> None:
    if len(sys.argv) != 4:
        raise SystemExit(
            "usage: check_backend_identity.py <expected-listing-sha256> "
            "<expected-health-sha256> <output-json>"
        )
    expected_listing = sys.argv[1].strip()
    expected_health = sys.argv[2].strip()
    output = Path(sys.argv[3])

    try:
        listing = fetch(MODELS_URL)
        health = fetch(HEALTH_URL)
    except Exception as error:  # noqa: BLE001
        output.write_text(
            json.dumps({"error": f"{type(error).__name__}: {error}", "gate": "failed"}, indent=2)
            + "\n",
            encoding="utf-8",
        )
        print(f"BACKEND IDENTITY GATE: control endpoint unreachable: {error}", file=sys.stderr)
        raise SystemExit(2)

    lc, lh = listing_identity(listing)
    hc, hh = health_identity(health)
    listing_ok = lh == expected_listing
    health_ok = hh == expected_health

    record = {
        "schema": "reviewgraphen.benchmark.backend_identity.v2",
        "listing": listing,
        "health": health,
        "listing_canonical": lc,
        "listing_sha256": lh,
        "listing_expected": expected_listing,
        "listing_gate": "passed" if listing_ok else "failed",
        "health_canonical": hc,
        "health_sha256": hh,
        "health_expected": expected_health,
        "health_gate": "passed" if health_ok else "failed",
        "gate": "passed" if (listing_ok and health_ok) else "failed",
    }
    output.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if not (listing_ok and health_ok):
        if not listing_ok:
            print(
                f"LISTING GATE FAILED\n  expected {expected_listing}\n  observed {lh}\n  canonical {lc}",
                file=sys.stderr,
            )
        if not health_ok:
            print(
                f"HEALTH GATE FAILED\n  expected {expected_health}\n  observed {hh}\n  canonical {hc}",
                file=sys.stderr,
            )
        raise SystemExit(3)
    print(f"backend identity verified: listing {lh[:16]}… health {hh[:16]}…")


if __name__ == "__main__":
    main()
