import json
from pathlib import Path

d = Path("/tmp/m9cache/requests")
a = (d / "002.json").read_bytes()
b = (d / "003.json").read_bytes()
c = (d / "004.json").read_bytes()


def first_div(x, y):
    n = min(len(x), len(y))
    for i in range(n):
        if x[i] != y[i]:
            return i
    return n


for name, x, y in (("002 vs 003", a, b), ("003 vs 004", b, c)):
    i = first_div(x, y)
    print(f"--- {name}: len {len(x)} vs {len(y)}, first divergence at byte {i} "
          f"({100*i/max(len(x),len(y)):.1f}% of the larger)")
    print("   before:", x[max(0, i - 60):i].decode("utf-8", "replace").replace("\n", "\\n"))
    print("   A next:", x[i:i + 80].decode("utf-8", "replace").replace("\n", "\\n"))
    print("   B next:", y[i:i + 80].decode("utf-8", "replace").replace("\n", "\\n"))
    print()

# What actually varies structurally
for name, raw in (("002", a), ("003", b), ("004", c)):
    o = json.loads(raw)
    msgs = o.get("messages", [])
    sys_blocks = o.get("system") or []
    print(f"{name}: messages={len(msgs)} system_blocks={len(sys_blocks)} "
          f"tools={len(o.get('tools') or [])} keys={sorted(o.keys())}")
