#!/usr/bin/env bash
set -uo pipefail
if [[ $# -ne 3 ]]; then
  echo "usage: run_trial.sh <variant> <replicate> <fresh-result-dir>" >&2
  exit 64
fi
variant=$1
replicate=$2
result=$3
case "$variant" in compact|graph-only|source-rich|full) ;; *) exit 64 ;; esac
[[ "$replicate" =~ ^[1-9][0-9]*$ ]] || exit 64
[[ ! -e "$result" ]] || { echo "result directory must be fresh" >&2; exit 64; }

root=/home/rizumita/workspace/reviewgraphen
exp=$root/benchmarks/m14-projection-payload-boundary-v1
base=$root/benchmarks/m11-review-agentic-local-v1
bwrap=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
trial=$variant-r$replicate
scratch=/tmp/m14-scratch-$trial
config=/tmp/m14-config-$trial
mkdir -p "$result"
python3 "$base/scripts/check_backend_identity.py" \
  "$(cat "$exp/PINNED_BACKEND_IDENTITY")" \
  "$(cat "$exp/PINNED_BACKEND_HEALTH")" \
  "$result/backend-identity.json" || exit 66
rm -rf "$scratch" "$config"
mkdir -p "$scratch/.reviewgraphen/bin" "$config"
chmod 700 "$config"
cp "$exp/scripts/reviewgraphen-context-probe" "$scratch/.reviewgraphen/bin/"
cp "$exp/payloads/$variant.json" "$scratch/.reviewgraphen/projection.json"
chmod 755 "$scratch/.reviewgraphen/bin/reviewgraphen-context-probe"
jq --arg variant "$variant" '.payloads[] | select(.variant == $variant)' "$exp/payloads/inventory.json" > "$result/expected.json"

started=$(date +%s)
set +e
"$bwrap" \
  --ro-bind / / --dev-bind /dev /dev --proc /proc \
  --tmpfs /home/rizumita/workspace --tmpfs /tmp/claude-1000 \
  --bind "$scratch" "$scratch" --bind "$config" "$config" \
  --ro-bind "$exp" "$exp" --ro-bind "$base" "$base" \
  --setenv CLAUDE_CONFIG_DIR "$config" \
  --setenv ANTHROPIC_BASE_URL http://192.168.68.71:11999 \
  --setenv ANTHROPIC_API_KEY ollama \
  --setenv CLAUDE_CODE_MAX_OUTPUT_TOKENS 4096 \
  --setenv PATH "$scratch/.reviewgraphen/bin:/home/rizumita/.local/share/mise/installs/claude/latest:/usr/local/bin:/usr/bin:/bin" \
  --setenv HOME /home/rizumita --chdir "$scratch" \
  timeout 600 claude --print --model Qwen3.8-27B-MLX-4bit --effort low \
    --output-format stream-json --verbose --permission-mode bypassPermissions \
    --tools "Bash,Write" --allowedTools "Bash,Write" \
    < "$exp/task/PROMPT.md" > "$result/stream.jsonl" 2> "$result/claude.stderr"
status=$?
set -e
finished=$(date +%s)
printf '%s\n' "$status" > "$result/agent-status"
printf '%s\n' "$((finished-started))" > "$result/elapsed-seconds"
printf '%s\n' Qwen3.8-27B-MLX-4bit > "$result/requested-model"
printf '%s\n' low > "$result/requested-effort"
if [[ -f "$scratch/probe.json" ]]; then cp "$scratch/probe.json" "$result/probe.json"; fi
analysis_ok=false
if python3 "$exp/scripts/analyze_trial.py" "$result"; then analysis_ok=true; fi
python3 "$base/scripts/output_profile.py" "$result" > /dev/null
if [[ $status -eq 0 && "$analysis_ok" == true ]]; then outcome=recognized
elif [[ $status -eq 124 ]]; then outcome=timeout
else outcome=not_recognized_or_invalid
fi
printf '%s\n' "$outcome" > "$result/outcome"
sha256sum "$result/stream.jsonl" > "$result/stream.jsonl.sha256"
rm -rf "$config"
echo "trial=$trial outcome=$outcome elapsed=$((finished-started))s"
[[ "$outcome" == recognized ]]

