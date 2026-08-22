#!/usr/bin/env bash
set -euo pipefail
root=/home/rizumita/workspace/reviewgraphen
exp=$root/benchmarks/m16-qwen-chained-reviewgraphen-v1
judge=$exp/judge
result=$judge/codex-result
[[ ! -e "$result" ]] || { echo "judge result directory must be fresh" >&2; exit 64; }
mkdir -p "$result"
codex_home=$(mktemp -d /tmp/m16-codex-home.XXXXXX)
chmod 700 "$codex_home"
cp /home/rizumita/.codex/auth.json "$codex_home/auth.json"
codex_bin=/home/rizumita/.local/share/mise/installs/codex/0.147.0/bin/codex
code_host=/home/rizumita/.local/share/mise/installs/codex/0.147.0/bin/codex-code-mode-host
bwrap=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
source_file=/home/rizumita/github/fsl/rust/fsl-core/src/compose.rs
started=$(date +%s)
set +e
"$bwrap" --die-with-parent --unshare-pid --unshare-ipc --unshare-uts --proc /proc --dev /dev \
  --ro-bind /usr /usr --ro-bind /bin /bin --ro-bind /lib /lib --ro-bind /lib64 /lib64 --ro-bind /etc /etc \
  --dir /run --dir /run/systemd --dir /run/systemd/resolve \
  --ro-bind /run/systemd/resolve/stub-resolv.conf /run/systemd/resolve/stub-resolv.conf \
  --dir /home --dir /home/codex --bind "$codex_home" /home/codex/.codex \
  --ro-bind "$codex_bin" /codex --ro-bind "$code_host" /codex-code-mode-host \
  --dir /workspace --dir /workspace/sources --dir /workspace/sources/rust --dir /workspace/sources/rust/fsl-core --dir /workspace/sources/rust/fsl-core/src \
  --ro-bind "$judge/00-instructions.md" /workspace/00-instructions.md \
  --ro-bind "$judge/01-findings.json" /workspace/01-findings.json \
  --ro-bind "$judge/output-schema.json" /workspace/output-schema.json \
  --ro-bind "$source_file" /workspace/sources/rust/fsl-core/src/compose.rs \
  --bind "$result" /output --tmpfs /tmp --chdir /workspace \
  --setenv HOME /home/codex --setenv CODEX_HOME /home/codex/.codex \
  /codex exec --dangerously-bypass-approvals-and-sandbox --dangerously-bypass-hook-trust \
    --ignore-user-config --ignore-rules --ephemeral --skip-git-repo-check \
    -m gpt-5.6-sol -c model_reasoning_effort="high" \
    --output-schema /workspace/output-schema.json -o /output/judgment.json --json \
    'Read only 00-instructions.md, 01-findings.json, and sources/. Follow 00-instructions.md exactly. Return only the schema-constrained JSON judgment. Set unit_id to m16-completed-01.' \
    > "$result/events.jsonl" 2> "$result/stderr.log"
status=$?
set -e
finished=$(date +%s)
printf '%s\n' "$status" > "$result/exit-status"
printf '%s\n' "$((finished-started))" > "$result/elapsed-seconds"
rm -rf "$codex_home"
exit "$status"

