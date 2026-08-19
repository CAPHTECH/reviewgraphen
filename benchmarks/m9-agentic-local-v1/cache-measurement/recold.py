#!/usr/bin/env python3
"""Re-measure cold prefill with a genuinely novel prefix, to check whether
the 809.63 s observation reproduces or was spurious.

Two sizes, each novel, each followed by an identical repeat.
"""

import hashlib
import json
import time
import urllib.request

URL = "http://192.168.68.71:11999/v1/messages"


def call(text, label):
    body = {
        "model": "qwen3.8:27b-mlx",
        "max_tokens": 8,
        "messages": [{"role": "user", "content": text}],
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
    with urllib.request.urlopen(req, timeout=3600) as r:
        payload = json.loads(r.read().decode())
    dt = time.monotonic() - t0
    u = payload.get("usage", {})
    n = u.get("input_tokens")
    rate = (n / dt) if dt else 0
    print(f"{label:34s} {dt:8.2f}s  in={n} out={u.get('output_tokens')}  ~{rate:7.1f} tok/s", flush=True)
    return dt, n


# Novel, non-repetitive filler so nothing can match an earlier prefix.
def novel(seed, words):
    out = []
    x = seed
    vocab = ["alpha","bravo","charlie","delta","echo","foxtrot","golf","hotel",
             "india","juliet","kilo","lima","mike","november","oscar","papa"]
    for i in range(words):
        x = (x * 1103515245 + 12345) & 0x7FFFFFFF
        out.append(vocab[x % len(vocab)])
        if i % 12 == 11:
            out.append("\n")
    return " ".join(out)

small = novel(20260819, 6000)
print("small prefix sha256:", hashlib.sha256(small.encode()).hexdigest()[:16], "chars", len(small), flush=True)
d1, n1 = call(small + "\n\nReply with the single word: ONE", "novel ~6k words (cold)")
d2, _ = call(small + "\n\nReply with the single word: ONE", "same, byte-identical (warm)")
print()
print(f"cold {d1:.2f}s -> warm {d2:.2f}s  ({(d1/d2 if d2 else 0):.0f}x)")
