#!/usr/bin/env python3
"""Detects the backend's truncated-tail bug on a trial's final output.

Operator's description: response tails cut unnaturally -- a sentence ending
mid-way, a code block left unclosed. Patched locally, and the patch can be
lost on a package update SILENTLY, which is why this runs on every trial
rather than only when something looks wrong.

Signatures, checked on the last assistant text block, the last thinking
block, and the terminal result text:

  1. an odd number of ``` fences -- a code block left open;
  2. the text ending without terminal punctuation or a closing bracket;
  3. the text ending mid-word (a trailing fragment with no whitespace and
     no punctuation for a long run).

Deliberately conservative about false positives: a turn that ends in a tool
call legitimately has no text, and is not flagged. Only non-empty tails are
examined.

A hit is a STOP condition, not a retry condition.

usage: detect_truncated_tail.py <result-dir>
"""

from __future__ import annotations

import gzip
import json
import sys
from pathlib import Path

TERMINAL = set(".!?)]}\"'`\n>:;,")


def load(result: Path) -> str:
    stream = result / "stream.jsonl"
    if stream.exists():
        return stream.read_text(encoding="utf-8", errors="replace")
    gz = result / "stream.jsonl.gz"
    if gz.exists():
        return gzip.open(gz, "rt", encoding="utf-8", errors="replace").read()
    raise SystemExit(f"no stream in {result}")


def inspect(label: str, text: str) -> dict | None:
    text = (text or "").rstrip()
    if not text:
        return None
    fences = text.count("```")
    findings = []
    if fences % 2 == 1:
        findings.append(f"odd ``` fence count ({fences}) -- code block left open")
    if text[-1] not in TERMINAL:
        findings.append(f"ends without terminal punctuation: {text[-40:]!r}")
    if not findings:
        return None
    return {"where": label, "tail": text[-200:], "findings": findings}


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: detect_truncated_tail.py <result-dir>")
    result = Path(sys.argv[1])

    last_text = ""
    last_thinking = ""
    result_text = ""
    for line in load(result).splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            e = json.loads(line)
        except json.JSONDecodeError:
            continue
        if e.get("type") == "assistant":
            for b in (e.get("message") or {}).get("content") or []:
                if b.get("type") == "text" and (b.get("text") or "").strip():
                    last_text = b["text"]
                elif b.get("type") == "thinking":
                    t = b.get("thinking") or b.get("text") or ""
                    if t.strip():
                        last_thinking = t
        elif e.get("type") == "result":
            result_text = e.get("result") or ""

    suspects = [
        s
        for s in (
            inspect("final_result_text", result_text),
            inspect("last_assistant_text", last_text),
            inspect("last_thinking_block", last_thinking),
        )
        if s
    ]

    record = {
        "schema": "reviewgraphen.benchmark.m10_truncated_tail.v1",
        "trial": result.name,
        "suspected": bool(suspects),
        "suspects": suspects,
        "note": "operator-described backend bug: tails cut mid-sentence or with an unclosed code block; the local patch can be lost silently on a package update",
        "handling": "a hit is a STOP condition -- record time and approximate token count, do not retry",
    }
    (result / "truncated-tail.json").write_text(
        json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps(record, indent=2, sort_keys=True))
    sys.exit(1 if suspects else 0)


if __name__ == "__main__":
    main()
