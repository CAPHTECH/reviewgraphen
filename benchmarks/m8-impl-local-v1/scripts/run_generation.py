#!/usr/bin/env python3
"""Issues exactly one generation request to the local Qwen server and records
its raw SSE stream plus derived metrics.

Deliberate properties, each of which is a standing operator constraint from
`.claude/skills/local-qwen/SKILL.md`:

- exactly one request, never concurrent, never retried (the no-retry rule);
- `max_output_tokens` = 131072, the value the m7 review campaign proved
  necessary — never lowered;
- `temperature`/`top_p`/`top_k`/penalties are never sent, so the server's own
  sampling configuration is used unmodified;
- `reasoning.effort` is sent as `high` to match the frozen m7 condition, and
  is documented there as a measured no-op on the LM Studio backend;
- the endpoint is the `cch`-normalizing proxy on 11999, never 11434.

Reasoning-vs-final attribution matches the m7 shaper: any event whose type
starts with `response.reasoning_` is reasoning; only exactly
`response.output_text.delta` is final content.

usage: run_generation.py <packet-file> <fresh-result-dir>
"""

from __future__ import annotations

import json
import sys
import time
import urllib.request
from pathlib import Path

BASE_URL = "http://192.168.68.71:11999/v1/responses"
MODEL = "qwen3.8:27b-mlx"
MAX_OUTPUT_TOKENS = 131072
MODEL_CONTEXT_WINDOW = 262144
REASONING_EFFORT = "high"
# Generous ceiling: the m7 campaign observed completions at 50-80 min and
# server-side deaths as late as 8 h. This is a client-side backstop only; it
# is not a retry budget, and a timeout is recorded and reported, not retried.
READ_TIMEOUT_SECONDS = 4 * 60 * 60


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: run_generation.py <packet-file> <fresh-result-dir>")
    packet_path = Path(sys.argv[1])
    result_dir = Path(sys.argv[2])
    if result_dir.exists():
        raise SystemExit(f"result directory must be fresh: {result_dir}")
    result_dir.mkdir(parents=True)

    packet = packet_path.read_text(encoding="utf-8")
    body = {
        "model": MODEL,
        "input": [{"role": "user", "content": [{"type": "input_text", "text": packet}]}],
        "max_output_tokens": MAX_OUTPUT_TOKENS,
        "reasoning": {"effort": REASONING_EFFORT},
        "stream": True,
    }
    (result_dir / "request-body.json").write_text(
        json.dumps(body, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    request = urllib.request.Request(
        BASE_URL,
        data=json.dumps(body).encode("utf-8"),
        headers={
            "Content-Type": "application/json",
            "Authorization": "Bearer local",
            "Accept": "text/event-stream",
        },
        method="POST",
    )

    counts: dict[str, int] = {}
    reasoning_bytes = 0
    output_bytes = 0
    final_text: list[str] = []
    completed: dict | None = None
    transport_error: str | None = None
    upstream_status: int | None = None
    started = time.monotonic()
    sse_path = result_dir / "provider-response.sse"

    try:
        with urllib.request.urlopen(request, timeout=READ_TIMEOUT_SECONDS) as response:
            upstream_status = response.status
            with sse_path.open("wb") as sink:
                for raw in response:
                    sink.write(raw)
                    line = raw.decode("utf-8", errors="replace")
                    if not line.startswith("data: "):
                        continue
                    try:
                        event = json.loads(line[len("data: ") :])
                    except json.JSONDecodeError:
                        continue
                    kind = event.get("type", "")
                    counts[kind] = counts.get(kind, 0) + 1
                    if kind.startswith("response.reasoning_") and kind.endswith(".delta"):
                        reasoning_bytes += len(event.get("delta", "").encode("utf-8"))
                    elif kind == "response.output_text.delta":
                        delta = event.get("delta", "")
                        output_bytes += len(delta.encode("utf-8"))
                        final_text.append(delta)
                    elif kind == "response.completed":
                        completed = event.get("response")
    except Exception as error:  # noqa: BLE001 - recorded verbatim, never retried
        transport_error = f"{type(error).__name__}: {error}"

    elapsed = time.monotonic() - started

    usage = (completed or {}).get("usage") or {}
    details = usage.get("output_tokens_details") or {}
    output_items = (completed or {}).get("output") or []
    item_types = [item.get("type") for item in output_items]
    message_item_created = "message" in item_types

    # If the stream carried a final message we prefer the completed record's
    # text; otherwise fall back to the concatenated deltas.
    completed_text = "".join(
        part.get("text", "")
        for item in output_items
        if item.get("type") == "message"
        for part in (item.get("content") or [])
        if part.get("type") == "output_text"
    )
    final = completed_text or "".join(final_text)
    (result_dir / "final-content.txt").write_text(final, encoding="utf-8")

    failure_class = classify(
        transport_error=transport_error,
        upstream_status=upstream_status,
        completed=completed,
        message_item_created=message_item_created,
        final=final,
        usage=usage,
    )

    metrics = {
        "schema": "reviewgraphen.benchmark.m8_impl_local_generation_metrics.v1",
        "packet": str(packet_path),
        "packet_bytes": len(packet.encode("utf-8")),
        "endpoint": BASE_URL,
        "model": MODEL,
        "max_output_tokens": MAX_OUTPUT_TOKENS,
        "model_context_window": MODEL_CONTEXT_WINDOW,
        "reasoning_effort_requested": REASONING_EFFORT,
        "upstream_status": upstream_status,
        "transport_error": transport_error,
        "elapsed_seconds": round(elapsed, 3),
        "elapsed_minutes": round(elapsed / 60, 2),
        "provider_input_tokens": usage.get("input_tokens"),
        "cached_input_tokens": (usage.get("input_tokens_details") or {}).get("cached_tokens"),
        "provider_output_tokens": usage.get("output_tokens"),
        "provider_reported_reasoning_tokens": details.get("reasoning_tokens"),
        "reasoning_share_of_output": share(details.get("reasoning_tokens"), usage.get("output_tokens")),
        "sse_event_counts": dict(sorted(counts.items())),
        "reasoning_delta_utf8_bytes": reasoning_bytes,
        "output_text_delta_utf8_bytes": output_bytes,
        "output_item_types": item_types,
        "message_item_created": message_item_created,
        "final_content_bytes": len(final.encode("utf-8")),
        "empty_final": final.strip() == "",
        "response_status": (completed or {}).get("status"),
        "response_error": (completed or {}).get("error"),
        "response_incomplete_details": (completed or {}).get("incomplete_details"),
        "effective_sampling_reported_by_server": {
            key: (completed or {}).get(key)
            for key in ("temperature", "top_p", "presence_penalty", "frequency_penalty")
        },
        "failure_class": failure_class,
    }
    (result_dir / "generation-metrics.json").write_text(
        json.dumps(metrics, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps(metrics, indent=2, sort_keys=True))
    sys.exit(0 if failure_class == "generation_ok" else 1)


def share(numerator: object, denominator: object) -> float | None:
    if isinstance(numerator, int) and isinstance(denominator, int) and denominator > 0:
        return round(numerator / denominator, 5)
    return None


def classify(
    *,
    transport_error: str | None,
    upstream_status: int | None,
    completed: dict | None,
    message_item_created: bool,
    final: str,
    usage: dict,
) -> str:
    """Failure-class taxonomy, reused from m7-head-local-v1 unchanged in
    meaning. `upstream_*` classes are server-side and are excluded from any
    semantic denominator; `empty_final_after_process_completion` is the
    model's own reasoning-budget failure."""
    if transport_error is not None:
        return "upstream_stream_closed_before_completion"
    if upstream_status is not None and upstream_status >= 500:
        return "upstream_server_failure"
    if completed is None:
        return "upstream_server_stream_incomplete"
    if completed.get("status") != "completed":
        return "upstream_server_stream_incomplete"
    if not message_item_created:
        output_tokens = usage.get("output_tokens")
        if isinstance(output_tokens, int) and output_tokens < 0.95 * MAX_OUTPUT_TOKENS:
            return "upstream_silent_truncation"
        return "empty_final_after_process_completion"
    if final.strip() == "":
        return "empty_final_after_process_completion"
    return "generation_ok"


if __name__ == "__main__":
    main()
