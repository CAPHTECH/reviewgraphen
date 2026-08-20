#!/usr/bin/env python3
import json
import pathlib
import sys

p = pathlib.Path(sys.argv[1])
try:
    x = json.loads(p.read_text())
    assert set(x) == {"schema", "summary", "findings", "limitations"}
    assert x["schema"] == "reviewgraphen.benchmark.agentic_review_output.v1"
    assert isinstance(x["summary"], str)
    assert isinstance(x["limitations"], list) and all(isinstance(v, str) for v in x["limitations"])
    assert isinstance(x["findings"], list)
    for f in x["findings"]:
        assert set(f) == {"title", "severity", "file", "line", "description", "evidence"}
        assert f["severity"] in {"critical", "high", "medium", "low"}
        assert f["file"] in {
            "rust/fsl-core/src/bin/fsl-parse-kernel.rs",
            "rust/fsl-core/src/compose.rs",
            "rust/fsl-core/src/db.rs",
            "rust/fsl-core/src/diagnostics.rs",
        }
        assert isinstance(f["line"], int) and f["line"] > 0
        assert all(isinstance(f[k], str) and f[k] for k in ("title", "description", "evidence"))
except Exception as e:
    print(f"invalid: {e}", file=sys.stderr)
    raise SystemExit(1)
print(json.dumps({"valid": True, "finding_count": len(x["findings"])}, sort_keys=True))

