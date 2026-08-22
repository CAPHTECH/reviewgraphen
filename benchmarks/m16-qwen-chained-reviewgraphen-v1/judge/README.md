# Blind Codex judge

The judge input contains only the frozen m7 judge instructions, normalized
candidate findings, the compatible output schema, and the full frozen
`compose.rs`.  It excludes Qwen cell identity, Projection trace, timing, token
usage, and completion metrics.

The judgment is non-authoritative review evidence.  It does not verify or
human-accept a finding.

