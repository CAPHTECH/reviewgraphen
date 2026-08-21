#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import pathlib

ROOT = pathlib.Path("/home/rizumita/workspace/reviewgraphen")
SOURCE = pathlib.Path("/home/rizumita/github/fsl/rust/fsl-core/src/compose.rs")
OUT = ROOT / "benchmarks/m15-qwen-intelligent-reviewgraphen-v1/cards"
EXPECTED_SOURCE_HASH = "00240296768317325e637b9d199815cc10a16cd6cb503f06a13c6c3487c51567"
source_bytes = SOURCE.read_bytes()
if hashlib.sha256(source_bytes).hexdigest() != EXPECTED_SOURCE_HASH:
    raise SystemExit("frozen source identity mismatch")
lines = source_bytes.decode().splitlines()

ranges = {
    "parse-front": (51, 154),
    "component-definitions": (155, 248),
    "target-body": (249, 372),
    "component-rewrite-front": (388, 499),
    "component-rewrite-tail": (500, 584),
    "statement-rewrite": (585, 740),
    "expression-rewrite": (741, 855),
    "compose-rewrite": (856, 1008),
    "sync-alias-boundary": (1009, 1120),
    "alias-resolution": (1121, 1240),
    "substitution-front": (1241, 1335),
    "substitution-tail": (1336, 1421),
}

OUT.mkdir(parents=True, exist_ok=True)
catalog = []
for card, (start, end) in ranges.items():
    source_id = f"rust/fsl-core/src/compose.rs:{start}-{end}"
    numbered = "\n".join(f"{line_no}: {lines[line_no - 1]}" for line_no in range(start, end + 1))
    payload = {
        "schema": "reviewgraphen.benchmark.incremental_source_projection.v1",
        "projection_id": f"m15:lower_compose:{card}",
        "card_id": card,
        "source_ids": [source_id],
        "source": {"path": "rust/fsl-core/src/compose.rs", "start_line": start, "end_line": end, "text": numbered},
        "authority": "accepted frozen source bytes; review conclusions remain claims",
        "information_loss": ["Only this declared line window is included.", "No type resolution, runtime trace, tests, or external definitions are included."],
    }
    encoded = (json.dumps(payload, ensure_ascii=False, separators=(",", ":")) + "\n").encode()
    (OUT / f"{card}.json").write_bytes(encoded)
    catalog.append({"card_id": card, "projection_id": payload["projection_id"], "source_id": source_id, "bytes": len(encoded), "sha256": hashlib.sha256(encoded).hexdigest()})

overview = {
    "schema": "reviewgraphen.benchmark.incremental_overview_projection.v1",
    "projection_id": "m15:lower_compose:overview",
    "target": {
        "kind": "function",
        "label": "crate::rust::fsl-core::src::compose::lower_compose",
        "source_id": "rust/fsl-core/src/compose.rs:249-372",
        "documented_contract": "Returns CoreError for missing/invalid components, unknown aliases, or incompatible interfaces."
    },
    "accepted_relations": [
        {"kind": "calls", "target": "parse_component"},
        {"kind": "calls", "target": "rewrite_component_item"},
        {"kind": "calls", "target": "rewrite_compose_statements"},
        {"kind": "calls", "target": "sync_action"}
    ],
    "available_expansions": [{"card_id": item["card_id"], "source_id": item["source_id"], "bytes": item["bytes"]} for item in catalog],
    "authority": "accepted symbol/one-hop syntactic facts; not a review claim or verification",
    "explicit_unknowns": ["callee behavior is absent until its source card is requested", "types and external syntax semantics are unresolved", "tests and runtime evidence are unavailable"],
    "information_loss": ["The overview contains no source body.", "Only declared cards may be expanded in this run."]
}
overview_encoded = (json.dumps(overview, ensure_ascii=False, separators=(",", ":")) + "\n").encode()
(OUT / "overview.json").write_bytes(overview_encoded)
inventory = {
    "schema": "reviewgraphen.benchmark.m15_card_inventory.v1",
    "source_sha256": EXPECTED_SOURCE_HASH,
    "overview": {"projection_id": overview["projection_id"], "bytes": len(overview_encoded), "sha256": hashlib.sha256(overview_encoded).hexdigest()},
    "cards": catalog,
}
(OUT / "inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")
