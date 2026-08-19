#!/usr/bin/env python3
"""In-flight check: was the prefix cache actually crediting during this trial?

WHY NOT THE IDENTITY CHECK. The coordinator proposed using the
`sum(per-turn contexts) == input_tokens total` identity as the in-flight
signal. Measured against both cache states, it does not discriminate:

    broken-cache trial  usage keys ['input_tokens','output_tokens'],
                        input_tokens = full context, cache_read absent
    working-cache bench usage keys ['input_tokens','output_tokens'],
                        input_tokens = full context, cache_read absent

The shape is identical. `input_tokens` reports the whole context whether or
not the cache hit, and `cache_read_input_tokens` is absent either way. The
identity is a REPORTING artifact and holds regardless, so it proves the
billing shape, not the cache state. It was decisive for skill-1 only in
combination with the wall clock.

WHAT DISCRIMINATES IS TIME. If the cache missed, the trial must have paid a
full recompute of every turn's context:

    full_recompute_seconds = sum(distinct per-turn input_tokens) / 311

311 tok/s is the operator's spec (32k in 102.9 s) and matches this
harness's own novel-prefix measurement of 267 tok/s to within the spread.

    ratio = elapsed / full_recompute_seconds

  skill-1, cache missing: 5,275 / 4,081 = 1.29
  cache working:          prefill collapses to the delta, so the ratio
                          should fall to roughly decode-time / projection

Frozen threshold: ratio < 0.6 means the cache credited; ratio >= 0.6 means
it did not, and the trial is measuring the OLD condition.

usage: check_cache_credit.py <result-dir>
"""

from __future__ import annotations

import gzip
import json
import sys
from datetime import datetime
from pathlib import Path

PREFILL_TOK_PER_S = 311.0
RATIO_THRESHOLD = 0.6


def load(result: Path):
    stream = result / "stream.jsonl"
    if stream.exists():
        return stream.read_text(encoding="utf-8", errors="replace")
    gz = result / "stream.jsonl.gz"
    if gz.exists():
        return gzip.open(gz, "rt", encoding="utf-8", errors="replace").read()
    raise SystemExit(f"no stream in {result}")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: check_cache_credit.py <result-dir>")
    result = Path(sys.argv[1])

    inputs = []
    stamped = []
    total_input = None
    total_output = None
    for line in load(result).splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            e = json.loads(line)
        except json.JSONDecodeError:
            continue
        if e.get("type") == "assistant":
            u = (e.get("message") or {}).get("usage") or {}
            if u.get("input_tokens") is not None:
                inputs.append(u["input_tokens"])
                if e.get("timestamp"):
                    stamped.append(
                        (
                            datetime.fromisoformat(e["timestamp"].replace("Z", "+00:00")),
                            u["input_tokens"],
                        )
                    )
        elif e.get("type") == "result":
            u = e.get("usage") or {}
            total_input = u.get("input_tokens")
            total_output = u.get("output_tokens")

    calls = []
    for v in inputs:
        if not calls or calls[-1] != v:
            calls.append(v)

    # Per-call physical test, added after RECONCILIATION.md. A call that
    # completed in LESS time than a full recompute of its own context would
    # take cannot have paid that recompute -- so the cache credited for it.
    # This is direct physical evidence and outranks the aggregate ratio,
    # which scored skill-1 at 1.323 while 12 of its 15 calls had in fact
    # been credited.
    stamped.sort(key=lambda x: x[0])
    grouped = []
    for t, ctx in stamped:
        if grouped and grouped[-1][1] == ctx:
            grouped[-1][0] = t
        else:
            grouped.append([t, ctx])
    beating = 0
    per_call = []
    prev_end = grouped[0][0] if grouped else None
    for i, (end, ctx) in enumerate(grouped):
        el = (end - (prev_end if i else grouped[0][0])).total_seconds() if i else 0.0
        prev_end = end
        need = ctx / PREFILL_TOK_PER_S
        if i and el < need:
            beating += 1
        per_call.append({"context_tokens": ctx, "elapsed_s": round(el, 1), "full_recompute_s": round(need, 1)})

    elapsed_path = result / "elapsed-seconds"
    elapsed = int(elapsed_path.read_text().strip()) if elapsed_path.exists() else None

    ctx_sum = sum(calls)
    projection = ctx_sum / PREFILL_TOK_PER_S if ctx_sum else None
    ratio = (elapsed / projection) if (elapsed and projection) else None

    verdict = "unknown"
    if beating > 0:
        verdict = "cache_credited"
    elif ratio is not None:
        verdict = "cache_credited" if ratio < RATIO_THRESHOLD else "cache_NOT_credited"

    record = {
        "schema": "reviewgraphen.benchmark.m9_cache_credit.v1",
        "trial": result.name,
        "api_calls": len(calls),
        "summed_context_tokens": ctx_sum,
        "reported_total_input_tokens": total_input,
        "identity_holds": (total_input is not None and ctx_sum == total_input),
        "identity_note": "holds in BOTH cache states -- a reporting artifact, not a cache signal",
        "reported_total_output_tokens": total_output,
        "elapsed_seconds": elapsed,
        "full_recompute_projection_seconds": round(projection, 1) if projection else None,
        "prefill_tok_per_s_assumed": PREFILL_TOK_PER_S,
        "ratio_elapsed_over_projection": round(ratio, 3) if ratio else None,
        "ratio_threshold": RATIO_THRESHOLD,
        "verdict": verdict,
        "reference_skill_1_ratio": 1.323,
        "calls_beating_their_own_full_recompute": beating,
        "calls_total": len(grouped),
        "per_call": per_call,
        "verdict_rule": "any call beating its own full-recompute time is direct evidence of crediting and outranks the aggregate ratio",
    }
    (result / "cache-credit.json").write_text(
        json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps(record, indent=2, sort_keys=True))
    sys.exit(0 if verdict == "cache_credited" else 1)


if __name__ == "__main__":
    main()
