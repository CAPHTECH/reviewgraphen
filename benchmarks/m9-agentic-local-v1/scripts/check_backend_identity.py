#!/usr/bin/env python3
"""Gates on the `/v1/models` listing hash. A mismatch is a hard stop.

This replaces asserting on `response.model`. That field is the server's
Claim about itself: the current backend echoes whatever id the request sent,
so `qwen3.8-27b-mlx` came back verbatim while `/v1/models` reported
`Qwen3.8-27B-MLX-4bit`. The old pin passed through an entire backend swap.

The listing is the signal that actually detected the swap, and the previous
harness already captured it per request -- and then asserted on the echoed
field instead. Recording without gating is what let a detectable change go
undetected, so this gate exists.

Hashed identity fields, `created` deliberately excluded because it is a
volatile timestamp:

    id, owned_by, x_mlx_dspark.mode, .target, .drafter

`.target` and `.drafter` are the loaded weights and the speculative-decoding
drafter. This backend volunteers both, which is strictly more identity
information than the previous one gave -- the old default id `qwen3.8-27b-mlx`
never stated which weights it resolved to, which is why that question is
permanently unanswerable for every earlier result.

This is a control-endpoint read, not a generation request. A responsive
control endpoint still does not mean generation is healthy.

usage: check_backend_identity.py <expected-sha256> <output-json>
"""

from __future__ import annotations

import hashlib
import json
import sys
import urllib.request
from pathlib import Path

MODELS_URL = "http://192.168.68.71:11999/v1/models"


def identity(listing: dict) -> tuple[str, str]:
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


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: check_backend_identity.py <expected-sha256> <output-json>")
    expected = sys.argv[1].strip()
    output = Path(sys.argv[2])

    try:
        with urllib.request.urlopen(MODELS_URL, timeout=30) as response:
            listing = json.loads(response.read().decode("utf-8"))
    except Exception as error:  # noqa: BLE001
        output.write_text(
            json.dumps(
                {"error": f"{type(error).__name__}: {error}", "gate": "failed"}, indent=2
            )
            + "\n",
            encoding="utf-8",
        )
        print(f"BACKEND IDENTITY GATE: could not read {MODELS_URL}: {error}", file=sys.stderr)
        raise SystemExit(2)

    canonical, observed = identity(listing)
    record = {
        "schema": "reviewgraphen.benchmark.backend_identity.v1",
        "url": MODELS_URL,
        "listing": listing,
        "canonical_identity": canonical,
        "identity_sha256": observed,
        "expected_sha256": expected,
        "gate": "passed" if observed == expected else "failed",
    }
    output.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if observed != expected:
        print(
            "BACKEND IDENTITY GATE FAILED\n"
            f"  expected {expected}\n  observed {observed}\n  canonical {canonical}",
            file=sys.stderr,
        )
        raise SystemExit(3)
    print(f"backend identity verified: {observed}")


if __name__ == "__main__":
    main()
