#!/usr/bin/env python3
"""Task-2 harness self-test with a known-correct solution.

Proves the task is solvable under the stated constraints, that the
edit-application path works for task 2, and that a correct edit turns the
red acceptance test green while the 115 existing tests keep passing. NOT an
arm and NOT a comparator, and never shown to any candidate or to the judge.

usage: make_reference_candidate_task2.py <fresh-result-dir>
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

OLD = "                syn::Stmt::Item(Item::Use(nested)) => {"

NEW = """                syn::Stmt::Item(Item::ForeignMod(nested)) => {
                    for foreign in &nested.items {
                        match foreign {
                            syn::ForeignItem::Fn(function) => {
                                hoisted.push(function.sig.ident.to_string());
                            }
                            syn::ForeignItem::Static(value) => {
                                hoisted.push(value.ident.to_string());
                            }
                            syn::ForeignItem::Type(_) => {}
                            _ => conservative_unresolved = true,
                        }
                    }
                }
                syn::Stmt::Item(Item::Use(nested)) => {"""

CANDIDATE = {
    "schema": "reviewgraphen.benchmark.implementation_candidate_output.v1",
    "outcome": "completed",
    "edits": [
        {"file": "crates/reviewgraphen-ingest/src/rust.rs", "old": OLD, "new": NEW}
    ],
    "limitations": ["harness self-test; not an experimental arm"],
}


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: make_reference_candidate_task2.py <fresh-result-dir>")
    result_dir = Path(sys.argv[1])
    result_dir.mkdir(parents=True, exist_ok=True)
    (result_dir / "final-content.txt").write_text(
        "```json\n" + json.dumps(CANDIDATE, indent=2) + "\n```\n", encoding="utf-8"
    )
    print(result_dir)


if __name__ == "__main__":
    main()
