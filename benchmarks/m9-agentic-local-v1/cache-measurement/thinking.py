import gzip
import json
from datetime import datetime
from pathlib import Path

p = Path(
    "/home/rizumita/workspace/reviewgraphen/.claude/worktrees/"
    "agent-abbf383d1b02d8726/benchmarks/m9-agentic-local-v1/runs-v2/skill-1/stream.jsonl.gz"
)

think_chars = 0
text_chars = 0
think_blocks = []
prev_ts = None
rows = []
for line in gzip.open(p, "rt", encoding="utf-8", errors="replace"):
    line = line.strip()
    if not line:
        continue
    try:
        e = json.loads(line)
    except json.JSONDecodeError:
        continue
    if e.get("type") != "assistant":
        continue
    ts = e.get("timestamp")
    t = datetime.fromisoformat(ts.replace("Z", "+00:00")) if ts else None
    for b in (e.get("message") or {}).get("content") or []:
        if b.get("type") == "thinking":
            s = b.get("thinking") or b.get("text") or ""
            think_chars += len(s)
            think_blocks.append(len(s))
            rows.append((t, "thinking", len(s)))
        elif b.get("type") == "text":
            s = b.get("text") or ""
            text_chars += len(s)
            rows.append((t, "text", len(s)))

print("thinking blocks:", len(think_blocks))
print("thinking chars total:", think_chars)
print("final-text chars total:", text_chars)
print("largest thinking blocks (chars):", sorted(think_blocks, reverse=True)[:8])
print()
print("operator's measured post-change figure: 2,942 thinking chars + 6,554 body, per response")
if think_blocks:
    print(f"observed mean thinking block: {think_chars/len(think_blocks):.0f} chars")
    print(f"observed max  thinking block: {max(think_blocks)} chars")
print()
print("rough token estimate (chars/4):")
print(f"  thinking ~ {think_chars//4:,} tokens")
print(f"  text     ~ {text_chars//4:,} tokens")
print(f"  reported output_tokens for the trial: 20,264")
