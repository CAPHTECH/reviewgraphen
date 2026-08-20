#!/usr/bin/env python3
"""Per-trial output-volume and depth profile.

This is the record the open question needs: whether v4 `skill-1`'s 3.5x
output increase over the dspark run is a property of the baseline decode
path, of this task at this depth, or one trial's variance. n=1 cannot
separate them; the series can, but only if every trial carries the same
decomposition.

Derived entirely from the retained stream. No requests, no re-runs. The m10
series records it after every completed trial.

usage: output_profile.py <result-dir>
"""

from __future__ import annotations

import gzip
import json
import sys
from datetime import datetime
from pathlib import Path

PREFILL_TOK_PER_S = 311.0
DEEP_OUTPUT_TOKENS = 1000


def load(result: Path) -> str:
    s = result / "stream.jsonl"
    if s.exists():
        return s.read_text(encoding="utf-8", errors="replace")
    g = result / "stream.jsonl.gz"
    if g.exists():
        return gzip.open(g, "rt", encoding="utf-8", errors="replace").read()
    raise SystemExit(f"no stream in {result}")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: output_profile.py <result-dir>")
    result = Path(sys.argv[1])

    events = []
    think = text = 0
    reported_out = reported_in = None
    for line in load(result).splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            e = json.loads(line)
        except json.JSONDecodeError:
            continue
        if e.get("type") == "result":
            u = e.get("usage") or {}
            reported_out = u.get("output_tokens")
            reported_in = u.get("input_tokens")
            continue
        if e.get("type") != "assistant" or "timestamp" not in e:
            continue
        m = e.get("message") or {}
        u = m.get("usage") or {}
        ch = 0
        for b in m.get("content") or []:
            if b.get("type") == "thinking":
                n = len(b.get("thinking") or b.get("text") or "")
                think += n
                ch += n
            elif b.get("type") == "text":
                n = len(b.get("text") or "")
                text += n
                ch += n
        events.append(
            (datetime.fromisoformat(e["timestamp"].replace("Z", "+00:00")), u.get("input_tokens"), ch)
        )

    events.sort(key=lambda x: x[0])
    calls = []
    for t, ctx, ch in events:
        if calls and calls[-1]["ctx"] == ctx:
            calls[-1]["end"] = t
            calls[-1]["chars"] += ch
        else:
            calls.append({"ctx": ctx, "end": t, "chars": ch})

    rows = []
    prev = None
    for i, c in enumerate(calls):
        base = prev if i else c["end"]
        rows.append((c["ctx"], c["chars"] / 4, (c["end"] - base).total_seconds()))
        prev = c["end"]

    deep = [r for r in rows if r[1] > DEEP_OUTPUT_TOKENS]
    shal = [r for r in rows if r[1] <= DEEP_OUTPUT_TOKENS]
    dt, dk = sum(r[2] for r in deep), sum(r[1] for r in deep)
    st, sk = sum(r[2] for r in shal), sum(r[1] for r in shal)
    span = (events[-1][0] - events[0][0]).total_seconds() if events else 0
    ctx_sum = sum(r[0] for r in rows if r[0])
    beating = sum(1 for i, r in enumerate(rows) if i and r[0] and r[2] < r[0] / PREFILL_TOK_PER_S)

    profile = {
        "schema": "reviewgraphen.benchmark.m10_output_profile.v1",
        "trial": result.name,
        "api_calls": len(rows),
        "span_seconds": round(span, 1),
        "reported_output_tokens": reported_out,
        "reported_input_tokens": reported_in,
        "char_estimated_output_tokens": round((think + text) / 4),
        "thinking_chars": think,
        "final_text_chars": text,
        "thinking_share": round(think / (think + text), 4) if (think + text) else None,
        "context_depth_min": min((r[0] for r in rows if r[0]), default=None),
        "context_depth_max": max((r[0] for r in rows if r[0]), default=None),
        "summed_context_tokens": ctx_sum,
        "full_recompute_seconds_if_no_cache": round(ctx_sum / PREFILL_TOK_PER_S, 1) if ctx_sum else None,
        "calls_beating_own_full_recompute": beating,
        "deep": {
            "calls": len(deep),
            "output_tokens": round(dk),
            "seconds": round(dt, 1),
            "tok_per_s": round(dk / dt, 1) if dt else None,
            "share_of_span": round(dt / span, 3) if span else None,
        },
        "shallow": {
            "calls": len(shal),
            "output_tokens": round(sk),
            "seconds": round(st, 1),
            "tok_per_s": round(sk / st, 1) if st else None,
        },
        "deep_threshold_output_tokens": DEEP_OUTPUT_TOKENS,
        "note": "deep/shallow split is by a call's own output volume; elapsed includes tool execution between calls and is not separated",
    }
    (result / "output-profile.json").write_text(
        json.dumps(profile, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps(profile, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
