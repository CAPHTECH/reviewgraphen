import gzip
import json
from datetime import datetime
from pathlib import Path

p = Path(
    "/home/rizumita/workspace/reviewgraphen/.claude/worktrees/"
    "agent-abbf383d1b02d8726/benchmarks/m9-agentic-local-v1/runs-v2/skill-1/stream.jsonl.gz"
)

events = []
for line in gzip.open(p, "rt", encoding="utf-8", errors="replace"):
    line = line.strip()
    if not line:
        continue
    try:
        e = json.loads(line)
    except json.JSONDecodeError:
        continue
    if "timestamp" in e:
        t = datetime.fromisoformat(e["timestamp"].replace("Z", "+00:00"))
        label = e.get("type")
        if label == "assistant":
            blocks = [b.get("type") for b in (e.get("message") or {}).get("content") or []]
            names = [
                b.get("name")
                for b in (e.get("message") or {}).get("content") or []
                if b.get("type") == "tool_use"
            ]
            label = f"assistant({','.join(blocks) or '-'}{'/' + ','.join(names) if names else ''})"
        elif label == "user":
            label = "user(tool_result)"
        events.append((t, label, e.get("duration_api_ms"), e.get("duration_ms")))

events.sort(key=lambda x: x[0])
print("timestamped events:", len(events))
print("span:", (events[-1][0] - events[0][0]).total_seconds(), "s")
print()

gaps = []
for i in range(len(events) - 1):
    dt = (events[i + 1][0] - events[i][0]).total_seconds()
    gaps.append((dt, events[i][1], events[i + 1][1]))

total = sum(g[0] for g in gaps)
print(f"sum of inter-event gaps: {total:.1f}s")
print()
print("largest 15 gaps (seconds, from -> to):")
for dt, a, b in sorted(gaps, reverse=True)[:15]:
    print(f"  {dt:8.1f}  {a[:44]:44s} -> {b[:34]}")

print()
after_user = sum(dt for dt, a, _ in gaps if a.startswith("user"))
after_asst = sum(dt for dt, a, _ in gaps if a.startswith("assistant"))
print(f"time following a tool_result (model thinking/prefill): {after_user:8.1f}s")
print(f"time following an assistant event:                     {after_asst:8.1f}s")
for _, _, dapi, dms in events:
    if dapi:
        print("result duration_api_ms:", dapi, " duration_ms:", dms)
