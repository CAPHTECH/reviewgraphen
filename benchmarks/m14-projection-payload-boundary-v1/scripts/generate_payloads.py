#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import pathlib

ROOT = pathlib.Path("/home/rizumita/workspace/reviewgraphen")
SOURCE = ROOT / "benchmarks/m11-review-agentic-local-v1/projection/lower_compose.json"
OUTPUT = ROOT / "benchmarks/m14-projection-payload-boundary-v1/payloads"
PROJECTION_ID = "benchmark-target-context:sha256:82272361a1f20d32bb4580f27626994fec18549ab6b1c78db5bcada984778177"

source = json.loads(SOURCE.read_text())
variants = {
    "compact": {
        key: source[key]
        for key in ("schema", "projection_id", "snapshot_id", "target", "information_loss")
    },
    "graph-only": {key: value for key, value in source.items() if key not in {"sources", "extraction"}},
    "source-rich": {key: value for key, value in source.items() if key != "extraction"},
    "full": source,
}

OUTPUT.mkdir(parents=True, exist_ok=True)
inventory = []
for variant, body in variants.items():
    header = f"m14-{variant}-header-6b21"
    trailer = f"m14-{variant}-trailer-a84e"
    payload = {
        "benchmark_header_nonce": header,
        "benchmark_variant": variant,
        **body,
        "benchmark_trailer_nonce": trailer,
    }
    encoded = (json.dumps(payload, ensure_ascii=False, separators=(",", ":")) + "\n").encode()
    path = OUTPUT / f"{variant}.json"
    path.write_bytes(encoded)
    inventory.append(
        {
            "variant": variant,
            "bytes": len(encoded),
            "sha256": hashlib.sha256(encoded).hexdigest(),
            "header_nonce": header,
            "trailer_nonce": trailer,
            "projection_id": PROJECTION_ID,
        }
    )

(OUTPUT / "inventory.json").write_text(
    json.dumps({"schema": "reviewgraphen.benchmark.m14_payload_inventory.v1", "payloads": inventory}, indent=2)
    + "\n"
)

