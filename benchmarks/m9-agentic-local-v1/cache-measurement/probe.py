#!/usr/bin/env python3
"""Cache measurement: send an identical prefix twice and time both.

Direct /v1/messages calls, not Claude Code, so the request bytes are fully
under our control and the prefix is provably identical. Timing settles what
token accounting cannot: the operator measured 0.147 s cache hits, so the
mechanism works for some shape; cache_read_input_tokens: 0 may be this
backend not reporting rather than not caching.
"""

import hashlib
import json
import time
import urllib.request

URL = "http://192.168.68.71:11999/v1/messages"


def call(body, label):
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        URL,
        data=data,
        headers={
            "Content-Type": "application/json",
            "x-api-key": "ollama",
            "anthropic-version": "2023-06-01",
        },
        method="POST",
    )
    t0 = time.monotonic()
    with urllib.request.urlopen(req, timeout=1800) as r:
        payload = json.loads(r.read().decode())
    dt = time.monotonic() - t0
    u = payload.get("usage", {})
    print(
        f"{label:32s} {dt:8.2f}s  in={u.get('input_tokens')} out={u.get('output_tokens')} "
        f"cache_read={u.get('cache_read_input_tokens')} cache_create={u.get('cache_creation_input_tokens')}",
        flush=True,
    )
    return dt, u


filler = "The quick brown fox jumps over the lazy dog. " * 900
print("prefix chars:", len(filler), "sha256:", hashlib.sha256(filler.encode()).hexdigest()[:16], flush=True)


def body(tail):
    return {
        "model": "qwen3.8:27b-mlx",
        "max_tokens": 8,
        "messages": [{"role": "user", "content": filler + "\n\n" + tail}],
    }


d1, _ = call(body("Reply with the single word: ONE"), "call 1 (cold)")
d2, _ = call(body("Reply with the single word: ONE"), "call 2 (byte-identical)")
d3, _ = call(body("Reply with the single word: TWO"), "call 3 (same prefix, new tail)")
print()
print(f"identical repeat   : {d1:.2f}s -> {d2:.2f}s  ({(d1/d2 if d2 else 0):.1f}x)")
print(f"shared-prefix reuse: {d1:.2f}s -> {d3:.2f}s  ({(d1/d3 if d3 else 0):.1f}x)")
