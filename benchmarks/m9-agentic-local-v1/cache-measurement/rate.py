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
    if "timestamp" not in e:
        continue
    t = datetime.fromisoformat(e["timestamp"].replace("Z", "+00:00"))
    kind = e.get("type")
    chars = 0
    btype = None
    if kind == "assistant":
        for b in (e.get("message") or {}).get("content") or []:
            btype = b.get("type")
            if btype == "thinking":
                chars = len(b.get("thinking") or b.get("text") or "")
            elif btype == "text":
                chars = len(b.get("text") or "")
    events.append((t, kind, btype, chars))

events.sort(key=lambda x: x[0])
print(f"{'gap_s':>9} {'block':>9} {'chars':>7} {'~tokens':>8} {'tok/s':>8}")
rows = []
for i in range(1, len(events)):
    dt = (events[i][0] - events[i - 1][0]).total_seconds()
    _, kind, btype, chars = events[i]
    if kind == "assistant" and chars > 0:
        tok = chars / 4
        rows.append((dt, btype, chars, tok, tok / dt if dt else 0))
for dt, btype, chars, tok, rate in sorted(rows, reverse=True)[:12]:
    print(f"{dt:9.1f} {btype:>9} {chars:7d} {tok:8.0f} {rate:8.1f}")

big = [r for r in rows if r[2] > 10000]
if big:
    tt = sum(r[0] for r in big)
    tk = sum(r[3] for r in big)
    print()
    print(f"the 3 largest thinking blocks: {tt:.0f}s for ~{tk:.0f} tokens = {tk/tt:.1f} tok/s")
    print(f"operator's measured code decode rate: 36.4 tok/s  -> {36.4/(tk/tt):.1f}x slower")
