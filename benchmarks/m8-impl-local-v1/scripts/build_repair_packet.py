#!/usr/bin/env python3
"""Builds the conditional repair packet.

Per `preregistration.json` arms.repair, this is run at most once, only when
the methodology arm produced a final message whose verification verdict is
one of edit_anchor_did_not_match / does_not_compile / tests_failed.

Content is the methodology packet unchanged, followed by the model's own
emitted candidate verbatim and the exact output of the failing command.
Nothing else is added and no criterion is relaxed. This is the only place in
the experiment where real executable Evidence is fed back to the model.

usage: build_repair_packet.py <methodology-result-dir> <output-file>
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

PACKET = Path("/tmp/m8-impl-local-v1-packets/packet-methodology.txt")

HEADER = """

---

# Evidence from the harness

Your previous answer to exactly this task is reproduced below, followed by
the verbatim output of the command that was actually run against it in an
isolated copy of the repository. That output is Evidence in the sense of
`docs/10 SS2.2`: a real compiler or test-runner result, not an opinion.

Produce one corrected JSON object in the same schema. The task,
the constraints, the file you may change, and the acceptance test are all
unchanged. Do not change the acceptance test.

## Your previous candidate

```json
%(candidate)s
```

## Harness result

Verdict: `%(verdict)s`

%(detail)s
"""


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: build_repair_packet.py <methodology-result-dir> <output-file>")
    result_dir = Path(sys.argv[1])
    output = Path(sys.argv[2])

    verification = json.loads((result_dir / "verification.json").read_text(encoding="utf-8"))
    verdict = verification.get("verdict")
    candidate_path = result_dir / "candidate.json"
    candidate = candidate_path.read_text(encoding="utf-8") if candidate_path.exists() else "{}"

    sections = []
    if verdict == "edit_anchor_did_not_match":
        sections.append(
            "Your `old` anchor did not match the file exactly and uniquely, so no\n"
            "edit was applied. Application record:\n\n```json\n"
            + json.dumps(verification.get("edit_application"), indent=2)
            + "\n```\n"
        )
    if "build" in verification:
        sections.append(
            "### `cargo build -p reviewgraphen-cli` (exit "
            f"{verification['build']['exit_code']})\n\n```\n"
            + verification["build"]["stderr"][-12000:]
            + "\n```\n"
        )
    if "test" in verification:
        sections.append(
            "### `cargo test -p reviewgraphen-cli` (exit "
            f"{verification['test']['exit_code']})\n\n```\n"
            + (verification["test"]["stdout"] + verification["test"]["stderr"])[-12000:]
            + "\n```\n"
        )

    body = PACKET.read_text(encoding="utf-8") + HEADER % {
        "candidate": candidate.strip(),
        "verdict": verdict,
        "detail": "\n".join(sections),
    }
    output.write_text(body, encoding="utf-8")
    print(json.dumps({"output": str(output), "bytes": len(body.encode("utf-8"))}, indent=2))


if __name__ == "__main__":
    main()
