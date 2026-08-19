#!/usr/bin/env python3
"""Is the first request after idle pathological?

Small identical prompt, three times back to back. If call 1 is far slower
than 2 and 3 on a prompt this small, the cost is not prefill volume -- it is
a per-idle-period startup (model or drafter load, or a queue wake).
"""

import json
import time
import urllib.request

URL = "http://192.168.68.71:11999/v1/messages"
PROMPT = "Reply with the single word: OK"


def call(label):
    body = {
        "model": "qwen3.8:27b-mlx",
        "max_tokens": 8,
        "messages": [{"role": "user", "content": PROMPT}],
    }
    req = urllib.request.Request(
        URL,
        data=json.dumps(body).encode(),
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
    print(f"{label:22s} {dt:8.2f}s  in={u.get('input_tokens')} out={u.get('output_tokens')}", flush=True)
    return dt


a = call("small call 1")
b = call("small call 2")
c = call("small call 3")
print()
print(f"tiny prompt, ~{7} input tokens: {a:.2f}s / {b:.2f}s / {c:.2f}s")
